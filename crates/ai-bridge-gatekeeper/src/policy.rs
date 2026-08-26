use ai_bridge_protocol::ExecutionPlan;

use crate::criteria::{evaluate_criteria, EvaluationStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Delegable,
    NonDelegable,
    PermanentSuspension,
}

pub fn decide_policy(plan: &ExecutionPlan) -> PolicyDecision {
    match evaluate_criteria(plan).status {
        EvaluationStatus::Delegable => PolicyDecision::Delegable,
        EvaluationStatus::NonDelegable => PolicyDecision::NonDelegable,
        EvaluationStatus::PermanentSuspension => PolicyDecision::PermanentSuspension,
    }
}
