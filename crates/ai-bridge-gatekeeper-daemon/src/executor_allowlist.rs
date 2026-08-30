//! Executor binary allowlist — independent allow/deny layer for `tokens[0]`.
//!
//! This is a **separate protection layer** from the Gatekeeper classification.
//! Even if `decide_policy` returns `Delegable`, the binary must be explicitly
//! listed here to execute. This prevents execution of arbitrary binaries
//! (e.g. `rm`, `dd`, `mkfs`) that might pass text-pattern checks but are
//! fundamentally unsafe to run unattended.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecutorAllowlistError {
    #[error("failed to read allowlist file {path}: {source}")]
    Read { path: PathBuf, #[source] source: std::io::Error },
    #[error("failed to parse allowlist TOML: {0}")]
    Parse(#[from] toml::de::Error),
}

#[derive(Debug, Clone, Deserialize)]
struct AllowlistConfig {
    binaries: Vec<BinaryEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct BinaryEntry {
    name: String,
    allowed: bool,
}

/// Allowlist of executable binaries (tokens[0] after shlex::split).
/// Only binaries with `allowed = true` may be executed via `execute_approved_task`.
#[derive(Debug, Clone)]
pub struct ExecutorAllowlist {
    allowed: HashSet<String>,
}

impl ExecutorAllowlist {
    /// Load allowlist from the default config path: `<workspace_root>/config/executor_allowlist.toml`
    pub fn load_default() -> Result<Self, ExecutorAllowlistError> {
        // Try multiple strategies to find the config file:
        // 1. Relative to current working directory (for tests and normal runs)
        // 2. Relative to CARGO_MANIFEST_DIR (for compiled binaries)
        let candidates = [
            PathBuf::from("config/executor_allowlist.toml"),
            PathBuf::from("../config/executor_allowlist.toml"),
            PathBuf::from("../../config/executor_allowlist.toml"),
        ];

        // Also try CARGO_MANIFEST_DIR based paths
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Some(ws_root) = manifest_dir.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
            candidates.iter().chain(std::iter::once(&ws_root.join("config/executor_allowlist.toml"))).find_map(|p| {
                if p.exists() { Some(p.clone()) } else { None }
            }).map_or_else(
                || Err(ExecutorAllowlistError::Read {
                    path: PathBuf::from("config/executor_allowlist.toml"),
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, "executor allowlist not found in any expected location"),
                }),
                |p| Self::load(&p)
            )
        } else {
            candidates.iter().find_map(|p| {
                if p.exists() { Some(p.clone()) } else { None }
            }).map_or_else(
                || Err(ExecutorAllowlistError::Read {
                    path: PathBuf::from("config/executor_allowlist.toml"),
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, "executor allowlist not found in any expected location"),
                }),
                |p| Self::load(&p)
            )
        }
    }

    /// Load allowlist from a specific path.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ExecutorAllowlistError> {
        let path = path.as_ref();
        let content = fs::read_to_string(path).map_err(|source| ExecutorAllowlistError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let config: AllowlistConfig = toml::from_str(&content)?;
        let allowed = config
            .binaries
            .into_iter()
            .filter(|b| b.allowed)
            .map(|b| b.name)
            .collect();
        Ok(Self { allowed })
    }

    /// Check if a binary name (tokens[0]) is allowed to execute.
    pub fn is_allowed(&self, binary: &str) -> bool {
        self.allowed.contains(binary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_allowlist_and_checks_binary() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("allowlist.toml");
        fs::write(&path, r#"
[[binaries]]
name = "cargo"
allowed = true

[[binaries]]
name = "rm"
allowed = false
"#).unwrap();

        let list = ExecutorAllowlist::load(&path).unwrap();
        assert!(list.is_allowed("cargo"));
        assert!(!list.is_allowed("rm"));
        assert!(!list.is_allowed("unknown"));
    }

    #[test]
    fn rejects_when_binary_not_in_list() {
        let list = ExecutorAllowlist { allowed: ["cargo".to_string()].into_iter().collect() };
        assert!(!list.is_allowed("rm"));
    }
}