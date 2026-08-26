use ai_bridge_protocol::{ContextQuery, ContextResponse, SubChatOpenRequest, SubChatResult};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchName {
    A,
    B,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: BranchName,
    pub task_id: String,
    pub sub_chat_counter: usize,
}

#[derive(Debug, Error)]
pub enum SubChatError {
    #[error("sub-chat limit exceeded for task {task_id}: maximum of 2 sub-chats allowed")]
    SubChatLimitExceeded { task_id: String },
    #[error("invalid task id: {0}")]
    InvalidTaskId(String),
    #[error("branch {branch} is not available")]
    BranchUnavailable { branch: String },
}

impl Branch {
    pub fn new(name: BranchName, task_id: impl Into<String>) -> Self {
        Self {
            name,
            task_id: task_id.into(),
            sub_chat_counter: 0,
        }
    }

    pub fn sub_chat_limit(&self) -> usize {
        2
    }

    pub fn open_sub_chat(&mut self, request: &SubChatOpenRequest) -> Result<SubChatResult, SubChatError> {
        if request.task_id != self.task_id {
            return Err(SubChatError::InvalidTaskId(request.task_id.clone()));
        }

        if self.sub_chat_counter >= self.sub_chat_limit() {
            return Err(SubChatError::SubChatLimitExceeded {
                task_id: self.task_id.clone(),
            });
        }

        self.sub_chat_counter += 1;

        let context_query = ContextQuery {
            task_id: self.task_id.clone(),
            query: request.prompt.clone(),
            limit: 10,
        };

        let context_response = ContextResponse {
            task_id: self.task_id.clone(),
            context: vec![request.prompt.clone()],
            status: "ready".to_string(),
        };

        let _ = (&context_query, &context_response);

        Ok(SubChatResult {
            task_id: self.task_id.clone(),
            result: format!("branch {:?} completed sub-chat {}", self.name, self.sub_chat_counter),
            status: "success".to_string(),
        })
    }

    pub fn can_open_more(&self) -> bool {
        self.sub_chat_counter < self.sub_chat_limit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_limits_sub_chats_to_two_per_task() {
        let mut branch = Branch::new(BranchName::A, "task-7");
        let first = SubChatOpenRequest {
            task_id: "task-7".to_string(),
            prompt: "First prompt".to_string(),
            branch: "A".to_string(),
        };
        let second = SubChatOpenRequest {
            task_id: "task-7".to_string(),
            prompt: "Second prompt".to_string(),
            branch: "A".to_string(),
        };
        let third = SubChatOpenRequest {
            task_id: "task-7".to_string(),
            prompt: "Third prompt".to_string(),
            branch: "A".to_string(),
        };

        assert!(branch.open_sub_chat(&first).is_ok());
        assert!(branch.open_sub_chat(&second).is_ok());
        assert!(matches!(branch.open_sub_chat(&third), Err(SubChatError::SubChatLimitExceeded { .. })));
        assert_eq!(branch.sub_chat_counter, 2);
    }
}
