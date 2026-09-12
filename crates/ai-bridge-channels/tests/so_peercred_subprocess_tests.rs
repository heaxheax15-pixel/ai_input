//! Subprocess regression tests for the `SO_PEERCRED` identity check.
//!
//! The historical bug was an incorrectly-scoped identity comparison that
//! rejected every legitimate client. The correct check validates the peer's
//! **UID** (kernel `SO_PEERCRED`), never the PID, so a genuinely separate
//! process (different PID, same UID) must be accepted — and a process with a
//! different UID (or wrong handshake token) must be rejected.
//!
//! These tests spawn a **real OS subprocess** (`channel_test_client`) so the
//! client has a genuinely different PID from the server side.

use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::thread;

use ai_bridge_channels::{ChannelError, PeerIdentity, connect};
use nix::unistd::getuid;

/// Path to the compiled test-client helper binary (available via cargo's
/// CARGO_BIN_EXE_<name> env var for integration tests).
fn client_bin() -> &'static str {
    env!("CARGO_BIN_EXE_channel_test_client")
}

fn my_uid() -> u32 {
    getuid().as_raw() as u32
}

/// Spawns the helper as a separate process that connects as a client and sends
/// the given token handshake (and optional payload). Returns the child.
fn spawn_client(socket_path: &str, token: &str, payload: Option<&str>) -> std::process::Child {
    let mut cmd = Command::new(client_bin());
    cmd.args([socket_path, token]);
    if let Some(p) = payload {
        cmd.arg(p);
    }
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    cmd.spawn().expect("failed to spawn test client subprocess")
}

/// Binds a PublicMaestro socket and spawns an accepting thread that validates
/// UID + handshake. Returns (socket_dir_path, socket_path, token, server_handle).
fn bind_public_maestro() -> (
    tempfile::TempDir,
    String,
    std::thread::JoinHandle<Result<PeerIdentity, ChannelError>>,
    String,
) {
    let dir = tempfile::tempdir().unwrap();
    let socket =
        ai_bridge_channels::BridgeSocket::bind(ai_bridge_channels::ChannelName::PublicMaestro, dir.path())
            .unwrap();
    let token = socket.secure_token().to_string();
    let path = socket.socket_path().to_str().unwrap().to_string();
    let handle = thread::spawn(move || socket.accept_handshake());
    (dir, path, handle, token)
}

#[test]
fn separate_process_same_uid_is_accepted() {
    let (_dir, path, server, token) = bind_public_maestro();

    // Spawn a genuinely separate process (different PID, same UID).
    let mut child = spawn_client(&path, &token, None);
    let status = child.wait().expect("child wait failed");
    let peer = server.join().expect("server thread panicked");

    assert!(status.success(), "client subprocess should exit 0");
    let peer = peer.expect("UID-based accept must succeed for a same-UID separate process");
    // The returned identity carries the registered UID (the daemon's own UID).
    assert_eq!(peer.uid, my_uid());
}

#[test]
fn separate_process_with_wrong_token_is_rejected() {
    let (_dir, path, server, _token) = bind_public_maestro();

    // Same UID but wrong handshake token: UID check passes, handshake rejects.
    let mut child = spawn_client(&path, "definitely-not-the-token", None);
    let status = child.wait().expect("child wait failed");
    let result = server.join().expect("server thread panicked");

    assert!(status.success(), "client writes its handshake fine even if wrong");
    match result {
        Err(ChannelError::HandshakeMismatch) => {}
        other => panic!("expected HandshakeMismatch, got {:?}", other),
    }
}

#[test]
fn uid_mismatch_is_rejected_directly() {
    // Direct check that a different UID is rejected (the kernel-level gate).
    let mine = my_uid();
    let other = if mine == 65534 { 0 } else { 65534 };
    assert!(ai_bridge_channels::BridgeSocket::validate_peer_uid(mine, mine).is_ok());
    assert!(matches!(
        ai_bridge_channels::BridgeSocket::validate_peer_uid(mine, other),
        Err(ChannelError::PeerUidMismatch { .. })
    ));
}

#[test]
fn in_process_connect_still_works_backward_compat() {
    // Same-process (thread) client also connects fine via the library helper.
    let dir = tempfile::tempdir().unwrap();
    let socket =
        ai_bridge_channels::BridgeSocket::bind(ai_bridge_channels::ChannelName::PrivateB, dir.path())
            .unwrap();
    let token = socket.secure_token().to_string();
    let path = socket.socket_path().to_path_buf();
    let server = thread::spawn(move || socket.accept_handshake());
    let mut stream: UnixStream = connect(&path, &token).unwrap();
    let _ = &mut stream;
    let peer = server.join().unwrap().expect("in-process connect should succeed");
    assert_eq!(peer.uid, my_uid());
}