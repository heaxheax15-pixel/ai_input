use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AllowlistRule {
    pub app_id: String,
    pub allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AllowlistConfig {
    pub apps: Vec<AllowlistRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowlist {
    pub apps: Vec<AllowlistRule>,
}

impl Allowlist {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let raw = fs::read_to_string(path)?;
        let config: AllowlistConfig = toml::from_str(&raw)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        Ok(Self { apps: config.apps })
    }

    pub fn is_allowed(&self, app_id: &str) -> bool {
        self.apps
            .iter()
            .any(|rule| rule.app_id == app_id && rule.allowed)
    }

    pub fn from_toml(raw: &str) -> Result<Self, std::io::Error> {
        let config: AllowlistConfig = toml::from_str(raw)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        Ok(Self { apps: config.apps })
    }

    pub fn path_for_config(directory: impl AsRef<Path>) -> PathBuf {
        let directory = directory.as_ref();
        directory.join("allowlist.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_allowlist_toml_from_string() {
        let raw = r#"
        [[apps]]
        app_id = "org.mozilla.firefox"
        allowed = true

        [[apps]]
        app_id = "org.gnome.Nautilus"
        allowed = false
        "#;

        let allowlist = Allowlist::from_toml(raw).unwrap();
        assert!(allowlist.is_allowed("org.mozilla.firefox"));
        assert!(!allowlist.is_allowed("org.gnome.Nautilus"));
    }
}
