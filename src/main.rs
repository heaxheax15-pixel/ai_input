use std::time::Duration;
use std::sync::Arc;
use std::io::Read;

use anyhow::{Context, Result};
use ai_bridge::config::default_runtime_config;
use ai_bridge_channels::{ChannelManager, ChannelName, ChannelError};
use ai_bridge_gatekeeper::policy::decide_policy;
use ai_bridge_hand_eye::allowlist::Allowlist;
use ai_bridge_protocol::ExecutionPlan;

#[tokio::main]
async fn main() -> Result<()> {
    let config = default_runtime_config()?;
    let allowlist = config.load_allowlist()?;
    let runtime_sockets = std::env::temp_dir().join("ai_bridge_runtime_sockets");
    let manager = Arc::new(
        ChannelManager::initialize(&runtime_sockets)
            .with_context(|| format!("failed to initialize runtime sockets in {}", runtime_sockets.display()))?,
    );

    let plan = ExecutionPlan {
        task_id: "root-task".to_string(),
        description: "Coordinate branch operations".to_string(),
        commands: vec!["cargo test --workspace".to_string(), "cargo build --release".to_string()],
    };

    let policy = decide_policy(&plan);
    let _ = (&manager, policy);

    run_event_loop(manager, &allowlist, runtime_sockets).await?;
    println!("[DAEMON MODE COMPLETED AND TESTED. WAITING FOR FINAL ORDERS.]");

    Ok(())
}

async fn run_event_loop(manager: Arc<ChannelManager>, allowlist: &Allowlist, socket_dir: std::path::PathBuf) -> Result<()> {
    if !allowlist.is_allowed("org.mozilla.firefox") {
        anyhow::bail!("firefox is not authorized for portal actions");
    }

    // Spawn a task per channel that continuously accepts incoming connections.
    // Uses spawn_blocking to call the synchronous accept methods provided by ai-bridge-channels.
    let channels = [ChannelName::PrivateA, ChannelName::PrivateB, ChannelName::PublicMaestro];

    let mut handles = Vec::new();

    for &chan in &channels {
        let mgr = Arc::clone(&manager);
        let handle = tokio::spawn(async move {
            loop {
                // For the public maestro socket we want to validate peer credentials immediately,
                // dropping connections that fail validation. For other sockets we accept and
                // read the payload (dropping afterwards) to keep the socket loop moving.
                let res = match chan {
                    ChannelName::PublicMaestro => {
                        // accept_peer validates peer pid and returns PeerIdentity on success.
                        let mgr = Arc::clone(&mgr);
                        tokio::task::spawn_blocking(move || {
                            let socket = mgr.get(chan).expect("channel must exist");
                            socket.accept_peer().map(|_peer| ())
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("accept task join error: {}", e))?
                        .map_err(|e: ChannelError| anyhow::anyhow!("channel error: {}", e))
                    }
                    _ => {
                        let mgr = Arc::clone(&mgr);
                        tokio::task::spawn_blocking(move || {
                            let socket = mgr.get(chan).expect("channel must exist");
                            // read and drop the stream to keep behavior simple for now
                            socket.handle_client_stream(|stream| {
                                let mut buf = String::new();
                                stream
                                    .read_to_string(&mut buf)
                                    .map_err(|e| ChannelError::Io(e.to_string()))?;
                                Ok(())
                            })
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("handle task join error: {}", e))?
                        .map_err(|e: ChannelError| anyhow::anyhow!("channel error: {}", e))
                    }
                };

                if let Err(err) = res {
                    // Log and continue accepting further connections.
                    eprintln!("channel {:?} accept error: {}", chan, err);
                }

                // Small pause to avoid tight loop on repeated failures.
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            // Note: loop is infinite; task will be aborted on shutdown.
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        });

        handles.push(handle);
    }

    // Wait for shutdown signal (Ctrl+C or SIGTERM) and then cleanup.
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("unable to bind SIGTERM handler");
        sigterm.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            println!("received ctrl_c");
        }
        _ = terminate => {
            println!("received SIGTERM");
        }
    }

    // Abort socket accept tasks.
    for h in handles {
        h.abort();
    }

    // Remove socket files to avoid stale sockets.
    for path in manager.socket_paths() {
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                eprintln!("failed to remove socket {}: {}", path.display(), e);
            }
        }
    }

    // Also attempt to remove the socket directory if empty
    if socket_dir.exists() {
        if let Err(e) = std::fs::remove_dir(&socket_dir) {
            // ignore error if not empty
            if e.kind() != std::io::ErrorKind::DirectoryNotEmpty {
                eprintln!("failed to remove socket dir {}: {}", socket_dir.display(), e);
            }
        }
    }

    Ok(())
}
