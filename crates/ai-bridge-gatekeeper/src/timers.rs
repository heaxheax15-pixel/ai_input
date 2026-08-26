use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerOutcome {
    AwaitingHuman,
    AutoApprove,
    PermanentSuspension,
}

pub const GATEKEEPER_TIMEOUT: Duration = Duration::from_secs(120);

pub fn evaluate_timeout(is_delegable: bool, elapsed: Duration) -> TimerOutcome {
    if elapsed >= GATEKEEPER_TIMEOUT {
        if is_delegable {
            TimerOutcome::AutoApprove
        } else {
            TimerOutcome::PermanentSuspension
        }
    } else {
        TimerOutcome::AwaitingHuman
    }
}

pub fn with_timeout<F>(is_delegable: bool, deadline: Instant, now: Instant, _action: F) -> TimerOutcome
where
    F: FnOnce(),
{
    evaluate_timeout(is_delegable, now.saturating_duration_since(deadline))
}
