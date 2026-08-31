use ai_bridge_gatekeeper_core::policy::decide_policy;
use ai_bridge_protocol::{ExecutionPlan, Symbol};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::Mutex;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchRole {
    None,
    Maestro,
    BranchA,
    BranchB,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub app_id: String,
    pub role: BranchRole,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleAssignmentSet {
    pub roles: Vec<RoleAssignment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum OutboundEvent {
    TaskResolved { task_id: String, approved: bool },
    ExecutionPlan {
        task_id: String,
        description: String,
        commands: Vec<String>,
    },
}

#[derive(PartialEq)]
enum Tab {
    ControlCenter,
    SubChatOps,
    ConfigPrompts,
}

#[derive(Debug, Clone)]
struct ManualCommandRow {
    command: String,
    symbol: Symbol,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyModeUi {
    Armed,
    Disarmed,
}

impl Default for SafetyModeUi {
    fn default() -> Self {
        Self::Armed
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SafetyUiState {
    pub mode: SafetyModeUi,
    pub override_reason: String,
    pub audit_entries: Vec<String>,
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
    role_assignments: RoleAssignmentSet,
    send_message: String,
    ui_message: String,
    manual_task_description: String,
    manual_commands: Vec<ManualCommandRow>,
    safety_state: SafetyUiState,
    safety_lock_mutex: Option<Mutex<()>>,
}

impl DashboardApp {
    pub fn new(rx: Receiver<UiEvent>, tx_out: UnboundedSender<OutboundEvent>) -> Self {
        let role_assignments = RoleAssignmentSet::load_default().unwrap_or_default();
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
            role_assignments,
            send_message: String::new(),
            ui_message: String::new(),
            manual_task_description: "Manual task from dashboard".to_string(),
            manual_commands: vec![ManualCommandRow {
                command: "cargo test".to_string(),
                symbol: Symbol::OpsTermRunLocal,
            }],
            safety_state: SafetyUiState {
                mode: SafetyModeUi::Armed,
                override_reason: String::new(),
                audit_entries: vec![
                    "Safety lock defaulted to Armed at startup".to_string(),
                ],
            },
            safety_lock_mutex: None,
        }
    }

    fn build_manual_plan(task_id: String, description: String, rows: &[ManualCommandRow]) -> Option<ExecutionPlan> {
        let commands: Vec<String> = rows
            .iter()
            .filter_map(|row| {
                let trimmed = row.command.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(row.symbol.with_tag(trimmed))
                }
            })
            .collect();

        if commands.is_empty() {
            return None;
        }

        Some(ExecutionPlan {
            task_id,
            description,
            commands,
        })
    }

    fn queue_manual_plan(&mut self) {
        let description = self.manual_task_description.trim();
        if description.is_empty() {
            self.ui_message = "A description is required before submitting a manual plan.".to_string();
            return;
        }

        let task_id = format!(
            "manual-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        let Some(plan) = Self::build_manual_plan(task_id.clone(), description.to_string(), &self.manual_commands) else {
            self.ui_message = "Add at least one non-empty command before sending the manual plan.".to_string();
            return;
        };

        // Intentionally emit the same plain `ExecutionPlan` JSON that a remote
        // maestro sends on `public_maestro.sock`. This keeps the UI path
        // equivalent to the external socket path and avoids a privileged shortcut.
        let _ = self.tx_out.send(OutboundEvent::ExecutionPlan {
            task_id: plan.task_id.clone(),
            description: plan.description.clone(),
            commands: plan.commands.clone(),
        });

        let decision = decide_policy(&plan);
        self.ui_message = format!(
            "Manual plan {} queued through the maestro socket path; policy result: {:?}",
            task_id, decision
        );
    }

    fn role_options_for_app(&self, app_id: &str) -> BranchRole {
        self.role_assignments
            .roles
            .iter()
            .find(|entry| entry.app_id == app_id)
            .map(|entry| entry.role)
            .unwrap_or(BranchRole::None)
    }

    fn set_role_for_app(&mut self, app_id: String, role: BranchRole) {
        let message = app_id.clone();
        match role {
            BranchRole::Maestro => {
                self.role_assignments.promote_maestro(&app_id);
            }
            _ => {
                self.role_assignments.set_role(app_id.clone(), role);
            }
        }
        if let Err(err) = self.role_assignments.save_default() {
            self.ui_message = format!("Role update failed to save: {err}");
        } else {
            self.ui_message = format!("Role updated: {message} -> {:?}", role);
        }
    }

    fn render_role_assignment_panel(&mut self, ui: &mut egui::Ui) {
        pipe_frame(ui, "ROLE ASSIGNMENT", true, |ui| {
            let app_ids = [
                "org.gnome.Terminal",
                "org.mozilla.firefox",
                "org.gnome.Nautilus",
            ];

            for app_id in app_ids {
                let current_role = self.role_options_for_app(app_id);
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{app_id}:"));
                    for candidate in [
                        (BranchRole::None, "None"),
                        (BranchRole::Maestro, "Maestro"),
                        (BranchRole::BranchA, "Branch A"),
                        (BranchRole::BranchB, "Branch B"),
                    ] {
                        let is_selected = current_role == candidate.0;
                        if ui
                            .selectable_label(is_selected, candidate.1)
                            .clicked()
                        {
                            self.set_role_for_app(app_id.to_string(), candidate.0);
                        }
                    }
                });
                ui.add_space(4.0);
            }

            if let Some(maestro) = self.role_assignments.current_maestro() {
                ui.label(format!("Current Maestro: {}", maestro.app_id));
            } else {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "No Maestro is currently assigned; select one before sending.",
                );
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Send message:");
                ui.text_edit_singleline(&mut self.send_message);
                if ui.button("Send").clicked() {
                    match self.role_assignments.current_maestro() {
                        Some(_) => {
                            self.ui_message = format!("Message sent as Maestro: {}", self.send_message);
                            self.send_message.clear();
                        }
                        None => {
                            self.ui_message = "No Maestro is currently assigned. Assign a Maestro before sending.".to_string();
                        }
                    }
                }
            });

            if !self.ui_message.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(135, 206, 250), &self.ui_message);
            }
        });
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

    fn render_safety_controls(&mut self, ui: &mut egui::Ui) {
        pipe_frame(ui, "SAFETY LOCK CONTROL", true, |ui| {
            ui.horizontal(|ui| {
                ui.label("Safety mode:");
                let mode_label = match self.safety_state.mode {
                    SafetyModeUi::Armed => "Armed",
                    SafetyModeUi::Disarmed => "Disarmed",
                };
                ui.colored_label(
                    if self.safety_state.mode == SafetyModeUi::Armed {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::YELLOW
                    },
                    mode_label,
                );
            });

            ui.horizontal(|ui| {
                if ui.button("Arm Safety").clicked() {
                    self.safety_state.mode = SafetyModeUi::Armed;
                    self.safety_state.audit_entries.push(
                        format!("{} :: user armed safety lock", chrono_or_now()),
                    );
                    self.ui_message = "Safety lock armed by user".to_string();
                }
                if ui.button("Disarm Safety").clicked() {
                    self.safety_state.mode = SafetyModeUi::Disarmed;
                    self.safety_state.audit_entries.push(
                        format!("{} :: user disarmed safety lock", chrono_or_now()),
                    );
                    self.ui_message = "Safety lock disarmed by user".to_string();
                }
            });

            ui.separator();
            ui.label("Override reason");
            ui.text_edit_singleline(&mut self.safety_state.override_reason);
            if ui.button("Override and Execute").clicked() {
                if self.safety_state.override_reason.trim().is_empty() {
                    self.ui_message = "Override requires a clear human reason".to_string();
                } else {
                    self.safety_state.audit_entries.push(format!(
                        "{} :: override granted :: {}",
                        chrono_or_now(),
                        self.safety_state.override_reason
                    ));
                    self.ui_message = "Human override logged and requires the same secure execution path".to_string();
                    self.safety_state.override_reason.clear();
                }
            }

            ui.separator();
            ui.label("Audit trail");
            for entry in &self.safety_state.audit_entries {
                ui.label(entry);
            }
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

        ui.add_space(12.0);

        pipe_frame(ui, "MANUAL MAESTRO PLAN", true, |ui| {
            ui.label("Description");
            ui.text_edit_multiline(&mut self.manual_task_description);

            let mut remove_index: Option<usize> = None;
            for index in 0..self.manual_commands.len() {
                let row = &mut self.manual_commands[index];
                ui.horizontal_wrapped(|ui| {
                    ui.label("Command");
                    ui.text_edit_singleline(&mut row.command);

                    ui.label("Symbol");
                    egui::ComboBox::from_label("Symbol")
                        .selected_text(row.symbol.literal_name())
                        .show_ui(ui, |ui| {
                            for symbol in Symbol::all() {
                                if ui.selectable_value(&mut row.symbol, *symbol, symbol.literal_name()).clicked() {
                                    // Keep the row update local and let the caller continue.
                                }
                            }
                        });

                    if ui.button("Remove").clicked() {
                        remove_index = Some(index);
                    }
                });
                ui.small(row.symbol.description());
                ui.add_space(6.0);
            }

            if let Some(index) = remove_index {
                self.manual_commands.remove(index);
            }

            ui.horizontal(|ui| {
                if ui.button("Add command").clicked() {
                    self.manual_commands.push(ManualCommandRow {
                        command: String::new(),
                        symbol: Symbol::OpsTermRunLocal,
                    });
                }
                if ui.button("Send manual plan").clicked() {
                    self.queue_manual_plan();
                }
            });
        });

        ui.add_space(12.0);
        self.render_safety_controls(ui);
        ui.add_space(12.0);
        self.render_role_assignment_panel(ui);
    }
}

fn chrono_or_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}", d.as_millis()))
        .unwrap_or_else(|_| "now".to_string())
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

impl RoleAssignmentSet {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let raw = fs::read_to_string(path)?;
        toml::from_str(&raw).map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    }

    pub fn load_default() -> Result<Self, std::io::Error> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let candidate_paths = [
            root.join("config/roles.toml"),
            PathBuf::from("config/roles.toml"),
            PathBuf::from("../config/roles.toml"),
        ];
        if let Some(path) = candidate_paths.into_iter().find(|path| path.exists()) {
            return Self::load(path);
        }
        Ok(Self::default())
    }

    pub fn save_default(&self) -> Result<(), std::io::Error> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let path = root.join("config/roles.toml");
        let raw = toml::to_string(self)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        fs::write(path, raw)
    }

    pub fn set_role(&mut self, app_id: String, role: BranchRole) {
        if role == BranchRole::None {
            self.roles.retain(|entry| entry.app_id != app_id);
            return;
        }

        self.roles.retain(|entry| entry.app_id != app_id);
        self.roles.push(RoleAssignment { app_id, role });
    }

    pub fn current_maestro(&self) -> Option<&RoleAssignment> {
        self.roles.iter().find(|entry| entry.role == BranchRole::Maestro)
    }

    pub fn promote_maestro(&mut self, app_id: &str) {
        let app_id = app_id.to_string();

        for entry in &mut self.roles {
            if entry.role == BranchRole::Maestro && entry.app_id != app_id {
                entry.role = BranchRole::None;
            }
        }

        if let Some(entry) = self.roles.iter_mut().find(|entry| entry.app_id == app_id) {
            entry.role = BranchRole::Maestro;
            return;
        }

        self.roles.push(RoleAssignment {
            app_id,
            role: BranchRole::Maestro,
        });
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_assignments_round_trip_through_toml() {
        let expected = RoleAssignmentSet {
            roles: vec![
                RoleAssignment { app_id: "org.gnome.Terminal".into(), role: BranchRole::Maestro },
                RoleAssignment { app_id: "org.mozilla.firefox".into(), role: BranchRole::BranchA },
            ],
        };

        let raw = toml::to_string(&expected).unwrap();
        let loaded: RoleAssignmentSet = toml::from_str(&raw).unwrap();
        assert_eq!(loaded.roles, expected.roles);
    }

    #[test]
    fn no_maestro_is_detected_when_missing() {
        let assignments = RoleAssignmentSet {
            roles: vec![
                RoleAssignment { app_id: "org.mozilla.firefox".into(), role: BranchRole::BranchA },
                RoleAssignment { app_id: "org.gnome.Nautilus".into(), role: BranchRole::BranchB },
            ],
        };

        let current = assignments.current_maestro();
        assert!(current.is_none());
    }

    #[test]
    fn switching_maestro_promotes_new_app_and_keeps_single_maestro() {
        let mut assignments = RoleAssignmentSet {
            roles: vec![
                RoleAssignment { app_id: "org.gnome.Terminal".into(), role: BranchRole::Maestro },
                RoleAssignment { app_id: "org.mozilla.firefox".into(), role: BranchRole::BranchA },
            ],
        };

        assignments.promote_maestro("org.mozilla.firefox");

        assert_eq!(assignments.current_maestro().unwrap().app_id, "org.mozilla.firefox");
        assert_eq!(assignments.roles.iter().filter(|r| r.role == BranchRole::Maestro).count(), 1);
    }

    #[test]
    fn manual_plan_matches_gatekeeper_policy_of_equivalent_external_plan() {
        let rows = vec![ManualCommandRow {
            command: "cargo test -- --nocapture".to_string(),
            symbol: Symbol::OpsTermRunLocal,
        }];

        let ui_plan = DashboardApp::build_manual_plan(
            "manual-1".to_string(),
            "Run tests".to_string(),
            &rows,
        )
        .unwrap();

        let external_plan = ExecutionPlan {
            task_id: "external-1".to_string(),
            description: "Run tests".to_string(),
            commands: vec!["[[AB:OPS.TERM.RUN.LOCAL]] cargo test -- --nocapture".to_string()],
        };

        assert_eq!(decide_policy(&ui_plan), decide_policy(&external_plan));
    }
}
