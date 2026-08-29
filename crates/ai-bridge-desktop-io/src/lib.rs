//! # ai-bridge-desktop-io
//!
//! A local-state reading module that replaces the previous OCR-based reading of
//! chat replies. Instead of processing pixels, it reads structured records that
//! chat applications persist to conventional on-disk stores (`SQLite`, cache or
//! JSON files) for the maestro, branch A and branch B.
//!
//! ## Security posture
//!
//! All reads are confined to conventional local storage paths and refuse to
//! traverse outside the explicitly provided store directory. This mirrors the
//! "eye reads storage, not pixels" policy of the hand-eye layer.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The chat identity for which a reply is being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChatIdentity {
    Maestro,
    A,
    B,
}

impl ChatIdentity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Maestro => "maestro",
            Self::A => "A",
            Self::B => "B",
        }
    }
}

impl std::fmt::Display for ChatIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A single chat record read from the on-disk store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatRecord {
    pub identity: ChatIdentity,
    pub task_id: String,
    pub content: String,
}

/// Errors that can occur while reading local chat state.
#[derive(Debug)]
pub enum DesktopIoError {
    /// The supplied path is not confined to the allowed local store directory.
    UnconfinedPath(PathBuf),
    /// The underlying filesystem operation failed.
    Io(std::io::Error),
    /// The file could not be parsed as a supported store format.
    Parse(String),
    /// The record could not be decoded from its representation.
    Decode(String),
}

impl std::fmt::Display for DesktopIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnconfinedPath(path) => {
                write!(f, "path is outside the allowed store: {}", path.display())
            }
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Parse(msg) => write!(f, "unable to parse store: {msg}"),
            Self::Decode(msg) => write!(f, "unable to decode record: {msg}"),
        }
    }
}

impl std::error::Error for DesktopIoError {}

impl From<std::io::Error> for DesktopIoError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// The conventional storage path for a given chat identity. By default this is
/// derived from a per-identity directory under the user's local data root.
pub struct StorePaths {
    root: PathBuf,
}

impl StorePaths {
    pub fn under(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The JSON records file for an identity (e.g. `maestro/messages.json`).
    pub fn json_records(&self, identity: ChatIdentity) -> PathBuf {
        self.root.join(identity.as_str()).join("messages.json")
    }

    /// The SQLite store for an identity (e.g. `maestro/chat.db`).
    pub fn sqlite_records(&self, identity: ChatIdentity) -> PathBuf {
        self.root.join(identity.as_str()).join("chat.db")
    }
}

/// Confines a candidate path to the store root, refusing traversal outside of it.
fn confine(base: &Path, candidate: &Path) -> Result<PathBuf, DesktopIoError> {
    let base_abs = base.canonicalize().map_err(DesktopIoError::Io)?;
    let candidate_abs = candidate.canonicalize().unwrap_or_else(|_| {
        if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(candidate)
        }
    });
    if candidate_abs.starts_with(&base_abs) {
        Ok(candidate_abs)
    } else {
        Err(DesktopIoError::UnconfinedPath(candidate.to_path_buf()))
    }
}

/// Reads live chat records for an identity directly from the conventional local
/// store. The store is auto-detected: JSON files are preferred, falling back to
/// an SQLite `chat_messages` table when present.
pub fn read_live_records(
    root: impl AsRef<Path>,
    identity: ChatIdentity,
) -> Result<Vec<ChatRecord>, DesktopIoError> {
    let paths = StorePaths::under(root);

    let json_path = paths.json_records(identity);
    if json_path.exists() {
        let confined = confine(paths.root(), &json_path)?;
        return read_json_records(&confined, identity);
    }

    let sqlite_path = paths.sqlite_records(identity);
    if sqlite_path.exists() {
        let confined = confine(paths.root(), &sqlite_path)?;
        return read_sqlite_records(&confined, identity);
    }

    Ok(Vec::new())
}

/// Reads chat records from a newline-delimited or array JSON file. Safe for any
/// conventional local JSON record store.
pub fn read_json_records(
    path: impl AsRef<Path>,
    identity: ChatIdentity,
) -> Result<Vec<ChatRecord>, DesktopIoError> {
    let raw = fs::read_to_string(path)?;

    let entries: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| DesktopIoError::Parse(e.to_string()))?;

    let mut records = Vec::new();
    match entries {
        serde_json::Value::Array(items) => {
            for item in items {
                records.push(decode_json_record(item, identity)?);
            }
        }
        serde_json::Value::String(line) => {
            for line in line.lines() {
                let value: serde_json::Value = serde_json::from_str(line)
                    .map_err(|e| DesktopIoError::Decode(e.to_string()))?;
                records.push(decode_json_record(value, identity)?);
            }
        }
        other => {
            records.push(decode_json_record(other, identity)?);
        }
    }

    Ok(records)
}

fn decode_json_record(
    value: serde_json::Value,
    identity: ChatIdentity,
) -> Result<ChatRecord, DesktopIoError> {
    let record: ChatRecord =
        serde_json::from_value(value).map_err(|e| DesktopIoError::Decode(e.to_string()))?;

    if record.identity != identity {
        return Err(DesktopIoError::Decode(format!(
            "record identity {:?} does not match requested identity {:?}",
            record.identity, identity
        )));
    }

    Ok(record)
}

/// Reads chat records from an SQLite store using the `chat_messages` table:
/// `(task_id TEXT, content TEXT)`. The identity column is optional and, when a
/// strict schema is used, must match the requested identity.
pub fn read_sqlite_records(
    path: impl AsRef<Path>,
    identity: ChatIdentity,
) -> Result<Vec<ChatRecord>, DesktopIoError> {
    let conn =
        rusqlite::Connection::open(path).map_err(|e| DesktopIoError::Parse(e.to_string()))?;

    let has_identity = conn
        .prepare("SELECT 1 FROM pragma_table_info('chat_messages') WHERE name='identity' LIMIT 1")
        .and_then(|mut stmt| stmt.exists([]))
        .unwrap_or(false);

    let mut stmt = if has_identity {
        conn.prepare("SELECT identity, task_id, content FROM chat_messages")
    } else {
        conn.prepare("SELECT task_id, content FROM chat_messages")
    }
    .map_err(|e| DesktopIoError::Parse(e.to_string()))?;

    let rows = stmt
        .query_map([], |row| {
            let task_id: String = row.get(if has_identity { 1 } else { 0 })?;
            let content: String = row.get(if has_identity { 2 } else { 1 })?;
            let identity_col: Option<String> = if has_identity {
                Some(row.get(0)?)
            } else {
                None
            };
            Ok((identity_col, task_id, content))
        })
        .map_err(|e| DesktopIoError::Parse(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| DesktopIoError::Parse(e.to_string()))?;

    let mut records = Vec::new();
    for (identity_col, task_id, content) in rows {
        if let Some(identity_col) = identity_col {
            let col_identity = match identity_col.as_str() {
                "maestro" => ChatIdentity::Maestro,
                "A" | "a" => ChatIdentity::A,
                "B" | "b" => ChatIdentity::B,
                other => {
                    return Err(DesktopIoError::Decode(format!(
                        "unknown identity column value: {other}"
                    )));
                }
            };
            if col_identity != identity {
                continue;
            }
        }
        records.push(ChatRecord {
            identity,
            task_id,
            content,
        });
    }

    Ok(records)
}

/// The conventional storage root for chat applications on this system. Uses the
/// XDG data home (or a fallback of `~/.local/share`) as required locally.
pub fn default_store_root() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("ai-bridge-chat");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("ai-bridge-chat");
        }
    }
    PathBuf::from("/tmp/ai-bridge-chat")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn reads_json_records_for_an_identity() {
        let dir = tempdir().unwrap();
        let paths = StorePaths::under(dir.path());
        let file = paths.json_records(ChatIdentity::Maestro);

        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(
            &file,
            serde_json::to_string(&vec![
                ChatRecord {
                    identity: ChatIdentity::Maestro,
                    task_id: "task-1".to_string(),
                    content: "ready".to_string(),
                },
                ChatRecord {
                    identity: ChatIdentity::Maestro,
                    task_id: "task-2".to_string(),
                    content: "done".to_string(),
                },
            ])
            .unwrap(),
        )
        .unwrap();

        let records = read_live_records(dir.path(), ChatIdentity::Maestro).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].content, "ready");
    }

    #[test]
    fn rejects_records_mismatching_requested_identity() {
        let dir = tempdir().unwrap();
        let paths = StorePaths::under(dir.path());
        let file = paths.json_records(ChatIdentity::A);

        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(
            &file,
            serde_json::to_string(&vec![ChatRecord {
                identity: ChatIdentity::Maestro,
                task_id: "task-9".to_string(),
                content: "wrong identity".to_string(),
            }])
            .unwrap(),
        )
        .unwrap();

        assert!(matches!(
            read_json_records(&file, ChatIdentity::A),
            Err(DesktopIoError::Decode(_))
        ));
    }

    #[test]
    fn empty_store_yields_no_records() {
        let dir = tempdir().unwrap();
        let records = read_live_records(dir.path(), ChatIdentity::B).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn path_is_confined_to_store_root() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&store).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("placeholder"), b"x").unwrap();

        let outside_file = outside.join("chat.db");
        assert!(matches!(
            confine(&store, &outside_file),
            Err(DesktopIoError::UnconfinedPath(_))
        ));
    }
}
