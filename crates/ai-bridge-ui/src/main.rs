mod app;

use app::{GatekeeperEvent, OutboundEvent, UiEvent};
use eframe::egui;
use std::path::Path;
use std::sync::mpsc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::runtime::Runtime;

fn main() -> Result<(), eframe::Error> {
    let rt = Runtime::new().expect("Failed to initialize tokio runtime");
    let (tx, rx) = mpsc::channel();
    let (tx_outbound, mut rx_outbound) = tokio::sync::mpsc::unbounded_channel::<OutboundEvent>();

    // Socket Monitor Thread
    let tx_status = tx.clone();
    rt.spawn(async move {
        loop {
            let maestro = Path::new("/tmp/ai_bridge_public_maestro.sock").exists();
            let sub_a = Path::new("/tmp/ai_bridge_private_a.sock").exists();
            let sub_b = Path::new("/tmp/ai_bridge_private_b.sock").exists();

            let _ = tx_status.send(UiEvent::SocketStatusUpdate {
                maestro_online: maestro,
                sub_a_online: sub_a,
                sub_b_online: sub_b,
            });

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });

    // Gatekeeper Stream Thread (bidirectional)
    let tx_events = tx;
    rt.spawn(async move {
        let socket_path = "/tmp/public_maestro.sock";
        loop {
            if Path::new(socket_path).exists() {
                if let Ok(stream) = UnixStream::connect(socket_path).await {
                    let (reader, mut writer) = stream.into_split();
                    let mut lines = BufReader::new(reader).lines();

                    loop {
                        tokio::select! {
                            Some(event) = rx_outbound.recv() => {
                                if let Ok(json) = serde_json::to_string(&event) {
                                    if writer.write_all(format!("{}\n", json).as_bytes()).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            line = lines.next_line() => {
                                match line {
                                    Ok(Some(line)) => {
                                        if let Ok(event) = serde_json::from_str::<GatekeeperEvent>(&line) {
                                            let _ = tx_events.send(UiEvent::Gatekeeper(event));
                                        }
                                    }
                                    _ => break,
                                }
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("AI-Bridge ⬡ Pipe Network Dashboard")
            .with_decorations(true)
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "AI-Bridge Dashboard",
        options,
        Box::new(|cc| {
            // Apply custom dark industrial visuals
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = egui::Color32::from_rgb(18, 19, 28);
            visuals.window_fill = egui::Color32::from_rgb(25, 27, 40);
            visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(30, 33, 50);
            visuals.widgets.noninteractive.bg_stroke =
                egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 229, 255));
            cc.egui_ctx.set_visuals(visuals);

            Ok(Box::new(app::DashboardApp::new(rx, tx_outbound)))
        }),
    )
}
