use ai_bridge_protocol::{ExecutionPlan, Symbol};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyMode {
    Armed,
    Disarmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyState {
    pub armed: bool,
    pub last_toggle_at: Option<SystemTime>,
    pub last_toggled_by: Option<String>,
    pub last_override_reason: Option<String>,
    pub last_override_user: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyDecision {
    Allow,
    Block,
    RequireHumanOverride,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideRequest {
    pub task_id: String,
    pub symbol: String,
    pub reason: String,
    pub requested_by: String,
    pub timestamp: SystemTime,
    pub command_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    pub timestamp: SystemTime,
    pub task_id: String,
    pub command_preview: String,
    pub user: String,
    pub action: String,
    pub result: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyGuard {
    pub state: SafetyState,
    pub audit_log: Vec<AuditRecord>,
}

impl Default for SafetyGuard {
    fn default() -> Self {
        Self {
            state: SafetyState {
                armed: true,
                last_toggle_at: None,
                last_toggled_by: None,
                last_override_reason: None,
                last_override_user: None,
            },
            audit_log: Vec::new(),
        }
    }
}

impl SafetyGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn arm(&mut self, user: impl Into<String>) -> bool {
        self.state.armed = true;
        self.state.last_toggle_at = Some(SystemTime::now());
        self.state.last_toggled_by = Some(user.into());
        self.audit_log.push(AuditRecord {
            timestamp: SystemTime::now(),
            task_id: "system".to_string(),
            command_preview: "safety_lock:arm".to_string(),
            user: self.state.last_toggled_by.clone().unwrap_or_default(),
            action: "arm".to_string(),
            result: "granted".to_string(),
            reason: "user armed the safety lock".to_string(),
        });
        true
    }

    pub fn disarm(&mut self, user: impl Into<String>, reason: impl Into<String>) -> bool {
        self.state.armed = false;
        self.state.last_toggle_at = Some(SystemTime::now());
        self.state.last_toggled_by = Some(user.into());
        self.state.last_override_reason = Some(reason.into());
        self.audit_log.push(AuditRecord {
            timestamp: SystemTime::now(),
            task_id: "system".to_string(),
            command_preview: "safety_lock:disarm".to_string(),
            user: self.state.last_toggled_by.clone().unwrap_or_default(),
            action: "disarm".to_string(),
            result: "granted".to_string(),
            reason: self.state.last_override_reason.clone().unwrap_or_default(),
        });
        true
    }

    pub fn evaluate_for_command(&self, command: &str) -> SafetyDecision {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return SafetyDecision::Allow;
        }

        let (symbol, _) = Symbol::extract_from_command(trimmed);
        if symbol.classification() == ai_bridge_protocol::SymbolClassification::Critical {
            if self.state.armed {
                return SafetyDecision::Block;
            }
            return SafetyDecision::Allow;
        }

        SafetyDecision::Allow
    }

    pub fn request_override(&mut self, request: OverrideRequest) -> SafetyDecision {
        self.state.last_override_reason = Some(request.reason.clone());
        self.state.last_override_user = Some(request.requested_by.clone());
        self.audit_log.push(AuditRecord {
            timestamp: request.timestamp,
            task_id: request.task_id.clone(),
            command_preview: request.command_preview.clone(),
            user: request.requested_by.clone(),
            action: "override_requested".to_string(),
            result: "pending".to_string(),
            reason: request.reason.clone(),
        });
        SafetyDecision::RequireHumanOverride
    }

    pub fn append_audit(&mut self, record: AuditRecord) {
        self.audit_log.push(record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_guard_is_armed() {
        let guard = SafetyGuard::default();
        assert!(guard.state.armed);
    }

    #[test]
    fn safe_command_is_allowed_when_armed() {
        let guard = SafetyGuard::default();
        assert_eq!(guard.evaluate_for_command("[[AB:OPS.TERM.RUN.LOCAL]] echo hello"), SafetyDecision::Allow);
    }

    #[test]
    fn critical_command_is_blocked_when_armed() {
        let guard = SafetyGuard::default();
        assert_eq!(
            guard.evaluate_for_command("[[AB:CRIT.SECRET]] echo token"),
            SafetyDecision::Block
        );
    }

    #[test]
    fn override_requires_reason() {
        let mut guard = SafetyGuard::default();
        let req = OverrideRequest {
            task_id: "task-42".to_string(),
            symbol: "CRIT.SECRET".to_string(),
            reason: "Emergency recovery".to_string(),
            requested_by: "alice".to_string(),
            timestamp: SystemTime::now(),
            command_preview: "[[AB:CRIT.SECRET]] echo token".to_string(),
        };

        let decision = guard.request_override(req);
        assert_eq!(decision, SafetyDecision::RequireHumanOverride);
        assert!(guard.state.last_override_reason.is_some());
    }
}
