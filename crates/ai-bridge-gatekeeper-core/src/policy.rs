use ai_bridge_protocol::ExecutionPlan;
use serde::{Deserialize, Serialize};

use crate::criteria::{evaluate_criteria_with_config, CriteriaConfig, EvaluationStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    Delegable,
    NonDelegable,
}

pub fn decide_policy(plan: &ExecutionPlan) -> PolicyDecision {
    decide_policy_with_config(plan, &CriteriaConfig::default_patterns())
}

/// Decide policy using an explicit criteria configuration (used by the admin
/// UI's live-test tool and for config-loaded evaluation).
pub fn decide_policy_with_config(plan: &ExecutionPlan, config: &CriteriaConfig) -> PolicyDecision {
    match evaluate_criteria_with_config(plan, config).status {
        EvaluationStatus::Delegable => PolicyDecision::Delegable,
        EvaluationStatus::NonDelegable => PolicyDecision::NonDelegable,
    }
}
