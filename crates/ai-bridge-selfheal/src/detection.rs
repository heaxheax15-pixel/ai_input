use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureMode {
    ExplicitError,
    SilentTimeout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureDetection {
    pub mode: FailureMode,
    pub message: String,
    pub elapsed: Duration,
}

pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);

pub fn detect_failure(message: &str, elapsed: Duration) -> FailureDetection {
    if message.trim().is_empty() && elapsed >= HEARTBEAT_TIMEOUT {
        FailureDetection {
            mode: FailureMode::SilentTimeout,
            message: "Maestro heartbeat timed out".to_string(),
            elapsed,
        }
    } else {
        FailureDetection {
            mode: FailureMode::ExplicitError,
            message: message.to_string(),
            elapsed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_explicit_error_when_message_is_present() {
        let detection = detect_failure("connection refused", Duration::from_secs(5));
        assert_eq!(detection.mode, FailureMode::ExplicitError);
    }

    #[test]
    fn detects_silent_timeout_when_no_message_and_elapsed_exceeds_threshold() {
        let detection = detect_failure("", Duration::from_secs(31));
        assert_eq!(detection.mode, FailureMode::SilentTimeout);
    }
}
