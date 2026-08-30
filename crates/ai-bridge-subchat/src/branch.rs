use ai_bridge_protocol::{ContextQuery, ContextResponse, SubChatOpenRequest, SubChatResult};
use thiserror::Error;

use crate::orphan::{OrphanError, OrphanWorker};

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
    pub master_message: String,
}

#[derive(Debug, Error)]
pub enum SubChatError {
    #[error("sub-chat limit exceeded for task {task_id}: maximum of 3 sub-chats allowed")]
    SubChatLimitExceeded { task_id: String },
    #[error("invalid task id: {0}")]
    InvalidTaskId(String),
    #[error("branch {branch} is not available")]
    BranchUnavailable { branch: String },
    #[error("orphan worker failed: {0}")]
    Orphan(String),
}

impl Branch {
    pub fn new(name: BranchName, task_id: impl Into<String>) -> Self {
        Self {
            name,
            task_id: task_id.into(),
            sub_chat_counter: 0,
            master_message: String::new(),
        }
    }

    pub fn with_master_message(mut self, master_message: impl Into<String>) -> Self {
        self.master_message = master_message.into();
        self
    }

    pub fn sub_chat_limit(&self) -> usize {
        3
    }

    fn await_maestro(&self, query: &ContextQuery) -> ContextResponse {
        ContextResponse {
            task_id: query.task_id.clone(),
            include_master_context: !self.master_message.is_empty(),
        }
    }

    pub fn open_sub_chat(
        &mut self,
        request: &SubChatOpenRequest,
    ) -> Result<SubChatResult, SubChatError> {
        // 1. receive the question
        if request.task_id != self.task_id {
            return Err(SubChatError::InvalidTaskId(request.task_id.clone()));
        }

        if self.sub_chat_counter >= self.sub_chat_limit() {
            return Err(SubChatError::SubChatLimitExceeded {
                task_id: self.task_id.clone(),
            });
        }

        self.sub_chat_counter += 1;

        // 2. send a ContextQuery to the maestro
        let context_query = ContextQuery {
            task_id: self.task_id.clone(),
        };

        // 3. wait for the ContextResponse
        let context_response = self.await_maestro(&context_query);

        // 4. build the final message and send it to the OrphanWorker,
        //    merging the master context when requested by the maestro.
        let final_message = if context_response.include_master_context {
            format!("{}\n{}", self.master_message, request.prompt)
        } else {
            request.prompt.clone()
        };

        let worker = OrphanWorker::new(final_message);
        let orphan_result = worker.run().map_err(|e| match e {
            OrphanError::Run(msg) => SubChatError::Orphan(msg),
            OrphanError::SubChatNotAllowed => {
                SubChatError::Orphan("sub-chat not allowed".to_string())
            }
        })?;

        // 5. receive the result, attaching the sub-chat call index
        let call_index = (self.sub_chat_counter - 1) as u8;

        Ok(SubChatResult {
            task_id: self.task_id.clone(),
            result: format!(
                "branch {:?} completed sub-chat {call_index}: {}",
                self.name, orphan_result.result
            ),
            status: orphan_result.status.clone(),
            call_index,
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
    fn branch_limits_sub_chats_to_three_per_task() {
        let mut branch =
            Branch::new(BranchName::A, "task-7").with_master_message("master context".to_string());
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
        let fourth = SubChatOpenRequest {
            task_id: "task-7".to_string(),
            prompt: "Fourth prompt".to_string(),
            branch: "A".to_string(),
        };

        let first_result = branch.open_sub_chat(&first).unwrap();
        assert_eq!(first_result.call_index, 0);
        assert!(first_result.result.contains("master context"));

        let second_result = branch.open_sub_chat(&second).unwrap();
        assert_eq!(second_result.call_index, 1);
        assert!(second_result.result.contains("master context"));

        let third_result = branch.open_sub_chat(&third).unwrap();
        assert_eq!(third_result.call_index, 2);
        assert!(third_result.result.contains("master context"));

        assert!(matches!(
            branch.open_sub_chat(&fourth),
            Err(SubChatError::SubChatLimitExceeded { .. })
        ));
        assert_eq!(branch.sub_chat_counter, 3);
    }

    #[test]
    fn branch_without_master_context_does_not_merge_master_message() {
        let mut branch = Branch::new(BranchName::B, "task-8");
        let request = SubChatOpenRequest {
            task_id: "task-8".to_string(),
            prompt: "Prompt only".to_string(),
            branch: "B".to_string(),
        };

        let result = branch.open_sub_chat(&request).unwrap();
        assert_eq!(result.call_index, 0);
        assert!(!result.result.contains("master context"));
    }
}
