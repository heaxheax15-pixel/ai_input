//! A tiny helper binary used by the `SO_PEERCRED` regression tests.
//!
//! It runs as a **genuinely separate OS process** (a distinct PID) and connects
//! to a channel socket as a client. It is invoked by the test harness with:
//!
//!   channel_test_client <socket_path> <secure_token> [json_payload]
//!
//! It writes the secure-token handshake first (matching `Writer`'s expected
//! wire format), then any optional JSON payload line, and exits 0 on success
//! or 1 on connection/handshake-write failure.
//!
//! Because this is a different process from the daemon side, it has a different
//! PID but (in the test) the same UID — exactly the case the fixed UID-based
//! identity check must accept.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: channel_test_client <socket_path> <token> [payload]");
        exit(2);
    }
    let socket_path = &args[1];
    let token = &args[2];
    let payload = args.get(3).cloned();

    let mut stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("channel_test_client: connect failed: {e}");
            exit(1);
        }
    };

    // Present the secure-token handshake first.
    if let Err(e) = writeln!(stream, "AI_BRIDGE_TOKEN:{token}") {
        eprintln!("channel_test_client: handshake write failed: {e}");
        exit(1);
    }
    if let Some(p) = payload {
        if let Err(e) = writeln!(stream, "{p}") {
            eprintln!("channel_test_client: payload write failed: {e}");
            exit(1);
        }
    }

    exit(0);
}
