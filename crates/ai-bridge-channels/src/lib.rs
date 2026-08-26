use std::collections::BTreeMap;
use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::os::unix::fs::PermissionsExt;
use nix::unistd::getuid;

use nix::sys::socket::{
    getsockopt,
    sockopt::PeerCredentials,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChannelName {
    PrivateA,
    PrivateB,
    PublicMaestro,
}

impl ChannelName {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PrivateA => "private_a.sock",
            Self::PrivateB => "private_b.sock",
            Self::PublicMaestro => "public_maestro.sock",
        }
    }

    pub fn all() -> [Self; 3] {
        [Self::PrivateA, Self::PrivateB, Self::PublicMaestro]
    }
}

#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("failed to bind socket at {path}: {source}")]
    SocketBind {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to accept connection: {0}")]
    SocketAccept(String),
    #[error("failed to read or write on the socket: {0}")]
    SocketIo(String),
    #[error("failed to parse JSON payload: {0}")]
    Json(#[from] serde_json::Error),
    #[error("socket was created for UID {expected}, but peer UID was {actual}")]
    PeerPidMismatch { expected: u32, actual: u32 },
    #[error("message payload contains a forbidden identity field: {field}")]
    ForbiddenField { field: &'static str },
    #[error("failed to access socket path: {0}")]
    PathAccess(String),
    #[error("IO operation failed: {0}")]
    Io(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerIdentity {
    pub uid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelEnvelope {
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
}

impl ChannelEnvelope {
    pub fn new<T: Serialize>(value: &T) -> Result<Self, ChannelError> {
        let payload = serde_json::to_value(value)?;
        validate_payload(&payload)?;
        Ok(Self { payload })
    }

    pub fn from_json_str(data: &str) -> Result<Self, ChannelError> {
        let value: Value = serde_json::from_str(data)?;
        validate_payload(&value)?;
        Ok(Self { payload: value })
    }

    pub fn to_json_string(&self) -> Result<String, ChannelError> {
        validate_payload(&self.payload)?;
        Ok(serde_json::to_string(&self)? )
    }
}

pub fn validate_payload(value: &Value) -> Result<(), ChannelError> {
    if let Some(field) = forbidden_identity_field(value) {
        return Err(ChannelError::ForbiddenField { field });
    }
    Ok(())
}

fn forbidden_identity_field(value: &Value) -> Option<&'static str> {
    match value {
        Value::Object(map) => {
            if map.contains_key("sender") {
                Some("sender")
            } else if map.contains_key("auth_token") {
                Some("auth_token")
            } else {
                map.values().find_map(forbidden_identity_field)
            }
        }
        Value::Array(items) => items.iter().find_map(forbidden_identity_field),
        _ => None,
    }
}

#[derive(Debug)]
pub struct BridgeSocket {
    name: ChannelName,
    path: PathBuf,
    listener: UnixListener,
    registered_uid: u32,
}

impl BridgeSocket {
    pub fn bind(name: ChannelName, socket_dir: impl AsRef<Path>) -> Result<Self, ChannelError> {
        let socket_dir = socket_dir.as_ref();
        fs::create_dir_all(socket_dir)
            .map_err(|err| ChannelError::PathAccess(err.to_string()))?;

        // Restrict the socket directory to the current user
        if let Err(e) = fs::set_permissions(socket_dir, std::fs::Permissions::from_mode(0o700)) {
            // Non-fatal; continue but emit a warning via stderr
            eprintln!("warning: failed to set permissions on socket dir {}: {}", socket_dir.display(), e);
        }

        let path = socket_dir.join(name.as_str());
        if path.exists() {
            let _ = fs::remove_file(&path);
        }

        let listener = UnixListener::bind(&path).map_err(|source| ChannelError::SocketBind {
            path: path.clone(),
            source,
        })?;

        // Restrict the socket file permissions so only the owner can connect
        if let Err(e) = fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
            eprintln!("warning: failed to set permissions on socket {}: {}", path.display(), e);
        }

        let registered_uid = getuid().as_raw() as u32;
        Ok(Self {
            name,
            path,
            listener,
            registered_uid,
        })
    }

    pub fn channel_name(&self) -> ChannelName {
        self.name
    }

    pub fn socket_path(&self) -> &Path {
        &self.path
    }

    pub fn registered_uid(&self) -> u32 {
        self.registered_uid
    }

    pub fn accept_peer(&self) -> Result<PeerIdentity, ChannelError> {
        let (stream, _) = self
            .listener
            .accept()
            .map_err(|err| ChannelError::SocketAccept(err.to_string()))?;
        let raw = getsockopt(&stream, PeerCredentials).map_err(|err| ChannelError::Io(err.to_string()))?;
        let actual = raw.uid() as u32;

        if actual != self.registered_uid {
            return Err(ChannelError::PeerPidMismatch {
                expected: self.registered_uid,
                actual,
            });
        }

        Ok(PeerIdentity { uid: actual })
    }

    pub fn handle_client_stream<F, T>(&self, handler: F) -> Result<T, ChannelError>
    where
        F: FnOnce(&mut UnixStream) -> Result<T, ChannelError>,
    {
        let (mut stream, _) = self
            .listener
            .accept()
            .map_err(|err| ChannelError::SocketAccept(err.to_string()))?;
        let raw = getsockopt(&stream, PeerCredentials).map_err(|err| ChannelError::Io(err.to_string()))?;
        let actual = raw.uid() as u32;

        if actual != self.registered_uid {
            return Err(ChannelError::PeerPidMismatch {
                expected: self.registered_uid,
                actual,
            });
        }

        handler(&mut stream)
    }

    pub fn validate_peer_uid(expected: u32, actual: u32) -> Result<(), ChannelError> {
        if expected != actual {
            Err(ChannelError::PeerPidMismatch {
                expected,
                actual,
            })
        } else {
            Ok(())
        }
    }
}

#[derive(Debug)]
pub struct ChannelManager {
    sockets: BTreeMap<ChannelName, BridgeSocket>,
}

impl ChannelManager {
    pub fn initialize(socket_dir: impl AsRef<Path>) -> Result<Self, ChannelError> {
        let socket_dir = socket_dir.as_ref();
        let mut sockets = BTreeMap::new();

        for channel in ChannelName::all() {
            let socket = BridgeSocket::bind(channel, socket_dir)?;
            sockets.insert(channel, socket);
        }

        Ok(Self { sockets })
    }

    pub fn get(&self, channel: ChannelName) -> Option<&BridgeSocket> {
        self.sockets.get(&channel)
    }

    pub fn socket_paths(&self) -> Vec<PathBuf> {
        self.sockets.values().map(|socket| socket.socket_path().to_path_buf()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn channel_names_map_to_expected_socket_files() {
        assert_eq!(ChannelName::PrivateA.as_str(), "private_a.sock");
        assert_eq!(ChannelName::PrivateB.as_str(), "private_b.sock");
        assert_eq!(ChannelName::PublicMaestro.as_str(), "public_maestro.sock");
    }

    #[test]
    fn payload_validation_rejects_sender_and_auth_token_fields() {
        let valid = serde_json::json!({"task":"ok","payload":{"value":42}});
        assert!(validate_payload(&valid).is_ok());

        let sender = serde_json::json!({"task":"bad","sender":"maestro"});
        assert!(matches!(
            validate_payload(&sender),
            Err(ChannelError::ForbiddenField { field: "sender" })
        ));

        let token = serde_json::json!({"task":"bad","auth_token":"secret"});
        assert!(matches!(
            validate_payload(&token),
            Err(ChannelError::ForbiddenField { field: "auth_token" })
        ));

        let nested = serde_json::json!({"task":"bad","meta":{"sender":"nested"}});
        assert!(matches!(
            validate_payload(&nested),
            Err(ChannelError::ForbiddenField { field: "sender" })
        ));
    }

    #[test]
    fn peer_uid_validation_matches_registered_uid() {
        let expected = getuid().as_raw() as u32;
        assert!(BridgeSocket::validate_peer_uid(expected, expected).is_ok());
        assert!(matches!(
            BridgeSocket::validate_peer_uid(expected, expected + 1),
            Err(ChannelError::PeerPidMismatch { expected: _, actual: _ })
        ));
    }

    #[test]
    fn channel_manager_initializes_all_expected_socket_paths() {
        let dir = tempdir().unwrap();
        let manager = ChannelManager::initialize(dir.path()).unwrap();

        let paths: Vec<_> = manager.socket_paths();
        assert_eq!(paths.len(), 3);
        assert!(paths.iter().any(|path| path.ends_with("private_a.sock")));
        assert!(paths.iter().any(|path| path.ends_with("private_b.sock")));
        assert!(paths.iter().any(|path| path.ends_with("public_maestro.sock")));
    }
}
