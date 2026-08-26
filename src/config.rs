use std::path::{Path, PathBuf};
use std::time::Duration;

use ai_bridge_hand_eye::allowlist::Allowlist;

pub const SYNC_TIMEOUT_SECS: u64 = 240;
pub const SYNC_TIMEOUT: Duration = Duration::from_secs(SYNC_TIMEOUT_SECS);

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub allowlist_path: PathBuf,
    pub sync_timeout: Duration,
}

impl RuntimeConfig {
    pub fn from_workspace_root(root: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let root = root.as_ref();
        let config_dir = root.join("config");
        let allowlist_path = config_dir.join("allowlist.toml");
        Ok(Self {
            allowlist_path,
            sync_timeout: SYNC_TIMEOUT,
        })
    }

    pub fn load_allowlist(&self) -> Result<Allowlist, std::io::Error> {
        Allowlist::load(&self.allowlist_path)
    }
}

pub fn default_runtime_config() -> Result<RuntimeConfig, std::io::Error> {
    RuntimeConfig::from_workspace_root(".")
}

pub fn load_allowlist_from_default_config() -> Result<Allowlist, std::io::Error> {
    default_runtime_config()?.load_allowlist()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_config_points_to_allowlist() {
        let config = RuntimeConfig::from_workspace_root(".").unwrap();
        assert!(config.allowlist_path.ends_with("config/allowlist.toml"));
        assert_eq!(config.sync_timeout, SYNC_TIMEOUT);
    }
}
