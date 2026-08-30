use eframe::egui;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::Receiver;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GatekeeperEvent {
    TaskPending {
        task_id: String,
        app_name: String,
        command: String,
        risk_level: String,
        is_critical: bool,
        triggered: Vec<String>,
    },
    TaskResolved {
        task_id: String,
        approved: bool,
    },
}

#[derive(Debug, Clone)]
pub struct PendingTask {
    pub task_id: String,
    pub app_name: String,
    pub command: String,
    pub risk_level: String,
    pub is_critical: bool,
    pub triggered: Vec<String>,
}

pub enum UiEvent {
    SocketStatusUpdate {
        maestro_online: bool,
        sub_a_online: bool,
        sub_b_online: bool,
    },
    Gatekeeper(GatekeeperEvent),
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum OutboundEvent {
    TaskResolved { task_id: String, approved: bool },
}

#[derive(PartialEq)]
enum Tab {
    ControlCenter,
    SubChatOps,
    ConfigPrompts,
}

pub struct DashboardApp {
    rx: Receiver<UiEvent>,
    tx_out: UnboundedSender<OutboundEvent>,
    current_tab: Tab,
    maestro_online: bool,
    sub_a_online: bool,
    sub_b_online: bool,
    pending_tasks: Vec<PendingTask>,
    allowlist_input: String,
    system_prompt: String,
    include_context: bool,
    priority: String,
}

impl DashboardApp {
    pub fn new(rx: Receiver<UiEvent>, tx_out: UnboundedSender<OutboundEvent>) -> Self {
        Self {
            rx,
            tx_out,
            current_tab: Tab::ControlCenter,
            maestro_online: false,
            sub_a_online: false,
            sub_b_online: false,
            pending_tasks: Vec::new(),
            allowlist_input: "[[apps]]\nname = \"org.gnome.Terminal\"\nkind = \"executor\""
                .to_string(),
            system_prompt: "You are Maestro...".to_string(),
            include_context: false,
            priority: "Maestro -> A -> B".to_string(),
        }
    }

    fn render_control_center(&mut self, ui: &mut egui::Ui) {
        // Top Pipe Segment: Network Channels
        pipe_frame(ui, "DATA PIPELINES (IPC SOCKETS)", true, |ui| {
            ui.horizontal(|ui| {
                let pipe_node = |ui: &mut egui::Ui, name: &str, online: bool| {
                    let color = if online {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::RED
                    };
                    let status = if online {
                        "CONNECTED [═ ONLINE ═]"
                    } else {
                        "OFFLINE [─ CLOSED ─]"
                    };
                    ui.group(|ui| {
                        ui.label(
                            egui::RichText::new(format!("══ Pipe: {} ══", name))
                                .color(color)
                                .strong(),
                        );
                        ui.label(egui::RichText::new(status).size(11.0));
                    });
                };

                pipe_node(ui, "private_a.sock", self.sub_a_online);
                ui.label(egui::RichText::new(" ═══ ").color(egui::Color32::from_rgb(0, 229, 255)));
                pipe_node(ui, "private_b.sock", self.sub_b_online);
                ui.label(egui::RichText::new(" ═══ ").color(egui::Color32::from_rgb(0, 229, 255)));
                pipe_node(ui, "public_maestro.sock", self.maestro_online);
            });
        });

        ui.add_space(14.0);

        // Main Pipe Segment: Gatekeeper Flow Filter
        pipe_frame(
            ui,
            "GATEKEEPER SECURITY VALVE",
            !self.pending_tasks.is_empty(),
            |ui| {
                if self.pending_tasks.is_empty() {
                    ui.label(
                        egui::RichText::new("✔ All pipelines clear. No blocked tasks.")
                            .color(egui::Color32::GRAY),
                    );
                } else {
                    let mut resolved_id: Option<(String, bool)> = None;

                    for task in &self.pending_tasks {
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(32, 35, 52))
                            .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 170, 0)))
                            .rounding(6.0)
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.strong(format!("[ID: {}]", task.task_id));
                                    ui.label(format!("App: {}", task.app_name));
                                    ui.label(
                                        egui::RichText::new(format!("Risk: {}", task.risk_level))
                                            .color(egui::Color32::RED),
                                    );
                                    // Criticality badge from server-computed decision
                                    let (badge_text, badge_color) = if task.is_critical {
                                        ("CRITICAL", egui::Color32::RED)
                                    } else {
                                        ("STANDARD", egui::Color32::GREEN)
                                    };
                                    ui.label(
                                        egui::RichText::new(format!(" [{}]", badge_text))
                                            .color(badge_color)
                                            .strong(),
                                    );
                                });
                                // Show triggered criteria if any
                                if !task.triggered.is_empty() {
                                    ui.label(
                                        egui::RichText::new(format!("Triggered: {}", task.triggered.join(", ")))
                                            .color(egui::Color32::YELLOW)
                                            .italics(),
                                    );
                                }
                                ui.code(format!("Command: {}", task.command));
                                ui.horizontal(|ui| {
                                    if ui.button("✅ VALVE OPEN (Approve)").clicked() {
                                        let _ = self.tx_out.send(OutboundEvent::TaskResolved {
                                            task_id: task.task_id.clone(),
                                            approved: true,
                                        });
                                        resolved_id = Some((task.task_id.clone(), true));
                                    }
                                    if ui.button("❌ VALVE CLOSE (Reject)").clicked() {
                                        let _ = self.tx_out.send(OutboundEvent::TaskResolved {
                                            task_id: task.task_id.clone(),
                                            approved: false,
                                        });
                                        resolved_id = Some((task.task_id.clone(), false));
                                    }
                                });
                            });
                        ui.add_space(4.0);
                    }

                    if let Some((id, approved)) = resolved_id {
                        self.pending_tasks.retain(|t| t.task_id != id);
                        let _ = approved;
                    }
                }
            },
        );
    }

    fn render_subchat_ops(&mut self, ui: &mut egui::Ui) {
        pipe_frame(ui, "SUB-CHAT ROUTING GRID", true, |ui| {
            ui.code("   [ Maestro Core ]\n          ║\n    ┌─────╩─────┐\n    ▼           ▼\n[Branch A]  [Branch B]");
        });

        ui.add_space(14.0);

        pipe_frame(ui, "DIAGNOSTIC TELEMETRY", self.maestro_online, |ui| {
            ui.horizontal(|ui| {
                ui.label("Active Node:");
                ui.strong("Maestro");
                ui.separator();
                ui.label("Pipeline Status:");
                let conn_status = if self.maestro_online {
                    egui::RichText::new("FLOW ACTIVE").color(egui::Color32::GREEN)
                } else {
                    egui::RichText::new("BLOCKED").color(egui::Color32::RED)
                };
                ui.label(conn_status);
            });
        });
    }

    fn render_config_prompts(&mut self, ui: &mut egui::Ui) {
        pipe_frame(ui, "PIPELINE CONFIGURATION & ALLOWLIST", true, |ui| {
            ui.label("Allowlist Configuration (allowlist.toml)");
            ui.text_edit_multiline(&mut self.allowlist_input);
            ui.separator();
            ui.label("System Prompts");
            ui.text_edit_multiline(&mut self.system_prompt);
            ui.checkbox(&mut self.include_context, "Include Master Context Flow");
            ui.separator();
            ui.label("AI Priority Pipeline");
            ui.radio_value(
                &mut self.priority,
                "Maestro -> A -> B".to_string(),
                "Maestro -> A -> B",
            );
            ui.radio_value(
                &mut self.priority,
                "Maestro -> B -> A".to_string(),
                "Maestro -> B -> A",
            );
        });
    }
}

// Custom helper to draw a pipe container block
fn pipe_frame(
    ui: &mut egui::Ui,
    title: &str,
    active: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let stroke_color = if active {
        egui::Color32::from_rgb(0, 229, 255) // Cyber Cyan
    } else {
        egui::Color32::from_rgb(60, 65, 90) // Inactive Metallic
    };

    egui::Frame::none()
        .fill(egui::Color32::from_rgb(24, 26, 38))
        .stroke(egui::Stroke::new(2.5, stroke_color))
        .rounding(10.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new(format!("⬡ {}", title))
                        .color(stroke_color)
                        .strong(),
                );
            });
            ui.add_space(6.0);
            add_contents(ui);
        });
}

impl eframe::App for DashboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                UiEvent::SocketStatusUpdate {
                    maestro_online,
                    sub_a_online,
                    sub_b_online,
                } => {
                    self.maestro_online = maestro_online;
                    self.sub_a_online = sub_a_online;
                    self.sub_b_online = sub_b_online;
                }
UiEvent::Gatekeeper(ev) => match ev {
                GatekeeperEvent::TaskPending {
                    task_id,
                    app_name,
                    command,
                    risk_level,
                    is_critical,
                    triggered,
                } => {
                    if !self.pending_tasks.iter().any(|t| t.task_id == task_id) {
                        self.pending_tasks.push(PendingTask {
                            task_id,
                            app_name,
                            command,
                            risk_level,
                            is_critical,
                            triggered,
                        });
                    }
                }
                    GatekeeperEvent::TaskResolved { task_id, .. } => {
                        self.pending_tasks.retain(|t| t.task_id != task_id);
                    }
                },
            }
        }

        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(14, 15, 22))
                    .inner_margin(8.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.current_tab,
                        Tab::ControlCenter,
                        "╔═ 1. Pipeline Control ═╗",
                    );
                    ui.selectable_value(
                        &mut self.current_tab,
                        Tab::SubChatOps,
                        "╔═ 2. Routing Ops ═╗",
                    );
                    ui.selectable_value(
                        &mut self.current_tab,
                        Tab::ConfigPrompts,
                        "╔═ 3. Configuration ═╗",
                    );
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| match self.current_tab {
            Tab::ControlCenter => self.render_control_center(ui),
            Tab::SubChatOps => self.render_subchat_ops(ui),
            Tab::ConfigPrompts => self.render_config_prompts(ui),
        });

        ctx.request_repaint_after(Duration::from_millis(100));
    }
}
