//! Event-loop and state-handling logic for the AI bridge daemon.
//!
//! The socket I/O lives in `main.rs`; the routing, branch dispatch, and
//! Gatekeeper decisions are kept here as pure, unit-testable logic so the full
//! execution cycle can be verified without a live daemon.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ai_bridge_channels::ChannelName;
use ai_bridge_gatekeeper_core::policy::{decide_policy, PolicyDecision};
use ai_bridge_gatekeeper_core::timers::{evaluate_timeout, gatekeeper_timeout, TimerOutcome};
use ai_bridge_protocol::{
    ExecutionPlan, ExecutionOutcome, MaestroMessage, SubChatOpenRequest, SubChatResult,
    TaskQueryResult, TaskResolved, TaskStatus, TaskSubmitAck,
};
#[cfg(test)]
use ai_bridge_protocol::TaskQuery;
use ai_bridge_subchat::branch::{Branch, BranchName, SubChatError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoopError {
    #[error(transparent)]
    SubChat(#[from] SubChatError),
    #[error("no branch sub-chat is available for channel {0:?}")]
    NoBranch(ChannelName),
    #[error("unsupported message on channel {0:?}")]
    UnsupportedMessage(ChannelName),
    #[error("task not found: {0}")]
    TaskNotFound(String),
    #[error("invalid message type for channel")]
    InvalidMessageType,
    #[error("execution error: {0}")]
    ExecutionError(String),
}

/// A registry of the two execution branches (A and B), each owning its own
/// `Branch` so that sub-chat call indexes are tracked independently.
pub struct BranchRegistry {
    pub branch_a: Branch,
    pub branch_b: Branch,
}

impl BranchRegistry {
    pub fn new(task_id: impl Into<String>) -> Self {
        let task_id = task_id.into();
        Self {
            branch_a: Branch::new(BranchName::A, task_id.clone()),
            branch_b: Branch::new(BranchName::B, task_id),
        }
    }

    fn branch_for(&mut self, channel: ChannelName) -> Result<&mut Branch, LoopError> {
        match channel {
            ChannelName::PrivateA => Ok(&mut self.branch_a),
            ChannelName::PrivateB => Ok(&mut self.branch_b),
            ChannelName::PublicMaestro => Err(LoopError::NoBranch(ChannelName::PublicMaestro)),
        }
    }
}

/// Routes a `SubChatOpenRequest` received on a branch channel to its `Branch`
/// and returns the resulting `SubChatResult`, preserving the assigned
/// `call_index` (never dropped).
pub fn route_branch_request(
    registry: &mut BranchRegistry,
    channel: ChannelName,
    request: &SubChatOpenRequest,
) -> Result<SubChatResult, LoopError> {
    let branch = registry.branch_for(channel)?;
    let result = branch.open_sub_chat(request)?;
    Ok(result)
}

/// Describes how the Gatekeeper decided to handle a maestro plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatekeeperVerdict {
    /// A delegable plan still within the 120 s auto-approval window.
    PendingTimeout,
    /// A delegable plan that reached the gatekeeper timeout and was auto-approved.
    AutoApproved,
    /// A critical (non-delegable) plan held indefinitely for manual owner approval.
    HeldForManualApproval,
}

/// Applies the Gatekeeper timeout policy to a single policy decision.
///
/// - `Delegable` plans are auto-approved after the runtime gatekeeper timeout.
/// - `NonDelegable` (critical) plans are held forever, awaiting owner approval.
pub fn apply_gatekeeper_policy(decision: PolicyDecision, elapsed: Duration) -> GatekeeperVerdict {
    match decision {
        PolicyDecision::Delegable => match evaluate_timeout(true, elapsed) {
            TimerOutcome::AutoApprove => GatekeeperVerdict::AutoApproved,
            TimerOutcome::AwaitingHuman => GatekeeperVerdict::PendingTimeout,
        },
        PolicyDecision::NonDelegable => GatekeeperVerdict::HeldForManualApproval,
    }
}

/// Internal state of a tracked task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskState {
    pub task_id: String,
    pub plan: ExecutionPlan,
    pub decision: PolicyDecision,
    pub status: TaskStatus,
    pub created_at: Instant,
    pub execution_result: Option<ExecutionOutcome>,
}

/// Tracks in-flight plans so the synchronous Gatekeeper timeout and the manual
/// approval queue are applied across repeated event-loop ticks.
#[derive(Debug, Default)]
pub struct PendingPlans {
    tasks: HashMap<String, TaskState>,
}

impl PendingPlans {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingests a maestro plan, records the start of its delegation window, and
    /// returns the resulting Gatekeeper verdict plus the TaskSubmitAck to send immediately.
    pub fn ingest(&mut self, plan: &ExecutionPlan, now: Instant) -> (GatekeeperVerdict, TaskSubmitAck) {
        let decision = decide_policy(plan);
        let task_id = plan.task_id.clone();

        let verdict = match decision {
            PolicyDecision::Delegable => {
                let task_state = self
                    .tasks
                    .entry(task_id.clone())
                    .or_insert_with(|| TaskState {
                        task_id: task_id.clone(),
                        plan: plan.clone(),
                        decision,
                        status: TaskStatus::Pending,
                        created_at: now,
                        execution_result: None,
                    });
                let started = task_state.created_at;

                let v = apply_gatekeeper_policy(decision, now.saturating_duration_since(started));
                let status = match v {
                    GatekeeperVerdict::AutoApproved => TaskStatus::DelegableAutoApproved,
                    GatekeeperVerdict::PendingTimeout => TaskStatus::Pending,
                    GatekeeperVerdict::HeldForManualApproval => TaskStatus::HeldForManualApproval,
                };

                // Update the task state
                task_state.status = status.clone();

                v
            }
            PolicyDecision::NonDelegable => {
                self.tasks.insert(
                    task_id.clone(),
                    TaskState {
                        task_id: task_id.clone(),
                        plan: plan.clone(),
                        decision,
                        status: TaskStatus::HeldForManualApproval,
                        created_at: now,
                        execution_result: None,
                    },
                );
                GatekeeperVerdict::HeldForManualApproval
            }
        };

        let ack = TaskSubmitAck { task_id };
        (verdict, ack)
    }

    /// Called periodically to check for delegable tasks that have timed out and should be auto-approved.
    pub fn check_timeouts(&mut self, now: Instant) -> Vec<(String, ExecutionPlan)> {
        let mut to_execute = Vec::new();

        for (task_id, task) in self.tasks.iter_mut() {
            if task.decision == PolicyDecision::Delegable && task.status == TaskStatus::Pending {
                let elapsed = now.saturating_duration_since(task.created_at);
                let verdict = apply_gatekeeper_policy(task.decision, elapsed);
                if verdict == GatekeeperVerdict::AutoApproved {
                    task.status = TaskStatus::DelegableAutoApproved;
                    to_execute.push((task_id.clone(), task.plan.clone()));
                }
            }
        }

        to_execute
    }

    /// Records the execution result for a task.
    pub fn record_execution(&mut self, task_id: &str, result: ExecutionOutcome) -> Result<(), LoopError> {
        let task = self.tasks.get_mut(task_id).ok_or_else(|| LoopError::TaskNotFound(task_id.to_string()))?;
        task.status = if result.success { TaskStatus::Executed } else { TaskStatus::ExecutionFailed };
        task.execution_result = Some(result);
        Ok(())
    }

    /// Handles an owner approval/denial for a held task.
    pub fn handle_owner_decision(&mut self, task_id: &str, approved: bool) -> Result<Option<ExecutionPlan>, LoopError> {
        let task = self.tasks.get_mut(task_id).ok_or_else(|| LoopError::TaskNotFound(task_id.to_string()))?;

        if task.status != TaskStatus::HeldForManualApproval {
            return Ok(None); // Not a held task, nothing to do
        }

        if approved {
            task.status = TaskStatus::Pending; // Will be picked up by check_timeouts or executed immediately
            Ok(Some(task.plan.clone()))
        } else {
            task.status = TaskStatus::RejectedByOwner;
            Ok(None)
        }
    }

    /// Returns the current status and result for a task.
    pub fn query(&self, task_id: &str) -> Result<TaskQueryResult, LoopError> {
        let task = self.tasks.get(task_id).ok_or_else(|| LoopError::TaskNotFound(task_id.to_string()))?;

        Ok(TaskQueryResult {
            task_id: task.task_id.clone(),
            status: task.status.clone(),
            result: task.execution_result.clone(),
        })
    }

    /// The set of `task_id`s currently held for manual owner approval.
    pub fn critical_held(&self) -> Vec<String> {
        self.tasks
            .iter()
            .filter(|(_, t)| t.status == TaskStatus::HeldForManualApproval)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Returns all tasks that are ready for execution (delegable and auto-approved, or just approved by owner).
    pub fn ready_for_execution(&self) -> Vec<(String, ExecutionPlan)> {
        self.tasks
            .iter()
            .filter(|(_, t)| {
                t.status == TaskStatus::DelegableAutoApproved || t.status == TaskStatus::Pending
            })
            .map(|(id, t)| (id.clone(), t.plan.clone()))
            .collect()
    }
}

/// Convenience accessor to surface the configured delegation timeout.
pub fn delegation_timeout() -> Duration {
    gatekeeper_timeout()
}

/// Process a maestro message (ExecutionPlan or TaskQuery) and return the appropriate response.
pub fn process_maestro_message(
    pending: &mut PendingPlans,
    now: Instant,
    msg: MaestroMessage,
) -> Result<Option<Vec<u8>>, LoopError> {
    match msg {
        MaestroMessage::ExecutionPlan(plan) => {
            let (verdict, ack) = pending.ingest(&plan, now);
            println!(
                "[GATEKEEPER] plan {} ({:?}) -> {:?}",
                plan.task_id,
                verdict,
                verdict
            );
            // Return TaskSubmitAck immediately
            Ok(Some(serde_json::to_vec(&ack).map_err(|e| LoopError::ExecutionError(e.to_string()))?))
        }
        MaestroMessage::TaskQuery(query) => {
            let result = pending.query(&query.task_id)?;
            Ok(Some(serde_json::to_vec(&result).map_err(|e| LoopError::ExecutionError(e.to_string()))?))
        }
    }
}

/// Process a TaskResolved message from the UI.
pub fn process_task_resolved(
    pending: &mut PendingPlans,
    resolved: TaskResolved,
) -> Result<Option<ExecutionPlan>, LoopError> {
    pending.handle_owner_decision(&resolved.task_id, resolved.approved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delegable_plan(task_id: &str) -> ExecutionPlan {
        ExecutionPlan {
            task_id: task_id.to_string(),
            description: "run the test suite".to_string(),
            commands: vec!["[[AB:OPS.TERM.RUN.LOCAL]] cargo test".to_string()],
        }
    }

    fn critical_plan(task_id: &str) -> ExecutionPlan {
        ExecutionPlan {
            task_id: task_id.to_string(),
            description: "rotate credentials".to_string(),
            commands: vec!["[[AB:OPS.TERM.RUN.LOCAL]] echo password=secret".to_string()],
        }
    }

    fn wipe_plan(task_id: &str) -> ExecutionPlan {
        ExecutionPlan {
            task_id: task_id.to_string(),
            description: "wipe the disk".to_string(),
            commands: vec!["[[AB:OPS.TERM.RUN.LOCAL]] dd if=/dev/zero of=/dev/sda bs=1M status=progress".to_string()],
        }
    }

    #[test]
    fn branch_routing_returns_call_index_without_dropping_results() {
        let mut registry = BranchRegistry::new("task-7");
        let first = SubChatOpenRequest {
            task_id: "task-7".to_string(),
            prompt: "first".to_string(),
            branch: "A".to_string(),
        };
        let second = SubChatOpenRequest {
            task_id: "task-7".to_string(),
            prompt: "second".to_string(),
            branch: "A".to_string(),
        };

        let first_result =
            route_branch_request(&mut registry, ChannelName::PrivateA, &first).unwrap();
        let second_result =
            route_branch_request(&mut registry, ChannelName::PrivateA, &second).unwrap();

        assert_eq!(first_result.call_index, 0);
        assert_eq!(second_result.call_index, 1);
    }

    #[test]
    fn branch_routing_rejects_maestro_channel() {
        let mut registry = BranchRegistry::new("task-7");
        let request = SubChatOpenRequest {
            task_id: "task-7".to_string(),
            prompt: "x".to_string(),
            branch: "A".to_string(),
        };
        assert!(matches!(
            route_branch_request(&mut registry, ChannelName::PublicMaestro, &request),
            Err(LoopError::NoBranch(ChannelName::PublicMaestro))
        ));
    }

    #[test]
    fn delegable_plan_auto_approves_after_timeout() {
        let mut pending = PendingPlans::new();
        let plan = delegable_plan("task-1");
        let start = Instant::now();

        let (verdict, _ack) = pending.ingest(&plan, start);
        assert_eq!(verdict, GatekeeperVerdict::PendingTimeout);

        let later = start + delegation_timeout();
        let (verdict, _ack) = pending.ingest(&plan, later);
        assert_eq!(verdict, GatekeeperVerdict::AutoApproved);
    }

    #[test]
    fn critical_plan_is_held_forever_for_manual_approval() {
        let mut pending = PendingPlans::new();
        let plan = critical_plan("task-2");
        let now = Instant::now();

        let (verdict, _ack) = pending.ingest(&plan, now);
        assert_eq!(verdict, GatekeeperVerdict::HeldForManualApproval);
        assert_eq!(pending.critical_held(), &["task-2".to_string()]);

        // Even far past the delegation timeout, a critical plan stays held.
        let later = now + delegation_timeout() + Duration::from_secs(3600);
        let (verdict, _ack) = pending.ingest(&plan, later);
        assert_eq!(verdict, GatekeeperVerdict::HeldForManualApproval);
    }

    #[test]
    fn manual_approval_releases_held_critical_task() {
        let mut pending = PendingPlans::new();
        let plan = critical_plan("task-3");
        let (_, _ack) = pending.ingest(&plan, Instant::now());

        assert!(pending.handle_owner_decision("task-3", true).unwrap().is_some());
        assert!(pending.critical_held().is_empty());
        assert!(pending.handle_owner_decision("task-3", true).unwrap().is_none());
    }

    #[test]
    fn wipe_plan_is_non_delegable_and_held_for_manual_approval() {
        let mut pending = PendingPlans::new();
        let plan = wipe_plan("task-4");
        let (verdict, _ack) = pending.ingest(&plan, Instant::now());
        assert_eq!(verdict, GatekeeperVerdict::HeldForManualApproval);
    }

    #[test]
    fn default_gatekeeper_timeout_is_120_seconds() {
        assert_eq!(delegation_timeout(), Duration::from_secs(120));
    }

    #[test]
    fn task_submit_ack_returned_on_ingest() {
        let mut pending = PendingPlans::new();
        let plan = delegable_plan("task-ack-1");
        let (_, ack) = pending.ingest(&plan, Instant::now());
        assert_eq!(ack.task_id, "task-ack-1");
    }

    #[test]
    fn task_query_returns_pending_for_new_delegable_task() {
        let mut pending = PendingPlans::new();
        let plan = delegable_plan("task-query-1");
        pending.ingest(&plan, Instant::now());

        let result = pending.query("task-query-1").unwrap();
        assert_eq!(result.task_id, "task-query-1");
        assert_eq!(result.status, TaskStatus::Pending);
        assert!(result.result.is_none());
    }

    #[test]
    fn task_query_returns_held_for_critical_task() {
        let mut pending = PendingPlans::new();
        let plan = critical_plan("task-query-2");
        pending.ingest(&plan, Instant::now());

        let result = pending.query("task-query-2").unwrap();
        assert_eq!(result.status, TaskStatus::HeldForManualApproval);
    }

    #[test]
    fn owner_approval_triggers_execution() {
        let mut pending = PendingPlans::new();
        let plan = critical_plan("task-owner-1");
        pending.ingest(&plan, Instant::now());

        let plan_to_exec = pending.handle_owner_decision("task-owner-1", true).unwrap();
        assert!(plan_to_exec.is_some());
        assert_eq!(plan_to_exec.unwrap().task_id, "task-owner-1");

        // Query should now show pending (ready for execution)
        let result = pending.query("task-owner-1").unwrap();
        assert_eq!(result.status, TaskStatus::Pending);
    }

    #[test]
    fn owner_rejection_marks_task_rejected() {
        let mut pending = PendingPlans::new();
        let plan = critical_plan("task-owner-2");
        pending.ingest(&plan, Instant::now());

        let plan_to_exec = pending.handle_owner_decision("task-owner-2", false).unwrap();
        assert!(plan_to_exec.is_none());

        let result = pending.query("task-owner-2").unwrap();
        assert_eq!(result.status, TaskStatus::RejectedByOwner);
    }

    #[test]
    fn record_execution_updates_status() {
        let mut pending = PendingPlans::new();
        let plan = delegable_plan("task-exec-1");
        pending.ingest(&plan, Instant::now());

        let outcome = ExecutionOutcome {
            stdout: "hello".to_string(),
            stderr: "".to_string(),
            exit_code: 0,
            success: true,
        };
        pending.record_execution("task-exec-1", outcome.clone()).unwrap();

        let result = pending.query("task-exec-1").unwrap();
        assert_eq!(result.status, TaskStatus::Executed);
        assert_eq!(result.result, Some(outcome));
    }

    #[test]
    fn process_maestro_message_execution_plan_returns_ack() {
        let mut pending = PendingPlans::new();
        let plan = delegable_plan("task-proc-1");
        let msg = MaestroMessage::ExecutionPlan(plan);

        let response = process_maestro_message(&mut pending, Instant::now(), msg).unwrap();
        assert!(response.is_some());

        let ack: TaskSubmitAck = serde_json::from_slice(&response.unwrap()).unwrap();
        assert_eq!(ack.task_id, "task-proc-1");
    }

    #[test]
    fn process_maestro_message_task_query_returns_result() {
        let mut pending = PendingPlans::new();
        let plan = critical_plan("task-proc-2");
        pending.ingest(&plan, Instant::now());

        let msg = MaestroMessage::TaskQuery(TaskQuery { task_id: "task-proc-2".to_string() });
        let response = process_maestro_message(&mut pending, Instant::now(), msg).unwrap();
        assert!(response.is_some());

        let result: TaskQueryResult = serde_json::from_slice(&response.unwrap()).unwrap();
        assert_eq!(result.task_id, "task-proc-2");
        assert_eq!(result.status, TaskStatus::HeldForManualApproval);
    }
}