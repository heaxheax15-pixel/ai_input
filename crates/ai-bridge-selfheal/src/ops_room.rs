use std::time::Duration;

use crate::detection::{detect_failure, FailureMode, HEARTBEAT_TIMEOUT};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchRole {
    Maestro,
    BranchA,
    BranchB,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleSwapDirective {
    pub active_maestro: BranchRole,
    pub continued_branch: BranchRole,
    pub directive: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfHealState {
    pub mode: FailureMode,
    pub directive: String,
    pub active_maestro: BranchRole,
    pub continued_branch: BranchRole,
}

pub fn self_heal_mode(message: &str, elapsed: Duration) -> SelfHealState {
    let detection = detect_failure(message, elapsed);

    match detection.mode {
        FailureMode::ExplicitError => SelfHealState {
            mode: FailureMode::ExplicitError,
            directive: "reconfigure connection layer; isolate faulted maestro and route task to healthy branch".to_string(),
            active_maestro: BranchRole::BranchA,
            continued_branch: BranchRole::BranchB,
        },
        FailureMode::SilentTimeout => SelfHealState {
            mode: FailureMode::SilentTimeout,
            directive: format!("heartbeat missed for {}s; swap maestro role and continue active branch until recovery", HEARTBEAT_TIMEOUT.as_secs()),
            active_maestro: BranchRole::BranchB,
            continued_branch: BranchRole::BranchA,
        },
    }
}

pub fn role_swap_on_failure(
    maestro_offline: bool,
    current_maestro: BranchRole,
) -> RoleSwapDirective {
    if maestro_offline {
        let active_maestro = match current_maestro {
            BranchRole::Maestro | BranchRole::BranchA => BranchRole::BranchA,
            BranchRole::BranchB => BranchRole::BranchB,
        };
        let continued_branch = match active_maestro {
            BranchRole::BranchA => BranchRole::BranchB,
            BranchRole::BranchB => BranchRole::BranchA,
            BranchRole::Maestro => BranchRole::BranchB,
        };

        RoleSwapDirective {
            active_maestro,
            continued_branch,
            directive: "Maestro offline; swap leadership to active branch and keep alternate branch running".to_string(),
        }
    } else {
        RoleSwapDirective {
            active_maestro: current_maestro,
            continued_branch: BranchRole::BranchB,
            directive: "No swap required".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_error_triggers_self_heal_mode() {
        let state = self_heal_mode("automation failure", Duration::from_secs(5));
        assert_eq!(state.mode, FailureMode::ExplicitError);
        assert_eq!(state.active_maestro, BranchRole::BranchA);
        assert_eq!(state.continued_branch, BranchRole::BranchB);
    }

    #[test]
    fn silent_timeout_triggers_self_heal_mode() {
        let state = self_heal_mode("", Duration::from_secs(31));
        assert_eq!(state.mode, FailureMode::SilentTimeout);
        assert_eq!(state.active_maestro, BranchRole::BranchB);
        assert_eq!(state.continued_branch, BranchRole::BranchA);
    }

    #[test]
    fn role_swap_logic_keeps_non_maestro_branch_running() {
        let directive = role_swap_on_failure(true, BranchRole::BranchA);
        assert_eq!(directive.active_maestro, BranchRole::BranchA);
        assert_eq!(directive.continued_branch, BranchRole::BranchB);
    }
}
