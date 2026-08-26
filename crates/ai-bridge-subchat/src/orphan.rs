use std::cell::Cell;

use ai_bridge_protocol::{ContextQuery, ContextResponse, SubChatOpenRequest, SubChatResult};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrphanError {
    #[error("orphan worker cannot request sub-chats")]
    SubChatNotAllowed,
    #[error("orphan worker run failed: {0}")]
    Run(String),
}

#[derive(Debug)]
pub struct OrphanWorker {
    context: String,
    executed: Cell<bool>,
}

impl Drop for OrphanWorker {
    fn drop(&mut self) {
        self.executed.set(false);
    }
}

impl OrphanWorker {
    pub fn new(context: impl Into<String>) -> Self {
        Self {
            context: context.into(),
            executed: Cell::new(false),
        }
    }

    pub fn run(&self) -> Result<SubChatResult, OrphanError> {
        if self.executed.replace(true) {
            return Err(OrphanError::Run("worker already executed once".to_string()));
        }

        let query = ContextQuery {
            task_id: "orphan-task".to_string(),
            query: self.context.clone(),
            limit: 1,
        };
        let response = ContextResponse {
            task_id: "orphan-task".to_string(),
            context: vec![self.context.clone()],
            status: "ready".to_string(),
        };

        let _ = (&query, &response);

        Ok(SubChatResult {
            task_id: "orphan-task".to_string(),
            result: self.context.clone(),
            status: "success".to_string(),
        })
    }

    pub fn request_sub_chat(&self, _request: &SubChatOpenRequest) -> Result<SubChatResult, OrphanError> {
        Err(OrphanError::SubChatNotAllowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orphan_worker_runs_once_and_is_dropped() {
        let worker = OrphanWorker::new("single-use prompt");
        let first = worker.run().unwrap();
        assert_eq!(first.status, "success");
        assert!(matches!(worker.run(), Err(OrphanError::Run(_))));

        let request = SubChatOpenRequest {
            task_id: "orphan-task".to_string(),
            prompt: "nested".to_string(),
            branch: "new_a1".to_string(),
        };
        assert!(matches!(worker.request_sub_chat(&request), Err(OrphanError::SubChatNotAllowed)));
    }
}
