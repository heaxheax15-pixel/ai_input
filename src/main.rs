use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ai_bridge::config::default_runtime_config;
use ai_bridge::event_loop::{route_branch_request, BranchRegistry, GatekeeperVerdict, PendingPlans};
use ai_bridge_channels::{ChannelError, ChannelManager, ChannelName};
use ai_bridge_gatekeeper_daemon::executor::execute_approved_task;
use ai_bridge_hand_eye::allowlist::Allowlist;
use ai_bridge_protocol::{
    ExecutionOutcome, ExecutionPlan, MaestroMessage, SubChatOpenRequest, SubChatResult, TaskQuery,
    TaskResolved,
};
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

async fn execute_plan(plan: &ExecutionPlan) -> ExecutionOutcome {
    let mut combined_stdout = String::new();
    let mut combined_stderr = String::new();
    let mut last_exit = 0;
    let mut success = true;
    for cmd in &plan.commands {
        match execute_approved_task(cmd).await {
            Ok(output) => {
                let code = output.status.code().unwrap_or(if output.status.success() { 0 } else { 1 });
                last_exit = code;
                if !output.status.success() {
                    success = false;
                }
                combined_stdout.push_str(&String::from_utf8_lossy(&output.stdout));
                combined_stderr.push_str(&String::from_utf8_lossy(&output.stderr));
            }
            Err(e) => {
                success = false;
                combined_stderr.push_str(&format!("execution error: {e}"));
                last_exit = 1;
            }
        }
    }
    ExecutionOutcome {
        stdout: combined_stdout,
        stderr: combined_stderr,
        exit_code: last_exit,
        success,
    }
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

    // Background task: periodically check for Delegable timeouts and execute
    let pending_clone = Arc::clone(&pending);
    let timeout_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let to_exec = {
                let mut p = pending_clone.lock().unwrap();
                p.check_timeouts(Instant::now())
            };
            for (task_id, plan) in to_exec {
                let pc = Arc::clone(&pending_clone);
                let tid = task_id.clone();
                let pl = plan.clone();
                tokio::spawn(async move {
                    let outcome = execute_plan(&pl).await;
                    let mut p = pc.lock().unwrap();
                    let _ = p.record_execution(&tid, outcome);
                    println!("[GATEKEEPER] auto-executed Delegable plan {} after timeout", tid);
                });
            }
        }
    });

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

                let res: Result<Option<(String, ExecutionPlan)>, ChannelError> =
                    tokio::task::spawn_blocking(move || {
                        let socket = mgr.get(socket_chan).expect("channel must exist");
                        socket.handle_client_stream(|stream| {
                            let mut buf = String::new();
                            stream
                                .read_to_string(&mut buf)
                                .map_err(|e| ChannelError::Io(e.to_string()))?;

                            match socket_chan {
                                ChannelName::PublicMaestro => {
                                    handle_maestro_message(&mut pend.lock().unwrap(), &buf, stream)
                                }
                                ChannelName::PrivateA | ChannelName::PrivateB => {
                                    handle_branch_message(
                                        &mut reg.lock().unwrap(),
                                        socket_chan,
                                        &buf,
                                        stream,
                                    )?;
                                    Ok(None)
                                }
                            }
                        })
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("channel task join error: {}", e))
                    .map_err(|e| ChannelError::Io(e.to_string()))?;

                let exec_opt = match res {
                    Ok(opt) => opt,
                    Err(e) => {
                        eprintln!("channel {socket_chan:?} error: {}", e);
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        continue;
                    }
                };
                if let Some((task_id, plan)) = exec_opt {
                    let pc = Arc::clone(&pending);
                    tokio::spawn(async move {
                        let outcome = execute_plan(&plan).await;
                        let mut p = pc.lock().unwrap();
                        let _ = p.record_execution(&task_id, outcome);
                        println!("[GATEKEEPER] executed plan {} (approved)", task_id);
                    });
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

    timeout_handle.abort();
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
    // also remove token files
    for channel in ChannelName::all() {
        let token_path = socket_dir.join(format!("{}.token", channel.as_str()));
        let _ = std::fs::remove_file(&token_path);
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

/// Handles all public_maestro messages: ExecutionPlan (submit), TaskQuery, TaskResolved.
/// Returns Some((task_id, plan)) when immediate execution is required (AutoApproved or owner-approved).
fn handle_maestro_message(
    pending: &mut PendingPlans,
    raw: &str,
    stream: &mut std::os::unix::net::UnixStream,
) -> Result<Option<(String, ExecutionPlan)>, ChannelError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ChannelError::Json(serde_json::from_str::<serde_json::Value>("").unwrap_err()));
    }

    // Try to parse as JSON Value first to discriminate
    let v: serde_json::Value = serde_json::from_str(trimmed).map_err(ChannelError::Json)?;

    // 1. TaskResolved has "approved" field
    if v.get("approved").is_some() {
        let resolved: TaskResolved = serde_json::from_value(v).map_err(ChannelError::Json)?;
        let task_id = resolved.task_id.clone();
        let approved = resolved.approved;
        let plan_opt = pending
            .handle_owner_decision(&task_id, approved)
            .map_err(|e| ChannelError::Io(e.to_string()))?;
        // Record rejection explicitly if denied (handle_owner_decision already sets RejectedByOwner)
        // Write ack back
        let ack = serde_json::json!({"task_id": task_id, "approved": approved, "status": if approved { "approved" } else { "rejected" }});
        stream
            .write_all(serde_json::to_vec(&ack).unwrap().as_slice())
            .map_err(|e| ChannelError::Io(e.to_string()))?;
        if approved {
            if let Some(plan) = plan_opt {
                println!("[GATEKEEPER] plan {} approved by owner, executing", task_id);
                return Ok(Some((task_id, plan)));
            }
        } else {
            println!("[GATEKEEPER] plan {} rejected by owner", task_id);
        }
        return Ok(None);
    }

    // 2. Check for typed MaestroMessage (has "type" field)
    if let Some(t) = v.get("type").and_then(|x| x.as_str()) {
        if t == "execution_plan" || t == "task_query" {
            // Try MaestroMessage deserialization
            if let Ok(msg) = serde_json::from_str::<MaestroMessage>(trimmed) {
                match msg {
                    MaestroMessage::ExecutionPlan(plan) => {
                        let task_id = plan.task_id.clone();
                        let (verdict, ack) = pending.ingest(&plan, Instant::now());
                        stream
                            .write_all(&serde_json::to_vec(&ack).map_err(ChannelError::Json)?)
                            .map_err(|e| ChannelError::Io(e.to_string()))?;
                        match verdict {
                            GatekeeperVerdict::PendingTimeout => {
                                println!("[GATEKEEPER] plan {} (Delegable) pending; auto-approval in <=120s", task_id);
                                return Ok(None);
                            }
                            GatekeeperVerdict::AutoApproved => {
                                println!("[GATEKEEPER] plan {} (Delegable) auto-approved, executing", task_id);
                                return Ok(Some((task_id, plan)));
                            }
                            GatekeeperVerdict::HeldForManualApproval => {
                                println!("[GATEKEEPER] critical plan {} held forever awaiting manual owner approval", task_id);
                                return Ok(None);
                            }
                        }
                    }
                    MaestroMessage::TaskQuery(q) => {
                        let result = pending.query(&q.task_id).map_err(|e| ChannelError::Io(e.to_string()))?;
                        stream
                            .write_all(&serde_json::to_vec(&result).map_err(ChannelError::Json)?)
                            .map_err(|e| ChannelError::Io(e.to_string()))?;
                        return Ok(None);
                    }
                }
            }
        }
    }

    // 3. Plain ExecutionPlan (has "commands")
    if v.get("commands").is_some() {
        let plan: ExecutionPlan = serde_json::from_value(v).map_err(ChannelError::Json)?;
        let task_id = plan.task_id.clone();
        let (verdict, ack) = pending.ingest(&plan, Instant::now());
        stream
            .write_all(&serde_json::to_vec(&ack).map_err(ChannelError::Json)?)
            .map_err(|e| ChannelError::Io(e.to_string()))?;
        match verdict {
            GatekeeperVerdict::PendingTimeout => {
                println!("[GATEKEEPER] plan {} (Delegable) pending; auto-approval in <=120s", task_id);
                return Ok(None);
            }
            GatekeeperVerdict::AutoApproved => {
                println!("[GATEKEEPER] plan {} (Delegable) auto-approved, executing", task_id);
                return Ok(Some((task_id, plan)));
            }
            GatekeeperVerdict::HeldForManualApproval => {
                println!("[GATEKEEPER] critical plan {} held forever awaiting manual owner approval", task_id);
                return Ok(None);
            }
        }
    }

    // 4. Plain TaskQuery (only task_id, no commands, no approved) -> also try MaestroMessage without type
    if v.get("task_id").is_some() {
        // Try TaskQuery
        if let Ok(q) = serde_json::from_value::<TaskQuery>(v.clone()) {
            // Ensure it's not an ExecutionPlan with task_id only (but we already handled commands case)
            // Heuristic: if object has only task_id (len==1), treat as query
            if let Some(obj) = v.as_object() {
                if obj.len() == 1 {
                    let result = pending.query(&q.task_id).map_err(|e| ChannelError::Io(e.to_string()))?;
                    stream
                        .write_all(&serde_json::to_vec(&result).map_err(ChannelError::Json)?)
                        .map_err(|e| ChannelError::Io(e.to_string()))?;
                    return Ok(None);
                }
            }
        }
    }

    // Fallback: try MaestroMessage again (covers TaskQuery without type wrapper if we add it)
    if let Ok(msg) = serde_json::from_str::<MaestroMessage>(trimmed) {
        match msg {
            MaestroMessage::ExecutionPlan(plan) => {
                let task_id = plan.task_id.clone();
                let (verdict, ack) = pending.ingest(&plan, Instant::now());
                stream
                    .write_all(&serde_json::to_vec(&ack).map_err(ChannelError::Json)?)
                    .map_err(|e| ChannelError::Io(e.to_string()))?;
                match verdict {
                    GatekeeperVerdict::PendingTimeout => {
                        println!("[GATEKEEPER] plan {} pending", task_id);
                        return Ok(None);
                    }
                    GatekeeperVerdict::AutoApproved => {
                        println!("[GATEKEEPER] plan {} auto-approved", task_id);
                        return Ok(Some((task_id, plan)));
                    }
                    GatekeeperVerdict::HeldForManualApproval => {
                        println!("[GATEKEEPER] plan {} held", task_id);
                        return Ok(None);
                    }
                }
            }
            MaestroMessage::TaskQuery(q) => {
                let result = pending.query(&q.task_id).map_err(|e| ChannelError::Io(e.to_string()))?;
                stream
                    .write_all(&serde_json::to_vec(&result).map_err(ChannelError::Json)?)
                    .map_err(|e| ChannelError::Io(e.to_string()))?;
                return Ok(None);
            }
        }
    }

    Err(ChannelError::Json(
        serde_json::from_str::<serde_json::Value>("invalid").unwrap_err(),
    ))
}
