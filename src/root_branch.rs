use std::time::Duration;

use ai_bridge_protocol::SubChatResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryDecision {
    FullDelivery {
        primary: SubChatResult,
        secondary: SubChatResult,
    },
    PartialDelivery {
        delivered: SubChatResult,
        dropped: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchRunState {
    pub branch_a_result: Option<SubChatResult>,
    pub branch_b_result: Option<SubChatResult>,
}

impl BranchRunState {
    pub fn new() -> Self {
        Self {
            branch_a_result: None,
            branch_b_result: None,
        }
    }

    pub fn mark_finished(&mut self, branch: &str, result: SubChatResult) {
        match branch {
            "A" => self.branch_a_result = Some(result),
            "B" => self.branch_b_result = Some(result),
            _ => {}
        }
    }

    pub fn wipe(&mut self) {
        self.branch_a_result = None;
        self.branch_b_result = None;
    }
}

pub fn resolve_delivery(
    first_result: SubChatResult,
    second_result: Option<SubChatResult>,
    elapsed: Duration,
    timeout: Duration,
) -> DeliveryDecision {
    if let Some(second) = second_result.clone() {
        if elapsed < timeout {
            return DeliveryDecision::FullDelivery {
                primary: first_result.clone(),
                secondary: second,
            };
        }
    }

    if elapsed >= timeout {
        DeliveryDecision::PartialDelivery {
            delivered: first_result,
            dropped: second_result.map(|result| result.task_id),
        }
    } else {
        DeliveryDecision::FullDelivery {
            primary: first_result,
            secondary: second_result.expect("second result must be present before timeout"),
        }
    }
}

pub async fn await_branch_completion(
    first_result: SubChatResult,
    second_result: Option<SubChatResult>,
    timeout: Duration,
) -> DeliveryDecision {
    if second_result.is_some() {
        tokio::time::sleep(Duration::from_millis(10)).await;
        return DeliveryDecision::FullDelivery {
            primary: first_result.clone(),
            secondary: second_result.expect("second result present"),
        };
    }

    tokio::time::sleep(timeout).await;
    DeliveryDecision::PartialDelivery {
        delivered: first_result,
        dropped: None,
    }
}

pub fn record_and_decide(
    state: &mut BranchRunState,
    branch: &str,
    result: SubChatResult,
    elapsed: Duration,
    timeout: Duration,
) -> DeliveryDecision {
    state.mark_finished(branch, result.clone());

    let branch_a = state.branch_a_result.clone();
    let branch_b = state.branch_b_result.clone();

    if let (Some(primary), Some(secondary)) = (branch_a, branch_b) {
        let decision = DeliveryDecision::FullDelivery {
            primary: primary.clone(),
            secondary: secondary.clone(),
        };
        state.wipe();
        decision
    } else {
        let result_for_timeout = if branch == "A" {
            state.branch_b_result.clone()
        } else {
            state.branch_a_result.clone()
        };

        let decision = resolve_delivery(result.clone(), result_for_timeout, elapsed, timeout);
        state.wipe();
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_delivery_on_timeout() {
        let result_a = SubChatResult {
            task_id: "task-9".to_string(),
            result: "branch-a complete".to_string(),
            status: "success".to_string(),
        };
        let decision = resolve_delivery(result_a.clone(), None, Duration::from_secs(241), Duration::from_secs(240));
        assert!(matches!(decision, DeliveryDecision::PartialDelivery { .. }));
    }

    #[test]
    fn full_delivery_when_second_branch_finishes_before_timeout() {
        let result_a = SubChatResult {
            task_id: "task-9".to_string(),
            result: "branch-a complete".to_string(),
            status: "success".to_string(),
        };
        let result_b = SubChatResult {
            task_id: "task-9".to_string(),
            result: "branch-b complete".to_string(),
            status: "success".to_string(),
        };
        let decision = resolve_delivery(result_a, Some(result_b), Duration::from_secs(120), Duration::from_secs(240));
        assert!(matches!(decision, DeliveryDecision::FullDelivery { .. }));
    }
}
