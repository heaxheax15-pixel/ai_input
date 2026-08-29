use nix::unistd::getuid;
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const SECURE_TOKEN_ENV_VAR: &str = "AI_BRIDGE_SECURE_TOKEN";
pub const HANDSHAKE_PREFIX: &str = "AI_BRIDGE_TOKEN:";

/// A randomly generated, high-entropy bearer token used as the first
/// (handshake) message exchanged over a channel socket. Because the maestro
/// and branches run under the same Linux UID, SO_PEERCRED alone cannot fully
/// distinguish processes; the secure token adds a per-process secret that the
/// bridge verifies before any JSON payload is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecureToken(String);

impl SecureToken {
    pub fn generate() -> Result<Self, ChannelError> {
        let mut buf = [0u8; 32];
        getrandom::getrandom(&mut buf)
            .map_err(|err| ChannelError::TokenGeneration(err.to_string()))?;
        Ok(Self(hex::encode(buf)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Injects this token into the shared process environment variable.
    pub fn inject_env(&self) {
        std::env::set_var(SECURE_TOKEN_ENV_VAR, self.as_str());
    }
}

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
    #[error("failed to generate secure token: {0}")]
    TokenGeneration(String),
    #[error("secure token handshake failed: supplied token does not match the registered one for this channel")]
    HandshakeMismatch,
    #[error("failed to write secure token handshake: {0}")]
    HandshakeWrite(String),
    #[error("failed to read secure token handshake: {0}")]
    HandshakeRead(String),
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
        Ok(serde_json::to_string(&self)?)
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
    secure_token: String,
}

impl BridgeSocket {
    pub fn bind(name: ChannelName, socket_dir: impl AsRef<Path>) -> Result<Self, ChannelError> {
        let socket_dir = socket_dir.as_ref();
        fs::create_dir_all(socket_dir).map_err(|err| ChannelError::PathAccess(err.to_string()))?;

        // Restrict the socket directory to the current user
        if let Err(e) = fs::set_permissions(socket_dir, std::fs::Permissions::from_mode(0o700)) {
            // Non-fatal; continue but emit a warning via stderr
            eprintln!(
                "warning: failed to set permissions on socket dir {}: {}",
                socket_dir.display(),
                e
            );
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
            eprintln!(
                "warning: failed to set permissions on socket {}: {}",
                path.display(),
                e
            );
        }

        let registered_uid = getuid().as_raw() as u32;
        let secure_token = SecureToken::generate()?.as_str().to_string();
        Ok(Self {
            name,
            path,
            listener,
            registered_uid,
            secure_token,
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

    /// The secure token that must be presented as the first handshake message
    /// by a connecting peer before any JSON payload is accepted.
    pub fn secure_token(&self) -> &str {
        &self.secure_token
    }

    fn accept_stream(&self) -> Result<UnixStream, ChannelError> {
        let (stream, _) = self
            .listener
            .accept()
            .map_err(|err| ChannelError::SocketAccept(err.to_string()))?;
        let raw = getsockopt(&stream, PeerCredentials)
            .map_err(|err| ChannelError::Io(err.to_string()))?;
        let actual = raw.uid() as u32;

        if actual != self.registered_uid {
            return Err(ChannelError::PeerPidMismatch {
                expected: self.registered_uid,
                actual,
            });
        }

        Ok(stream)
    }

    fn verify_handshake_token(&self, received: &str) -> Result<(), ChannelError> {
        if received != self.secure_token {
            return Err(ChannelError::HandshakeMismatch);
        }
        Ok(())
    }

    /// Accepts a connection, validates the peer UID, then reads and verifies the
    /// secure token handshake. Returns the peer identity on success.
    pub fn accept_handshake(&self) -> Result<PeerIdentity, ChannelError> {
        let mut stream = self.accept_stream()?;
        let received = read_handshake_token(&mut stream)?;
        self.verify_handshake_token(&received)?;
        Ok(PeerIdentity {
            uid: self.registered_uid,
        })
    }

    pub fn accept_peer(&self) -> Result<PeerIdentity, ChannelError> {
        let _stream = self.accept_stream()?;
        Ok(PeerIdentity {
            uid: self.registered_uid,
        })
    }

    pub fn handle_client_stream<F, T>(&self, handler: F) -> Result<T, ChannelError>
    where
        F: FnOnce(&mut UnixStream) -> Result<T, ChannelError>,
    {
        let mut stream = self.accept_stream()?;
        let received = read_handshake_token(&mut stream)?;
        self.verify_handshake_token(&received)?;
        handler(&mut stream)
    }

    pub fn validate_peer_uid(expected: u32, actual: u32) -> Result<(), ChannelError> {
        if expected != actual {
            Err(ChannelError::PeerPidMismatch { expected, actual })
        } else {
            Ok(())
        }
    }
}

/// Writes the secure token as the first handshake message over the stream,
/// before any JSON payload is sent.
pub fn write_handshake(stream: &mut UnixStream, token: &str) -> Result<(), ChannelError> {
    let line = format!("{HANDSHAKE_PREFIX}{token}\n");
    stream
        .write_all(line.as_bytes())
        .map_err(|err| ChannelError::HandshakeWrite(err.to_string()))
}

/// Reads the first (handshake) line from the stream and returns the token that
/// was presented by the peer.
fn read_handshake_token(stream: &mut UnixStream) -> Result<String, ChannelError> {
    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    reader
        .read_line(&mut line)
        .map_err(|err| ChannelError::HandshakeRead(err.to_string()))?;
    let trimmed = line.trim_end();
    trimmed
        .strip_prefix(HANDSHAKE_PREFIX)
        .map(str::to_string)
        .ok_or_else(|| {
            ChannelError::HandshakeRead("malformed handshake: missing token prefix".to_string())
        })
}

/// Connects to a channel socket, validates the peer UID, and presents the secure
/// token handshake before any JSON payload is exchanged.
pub fn connect(path: impl AsRef<Path>, token: &str) -> Result<UnixStream, ChannelError> {
    let mut stream =
        UnixStream::connect(path.as_ref()).map_err(|err| ChannelError::Io(err.to_string()))?;
    write_handshake(&mut stream, token)?;
    Ok(stream)
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
        self.sockets
            .values()
            .map(|socket| socket.socket_path().to_path_buf())
            .collect()
    }

    /// Injects each channel's secure token into the process environment at boot,
    /// so a spawned peer process can retrieve the token it must present as the
    /// handshake before exchanging any JSON payload.
    pub fn inject_tokens_into_env(&self) {
        for (channel, socket) in &self.sockets {
            let var = format!("{}_{}", SECURE_TOKEN_ENV_VAR, channel.as_str());
            std::env::set_var(var, socket.secure_token());
            if *channel == ChannelName::PublicMaestro {
                std::env::set_var(SECURE_TOKEN_ENV_VAR, socket.secure_token());
            }
        }
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
            Err(ChannelError::ForbiddenField {
                field: "auth_token"
            })
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
            Err(ChannelError::PeerPidMismatch {
                expected: _,
                actual: _
            })
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
        assert!(paths
            .iter()
            .any(|path| path.ends_with("public_maestro.sock")));
    }

    #[test]
    fn secure_token_is_unique_per_generated_instance() {
        let a = SecureToken::generate().unwrap();
        let b = SecureToken::generate().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.as_str().len(), 64);
    }

    #[test]
    fn handshake_succeeds_with_correct_token() {
        let dir = tempdir().unwrap();
        let socket = BridgeSocket::bind(ChannelName::PrivateA, dir.path()).unwrap();
        let token = socket.secure_token().to_string();
        let path = socket.socket_path().to_path_buf();

        let server = std::thread::spawn(move || socket.accept_handshake());
        let mut stream = connect(&path, &token).unwrap();
        let _ = &mut stream;

        let peer = server.join().unwrap().expect("handshake should succeed");
        assert_eq!(peer.uid, getuid().as_raw() as u32);
    }

    #[test]
    fn handshake_fails_with_wrong_token() {
        let dir = tempdir().unwrap();
        let socket = BridgeSocket::bind(ChannelName::PrivateA, dir.path()).unwrap();
        let path = socket.socket_path().to_path_buf();

        let server = std::thread::spawn(move || socket.accept_handshake());
        let mut stream = connect(&path, "wrong-token").unwrap();
        let _ = &mut stream;

        let result = server.join().unwrap();
        assert!(matches!(result, Err(ChannelError::HandshakeMismatch)));
    }

    #[test]
    fn handle_client_stream_rejects_wrong_handshake_before_processing() {
        let dir = tempdir().unwrap();
        let socket = BridgeSocket::bind(ChannelName::PrivateA, dir.path()).unwrap();
        let path = socket.socket_path().to_path_buf();

        let server = std::thread::spawn(move || -> Result<(), ChannelError> {
            socket.handle_client_stream::<_, ()>(|_stream| {
                // This should never be reached because the handshake fails first.
                unreachable!("handler must not run when handshake fails");
            })
        });

        let mut stream = UnixStream::connect(&path).unwrap();
        write_handshake(&mut stream, "wrong-token").unwrap();

        let result = server.join().unwrap();
        assert!(matches!(result, Err(ChannelError::HandshakeMismatch)));
    }
}
