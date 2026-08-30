use serde::Deserialize;
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::oneshot;

use ai_bridge_gatekeeper_core::criteria::Criterion;

/// A human decision returned by the UI for a task that was blocked and awaiting
/// approval. Arrives over the UI socket as a JSON line shaped
/// `{ "task_id": String, "approved": bool }` and is used to unblock (approved)
/// or abort (denied) the agent's command execution.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TaskResolved {
    pub task_id: String,
    pub approved: bool,
}

/// A task that an agent has submitted and which is now blocked, awaiting a human
/// decision. The display metadata is advertised to the UI as a
/// `{ "type": "task_pending", ... }` JSON line so the operator can review the
/// exact command before opening (approve) or closing (reject) the valve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTask {
    pub task_id: String,
    pub app_name: String,
    pub command: String,
    pub risk_level: String,
    /// Whether this task is critical (never delegable). Critical tasks wait for
    /// the human indefinitely; non-critical tasks auto-approve after the
    /// delegation timeout elapses.
    pub is_critical: bool,
    /// The criteria that triggered the classification, advertised to the UI so
    /// the operator can see why the task was held for approval.
    pub triggered: Vec<Criterion>,
}

/// Per-task registry entry: the display metadata plus the oneshot sender used to
/// unblock the agent's waiting command once the UI resolves the task.
#[derive(Debug)]
struct TaskEntry {
    info: PendingTask,
    sender: oneshot::Sender<bool>,
}

/// The internal registry of tasks that are currently blocked and awaiting a
/// human decision. Each pending task is keyed by `task_id` and carries a
/// oneshot response channel over which the approval boolean is sent back to the
/// waiting agent command.
#[derive(Debug, Default)]
pub struct ActiveTasks {
    pending: HashMap<String, TaskEntry>,
}

impl ActiveTasks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a task that is now blocked and awaiting a human decision. The
    /// provided sender is how the agent command will be unblocked or aborted
    /// once the UI resolves the task.
    pub fn register(&mut self, task: PendingTask, tx: oneshot::Sender<bool>) {
        let task_id = task.task_id.clone();
        self.pending.insert(
            task_id,
            TaskEntry {
                info: task,
                sender: tx,
            },
        );
    }

    /// Returns `true` if the given task is still awaiting a decision.
    pub fn contains(&self, task_id: &str) -> bool {
        self.pending.contains_key(task_id)
    }

    /// Iterates over every task that is still awaiting a decision, in
    /// unspecified order. Used by the server to advertise pending tasks to a UI
    /// session as it connects.
    pub fn pending_tasks(&self) -> impl Iterator<Item = &PendingTask> {
        self.pending.values().map(|entry| &entry.info)
    }

    /// Resolves a pending task, sending `approved` back through its oneshot
    /// response channel. Returns `None` if no task matches `task_id`. The task
    /// is removed from the registry whether or not the send succeeds (the
    /// receiver may have already been dropped).
    pub fn resolve(&mut self, task_id: &str, approved: bool) -> Option<()> {
        let entry = self.pending.remove(task_id)?;
        let _ = entry.sender.send(approved);
        Some(())
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// Serializes a pending task into the `task_pending` JSON line the UI expects.
pub fn pending_notification(task: &PendingTask) -> String {
    serde_json::json!({
        "type": "task_pending",
        "task_id": task.task_id,
        "app_name": task.app_name,
        "command": task.command,
        "risk_level": task.risk_level,
        "is_critical": task.is_critical,
        "triggered": task
            .triggered
            .iter()
            .map(|c| c.label())
            .collect::<Vec<_>>(),
    })
    .to_string()
}

/// Handles a single inbound line read from the UI socket. If the line
/// deserializes into a `TaskResolved`, the matching pending task is resolved
/// with the supplied `approved` boolean. Returns the `task_id` that was
/// resolved, or `None` if the line did not describe a resolvable task.
pub fn handle_inbound(
    line: &str,
    tasks: &mut ActiveTasks,
) -> Result<Option<String>, serde_json::Error> {
    let resolved: TaskResolved = serde_json::from_str(line)?;
    let task_id = resolved.task_id.clone();
    tasks.resolve(&task_id, resolved.approved);
    Ok(Some(task_id))
}

/// Services one UI connection over the given Unix stream: it advertises every
/// task currently awaiting a decision, then reads decision lines until the UI
/// disconnects (EOF). Returns the `task_id`s that were resolved during the
/// session. Malformed inbound lines are reported on stderr and ignored so a
/// single bad line cannot take the daemon down.
pub async fn serve_ui_session(
    stream: UnixStream,
    active: &mut ActiveTasks,
) -> std::io::Result<Vec<String>> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    for task in active.pending_tasks() {
        let line = pending_notification(task);
        writer.write_all(format!("{line}\n").as_bytes()).await?;
    }

    let mut resolved = Vec::new();
    while let Some(line) = lines.next_line().await? {
        match handle_inbound(&line, active) {
            Ok(Some(task_id)) => resolved.push(task_id),
            Ok(None) => {}
            Err(err) => eprintln!("[Gatekeeper] Failed to parse inbound line: {err}"),
        }
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pending(task_id: &str) -> PendingTask {
        PendingTask {
            task_id: task_id.to_string(),
            app_name: "org.gnome.Terminal".to_string(),
            command: "rm -rf /tmp/test_dir".to_string(),
            risk_level: "HIGH".to_string(),
            is_critical: false,
            triggered: Vec::new(),
        }
    }

    #[test]
    fn resolves_pending_task_with_approved_true() {
        let mut tasks = ActiveTasks::new();
        let (tx, mut rx) = oneshot::channel();
        tasks.register(pending("TASK-1"), tx);
        assert!(tasks.contains("TASK-1"));

        let line = json!({ "task_id": "TASK-1", "approved": true }).to_string();
        let resolved = handle_inbound(&line, &mut tasks).expect("should parse");

        assert_eq!(resolved.as_deref(), Some("TASK-1"));
        assert!(!tasks.contains("TASK-1"));
        assert_eq!(rx.try_recv(), Ok(true));
    }

    #[test]
    fn resolves_pending_task_with_approved_false() {
        let mut tasks = ActiveTasks::new();
        let (tx, mut rx) = oneshot::channel();
        tasks.register(pending("TASK-2"), tx);

        let line = json!({ "task_id": "TASK-2", "approved": false }).to_string();
        handle_inbound(&line, &mut tasks).expect("should parse");

        assert_eq!(rx.try_recv(), Ok(false));
    }

    #[test]
    fn unknown_task_id_is_ignored() {
        let mut tasks = ActiveTasks::new();
        let (tx, _rx) = oneshot::channel();
        tasks.register(pending("TASK-KNOWN"), tx);
        let line = json!({ "task_id": "TASK-MISSING", "approved": true }).to_string();
        let _ = handle_inbound(&line, &mut tasks);
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn malformed_line_returns_error() {
        let mut tasks = ActiveTasks::new();
        assert!(handle_inbound("not-json", &mut tasks).is_err());
    }

    #[test]
    fn missing_fields_fail_to_deserialize() {
        let mut tasks = ActiveTasks::new();
        let line = json!({ "task_id": "TASK-3" }).to_string();
        assert!(handle_inbound(&line, &mut tasks).is_err());
    }

    #[test]
    fn pending_notification_serializes_task_pending_line() {
        let line = pending_notification(&pending("TASK-9"));
        let value: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
        assert_eq!(value["type"], "task_pending");
        assert_eq!(value["task_id"], "TASK-9");
        assert_eq!(value["risk_level"], "HIGH");
        assert_eq!(value["is_critical"], false);
        assert_eq!(value["triggered"], serde_json::json!([]));
    }

    #[test]
    fn pending_notification_includes_critical_flag_and_triggered_reasons() {
        let mut task = pending("TASK-10");
        task.is_critical = true;
        task.triggered = vec![Criterion::Irreversibility, Criterion::CredentialsSecrets];
        let line = pending_notification(&task);
        let value: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
        assert_eq!(value["is_critical"], true);
        assert_eq!(
            value["triggered"],
            serde_json::json!(["Irreversibility", "CredentialsSecrets"])
        );
    }

    #[test]
    fn defaults_to_empty_registry() {
        let tasks = ActiveTasks::new();
        assert!(tasks.is_empty());
        assert_eq!(tasks.len(), 0);
    }
}
