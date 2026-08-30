use std::time::Duration;
use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerOutcome {
    AwaitingHuman,
    AutoApprove,
}

pub const GATEKEEPER_TIMEOUT_ENV: &str = "AI_BRIDGE_GATEKEEPER_TIMEOUT_SECS";
pub const DEFAULT_GATEKEEPER_TIMEOUT: Duration = Duration::from_secs(120);

/// Resolves the effective gatekeeper delegation timeout as a runtime
/// configuration value, read from the `AI_BRIDGE_GATEKEEPER_TIMEOUT_SECS`
/// environment variable (in seconds). When the variable is unset or unparsable,
/// it falls back to [`DEFAULT_GATEKEEPER_TIMEOUT`] (120 s).
///
/// The timeout is intentionally a runtime setting, not a compile-time constant,
/// so operators can tune the delegation window without rebuilding.
pub fn gatekeeper_timeout() -> Duration {
    std::env::var(GATEKEEPER_TIMEOUT_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_GATEKEEPER_TIMEOUT)
}

pub fn evaluate_timeout(is_delegable: bool, elapsed: Duration) -> TimerOutcome {
    if elapsed >= gatekeeper_timeout() && is_delegable {
        // A delegable task is auto-approved after the delegation timeout.
        TimerOutcome::AutoApprove
    } else {
        // A non-delegable task is never auto-resolved: it waits for the human
        // (the owner) indefinitely, even past the delegation timeout.
        TimerOutcome::AwaitingHuman
    }
}

/// Awaits the human's decision on a task's oneshot channel, using
/// [`evaluate_timeout`] as the single source of truth for what happens at
/// timeout silence.
///
/// * A delegable task races against the runtime gateway timeout: an explicit
///   reply wins immediately, a dropped channel denies the task, and silence past
///   the timeout auto-approves it (per [`evaluate_timeout`]).
/// * A non-delegable task is never auto-resolved: it waits for the human
///   indefinitely, and a dropped channel denies it.
pub async fn await_decision(rx: oneshot::Receiver<bool>, is_delegable: bool) -> bool {
    if !is_delegable {
        match evaluate_timeout(false, gatekeeper_timeout()) {
            TimerOutcome::AwaitingHuman => rx.await.ok().unwrap_or(false),
            TimerOutcome::AutoApprove => unreachable!("non-delegable never auto-approves"),
        }
    } else {
        match tokio::time::timeout(gatekeeper_timeout(), rx).await {
            Ok(Ok(approved)) => approved,
            Ok(Err(_)) => false,
            Err(_) => {
                // The timeout elapsed (>= gatekeeper_timeout) and the task is
                // delegable: silence auto-approves per the single decision
                // source `evaluate_timeout`.
                match evaluate_timeout(true, gatekeeper_timeout()) {
                    TimerOutcome::AutoApprove => true,
                    TimerOutcome::AwaitingHuman => false,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gatekeeper_timeout_defaults_to_120_seconds() {
        if std::env::var(GATEKEEPER_TIMEOUT_ENV).is_err() {
            assert_eq!(gatekeeper_timeout(), Duration::from_secs(120));
        }
    }

    #[tokio::test]
    async fn non_delegable_task_never_auto_approves_with_dropped_channel() {
        let (tx, rx) = oneshot::channel();
        drop(tx);

        // A non-delegable task with a dropped channel never auto-approves; it
        // is denied because the determining side is gone.
        assert!(!await_decision(rx, false).await, "non-delegable must not auto-delegate");
    }

    #[tokio::test]
    async fn non_delegable_task_waits_for_human_approval() {
        let (tx, rx) = oneshot::channel();
        let decision = tokio::spawn(async move { await_decision(rx, false).await });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        tx.send(true).unwrap();
        assert!(decision.await.unwrap(), "approval must reach the held task");
    }

    #[tokio::test]
    async fn delegable_task_denied_when_channel_dropped() {
        let (tx, rx) = oneshot::channel();
        drop(tx);

        // A dropped channel resolves immediately (before any timeout), so the
        // delegable task is denied rather than silently auto-approved.
        assert!(!await_decision(rx, true).await);
    }

    #[tokio::test]
    async fn delegable_task_respects_explicit_approval() {
        let (tx, rx) = oneshot::channel();
        tx.send(true).unwrap();
        assert!(await_decision(rx, true).await);
    }

    #[tokio::test]
    async fn delegable_task_respects_explicit_rejection() {
        let (tx, rx) = oneshot::channel();
        tx.send(false).unwrap();
        assert!(!await_decision(rx, true).await);
    }
}
