use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};

use crate::admin_config::audit_dir;

/// A single immutable audit log entry stored as one JSON line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditEntry {
    pub timestamp: String,
    pub operator: String,
    pub changed_file: String,
    pub action: String,
    pub old_value: String,
    pub new_value: String,
    pub security_lowering: bool,
}

/// Append-only JSONL audit log. Each file covers one month:
/// `audit-YYYY-MM.jsonl`.
pub struct AuditLog {
    dir: PathBuf,
}

impl AuditLog {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Use the default audit directory (`~/.config/ai-bridge/audit/`).
    pub fn default_dir() -> Self {
        Self::new(audit_dir())
    }

    /// Append a single entry. Creates the directory and file if needed.
    pub fn append(&self, entry: &AuditEntry) -> Result<(), std::io::Error> {
        fs::create_dir_all(&self.dir)?;
        let filename = self.current_month_filename();
        let path = self.dir.join(&filename);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        let mut line = serde_json::to_vec(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        line.push(b'\n');
        file.write_all(&line)
    }

    /// Read all entries from the current month's log file.
    pub fn read_current_month(&self) -> Result<Vec<AuditEntry>, std::io::Error> {
        let path = self.dir.join(self.current_month_filename());
        self.read_file(&path)
    }

    /// Read all entries from all log files in the directory.
    pub fn read_all(&self) -> Result<Vec<AuditEntry>, std::io::Error> {
        let mut entries = Vec::new();
        if !self.dir.exists() {
            return Ok(entries);
        }
        let mut files: Vec<PathBuf> = fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|e| e == "jsonl").unwrap_or(false))
            .collect();
        files.sort();
        for file in files {
            entries.extend(self.read_file(&file)?);
        }
        Ok(entries)
    }

    /// Read entries from a specific file, one JSON object per line.
    fn read_file(&self, path: &Path) -> Result<Vec<AuditEntry>, std::io::Error> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(path)?;
        let mut entries = Vec::new();
        for (line_no, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<AuditEntry>(trimmed) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    eprintln!(
                        "Warning: malformed audit entry at {}:{}: {}",
                        path.display(),
                        line_no + 1,
                        e
                    );
                }
            }
        }
        Ok(entries)
    }

    fn current_month_filename(&self) -> String {
        let now: DateTime<Utc> = Utc::now();
        format!("audit-{:04}-{:02}.jsonl", now.year(), now.month())
    }
}

/// Create a formatted timestamp string for audit entries (ISO 8601 UTC).
pub fn timestamp_iso8601() -> String {
    let now: DateTime<Utc> = Utc::now();
    now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn append_and_read_current_month() {
        let dir = std::env::temp_dir().join("ai_bridge_audit_test");
        let _ = fs::remove_dir_all(&dir);

        let log = AuditLog::new(&dir);
        let entry = AuditEntry {
            timestamp: "2026-08-31T12:00:00Z".to_string(),
            operator: "admin".to_string(),
            changed_file: "allowlist.toml".to_string(),
            action: "add".to_string(),
            old_value: "[]".to_string(),
            new_value: "[[binaries]]\nname = \"cargo\"\nallowed = true".to_string(),
            security_lowering: false,
        };

        log.append(&entry).unwrap();
        let entries = log.read_current_month().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].operator, "admin");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_all_returns_sorted_entries() {
        let dir = std::env::temp_dir().join("ai_bridge_audit_test_sorted");
        let _ = fs::remove_dir_all(&dir);

        let log = AuditLog::new(&dir);
        for i in 0..3 {
            log.append(&AuditEntry {
                timestamp: format!("2026-08-31T12:00:0{}Z", i),
                operator: "admin".to_string(),
                changed_file: "test.toml".to_string(),
                action: "edit".to_string(),
                old_value: String::new(),
                new_value: String::new(),
                security_lowering: false,
            })
            .unwrap();
        }

        let entries = log.read_all().unwrap();
        assert_eq!(entries.len(), 3);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn timestamp_iso8601_is_valid_rfc3339() {
        let ts = timestamp_iso8601();
        // Should parse as valid RFC3339
        let parsed: DateTime<Utc> = ts.parse().expect("timestamp should be valid RFC3339");
        // Should be very close to now (within a few seconds)
        let now = Utc::now();
        let diff = (parsed - now).num_seconds().abs();
        assert!(diff < 5, "timestamp should be within 5 seconds of now, got diff={diff}s");
    }

    #[test]
    fn audit_entry_preserves_old_and_new_values() {
        let dir = std::env::temp_dir().join("ai_bridge_audit_test_old_new");
        let _ = fs::remove_dir_all(&dir);

        let log = AuditLog::new(&dir);
        let old_config = r#"[[binaries]]
name = "cargo"
allowed = true
"#;
        let new_config = r#"[[binaries]]
name = "cargo"
allowed = true
[[binaries]]
name = "rm"
allowed = true
"#;
        let entry = AuditEntry {
            timestamp: timestamp_iso8601(),
            operator: "admin".to_string(),
            changed_file: "executor_allowlist.toml".to_string(),
            action: "add".to_string(),
            old_value: old_config.to_string(),
            new_value: new_config.to_string(),
            security_lowering: false,
        };

        log.append(&entry).unwrap();
        let entries = log.read_current_month().unwrap();
        assert_eq!(entries.len(), 1);
        let read_entry = &entries[0];
        assert_eq!(read_entry.old_value, old_config);
        assert_eq!(read_entry.new_value, new_config);
        assert_ne!(read_entry.old_value, read_entry.new_value);
        assert!(!read_entry.old_value.is_empty());
        assert!(!read_entry.new_value.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
