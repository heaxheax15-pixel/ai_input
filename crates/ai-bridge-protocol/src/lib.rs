use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubChatOpenRequest {
    pub task_id: String,
    pub prompt: String,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextQuery {
    pub task_id: String,
    pub query: String,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextResponse {
    pub task_id: String,
    pub context: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubChatResult {
    pub task_id: String,
    pub result: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub task_id: String,
    pub description: String,
    pub commands: Vec<String>,
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
            query: "Find relevant files".to_string(),
            limit: 5,
        };

        let raw = serialize(&original).unwrap();
        let decoded: ContextQuery = deserialize(&raw).unwrap();

        assert_eq!(decoded, original);
        assert!(raw.contains("\"limit\":5"));
        assert!(!raw.contains("sender"));
        assert!(!raw.contains("auth_token"));
    }

    #[test]
    fn serializes_and_deserializes_context_response() {
        let original = ContextResponse {
            task_id: "task-42".to_string(),
            context: vec!["file1.rs".to_string(), "file2.rs".to_string()],
            status: "ok".to_string(),
        };

        let raw = serialize(&original).unwrap();
        let decoded: ContextResponse = deserialize(&raw).unwrap();

        assert_eq!(decoded, original);
        assert!(raw.contains("\"context\":[\"file1.rs\",\"file2.rs\"]"));
        assert!(!raw.contains("sender"));
        assert!(!raw.contains("auth_token"));
    }

    #[test]
    fn serializes_and_deserializes_subchat_result() {
        let original = SubChatResult {
            task_id: "task-42".to_string(),
            result: "Completed successfully".to_string(),
            status: "success".to_string(),
        };

        let raw = serialize(&original).unwrap();
        let decoded: SubChatResult = deserialize(&raw).unwrap();

        assert_eq!(decoded, original);
        assert!(raw.contains("\"status\":\"success\""));
        assert!(!raw.contains("sender"));
        assert!(!raw.contains("auth_token"));
    }

    #[test]
    fn serializes_and_deserializes_execution_plan() {
        let original = ExecutionPlan {
            task_id: "task-42".to_string(),
            description: "Run the test suite".to_string(),
            commands: vec!["cargo test".to_string(), "cargo clippy --all-targets".to_string()],
        };

        let raw = serialize(&original).unwrap();
        let decoded: ExecutionPlan = deserialize(&raw).unwrap();

        assert_eq!(decoded, original);
        assert!(raw.contains("\"commands\":[\"cargo test\",\"cargo clippy --all-targets\"]"));
        assert!(!raw.contains("sender"));
        assert!(!raw.contains("auth_token"));
    }
}
