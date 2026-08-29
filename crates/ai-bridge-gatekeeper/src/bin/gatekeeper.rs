use ai_bridge_gatekeeper::criteria::is_critical_task;
use ai_bridge_gatekeeper::executor::execute_approved_task;
use ai_bridge_gatekeeper::gatekeeper::{
    await_human_decision, serve_ui_session, ActiveTasks, PendingTask,
};
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::sync::oneshot;

const SOCKET_PATH: &str = "/tmp/public_maestro.sock";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if Path::new(SOCKET_PATH).exists() {
        let _ = fs::remove_file(SOCKET_PATH);
    }

    let listener = UnixListener::bind(SOCKET_PATH)?;
    println!("[Gatekeeper] Listening on {}", SOCKET_PATH);

    // Internal registry of tasks blocked and awaiting a human decision.
    let mut active: ActiveTasks = ActiveTasks::new();

    loop {
        let (stream, _) = listener.accept().await?;
        println!("[Gatekeeper] UI connected");

        // Simulated agent: submit one pending command for the operator to
        // review and hold the channel that will unblock execution.
        let command = "rm -rf /tmp/test_dir".to_string();
        let task = PendingTask {
            task_id: "TASK-101".to_string(),
            app_name: "org.gnome.Terminal".to_string(),
            command: command.clone(),
            risk_level: "HIGH".to_string(),
            is_critical: is_critical_task(&command),
        };
        let await_task_id = task.task_id.clone();
        let await_command = task.command.clone();
        let await_is_critical = task.is_critical;
        let (tx, rx) = oneshot::channel();
        active.register(task, tx);

        tokio::spawn(async move {
            // Wait for the human decision with the architecture's delegation
            // timeout: critical tasks hang forever for the operator, while
            // non-critical tasks auto-approve after 120 seconds of silence.
            let approved =
                await_human_decision(rx, await_is_critical, Duration::from_secs(120)).await;

            if approved {
                // Approved (or auto-approved via a delegation timeout): execute
                // the exact program + arguments, never a shell.
                match execute_approved_task(&await_command).await {
                    Ok(output) => {
                        println!(
                            "[Gatekeeper] Command {} executed (exit: {})",
                            await_task_id, output.status
                        );
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        if !stderr.trim().is_empty() {
                            eprintln!("[Gatekeeper] Command {} stderr: {stderr}", await_task_id);
                        }
                    }
                    Err(err) => {
                        eprintln!("[Gatekeeper] Command {await_task_id} execution failed: {err}")
                    }
                }
            } else {
                // Rejected by the operator: drop and log the rebuffal.
                println!(
                    "[Gatekeeper] Command {} ABORTED (rejected by operator)",
                    await_task_id
                );
            }
        });

        // Advertise pending tasks to the UI and read decisions until it
        // disconnects. The connection stays open for the lifetime of the UI
        // session.
        if let Ok(resolved) = serve_ui_session(stream, &mut active).await {
            for task_id in resolved {
                println!("[Gatekeeper] Resolved task {task_id}");
            }
        }
    }
}
