use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolClassification {
    Delegable,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Symbol {
    // قابلة للتفويض
    OpsAppOpen,
    OpsAppBrowse,
    OpsFileCreate,
    OpsFileEdit,
    OpsFileDeleteRecoverable,
    OpsTermRunLocal,
    OpsInputHid,
    // حرجة
    CritIrreversible,
    CritSysSecurity,
    CritSecret,
    CritEgress,
    CritFinancial,
    CritThirdParty,
    CritUnclassified,
}

impl Symbol {
    pub fn literal_name(&self) -> &'static str {
        match self {
            Self::OpsAppOpen => "OPS.APP.OPEN",
            Self::OpsAppBrowse => "OPS.APP.BROWSE",
            Self::OpsFileCreate => "OPS.FILE.CREATE",
            Self::OpsFileEdit => "OPS.FILE.EDIT",
            Self::OpsFileDeleteRecoverable => "OPS.FILE.DELETE.RECOVERABLE",
            Self::OpsTermRunLocal => "OPS.TERM.RUN.LOCAL",
            Self::OpsInputHid => "OPS.INPUT.HID",
            Self::CritIrreversible => "CRIT.IRREVERSIBLE",
            Self::CritSysSecurity => "CRIT.SYS.SECURITY",
            Self::CritSecret => "CRIT.SECRET",
            Self::CritEgress => "CRIT.EGRESS",
            Self::CritFinancial => "CRIT.FINANCIAL",
            Self::CritThirdParty => "CRIT.THIRD.PARTY",
            Self::CritUnclassified => "CRIT.UNCLASSIFIED",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::OpsAppOpen => "Open an application window or first launcher action.",
            Self::OpsAppBrowse => "Browse local or remote application content without destructive effects.",
            Self::OpsFileCreate => "Create a new file or directory in a scoped workspace.",
            Self::OpsFileEdit => "Modify a tracked file or text document in-place.",
            Self::OpsFileDeleteRecoverable => "Delete a recoverable file or temporary artifact in a bounded scope.",
            Self::OpsTermRunLocal => "Run a local terminal command in a constrained user session.",
            Self::OpsInputHid => "Inject a low-level keyboard or pointer event into a desktop session.",
            Self::CritIrreversible => "Delete, wipe, or overwrite data in a way that cannot be easily reversed.",
            Self::CritSysSecurity => "Affect system security, permissions, users, or service control settings.",
            Self::CritSecret => "Handle credentials, secrets, tokens, or private keys.",
            Self::CritEgress => "Move sensitive data outside the local boundary or to an external sink.",
            Self::CritFinancial => "Move money, make payment decisions, or trigger financial-side effects.",
            Self::CritThirdParty => "Trigger external social, messaging, or third-party publication behavior.",
            Self::CritUnclassified => "No recognized symbol matched; this command is treated as critical by default.",
        }
    }

    pub fn classification(&self) -> SymbolClassification {
        match self {
            Self::OpsAppOpen
            | Self::OpsAppBrowse
            | Self::OpsFileCreate
            | Self::OpsFileEdit
            | Self::OpsFileDeleteRecoverable
            | Self::OpsTermRunLocal
            | Self::OpsInputHid => SymbolClassification::Delegable,
            Self::CritIrreversible
            | Self::CritSysSecurity
            | Self::CritSecret
            | Self::CritEgress
            | Self::CritFinancial
            | Self::CritThirdParty
            | Self::CritUnclassified => SymbolClassification::Critical,
        }
    }

    pub fn all() -> &'static [Symbol] {
        &[
            Self::OpsAppOpen,
            Self::OpsAppBrowse,
            Self::OpsFileCreate,
            Self::OpsFileEdit,
            Self::OpsFileDeleteRecoverable,
            Self::OpsTermRunLocal,
            Self::OpsInputHid,
            Self::CritIrreversible,
            Self::CritSysSecurity,
            Self::CritSecret,
            Self::CritEgress,
            Self::CritFinancial,
            Self::CritThirdParty,
            Self::CritUnclassified,
        ]
    }

    pub fn from_literal(name: &str) -> Option<Self> {
        Self::all().iter().copied().find(|symbol| symbol.literal_name() == name)
    }

    pub fn with_tag(&self, command: &str) -> String {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            format!("[[AB:{}]]", self.literal_name())
        } else {
            format!("[[AB:{}]] {}", self.literal_name(), trimmed)
        }
    }

    pub fn extract_from_command(command: &str) -> (Self, String) {
        let trimmed = command.trim();
        let Some(stripped) = trimmed.strip_prefix("[[AB:") else {
            return (Self::CritUnclassified, command.to_string());
        };

        let Some(end) = stripped.find("]]") else {
            return (Self::CritUnclassified, command.to_string());
        };

        let symbol_name = stripped[..end].trim();
        let remaining = stripped[end + 2..].trim().to_string();

        match Self::from_literal(symbol_name) {
            Some(symbol) => (symbol, remaining),
            None => (Self::CritUnclassified, command.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubChatOpenRequest {
    pub task_id: String,
    pub prompt: String,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextQuery {
    pub task_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextResponse {
    pub task_id: String,
    pub include_master_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubChatResult {
    pub task_id: String,
    pub result: String,
    pub status: String,
    pub call_index: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub task_id: String,
    pub description: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MaestroMessage {
    ExecutionPlan(ExecutionPlan),
    TaskQuery(TaskQuery),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskQuery {
    pub task_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSubmitAck {
    pub task_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionOutcome {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    DelegableAutoApproved,
    HeldForManualApproval,
    Executed,
    RejectedByOwner,
    ExecutionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskQueryResult {
    pub task_id: String,
    pub status: TaskStatus,
    pub result: Option<ExecutionOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskResolved {
    pub task_id: String,
    pub approved: bool,
}

pub fn serialize<T>(value: &T) -> Result<String, serde_json::Error>
where
    T: Serialize,
{
    serde_json::to_string(value)
}

pub fn deserialize<T>(raw: &str) -> Result<T, serde_json::Error>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_and_deserializes_subchat_open_request() {
        let original = SubChatOpenRequest {
            task_id: "task-42".to_string(),
            prompt: "Summarize the architecture".to_string(),
            branch: "A".to_string(),
        };

        let raw = serialize(&original).unwrap();
        let decoded: SubChatOpenRequest = deserialize(&raw).unwrap();

        assert_eq!(decoded, original);
        assert!(raw.contains("\"task_id\":\"task-42\""));
        assert!(!raw.contains("sender"));
        assert!(!raw.contains("auth_token"));
    }

    #[test]
    fn serializes_and_deserializes_context_query() {
        let original = ContextQuery {
            task_id: "task-42".to_string(),
        };

        let raw = serialize(&original).unwrap();
        let decoded: ContextQuery = deserialize(&raw).unwrap();

        assert_eq!(decoded, original);
        assert!(raw.contains("\"task_id\":\"task-42\""));
        assert!(!raw.contains("sender"));
        assert!(!raw.contains("auth_token"));
    }

    #[test]
    fn serializes_and_deserializes_context_response() {
        let original = ContextResponse {
            task_id: "task-42".to_string(),
            include_master_context: true,
        };

        let raw = serialize(&original).unwrap();
        let decoded: ContextResponse = deserialize(&raw).unwrap();

        assert_eq!(decoded, original);
        assert!(raw.contains("\"include_master_context\":true"));
        assert!(!raw.contains("sender"));
        assert!(!raw.contains("auth_token"));
    }

    #[test]
    fn serializes_and_deserializes_subchat_result() {
        let original = SubChatResult {
            task_id: "task-42".to_string(),
            result: "Completed successfully".to_string(),
            status: "success".to_string(),
            call_index: 0,
        };

        let raw = serialize(&original).unwrap();
        let decoded: SubChatResult = deserialize(&raw).unwrap();

        assert_eq!(decoded, original);
        assert!(raw.contains("\"status\":\"success\""));
        assert!(raw.contains("\"call_index\":0"));
        assert!(!raw.contains("sender"));
        assert!(!raw.contains("auth_token"));
    }

    #[test]
    fn serializes_and_deserializes_execution_plan() {
        let original = ExecutionPlan {
            task_id: "task-42".to_string(),
            description: "Run the test suite".to_string(),
            commands: vec![
                "cargo test".to_string(),
                "cargo clippy --all-targets".to_string(),
            ],
        };

        let raw = serialize(&original).unwrap();
        let decoded: ExecutionPlan = deserialize(&raw).unwrap();

        assert_eq!(decoded, original);
        assert!(raw.contains("\"commands\":[\"cargo test\",\"cargo clippy --all-targets\"]"));
        assert!(!raw.contains("sender"));
        assert!(!raw.contains("auth_token"));
    }

    #[test]
    fn symbol_extraction_stays_strict_and_uses_critical_default() {
        let (symbol, remaining) = Symbol::extract_from_command("[[AB:OPS.APP.OPEN]] cargo test");
        assert_eq!(symbol, Symbol::OpsAppOpen);
        assert_eq!(remaining, "cargo test");

        let (symbol, remaining) = Symbol::extract_from_command("cargo test");
        assert_eq!(symbol, Symbol::CritUnclassified);
        assert_eq!(remaining, "cargo test");

        let (symbol, remaining) = Symbol::extract_from_command("[[AB:UNKNOWN]] cargo test");
        assert_eq!(symbol, Symbol::CritUnclassified);
        assert_eq!(remaining, "[[AB:UNKNOWN]] cargo test");
    }
}
