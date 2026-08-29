use serde_json::json;
use std::fs;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = "/tmp/public_maestro.sock";

    if Path::new(socket_path).exists() {
        let _ = fs::remove_file(socket_path);
    }

    let listener = UnixListener::bind(socket_path)?;
    println!("[Mock Gatekeeper] Listening on {}", socket_path);

    loop {
        if let Ok((mut stream, _)) = listener.accept().await {
            println!("[Mock Gatekeeper] UI Client connected!");

            // Event 1: Pending Task
            let task1 = json!({
                "type": "task_pending",
                "task_id": "TASK-101",
                "app_name": "org.gnome.Terminal",
                "command": "rm -rf /tmp/test_dir",
                "risk_level": "HIGH"
            });
            let _ = stream
                .write_all(format!("{}\n", task1.to_string()).as_bytes())
                .await;

            sleep(Duration::from_secs(3)).await;

            // Event 2: Second Pending Task
            let task2 = json!({
                "type": "task_pending",
                "task_id": "TASK-102",
                "app_name": "bash",
                "command": "cargo build --release",
                "risk_level": "LOW"
            });
            let _ = stream
                .write_all(format!("{}\n", task2.to_string()).as_bytes())
                .await;
        }
    }
}
