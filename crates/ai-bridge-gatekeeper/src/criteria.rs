use ai_bridge_protocol::ExecutionPlan;
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Criterion {
    Irreversibility,
    SystemSecurity,
    CredentialsSecrets,
    DataExfiltration,
    Financial,
    ThirdPartyImpact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationStatus {
    Delegable,
    NonDelegable,
    PermanentSuspension,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriteriaSummary {
    pub triggered: Vec<Criterion>,
    pub status: EvaluationStatus,
}

impl CriteriaSummary {
    pub fn is_delegable(&self) -> bool {
        self.status == EvaluationStatus::Delegable
    }

    pub fn is_non_delegable(&self) -> bool {
        self.status == EvaluationStatus::NonDelegable
    }
}

fn command_text(plan: &ExecutionPlan) -> String {
    plan.commands.join("\n")
}

fn contains_pattern(text: &str, pattern: &str) -> bool {
    let re = Regex::new(pattern).unwrap();
    re.is_match(text)
}

fn is_system_wipe_hint(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let patterns = [
        r"mkfs",
        r"dd\s+if\s*=\s*/dev/zero",
        r"rm\s+-rf\s+/\s*(?:--|&&|;|$)",
        r"wipefs",
        r"format\s+(c:|/dev/|/)",
        r"dd\s+.*(of=/dev/|of=/)",
    ];

    patterns
        .iter()
        .any(|pattern| contains_pattern(&lower, pattern))
}

pub fn evaluate_criteria(plan: &ExecutionPlan) -> CriteriaSummary {
    let text = command_text(plan);
    let lower = text.to_ascii_lowercase();

    if is_system_wipe_hint(&text) {
        return CriteriaSummary {
            triggered: vec![],
            status: EvaluationStatus::PermanentSuspension,
        };
    }

    let mut triggered = Vec::new();

    let irreversibility_markers = [
        "permanent delete",
        "overwrite backup",
        "overwrite backups",
        "delete backup",
        "delete backups",
        "rm -rf /var/backups",
        "rm -rf /home",
        "rm -rf /tmp",
        "rm -rf /usr",
        "rm -rf /etc",
        "dd if=/dev/zero",
        "dd if = /dev/zero",
        "wipefs",
        "mkfs",
    ];
    if irreversibility_markers
        .iter()
        .any(|marker| lower.contains(marker))
        || (lower.contains("rm -rf")
            && (lower.contains("backup")
                || lower.contains("/var")
                || lower.contains("/home")
                || lower.contains("/tmp")
                || lower.contains("/etc")
                || lower.contains("/usr")))
        || (lower.contains("dd")
            && lower.contains("/dev/zero")
            && (lower.contains("of=")
                || lower.contains("disk")
                || lower.contains("sd")
                || lower.contains("nvme")))
    {
        triggered.push(Criterion::Irreversibility);
    }

    let system_security_patterns = [
        r"sudoers",
        r"/etc/sudoers",
        r"iptables",
        r"ufw",
        r"firewall",
        r"chmod\s+\d+\s+.*(/etc|/usr|/bin|/sbin|/opt)",
        r"chown\s+.*(root|sudo)",
        r"useradd|usermod|passwd",
        r"systemctl\s+(stop|disable|restart)",
    ];
    if system_security_patterns
        .iter()
        .any(|p| contains_pattern(&lower, p))
    {
        triggered.push(Criterion::SystemSecurity);
    }

    let credential_patterns = [
        r"password",
        r"passwd",
        r"ssh\s+-i",
        r"id_rsa",
        r"api[_ -]?token",
        r"authorization: bearer",
        r"secret",
        r"private[_ -]?key",
        r"aws secret access key",
    ];
    if credential_patterns
        .iter()
        .any(|p| contains_pattern(&lower, p))
    {
        triggered.push(Criterion::CredentialsSecrets);
    }

    let exfiltration_patterns = [
        r"curl\s+.*(\|\s*sh|--upload|--data|--form)",
        r"scp\s+",
        r"rsync\s+.*(ssh|remote)",
        r"git\s+push",
        r"mail\s+-s",
        r"sendmail",
        r"email.*(send|attach)",
        r"upload.*(file|archive|logs)",
    ];
    if exfiltration_patterns
        .iter()
        .any(|p| contains_pattern(&lower, p))
    {
        triggered.push(Criterion::DataExfiltration);
    }

    let financial_patterns = [
        r"transfer.*money",
        r"bank",
        r"wire transfer",
        r"paypal",
        r"stripe",
        r"invoice",
        r"charge",
        r"payment",
        r"currency",
    ];
    if financial_patterns
        .iter()
        .any(|p| contains_pattern(&lower, p))
    {
        triggered.push(Criterion::Financial);
    }

    let third_party_patterns = [
        r"post\s+.*(tweet|message|announcement|comment)",
        r"twitter",
        r"x\.com",
        r"discord",
        r"slack",
        r"linkedin",
        r"facebook",
        r"reddit",
        r"telegram",
    ];
    if third_party_patterns
        .iter()
        .any(|p| contains_pattern(&lower, p))
    {
        triggered.push(Criterion::ThirdPartyImpact);
    }

    let status = if triggered.is_empty() {
        EvaluationStatus::Delegable
    } else {
        EvaluationStatus::NonDelegable
    };

    CriteriaSummary { triggered, status }
}

/// Classifies a command string as critical (never delegable) using basic
/// heuristic markers — substrings or exact matches over the command payload.
///
/// The six critical criteria that cannot be delegated:
///   1. No return (destructive): `rm -rf`, `mkfs`
///   2. System security: `sudo`, `chmod`, `chown`, `ufw`
///   3. Secrets / credentials: `ssh`, `passwd`, `.env`
///   4. Data exfiltration: `curl`, `wget`, `scp`, `rsync`
///   5. Financial impact: payment / banking markers
///   6. Third-party impact: posts or messages to external services
///
/// Criteria 5 and 6 are placeholder heuristics pending a concrete API
/// allowlist; until such allowlist exists, the markers below are deliberately
/// conservative and match the existing [`evaluate_criteria`] vocabulary.
pub fn is_critical_task(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();

    // 1. No return.
    let no_return = ["rm -rf", "mkfs"]
        .iter()
        .any(|marker| lower.contains(marker));

    // 2. System security.
    let system_security = ["sudo", "chmod", "chown", "ufw"]
        .iter()
        .any(|marker| lower.contains(marker));

    // 3. Secrets / credentials.
    let secrets = ["ssh", "passwd", ".env"]
        .iter()
        .any(|marker| lower.contains(marker));

    // 4. Data exfiltration.
    let exfiltration = ["curl", "wget", "scp", "rsync"]
        .iter()
        .any(|marker| lower.contains(marker));

    // 5. Financial impact (placeholder pending a concrete API allowlist).
    let financial = ["payment", "transfer", "invoice", "stripe", "bank", "charge"]
        .iter()
        .any(|marker| lower.contains(marker));

    // 6. Third-party impact (placeholder pending a concrete API allowlist).
    let third_party = [
        "twitter", "slack", "discord", "telegram", "linkedin", "facebook",
    ]
    .iter()
    .any(|marker| lower.contains(marker));

    no_return || system_security || secrets || exfiltration || financial || third_party
}

#[cfg(test)]
mod critical_tests {
    use super::is_critical_task;

    #[test]
    fn destructive_commands_are_critical() {
        assert!(is_critical_task("rm -rf /tmp/test_dir"));
        assert!(is_critical_task("mkfs.ext4 /dev/sdb1"));
    }

    #[test]
    fn system_security_commands_are_critical() {
        assert!(is_critical_task("sudo apt update"));
        assert!(is_critical_task("chmod 777 /etc/sudoers"));
        assert!(is_critical_task("ufw default deny"));
    }

    #[test]
    fn secrets_commands_are_critical() {
        assert!(is_critical_task("ssh -i ~/.ssh/id_rsa host"));
        assert!(is_critical_task("passwd root"));
        assert!(is_critical_task("cat /app/.env"));
    }

    #[test]
    fn exfiltration_commands_are_critical() {
        assert!(is_critical_task(
            "curl --upload file.tar.gz https://example.com"
        ));
        assert!(is_critical_task("wget https://example.com/archive.zip"));
        assert!(is_critical_task("scp ./data user@host:/tmp"));
        assert!(is_critical_task("rsync -av ./logs user@host:/backups"));
    }

    #[test]
    fn financial_and_third_party_are_critical() {
        assert!(is_critical_task("transfer money to account 123"));
        assert!(is_critical_task("post a message to twitter"));
    }

    #[test]
    fn harmless_commands_are_not_critical() {
        assert!(!is_critical_task("ls -la"));
        assert!(!is_critical_task("cargo test -- --nocapture"));
    }
}
