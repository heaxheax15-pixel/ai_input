//! End-to-end IPC integration test proving the full Gatekeeper flow with 100%
//! Rust: an agent submits a pending task, a UI connects over the Unix socket,
//! reads the advertised `task_pending` line, writes back a `TaskResolved`
//! decision, and the agent's decision channel resolves with `true`.
//!
//! Every component here is exercised over a real `tokio::net::UnixStream`, bound
//! to a unique temporary socket path. No external tools, no scripts.

use ai_bridge_gatekeeper::gatekeeper::{serve_ui_session, ActiveTasks, PendingTask};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;
use tokio::time::timeout;

/// Returns a socket path unique to this process and instant, so a stale socket
/// from a crashed earlier run can never collide.
fn unique_socket_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    PathBuf::from(format!(
        "/tmp/ai_bridge_gatekeeper_e2e_{}_{}.sock",
        std::process::id(),
        nanos
    ))
}

/// Drives the daemon loop: accept UI connections and service each session until
/// it disconnects.
async fn daemon_loop(listener: &UnixListener, active: &mut ActiveTasks) -> std::io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let _ = serve_ui_session(stream, active).await;
    }
}

#[tokio::test]
async fn ui_decision_unblocks_agent_over_unix_socket() {
    let socket = unique_socket_path();
    if socket.exists() {
        let _ = std::fs::remove_file(&socket);
    }

    let listener = UnixListener::bind(&socket).expect("gatekeeper must bind its listener");

    // --- Agent side -------------------------------------------------------
    // The agent submits a command that is blocked, and holds the receiver of
    // the channel that will unblock (or abort) its execution.
    let (tx, rx) = oneshot::channel();
    let mut active = ActiveTasks::new();
    active.register(
        PendingTask {
            task_id: "TASK-42".to_string(),
            app_name: "org.gnome.Terminal".to_string(),
            command: "rm -rf /tmp/scratch".to_string(),
            risk_level: "HIGH".to_string(),
            is_critical: true,
        },
        tx,
    );

    // --- Spawn the Gatekeeper daemon on the unique temp socket -------------
    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        tokio::select! {
            _ = &mut stop_rx => {}
            result = daemon_loop(&listener, &mut active) => {
                // The listener failing is the only way the loop returns; report
                // it so the test surface is visible if it happens.
                if let Err(err) = result {
                    eprintln!("gatekeeper daemon exited: {err}");
                }
            }
        }
        // Return the registry so the test can assert it was drained.
        active
    });

    // --- UI side ----------------------------------------------------------
    // Connect, read the advertised pending task, approve it, and disconnect.
    // The decision JSON is exactly the flat `{task_id, approved}` wire form
    // produced by `ai_bridge_ui::OutboundEvent::TaskResolved` under
    // `#[serde(untagged)]`.
    let ui = async {
        let stream = UnixStream::connect(&socket)
            .await
            .expect("UI must connect to the gatekeeper socket");
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        let line = lines
            .next_line()
            .await
            .expect("stream must yield a line")
            .expect("stream closed before any pending task was advertised");
        let value: serde_json::Value =
            serde_json::from_str(&line).expect("pending line must be valid JSON");
        assert_eq!(value["type"], "task_pending");
        assert_eq!(value["task_id"], "TASK-42");
        assert_eq!(value["app_name"], "org.gnome.Terminal");
        assert_eq!(value["risk_level"], "HIGH");

        let decision = json!({ "task_id": "TASK-42", "approved": true }).to_string();
        writer
            .write_all(format!("{decision}\n").as_bytes())
            .await
            .expect("UI must write its decision");
        writer
            .shutdown()
            .await
            .expect("UI must cleanly close its write half");
    };

    // Run the agent's wait and the UI concurrently; the agent only completes
    // once the daemon has routed the UI's approval back through its channel.
    let (agent_decision, _) = tokio::join!(
        async {
            timeout(Duration::from_secs(5), rx)
                .await
                .expect("timed out waiting for the agent's decision channel")
        },
        ui,
    );

    let decision = agent_decision.expect("agent decision channel must not be dropped");
    assert!(decision, "the UI's approval must reach the agent as `true`");

    // --- Tear down --------------------------------------------------------
    let _ = stop_tx.send(());
    let drained = server.await.expect("daemon task must join cleanly");
    assert!(
        drained.is_empty(),
        "the resolved task must be removed from the registry"
    );

    let _ = std::fs::remove_file(&socket);
}
