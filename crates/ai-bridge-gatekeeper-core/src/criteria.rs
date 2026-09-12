use ai_bridge_protocol::{ExecutionPlan, Symbol, SymbolClassification};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Configuration for the text-scan pattern lists used in criteria evaluation.
/// These are the tunable patterns that previously lived as hardcoded constants;
/// the shipped defaults match the historical behavior exactly, and an operator
/// may edit them via the admin UI / config files.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CriteriaConfig {
    pub irreversibility: Vec<String>,
    pub system_security: Vec<String>,
    pub credentials_secrets: Vec<String>,
    pub data_exfiltration: Vec<String>,
    pub financial: Vec<String>,
    pub third_party: Vec<String>,
}

impl CriteriaConfig {
    /// The shipped default patterns — these are byte-for-byte the historical
    /// hardcoded markers, so behavior is unchanged until edited.
    pub fn default_patterns() -> Self {
        Self {
            irreversibility: vec![
                "permanent delete".to_string(),
                "overwrite backup".to_string(),
                "overwrite backups".to_string(),
                "delete backup".to_string(),
                "delete backups".to_string(),
                "rm -rf /".to_string(),
                "rm -rf --no-preserve-root /".to_string(),
                "sudo rm -rf /".to_string(),
                "rm -rf /var/backups".to_string(),
                "rm -rf /home".to_string(),
                "rm -rf /tmp".to_string(),
                "rm -rf /usr".to_string(),
                "rm -rf /etc".to_string(),
                "dd if=/dev/zero".to_string(),
                "dd if = /dev/zero".to_string(),
                "wipefs".to_string(),
                "mkfs".to_string(),
            ],
            system_security: vec![
                r"sudoers".to_string(),
                r"/etc/sudoers".to_string(),
                r"iptables".to_string(),
                r"ufw".to_string(),
                r"firewall".to_string(),
                r"chmod\s+\d+\s+.*(/etc|/usr|/bin|/sbin|/opt)".to_string(),
                r"chown\s+.*(root|sudo)".to_string(),
                r"useradd|usermod|passwd".to_string(),
                r"systemctl\s+(stop|disable|restart)".to_string(),
            ],
            credentials_secrets: vec![
                r"password".to_string(),
                r"passwd".to_string(),
                r"ssh\s+-i".to_string(),
                r"id_rsa".to_string(),
                r"api[_ -]?token".to_string(),
                r"authorization: bearer".to_string(),
                r"secret".to_string(),
                r"private[_ -]?key".to_string(),
                r"aws secret access key".to_string(),
            ],
            data_exfiltration: vec![
                r"curl\s+.*(\|\s*sh|--upload|--data|--form)".to_string(),
                r"scp\s+".to_string(),
                r"rsync\s+.*(ssh|remote)".to_string(),
                r"git\s+push".to_string(),
                r"mail\s+-s".to_string(),
                r"sendmail".to_string(),
                r"email.*(send|attach)".to_string(),
                r"upload.*(file|archive|logs)".to_string(),
            ],
            financial: vec![
                r"transfer.*money".to_string(),
                r"bank".to_string(),
                r"wire transfer".to_string(),
                r"paypal".to_string(),
                r"stripe".to_string(),
                r"invoice".to_string(),
                r"charge".to_string(),
                r"payment".to_string(),
                r"currency".to_string(),
            ],
            third_party: vec![
                r"post\s+.*(tweet|message|announcement|comment)".to_string(),
                r"twitter".to_string(),
                r"x\.com".to_string(),
                r"discord".to_string(),
                r"slack".to_string(),
                r"linkedin".to_string(),
                r"facebook".to_string(),
                r"reddit".to_string(),
                r"telegram".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Criterion {
    Irreversibility,
    SystemSecurity,
    CredentialsSecrets,
    DataExfiltration,
    Financial,
    ThirdPartyImpact,
}

impl Criterion {
    /// A human-readable label for this criterion, used when advertising the
    /// triggered classification reasons to the approval UI.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Irreversibility => "Irreversibility",
            Self::SystemSecurity => "SystemSecurity",
            Self::CredentialsSecrets => "CredentialsSecrets",
            Self::DataExfiltration => "DataExfiltration",
            Self::Financial => "Financial",
            Self::ThirdPartyImpact => "ThirdPartyImpact",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationStatus {
    Delegable,
    NonDelegable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

fn contains_pattern(text: &str, pattern: &str) -> bool {
    let re = Regex::new(pattern).unwrap();
    re.is_match(text)
}

fn evaluate_text_command(command: &str, config: &CriteriaConfig) -> CriteriaSummary {
    let lower = command.trim().to_ascii_lowercase();

    let mut triggered = Vec::new();

    // The irreversibility scan is the only one with additional heuristic
    // compound rules (the historical `rm -rf <path>` and `dd ... /dev/zero`
    // special cases). The pattern list captures the direct markers.
    if config
        .irreversibility
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

    if config
        .system_security
        .iter()
        .any(|p| contains_pattern(&lower, p))
    {
        triggered.push(Criterion::SystemSecurity);
    }

    if config
        .credentials_secrets
        .iter()
        .any(|p| contains_pattern(&lower, p))
    {
        triggered.push(Criterion::CredentialsSecrets);
    }

    if config
        .data_exfiltration
        .iter()
        .any(|p| contains_pattern(&lower, p))
    {
        triggered.push(Criterion::DataExfiltration);
    }

    if config
        .financial
        .iter()
        .any(|p| contains_pattern(&lower, p))
    {
        triggered.push(Criterion::Financial);
    }

    if config
        .third_party
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

/// Evaluate an execution plan against the provided criteria configuration.
pub fn evaluate_criteria_with_config(plan: &ExecutionPlan, config: &CriteriaConfig) -> CriteriaSummary {
    let mut triggered = Vec::new();
    let mut status = EvaluationStatus::Delegable;

    for command in &plan.commands {
        let (symbol, remaining_command) = Symbol::extract_from_command(command);
        let text_summary = evaluate_text_command(&remaining_command, config);

        // Security rule: a command without a valid symbol tag is always treated as
        // critical. The legacy text scan is only consulted for lines that already
        // carry an explicit valid symbol tag.
        let has_valid_tag = command.trim_start().starts_with("[[AB:")
            && symbol != Symbol::CritUnclassified;

        if !has_valid_tag {
            status = EvaluationStatus::NonDelegable;
            continue;
        }

        let symbol_status = match symbol.classification() {
            SymbolClassification::Delegable => EvaluationStatus::Delegable,
            SymbolClassification::Critical => EvaluationStatus::NonDelegable,
        };

        if symbol_status == EvaluationStatus::NonDelegable || text_summary.status == EvaluationStatus::NonDelegable {
            status = EvaluationStatus::NonDelegable;
        }

        for criterion in text_summary.triggered {
            if !triggered.contains(&criterion) {
                triggered.push(criterion);
            }
        }
    }

    if triggered.is_empty() && status == EvaluationStatus::Delegable {
        return CriteriaSummary {
            triggered: Vec::new(),
            status: EvaluationStatus::Delegable,
        };
    }

    CriteriaSummary { triggered, status }
}

pub fn evaluate_criteria(plan: &ExecutionPlan) -> CriteriaSummary {
    evaluate_criteria_with_config(plan, &CriteriaConfig::default_patterns())
}

