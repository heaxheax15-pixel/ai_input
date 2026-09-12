use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::criteria::CriteriaConfig;

// ---------------------------------------------------------------------------
// Core config structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AllowlistEntry {
    pub name: String,
    pub path: String,
    pub allowed: bool,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SymbolDefinition {
    pub name: String,
    pub classification: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicySymbolEntry {
    pub symbol: String,
    pub classification: String,
    pub auto_approve_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CriteriaPatternSet {
    pub category: String,
    pub patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SocketDefinition {
    pub name: String,
    pub path: String,
    pub permissions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BranchConfig {
    pub name: String,
    pub sub_chat_limit: usize,
    pub context_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdminAuditEntry {
    pub timestamp: String,
    pub action: String,
    pub summary: String,
    pub old_value: String,
    pub new_value: String,
}

// ---------------------------------------------------------------------------
// Top-level admin config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AdminConfig {
    #[serde(default)]
    pub allowlist: Vec<AllowlistEntry>,
    #[serde(default)]
    pub symbols: Vec<SymbolDefinition>,
    #[serde(default)]
    pub policy: Vec<PolicySymbolEntry>,
    #[serde(default)]
    pub criteria: Vec<CriteriaPatternSet>,
    #[serde(default)]
    pub sockets: Vec<SocketDefinition>,
    #[serde(default)]
    pub branches: Vec<BranchConfig>,
    #[serde(default)]
    pub audit: Vec<AdminAuditEntry>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum PolicyValidationError {
    #[error("Unsafe allowlist value: wildcard '*' not permitted in path for '{name}'")]
    UnsafeAllowlistValue { name: String },

    #[error("Critical symbol '{symbol}' must not be classified as delegable")]
    CriticalSymbolReclassified { symbol: String },

    #[error("Critical symbol '{symbol}' must never auto-approve; auto_approve_seconds must be 0 (got {seconds}s)")]
    CriticalSymbolAutoApprove { symbol: String, seconds: u64 },

    #[error("Duplicate allowlist entry: '{name}' appears more than once")]
    DuplicateAllowlistEntry { name: String },

    #[error("Invalid criteria regex for category '{category}': {error}")]
    InvalidCriteriaRegex { category: String, error: String },

    #[error("Socket permissions must be octal, got '{permissions}'")]
    InvalidSocketPermissions { name: String, permissions: String },

    #[error("Duplicate socket definition: '{name}' appears more than once")]
    DuplicateSocket { name: String },
}

/// Known critical symbols that must never be reclassified as delegable via the
/// admin UI. This is the security boundary: the GUI may edit classification
/// mappings, but it cannot promote these to delegable.
const CRITICAL_SYMBOLS: &[&str] = &[
    "CRIT.IRREVERSIBLE",
    "CRIT.SYS.SECURITY",
    "CRIT.SECRET",
    "CRIT.EGRESS",
    "CRIT.FINANCIAL",
    "CRIT.THIRD.PARTY",
    "CRIT.UNCLASSIFIED",
];

impl AdminConfig {
    /// Validate the entire configuration. Returns the first error encountered.
    pub fn validate(&self) -> Result<(), PolicyValidationError> {
        self.validate_allowlist()?;
        self.validate_symbols_and_policy()?;
        self.validate_criteria()?;
        self.validate_sockets()?;
        Ok(())
    }

    fn validate_allowlist(&self) -> Result<(), PolicyValidationError> {
        let mut seen = std::collections::HashSet::new();
        for entry in &self.allowlist {
            if entry.path == "*" {
                return Err(PolicyValidationError::UnsafeAllowlistValue {
                    name: entry.name.clone(),
                });
            }
            if !seen.insert(entry.name.clone()) {
                return Err(PolicyValidationError::DuplicateAllowlistEntry {
                    name: entry.name.clone(),
                });
            }
        }
        Ok(())
    }

    fn validate_symbols_and_policy(&self) -> Result<(), PolicyValidationError> {
        for entry in &self.policy {
            let is_known_critical = CRITICAL_SYMBOLS.contains(&entry.symbol.as_str());
            if is_known_critical && entry.classification == "delegable" {
                return Err(PolicyValidationError::CriticalSymbolReclassified {
                    symbol: entry.symbol.clone(),
                });
            }
            // A critical symbol must never auto-approve: positive windows would
            // let a dangerous action through silently.
            if is_known_critical && entry.auto_approve_seconds > 0 {
                return Err(PolicyValidationError::CriticalSymbolAutoApprove {
                    symbol: entry.symbol.clone(),
                    seconds: entry.auto_approve_seconds,
                });
            }
        }
        Ok(())
    }

    fn validate_criteria(&self) -> Result<(), PolicyValidationError> {
        for set in &self.criteria {
            for pattern in &set.patterns {
                if regex::Regex::new(pattern).is_err() {
                    return Err(PolicyValidationError::InvalidCriteriaRegex {
                        category: set.category.clone(),
                        error: pattern.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_sockets(&self) -> Result<(), PolicyValidationError> {
        let mut seen = std::collections::HashSet::new();
        for sock in &self.sockets {
            // Permissions must be a valid octal string like "0600" or "0700"
            let valid = sock.permissions.len() == 4
                && sock.permissions.starts_with('0')
                && sock.permissions[1..].chars().all(|c| c.is_ascii_digit());
            if !valid {
                return Err(PolicyValidationError::InvalidSocketPermissions {
                    name: sock.name.clone(),
                    permissions: sock.permissions.clone(),
                });
            }
            if !seen.insert(sock.name.clone()) {
                return Err(PolicyValidationError::DuplicateSocket {
                    name: sock.name.clone(),
                });
            }
        }
        Ok(())
    }

/// Returns true if the proposed change lowers the security posture.
///
/// Security-lowering changes include:
/// - Adding a wildcard allowlist entry
/// - Reclassifying a Critical symbol as Delegable
/// - Enabling auto-approval for a Critical symbol (0 -> >0 seconds)
/// - Removing a socket entry
pub fn is_security_lowering_change(&self, previous: &AdminConfig) -> bool {
        // Check for new wildcard allowlist entries
        for entry in &self.allowlist {
            if entry.path == "*" && !previous.allowlist.iter().any(|e| e.name == entry.name) {
                return true;
            }
        }

        // Check for classification changes that lower security
        for policy in &self.policy {
            if let Some(prev) = previous.policy.iter().find(|p| p.symbol == policy.symbol) {
                if prev.classification == "critical" && policy.classification == "delegable" {
                    return true;
                }
                // Enabling silent auto-approval on a critical symbol is dangerous.
                if CRITICAL_SYMBOLS.contains(&policy.symbol.as_str())
                    && prev.auto_approve_seconds == 0
                    && policy.auto_approve_seconds > 0
                {
                    return true;
                }
            }
        }

        // Fewer sockets than before
        if self.sockets.len() < previous.sockets.len() {
            return true;
        }

        false
    }
}

// ---------------------------------------------------------------------------
// Config path resolution
// ---------------------------------------------------------------------------

/// Returns the platform-appropriate config root directory.
///
/// - Linux/macOS: `$XDG_CONFIG_HOME/ai-bridge` or `~/.config/ai-bridge`
/// - Windows: `%APPDATA%\ai-bridge`
pub fn config_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("ai-bridge")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config").join("ai-bridge")
    } else if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("ai-bridge")
    } else {
        PathBuf::from(".").join("ai-bridge-config")
    }
}

/// Returns the audit log directory.
pub fn audit_dir() -> PathBuf {
    config_dir().join("audit")
}

/// Resolve the path for a named config file under the config directory.
pub fn config_file(name: &str) -> PathBuf {
    config_dir().join(name)
}

/// Load an `AdminConfig` from a TOML file, or return the default.
pub fn load_config(path: impl AsRef<Path>) -> Result<AdminConfig, std::io::Error> {
    let raw = std::fs::read_to_string(path)?;
    toml::from_str(&raw).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Save an `AdminConfig` to a TOML file.
pub fn save_config(config: &AdminConfig, path: impl AsRef<Path>) -> Result<(), std::io::Error> {
    let raw = toml::to_string(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, raw)
}

/// Create a backup of the config file before writing. Returns the backup path.
pub fn backup_config(path: impl AsRef<Path>) -> Result<PathBuf, std::io::Error> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let backup = path.with_file_name(format!("{}{}.bak.{}", stem, ext, timestamp));
    std::fs::copy(path, &backup)?;
    Ok(backup)
}

/// Validate a config by loading it in a dry-run pass. Returns Ok(()) if valid.
pub fn dry_run_validate(path: impl AsRef<Path>) -> Result<AdminConfig, PolicyValidationError> {
    let config = load_config(path).map_err(|e| PolicyValidationError::InvalidCriteriaRegex {
        category: "file".to_string(),
        error: e.to_string(),
    })?;
    config.validate()?;
    Ok(config)
}

impl AdminConfig {
    /// Convert the criteria pattern sets stored in this config into a
    /// [`CriteriaConfig`] the criteria engine can evaluate with.
    pub fn to_criteria_config(&self) -> CriteriaConfig {
        let mut config = CriteriaConfig::default_patterns();
        // Criteria config only exists for categories actually present in the
        // admin config; missing categories retain the shipped defaults.
        for set in &self.criteria {
            match set.category.as_str() {
                "irreversibility" => config.irreversibility = set.patterns.clone(),
                "system_security" => config.system_security = set.patterns.clone(),
                "secrets" | "credentials_secrets" => {
                    config.credentials_secrets = set.patterns.clone()
                }
                "exfiltration" | "data_exfiltration" => {
                    config.data_exfiltration = set.patterns.clone()
                }
                "financial" => config.financial = set.patterns.clone(),
                "third_party" => config.third_party = set.patterns.clone(),
                _ => {}
            }
        }
        config
    }

    /// Seed the criteria list with the shipped default patterns if empty.
    pub fn seed_default_criteria(&mut self) {
        if self.criteria.is_empty() {
            let defaults = CriteriaConfig::default_patterns();
            self.criteria.push(CriteriaPatternSet {
                category: "irreversibility".to_string(),
                patterns: defaults.irreversibility,
            });
            self.criteria.push(CriteriaPatternSet {
                category: "system_security".to_string(),
                patterns: defaults.system_security,
            });
            self.criteria.push(CriteriaPatternSet {
                category: "secrets".to_string(),
                patterns: defaults.credentials_secrets,
            });
            self.criteria.push(CriteriaPatternSet {
                category: "exfiltration".to_string(),
                patterns: defaults.data_exfiltration,
            });
            self.criteria.push(CriteriaPatternSet {
                category: "financial".to_string(),
                patterns: defaults.financial,
            });
            self.criteria.push(CriteriaPatternSet {
                category: "third_party".to_string(),
                patterns: defaults.third_party,
            });
        }
    }
}
