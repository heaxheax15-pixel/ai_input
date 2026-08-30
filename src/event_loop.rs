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
use ai_bridge_protocol::{ExecutionPlan, SubChatOpenRequest, SubChatResult};
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

/// Tracks in-flight plans so the synchronous Gatekeeper timeout and the manual
/// approval queue are applied across repeated event-loop ticks.
#[derive(Debug, Default)]
pub struct PendingPlans {
    delegable_deadlines: HashMap<String, Instant>,
    critical_held: Vec<String>,
}

impl PendingPlans {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingests a maestro plan, records the start of its delegation window, and
    /// returns the resulting Gatekeeper verdict.
    pub fn ingest(&mut self, plan: &ExecutionPlan, now: Instant) -> GatekeeperVerdict {
        let decision = decide_policy(plan);

        match decision {
            PolicyDecision::Delegable => {
                let started = *self
                    .delegable_deadlines
                    .entry(plan.task_id.clone())
                    .or_insert(now);

                let verdict =
                    apply_gatekeeper_policy(decision, now.saturating_duration_since(started));

                if verdict == GatekeeperVerdict::AutoApproved {
                    self.delegable_deadlines.remove(&plan.task_id);
                }

                verdict
            }
            PolicyDecision::NonDelegable => {
                if !self.critical_held.contains(&plan.task_id) {
                    self.critical_held.push(plan.task_id.clone());
                }
                GatekeeperVerdict::HeldForManualApproval
            }
        }
    }

    /// The set of `task_id`s currently held for manual owner approval.
    pub fn critical_held(&self) -> &[String] {
        &self.critical_held
    }

    /// Processes a manual owner approval for a held critical task, removing it
    /// from the queue. Returns `true` when the task was approved and released,
    /// and `false` if the task was not held.
    pub fn acknowledge_manual_approval(&mut self, task_id: &str) -> bool {
        if let Some(index) = self.critical_held.iter().position(|id| id == task_id) {
            self.critical_held.remove(index);
            true
        } else {
            false
        }
    }
}

/// Convenience accessor to surface the configured delegation timeout.
pub fn delegation_timeout() -> Duration {
    gatekeeper_timeout()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delegable_plan(task_id: &str) -> ExecutionPlan {
        ExecutionPlan {
            task_id: task_id.to_string(),
            description: "run the test suite".to_string(),
            commands: vec!["cargo test".to_string()],
        }
    }

    fn critical_plan(task_id: &str) -> ExecutionPlan {
        ExecutionPlan {
            task_id: task_id.to_string(),
            description: "rotate credentials".to_string(),
            commands: vec!["echo password=secret".to_string()],
        }
    }

    fn wipe_plan(task_id: &str) -> ExecutionPlan {
        ExecutionPlan {
            task_id: task_id.to_string(),
            description: "wipe the disk".to_string(),
            commands: vec!["dd if=/dev/zero of=/dev/sda bs=1M status=progress".to_string()],
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

        assert_eq!(
            pending.ingest(&plan, start),
            GatekeeperVerdict::PendingTimeout
        );

        let later = start + delegation_timeout();
        assert_eq!(
            pending.ingest(&plan, later),
            GatekeeperVerdict::AutoApproved
        );
    }

    #[test]
    fn critical_plan_is_held_forever_for_manual_approval() {
        let mut pending = PendingPlans::new();
        let plan = critical_plan("task-2");
        let now = Instant::now();

        assert_eq!(
            pending.ingest(&plan, now),
            GatekeeperVerdict::HeldForManualApproval
        );
        assert_eq!(pending.critical_held(), &["task-2".to_string()]);

        // Even far past the delegation timeout, a critical plan stays held.
        let later = now + delegation_timeout() + Duration::from_secs(3600);
        assert_eq!(
            pending.ingest(&plan, later),
            GatekeeperVerdict::HeldForManualApproval
        );
    }

    #[test]
    fn manual_approval_releases_held_critical_task() {
        let mut pending = PendingPlans::new();
        let plan = critical_plan("task-3");
        pending.ingest(&plan, Instant::now());

        assert!(pending.acknowledge_manual_approval("task-3"));
        assert!(pending.critical_held().is_empty());
        assert!(!pending.acknowledge_manual_approval("task-3"));
    }

    #[test]
    fn wipe_plan_is_non_delegable_and_held_for_manual_approval() {
        // A system-wipe pattern is non-delegable: it awaits the owner's manual
        // approval (no automatic refusal, no exception).
        let mut pending = PendingPlans::new();
        let plan = wipe_plan("task-4");
        assert_eq!(
            pending.ingest(&plan, Instant::now()),
            GatekeeperVerdict::HeldForManualApproval
        );
    }

    #[test]
    fn default_gatekeeper_timeout_is_120_seconds() {
        assert_eq!(delegation_timeout(), Duration::from_secs(120));
    }
}
