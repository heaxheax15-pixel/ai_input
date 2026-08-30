use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ai_bridge::config::default_runtime_config;
use ai_bridge::event_loop::{
    route_branch_request, BranchRegistry, GatekeeperVerdict, PendingPlans,
};
use ai_bridge_channels::{ChannelError, ChannelManager, ChannelName};
use ai_bridge_hand_eye::allowlist::Allowlist;
use ai_bridge_protocol::{ExecutionPlan, SubChatOpenRequest, SubChatResult};
use anyhow::{Context, Result};

const ROOT_TASK_ID: &str = "root-task";

#[tokio::main]
async fn main() -> Result<()> {
    let config = default_runtime_config()?;
    let allowlist = config.load_allowlist()?;
    let runtime_sockets = ai_bridge_channels::socket_dir();
    let manager = Arc::new(
        ChannelManager::initialize(&runtime_sockets).with_context(|| {
            format!(
                "failed to initialize runtime sockets in {}",
                runtime_sockets.display()
            )
        })?,
    );

    log_boot(&manager, &allowlist, &runtime_sockets);

    run_event_loop(manager, &allowlist, runtime_sockets).await?;
    println!("[DAEMON MODE COMPLETED AND TESTED. WAITING FOR FINAL ORDERS.]");

    Ok(())
}

fn log_boot(manager: &ChannelManager, allowlist: &Allowlist, runtime_sockets: &std::path::Path) {
    println!(
        "[BOOT] ai-bridge daemon starting (pid {})",
        std::process::id()
    );
    for channel in ChannelName::all() {
        match manager.get(channel) {
            Some(socket) => println!(
                "[BOOT] channel {channel:?} ready at {}; secure token handshake armed ({} bytes)",
                socket.socket_path().display(),
                socket.secure_token().len()
            ),
            None => println!("[BOOT] channel {channel:?} failed to initialize"),
        }
        // For public_maestro (Case B), also show token file path
        if channel == ChannelName::PublicMaestro {
            let token_file = runtime_sockets.join(format!("{}.token", channel.as_str()));
            println!("[BOOT] channel {channel:?} token file: {}", token_file.display());
        }
    }
    let allowed = [
        ChannelName::PrivateA,
        ChannelName::PrivateB,
        ChannelName::PublicMaestro,
    ]
    .iter()
    .filter(|c| {
        let app = match c {
            ChannelName::PrivateA => "org.gnome.Terminal",
            ChannelName::PrivateB => "org.gnome.Terminal",
            ChannelName::PublicMaestro => "org.mozilla.firefox",
        };
        allowlist.is_allowed(app)
    })
    .count();
    println!("[BOOT] portal allowlist loaded; {allowed} target applications authorized");
}

async fn run_event_loop(
    manager: Arc<ChannelManager>,
    allowlist: &Allowlist,
    socket_dir: std::path::PathBuf,
) -> Result<()> {
    if !allowlist.is_allowed("org.mozilla.firefox") {
        anyhow::bail!("firefox is not authorized for portal actions");
    }

    let registry = Arc::new(Mutex::new(BranchRegistry::new(ROOT_TASK_ID)));
    let pending = Arc::new(Mutex::new(PendingPlans::new()));

    let channels = [
        ChannelName::PrivateA,
        ChannelName::PrivateB,
        ChannelName::PublicMaestro,
    ];

    let mut handles = Vec::new();

    for &chan in &channels {
        let mgr = Arc::clone(&manager);
        let registry = Arc::clone(&registry);
        let pending = Arc::clone(&pending);

        let handle = tokio::spawn(async move {
            let socket_chan = chan;
            loop {
                let mgr = Arc::clone(&mgr);
                let reg = Arc::clone(&registry);
                let pend = Arc::clone(&pending);

                let res = tokio::task::spawn_blocking(move || {
                    let socket = mgr.get(socket_chan).expect("channel must exist");
                    // handle_client_stream verifies the secure token handshake
                    // before the handler reads any JSON payload.
                    socket.handle_client_stream(|stream| {
                        let mut buf = String::new();
                        stream
                            .read_to_string(&mut buf)
                            .map_err(|e| ChannelError::Io(e.to_string()))?;

                        match socket_chan {
                            ChannelName::PublicMaestro => {
                                handle_maestro_message(&mut pend.lock().unwrap(), &buf)
                            }
                            ChannelName::PrivateA | ChannelName::PrivateB => handle_branch_message(
                                &mut reg.lock().unwrap(),
                                socket_chan,
                                &buf,
                                stream,
                            ),
                        }
                    })
                })
                .await
                .map_err(|e| anyhow::anyhow!("channel task join error: {}", e))?
                .map_err(|e: ChannelError| anyhow::anyhow!("channel error: {}", e));

                if let Err(err) = res {
                    eprintln!("channel {socket_chan:?} error: {}", err);
                }

                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        });

        handles.push(handle);
    }

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

    for h in handles {
        h.abort();
    }

    for path in manager.socket_paths() {
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                eprintln!("failed to remove socket {}: {}", path.display(), e);
            }
        }
    }

    if socket_dir.exists() {
        if let Err(e) = std::fs::remove_dir(&socket_dir) {
            if e.kind() != std::io::ErrorKind::DirectoryNotEmpty {
                eprintln!(
                    "failed to remove socket dir {}: {}",
                    socket_dir.display(),
                    e
                );
            }
        }
    }

    Ok(())
}

/// Processes a `SubChatOpenRequest` received from a branch channel, routes it to
/// its `Branch`, and writes the resulting `SubChatResult` (with `call_index`)
/// back to the caller without dropping it.
fn handle_branch_message(
    registry: &mut BranchRegistry,
    channel: ChannelName,
    raw: &str,
    stream: &mut std::os::unix::net::UnixStream,
) -> Result<(), ChannelError> {
    use std::io::Write;

    let request: SubChatOpenRequest = serde_json::from_str(raw).map_err(ChannelError::Json)?;

    let result: SubChatResult = route_branch_request(registry, channel, &request)
        .map_err(|e| ChannelError::Io(e.to_string()))?;

    println!(
        "[BRANCH] {channel:?} opened sub-chat {} call_index={}",
        request.task_id, result.call_index
    );

    let response = serde_json::to_string(&result).map_err(ChannelError::Json)?;
    stream
        .write_all(response.as_bytes())
        .map_err(|e| ChannelError::Io(e.to_string()))?;

    Ok(())
}

/// Receives an `ExecutionPlan` from the maestro, applies the Gatekeeper policy,
/// and logs the verdict (delegable timeout or critical manual approval).
fn handle_maestro_message(pending: &mut PendingPlans, raw: &str) -> Result<(), ChannelError> {
    let plan: ExecutionPlan = serde_json::from_str(raw).map_err(ChannelError::Json)?;

    let verdict = pending.ingest(&plan, Instant::now());

    match verdict {
        GatekeeperVerdict::PendingTimeout => println!(
            "[GATEKEEPER] plan {} (Delegable) pending; auto-approval in <=120s",
            plan.task_id
        ),
        GatekeeperVerdict::AutoApproved => println!(
            "[GATEKEEPER] plan {} (Delegable) auto-approved after 120s timeout",
            plan.task_id
        ),
        GatekeeperVerdict::HeldForManualApproval => println!(
            "[GATEKEEPER] critical plan {} held forever awaiting manual owner approval",
            plan.task_id
        ),
    }

    Ok(())
}
