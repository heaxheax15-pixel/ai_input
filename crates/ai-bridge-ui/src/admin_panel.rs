use std::fs;
use std::path::{Path, PathBuf};

use ai_bridge_gatekeeper_core::admin_config::{
    self, AdminConfig, BranchConfig, CriteriaPatternSet, SocketDefinition,
};
use ai_bridge_gatekeeper_core::audit::{AuditEntry, AuditLog};
use ai_bridge_gatekeeper_core::criteria::CriteriaSummary;
use ai_bridge_gatekeeper_daemon::executor_allowlist::{AllowlistConfig, BinaryEntry};
use ai_bridge_protocol::ExecutionPlan;
use eframe::egui;
use egui::{Color32, RichText, Stroke};

/// Consistent spacing constants for the admin panel
const SECTION_SPACING: f32 = 12.0;
const ROW_SPACING: f32 = 8.0;
const PANEL_PADDING: f32 = 16.0;
const CORNER_RADIUS: f32 = 6.0;

// ---------------------------------------------------------------------------
// Sub-tab enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminTab {
    Allowlist,
    Symbols,
    Criteria,
    Sockets,
    Branches,
    AuditLog,
}

// ---------------------------------------------------------------------------
// Per-entry editable state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ExecAllowlistRow {
    pub name: String,
    pub path: String,
    pub allowed: bool,
    pub args: String,
}

impl ExecAllowlistRow {
    fn from_binary(entry: &BinaryEntry) -> Self {
        Self {
            name: entry.name.clone(),
            path: entry.path.clone().unwrap_or_default(),
            allowed: entry.allowed,
            args: entry.args.join(", "),
        }
    }
    fn to_binary(&self) -> BinaryEntry {
        let args: Vec<String> = self
            .args
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        BinaryEntry {
            name: self.name.clone(),
            allowed: self.allowed,
            path: if self.path.is_empty() {
                None
            } else {
                Some(self.path.clone())
            },
            args,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SymbolPolicyRow {
    pub name: String,
    pub description: String,
    pub classification: String,
    pub auto_approve_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct CriteriaRow {
    pub category: String,
    pub patterns_text: String,
}

impl CriteriaRow {
    fn from_set(set: &CriteriaPatternSet) -> Self {
        Self {
            category: set.category.clone(),
            patterns_text: set.patterns.join("\n"),
        }
    }
    fn to_pattern_set(&self) -> CriteriaPatternSet {
        let patterns: Vec<String> = self
            .patterns_text
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        CriteriaPatternSet {
            category: self.category.clone(),
            patterns,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SocketRow {
    pub name: String,
    pub path: String,
    pub permissions: String,
}

#[derive(Debug, Clone)]
pub struct BranchRow {
    pub name: String,
    pub sub_chat_limit: usize,
    pub context_enabled: bool,
}

// ---------------------------------------------------------------------------
// Confirmation dialog state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ConfirmDialog {
    message: String,
    action: String,
}

// ---------------------------------------------------------------------------
// Main AdminPanel state
// ---------------------------------------------------------------------------

pub struct AdminPanel {
    pub active_tab: AdminTab,
    config_root: PathBuf,

    exec_allowlist: Vec<ExecAllowlistRow>,
    exec_allowlist_dirty: bool,

    symbol_policy: Vec<SymbolPolicyRow>,
    symbol_policy_dirty: bool,

    criteria: Vec<CriteriaRow>,
    criteria_dirty: bool,
    test_command: String,
    test_result: Option<CriteriaSummary>,

    sockets: Vec<SocketRow>,
    sockets_dirty: bool,

    branches: Vec<BranchRow>,
    branches_dirty: bool,

    audit_entries: Vec<AuditEntry>,

    status_message: String,
    confirm_dialog: Option<ConfirmDialog>,
}

impl AdminPanel {
    pub fn new() -> Self {
        let config_root = admin_config::config_dir();
        let mut panel = Self {
            active_tab: AdminTab::Allowlist,
            config_root,
            exec_allowlist: Vec::new(),
            exec_allowlist_dirty: false,
            symbol_policy: Vec::new(),
            symbol_policy_dirty: false,
            criteria: Vec::new(),
            criteria_dirty: false,
            test_command: String::new(),
            test_result: None,
            sockets: Vec::new(),
            sockets_dirty: false,
            branches: Vec::new(),
            branches_dirty: false,
            audit_entries: Vec::new(),
            status_message: String::new(),
            confirm_dialog: None,
        };
        panel.load_all();
        panel
    }

    /// Workspace config fallback path (for dev).
    fn workspace_config_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../config")
    }

    fn find_file(&self, name: &str) -> PathBuf {
        let user_path = self.config_root.join(name);
        if user_path.exists() {
            return user_path;
        }
        Self::workspace_config_dir().join(name)
    }

    // -----------------------------------------------------------------------
    // Load all config sections from their respective files
    // -----------------------------------------------------------------------

    pub fn load_all(&mut self) {
        self.load_exec_allowlist();
        self.load_symbol_policy();
        self.load_criteria();
        self.load_sockets();
        self.load_branches();
        self.load_audit();
    }

    fn load_exec_allowlist(&mut self) {
        let path = self.find_file("executor_allowlist.toml");
        self.exec_allowlist = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| AllowlistConfig::parse(&raw).ok())
            .map(|cfg| cfg.binaries.iter().map(ExecAllowlistRow::from_binary).collect())
            .unwrap_or_default();
        self.exec_allowlist_dirty = false;
    }

    fn load_symbol_policy(&mut self) {
        let path = self.find_file("symbol_policy.toml");
        self.symbol_policy = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| toml::from_str::<AdminConfig>(&raw).ok())
            .map(|cfg| {
                cfg.policy
                    .iter()
                    .map(|p| {
                        let sym = cfg
                            .symbols
                            .iter()
                            .find(|s| s.name == p.symbol);
                        SymbolPolicyRow {
                            name: p.symbol.clone(),
                            description: sym.map(|s| s.description.clone()).unwrap_or_default(),
                            classification: p.classification.clone(),
                            auto_approve_seconds: p.auto_approve_seconds,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.symbol_policy_dirty = false;
    }

    fn load_criteria(&mut self) {
        let path = self.find_file("criteria.toml");
        self.criteria = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| toml::from_str::<AdminConfig>(&raw).ok())
            .map(|cfg| cfg.criteria.iter().map(CriteriaRow::from_set).collect())
            .unwrap_or_default();
        self.criteria_dirty = false;
    }

    fn load_sockets(&mut self) {
        let path = self.find_file("sockets.toml");
        self.sockets = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| toml::from_str::<AdminConfig>(&raw).ok())
            .map(|cfg| {
                cfg.sockets
                    .iter()
                    .map(|s| SocketRow {
                        name: s.name.clone(),
                        path: s.path.clone(),
                        permissions: s.permissions.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.sockets_dirty = false;
    }

    fn load_branches(&mut self) {
        let path = self.find_file("branches.toml");
        self.branches = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| toml::from_str::<AdminConfig>(&raw).ok())
            .map(|cfg| {
                cfg.branches
                    .iter()
                    .map(|b| BranchRow {
                        name: b.name.clone(),
                        sub_chat_limit: b.sub_chat_limit,
                        context_enabled: b.context_enabled,
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.branches_dirty = false;
    }

    fn load_audit(&mut self) {
        let log = AuditLog::new(self.config_root.join("audit"));
        self.audit_entries = log.read_all().unwrap_or_default();
    }

    // -----------------------------------------------------------------------
    // Save helpers
    // -----------------------------------------------------------------------

    fn save_exec_allowlist(&mut self) {
        let path = self.config_root.join("executor_allowlist.toml");
        let old_value = fs::read_to_string(&path).unwrap_or_default();
        let config = AllowlistConfig {
            binaries: self.exec_allowlist.iter().map(|r| r.to_binary()).collect(),
        };
        let raw = match config.to_toml() {
            Ok(raw) => raw,
            Err(e) => {
                self.status_message = format!("Serialize error: {e}");
                return;
            }
        };
        if let Err(e) = write_with_backup(&path, &raw) {
            self.status_message = format!("Save failed: {e}");
            return;
        }
        self.exec_allowlist_dirty = false;
        self.status_message = "Executor allowlist saved.".to_string();
        self.record_audit("allowlist.save", "executor_allowlist.toml", &old_value, &raw);
    }

    fn save_symbol_policy(&mut self) {
        let path = self.config_root.join("symbol_policy.toml");
        let old_value = fs::read_to_string(&path).unwrap_or_default();
        let mut config = AdminConfig::default();
        for row in &self.symbol_policy {
            config.symbols.push(admin_config::SymbolDefinition {
                name: row.name.clone(),
                classification: row.classification.clone(),
                description: row.description.clone(),
            });
            config.policy.push(admin_config::PolicySymbolEntry {
                symbol: row.name.clone(),
                classification: row.classification.clone(),
                auto_approve_seconds: row.auto_approve_seconds,
            });
        }
        if let Err(e) = config.validate() {
            self.status_message = format!("Validation failed: {e}");
            return;
        }
        let raw = match toml::to_string(&config) {
            Ok(r) => r,
            Err(e) => {
                self.status_message = format!("Serialize error: {e}");
                return;
            }
        };
        if let Err(e) = write_with_backup(&path, &raw) {
            self.status_message = format!("Save failed: {e}");
            return;
        }
        self.symbol_policy_dirty = false;
        self.status_message = "Symbol policy saved. Daemon restart required.".to_string();
        self.record_audit("symbol_policy.save", "symbol_policy.toml", &old_value, &raw);
    }

    fn save_criteria(&mut self) {
        let path = self.config_root.join("criteria.toml");
        let old_value = fs::read_to_string(&path).unwrap_or_default();
        let mut config = AdminConfig::default();
        for row in &self.criteria {
            config.criteria.push(row.to_pattern_set());
        }
        if let Err(e) = config.validate() {
            self.status_message = format!("Validation failed: {e}");
            return;
        }
        let raw = match toml::to_string(&config) {
            Ok(r) => r,
            Err(e) => {
                self.status_message = format!("Serialize error: {e}");
                return;
            }
        };
        if let Err(e) = write_with_backup(&path, &raw) {
            self.status_message = format!("Save failed: {e}");
            return;
        }
        self.criteria_dirty = false;
        self.status_message = "Criteria patterns saved. Daemon restart required.".to_string();
        self.record_audit("criteria.save", "criteria.toml", &old_value, &raw);
    }

    fn save_sockets(&mut self) {
        let path = self.config_root.join("sockets.toml");
        let old_value = fs::read_to_string(&path).unwrap_or_default();
        let mut config = AdminConfig::default();
        for row in &self.sockets {
            config.sockets.push(SocketDefinition {
                name: row.name.clone(),
                path: row.path.clone(),
                permissions: row.permissions.clone(),
            });
        }
        if let Err(e) = config.validate() {
            self.status_message = format!("Validation failed: {e}");
            return;
        }
        let raw = match toml::to_string(&config) {
            Ok(r) => r,
            Err(e) => {
                self.status_message = format!("Serialize error: {e}");
                return;
            }
        };
        if let Err(e) = write_with_backup(&path, &raw) {
            self.status_message = format!("Save failed: {e}");
            return;
        }
        self.sockets_dirty = false;
        self.status_message = "Sockets config saved. Daemon restart required.".to_string();
        self.record_audit("sockets.save", "sockets.toml", &old_value, &raw);
    }

    fn save_branches(&mut self) {
        let path = self.config_root.join("branches.toml");
        let old_value = fs::read_to_string(&path).unwrap_or_default();
        let mut config = AdminConfig::default();
        for row in &self.branches {
            config.branches.push(BranchConfig {
                name: row.name.clone(),
                sub_chat_limit: row.sub_chat_limit,
                context_enabled: row.context_enabled,
            });
        }
        let raw = match toml::to_string(&config) {
            Ok(r) => r,
            Err(e) => {
                self.status_message = format!("Serialize error: {e}");
                return;
            }
        };
        if let Err(e) = write_with_backup(&path, &raw) {
            self.status_message = format!("Save failed: {e}");
            return;
        }
        self.branches_dirty = false;
        self.status_message = "Branch config saved. Daemon restart required.".to_string();
        self.record_audit("branches.save", "branches.toml", &old_value, &raw);
    }

    // -----------------------------------------------------------------------
    // Audit helper
    // -----------------------------------------------------------------------

    fn record_audit(&mut self, action: &str, file: &str, old_value: &str, new_value: &str) {
        let operator = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "admin".to_string());
        let entry = AuditEntry {
            timestamp: ai_bridge_gatekeeper_core::audit::timestamp_iso8601(),
            operator,
            changed_file: file.to_string(),
            action: action.to_string(),
            old_value: old_value.to_string(),
            new_value: new_value.to_string(),
            security_lowering: false,
        };
        let log = AuditLog::new(self.config_root.join("audit"));
        let _ = log.append(&entry);
        self.audit_entries.push(entry);
    }

    // -----------------------------------------------------------------------
    // Security-lowering check (uses the AdminConfig validation path)
    // -----------------------------------------------------------------------

    fn build_admin_config_from_rows(
        exec: &[ExecAllowlistRow],
        sp: &[SymbolPolicyRow],
        cr: &[CriteriaRow],
        so: &[SocketRow],
        br: &[BranchRow],
    ) -> AdminConfig {
        AdminConfig {
            allowlist: exec
                .iter()
                .map(|r| admin_config::AllowlistEntry {
                    name: r.name.clone(),
                    path: r.path.clone(),
                    allowed: r.allowed,
                    args: r
                        .args
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                })
                .collect(),
            symbols: sp
                .iter()
                .map(|r| admin_config::SymbolDefinition {
                    name: r.name.clone(),
                    classification: r.classification.clone(),
                    description: r.description.clone(),
                })
                .collect(),
            policy: sp
                .iter()
                .map(|r| admin_config::PolicySymbolEntry {
                    symbol: r.name.clone(),
                    classification: r.classification.clone(),
                    auto_approve_seconds: r.auto_approve_seconds,
                })
                .collect(),
            criteria: cr.iter().map(|r| r.to_pattern_set()).collect(),
            sockets: so
                .iter()
                .map(|r| SocketDefinition {
                    name: r.name.clone(),
                    path: r.path.clone(),
                    permissions: r.permissions.clone(),
                })
                .collect(),
            branches: br
                .iter()
                .map(|r| BranchConfig {
                    name: r.name.clone(),
                    sub_chat_limit: r.sub_chat_limit,
                    context_enabled: r.context_enabled,
                })
                .collect(),
            audit: vec![],
        }
    }

    // -----------------------------------------------------------------------
    // Live test against current criteria config
    // -----------------------------------------------------------------------

    fn run_test_command(&self) -> Option<CriteriaSummary> {
        if self.test_command.trim().is_empty() {
            return None;
        }
        let mut config = AdminConfig::default();
        for row in &self.criteria {
            config.criteria.push(row.to_pattern_set());
        }
        let criteria_config = config.to_criteria_config();
        let plan = ExecutionPlan {
            task_id: "test".to_string(),
            description: "UI live test".to_string(),
            commands: vec![self.test_command.clone()],
        };
        Some(ai_bridge_gatekeeper_core::criteria::evaluate_criteria_with_config(
            &plan,
            &criteria_config,
        ))
    }

    // -----------------------------------------------------------------------
    // Render
    // -----------------------------------------------------------------------

    pub fn render(&mut self, ui: &mut egui::Ui) {
        // Apply consistent visual styling
        let style = ui.style_mut();
        style.visuals.window_rounding = CORNER_RADIUS.into();
        style.visuals.menu_rounding = CORNER_RADIUS.into();
        style.visuals.widgets.noninteractive.rounding = CORNER_RADIUS.into();
        style.visuals.widgets.inactive.rounding = CORNER_RADIUS.into();
        style.visuals.widgets.hovered.rounding = CORNER_RADIUS.into();
        style.visuals.widgets.active.rounding = CORNER_RADIUS.into();
        style.visuals.widgets.open.rounding = CORNER_RADIUS.into();
        style.spacing.item_spacing = egui::vec2(8.0, ROW_SPACING);
        style.spacing.window_margin = egui::Margin::same(PANEL_PADDING);

        // Sub-tab bar with better styling
        ui.add_space(SECTION_SPACING);
        egui::Frame::none()
            .fill(ui.style().visuals.widgets.inactive.weak_bg_fill)
            .rounding(CORNER_RADIUS)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (tab, label) in &[
                        (AdminTab::Allowlist, "Executor Allowlist"),
                        (AdminTab::Symbols, "Symbols & Policy"),
                        (AdminTab::Criteria, "Criteria Patterns"),
                        (AdminTab::Sockets, "Sockets"),
                        (AdminTab::Branches, "Branches"),
                        (AdminTab::AuditLog, "Audit Log"),
                    ] {
                        let is_active = self.active_tab == *tab;
                        let btn = if is_active {
                            egui::Button::new(RichText::new(*label).strong())
                                .fill(ui.style().visuals.selection.bg_fill)
                        } else {
                            egui::Button::new(*label)
                        };
                        if ui.add(btn).clicked() {
                            self.active_tab = *tab;
                        }
                    }
                });
            });

        ui.add_space(SECTION_SPACING);

        match self.active_tab {
            AdminTab::Allowlist => self.render_allowlist(ui),
            AdminTab::Symbols => self.render_symbols(ui),
            AdminTab::Criteria => self.render_criteria(ui),
            AdminTab::Sockets => self.render_sockets(ui),
            AdminTab::Branches => self.render_branches(ui),
            AdminTab::AuditLog => self.render_audit_log(ui),
        }

        // Status bar
        if !self.status_message.is_empty() {
            ui.add_space(SECTION_SPACING);
            ui.separator();
            ui.horizontal(|ui| {
                ui.colored_label(
                    Color32::from_rgb(135, 206, 250),
                    &self.status_message,
                );
                if ui.small_button("Dismiss").clicked() {
                    self.status_message.clear();
                }
            });
        }

        // Pending confirm dialog
        if let Some(ref dialog) = self.confirm_dialog.clone() {
            egui::Window::new("Confirm Security-Lowering Change")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.colored_label(Color32::YELLOW, &dialog.message);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("CANCEL").clicked() {
                            self.confirm_dialog = None;
                            self.status_message = "Change cancelled.".to_string();
                        }
                        if ui
                            .button("CONFIRM & SAVE")
                            .clicked()
                        {
                            // Dismiss dialog and proceed to save
                            self.confirm_dialog = None;
                            match dialog.action.as_str() {
                                "save_exec" => self.save_exec_allowlist(),
                                "save_symbol_policy" => self.save_symbol_policy(),
                                "save_criteria" => self.save_criteria(),
                                "save_sockets" => self.save_sockets(),
                                "save_branches" => self.save_branches(),
                                _ => {}
                            }
                        }
                    });
                });
        }
    }

    // -----------------------------------------------------------------------
    // Panel renderers
    // -----------------------------------------------------------------------

    fn render_allowlist(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.render_section_header(ui, "Executor Binary Allowlist", 
                "Binaries listed here with `allowed = true` may be executed by the gatekeeper daemon. Shell-free execution is always enforced.");

            let mut remove_idx: Option<usize> = None;
            let mut changed = false;

            egui::Grid::new("allowlist_grid")
                .num_columns(4)
                .striped(true)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    self.render_grid_header(ui, &["Binary", "Path", "Allowed", "Args"]);

                    for (i, row) in self.exec_allowlist.iter_mut().enumerate() {
                        changed |= ui.text_edit_singleline(&mut row.name).changed();
                        changed |= ui.text_edit_singleline(&mut row.path).changed();
                        changed |= ui.checkbox(&mut row.allowed, "").changed();
                        changed |= ui.text_edit_singleline(&mut row.args).changed();
                        if ui.small_button("✕").on_hover_text("Remove entry").clicked() {
                            remove_idx = Some(i);
                        }
                        ui.end_row();
                    }
                });

            if changed {
                self.exec_allowlist_dirty = true;
            }
            if let Some(idx) = remove_idx {
                self.exec_allowlist.remove(idx);
                self.exec_allowlist_dirty = true;
            }

            ui.add_space(SECTION_SPACING);
            ui.horizontal(|ui| {
                if ui.button("➕ Add entry").clicked() {
                    self.exec_allowlist.push(ExecAllowlistRow {
                        name: "new_binary".to_string(),
                        path: String::new(),
                        allowed: true,
                        args: String::new(),
                    });
                    self.exec_allowlist_dirty = true;
                }
                if ui.button("🔄 Reload from disk").clicked() {
                    self.load_exec_allowlist();
                }
                let save_enabled = self.exec_allowlist_dirty && self.confirm_dialog.is_none();
                ui.add_enabled_ui(save_enabled, |ui| {
                    if ui.add(egui::Button::new(RichText::new("💾 Save").strong())).clicked() {
                        self.save_exec_allowlist();
                    }
                });
            });
        });
    }

    /// Helper to render a consistent section header
    fn render_section_header(&self, ui: &mut egui::Ui, title: &str, description: &str) {
        egui::Frame::none()
            .fill(ui.style().visuals.widgets.inactive.weak_bg_fill)
            .rounding(CORNER_RADIUS)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading(RichText::new(title).size(20.0));
                    ui.add_space(4.0);
                    ui.small(description);
                });
            });
        ui.add_space(SECTION_SPACING);
    }

    /// Helper to render grid headers consistently
    fn render_grid_header(&self, ui: &mut egui::Ui, headers: &[&str]) {
        for header in headers {
            ui.strong(RichText::new(*header).size(13.0));
        }
        ui.end_row();
    }

    fn render_symbols(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.render_section_header(ui, "Symbol & Policy Editor",
                "Edit the Delegable/NonDelegable classification and auto-approval timing for each symbol. CRITICAL symbols may NEVER be reclassified as Delegable — validation rejects this.");

            let mut changed = false;

            egui::Grid::new("symbols_grid")
                .num_columns(5)
                .striped(true)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    self.render_grid_header(ui, &["Symbol", "Description", "Classification", "Auto-approve (s)", ""]);

                    for (i, row) in self.symbol_policy.iter_mut().enumerate() {
                        ui.label(RichText::new(&row.name).strong());
                        ui.label(&row.description);
                        let is_critical = row.name.starts_with("CRIT");
                        egui::ComboBox::from_id_salt(format!("cls_{i}"))
                            .selected_text(&row.classification)
                            .show_ui(ui, |ui| {
                                if ui.selectable_value(&mut row.classification, "delegable".to_string(), "Delegable").clicked() && is_critical {
                                    self.status_message = format!(
                                        "WARNING: attempting to reclassify {} (critical) as delegable — will be rejected on save.",
                                        row.name
                                    );
                                }
                                ui.selectable_value(&mut row.classification, "critical".to_string(), "Critical");
                            });
                        if is_critical {
                            ui.label(RichText::new("N/A (Critical)").color(Color32::GRAY));
                        } else {
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut row.auto_approve_seconds)
                                        .speed(10)
                                        .range(0..=3600),
                                )
                                .changed();
                        }
                        if ui.small_button("Remove").clicked() {
                            self.status_message = format!(
                                "Cannot remove symbol {} from the config — symbol vocabulary is fixed in code.",
                                row.name
                            );
                        }
                        ui.end_row();
                    }
                });

            if changed {
                self.symbol_policy_dirty = true;
            }

            ui.add_space(SECTION_SPACING);
            ui.horizontal(|ui| {
                if ui.button("🔄 Reload from disk").clicked() {
                    self.load_symbol_policy();
                }
                let save_enabled = self.symbol_policy_dirty && self.confirm_dialog.is_none();
                ui.add_enabled_ui(save_enabled, |ui| {
                    if ui.add(egui::Button::new(RichText::new("💾 Save").strong())).clicked() {
                        // Check for security-lowering before saving
                        let new_cfg =
                            Self::build_admin_config_from_rows(&[], &self.symbol_policy, &[], &[], &[]);
                        if new_cfg.validate().is_err() {
                            self.save_symbol_policy();
                        } else {
                            // Check classification changes
                            let path = self.find_file("symbol_policy.toml");
                            if let Ok(raw) = fs::read_to_string(&path) {
                                if let Ok(prev) = toml::from_str::<AdminConfig>(&raw) {
                                    if new_cfg.is_security_lowering_change(&prev) {
                                        self.confirm_dialog = Some(ConfirmDialog {
                                            message:
                                                "This change lowers the security posture (e.g. reclassifying a \
                                                 critical symbol or enabling auto-approval for critical symbols). \
                                                 Are you sure you want to proceed? This will be recorded in the audit log."
                                                    .to_string(),
                                            action: "save_symbol_policy".to_string(),
                                        });
                                        return;
                                    }
                                }
                            }
                            self.save_symbol_policy();
                        }
                    }
                });
            });
        });
    }

    fn render_criteria(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.render_section_header(ui, "Safety Criteria Pattern Editor",
                "These keyword/pattern lists are used by the criteria engine to classify commands as NonDelegable. Relaxing patterns (removing triggers) lowers security and is flagged.");

            let mut changed = false;
            let mut remove_idx: Option<usize> = None;

            for (i, row) in self.criteria.iter_mut().enumerate() {
                egui::Frame::none()
                    .fill(ui.style().visuals.widgets.inactive.weak_bg_fill)
                    .stroke(Stroke::new(1.0, Color32::from_rgb(50, 55, 75)))
                    .rounding(CORNER_RADIUS)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.strong(&row.category);
                            if ui.small_button("✕ Remove category").clicked() {
                                remove_idx = Some(i);
                            }
                        });
                        ui.add_space(4.0);
                        ui.label("Patterns (one per line, regex):");
                        changed |= ui
                            .add(
                                egui::TextEdit::multiline(&mut row.patterns_text)
                                    .desired_rows(4)
                                    .desired_width(f32::INFINITY)
                                    .font(egui::FontId::monospace(13.0)),
                            )
                            .changed();
                    });
                ui.add_space(4.0);
            }

            if let Some(idx) = remove_idx {
                self.criteria.remove(idx);
                self.criteria_dirty = true;
            }
            if changed {
                self.criteria_dirty = true;
            }

            ui.add_space(SECTION_SPACING);
            ui.horizontal(|ui| {
                if ui.button("➕ Add category").clicked() {
                    self.criteria.push(CriteriaRow {
                        category: "new_category".to_string(),
                        patterns_text: String::new(),
                    });
                    self.criteria_dirty = true;
                }
            });

            // Live test tool
            ui.add_space(SECTION_SPACING);
            egui::Frame::none()
                .fill(ui.style().visuals.widgets.inactive.weak_bg_fill)
                .rounding(CORNER_RADIUS)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.heading(RichText::new("Live Test: Evaluate a sample command").size(16.0));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Command:");
                        let resp = ui.text_edit_singleline(&mut self.test_command);
                        if ui.button("Evaluate").clicked()
                            || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            self.test_result = self.run_test_command();
                        }
                    });
                    if let Some(ref result) = self.test_result {
                        let color = if result.status == ai_bridge_gatekeeper_core::criteria::EvaluationStatus::Delegable {
                            Color32::GREEN
                        } else {
                            Color32::RED
                        };
                        let status_text = if result.status == ai_bridge_gatekeeper_core::criteria::EvaluationStatus::Delegable {
                            "DELEGABLE"
                        } else {
                            "NON-DELEGABLE"
                        };
                        ui.horizontal(|ui| {
                            ui.label("Result:");
                            ui.colored_label(color, RichText::new(status_text).strong());
                        });
                        if !result.triggered.is_empty() {
                            ui.label(format!(
                                "Triggered criteria: {}",
                                result
                                    .triggered
                                    .iter()
                                    .map(|c| c.label())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
                    }
                });

            ui.add_space(SECTION_SPACING);
            ui.horizontal(|ui| {
                if ui.button("🔄 Reload from disk").clicked() {
                    self.load_criteria();
                }
                let save_enabled = self.criteria_dirty && self.confirm_dialog.is_none();
                ui.add_enabled_ui(save_enabled, |ui| {
                    if ui.add(egui::Button::new(RichText::new("💾 Save").strong())).clicked() {
                        self.save_criteria();
                    }
                });
            });
        });
    }

    fn render_sockets(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.render_section_header(ui, "Channels & Sockets Manager",
                "Socket definitions are loaded at daemon startup. Changes require a daemon restart. Identity verification (SO_PEERCRED UID binding) is enforced in code and cannot be disabled or bypassed from this UI.");

            let mut changed = false;
            let mut remove_idx: Option<usize> = None;

            egui::Grid::new("sockets_grid")
                .num_columns(4)
                .striped(true)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    self.render_grid_header(ui, &["Name", "Path", "Permissions", ""]);

                    for (i, row) in self.sockets.iter_mut().enumerate() {
                        ui.label(&row.name);
                        changed |= ui.text_edit_singleline(&mut row.path).changed();
                        changed |= ui.text_edit_singleline(&mut row.permissions).changed();
                        if ui.small_button("✕").on_hover_text("Remove socket").clicked() {
                            remove_idx = Some(i);
                        }
                        ui.end_row();
                    }
                });

            if changed {
                self.sockets_dirty = true;
            }
            if let Some(idx) = remove_idx {
                self.sockets.remove(idx);
                self.sockets_dirty = true;
            }

            ui.add_space(SECTION_SPACING);
            ui.horizontal(|ui| {
                if ui.button("➕ Add socket").clicked() {
                    self.sockets.push(SocketRow {
                        name: "new_socket".to_string(),
                        path: "new_socket.sock".to_string(),
                        permissions: "0600".to_string(),
                    });
                    self.sockets_dirty = true;
                }
                if ui.button("🔄 Reload from disk").clicked() {
                    self.load_sockets();
                }
                let save_enabled = self.sockets_dirty && self.confirm_dialog.is_none();
                ui.add_enabled_ui(save_enabled, |ui| {
                    if ui.add(egui::Button::new(RichText::new("💾 Save").strong())).clicked() {
                        // Check for security-lowering (removing a socket)
                        let new_cfg = Self::build_admin_config_from_rows(
                            &[], &[], &[], &self.sockets, &[],
                        );
                        let path = self.find_file("sockets.toml");
                        if let Ok(raw) = fs::read_to_string(&path) {
                            if let Ok(prev) = toml::from_str::<AdminConfig>(&raw) {
                                if new_cfg.is_security_lowering_change(&prev) {
                                    self.confirm_dialog = Some(ConfirmDialog {
                                        message:
                                            "This change removes a socket definition, which may reduce \
                                             the monitored attack surface. Proceed?"
                                                .to_string(),
                                        action: "save_sockets".to_string(),
                                    });
                                    return;
                                }
                            }
                        }
                        self.save_sockets();
                    }
                });
            });
        });
    }

    fn render_branches(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.render_section_header(ui, "Sub-chat / Branch Configuration",
                "Adjust the per-task sub-chat limit for each branch. Context merging can be toggled. The `call_index` field is derived from the sub-chat counter and cannot be overridden.");

            let mut changed = false;

            egui::Grid::new("branches_grid")
                .num_columns(3)
                .striped(true)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    self.render_grid_header(ui, &["Branch", "Sub-chat limit", "Context enabled"]);

                    for row in self.branches.iter_mut() {
                        ui.label(&row.name);
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut row.sub_chat_limit)
                                    .speed(1)
                                    .range(1..=10),
                            )
                            .changed();
                        changed |= ui.checkbox(&mut row.context_enabled, "").changed();
                        ui.end_row();
                    }
                });

            if changed {
                self.branches_dirty = true;
            }

            ui.add_space(SECTION_SPACING);
            ui.horizontal(|ui| {
                if ui.button("🔄 Reload from disk").clicked() {
                    self.load_branches();
                }
                let save_enabled = self.branches_dirty && self.confirm_dialog.is_none();
                ui.add_enabled_ui(save_enabled, |ui| {
                    if ui.add(egui::Button::new(RichText::new("💾 Save").strong())).clicked() {
                        self.save_branches();
                    }
                });
            });
        });
    }

    fn render_audit_log(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.render_section_header(ui, "Execution Log & Audit Trail",
                "Every configuration change made through this UI is recorded here. Entries are append-only and stored in the `audit/` directory.");

            if self.audit_entries.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new("No audit entries yet.").color(Color32::GRAY).size(14.0));
                });
                return;
            }

            egui::ScrollArea::vertical()
                .max_height(500.0)
                .show(ui, |ui| {
                    for entry in self.audit_entries.iter().rev() {
                        let is_security = entry.security_lowering;
                        egui::Frame::none()
                            .fill(if is_security {
                                Color32::from_rgb(40, 20, 20)
                            } else {
                                ui.style().visuals.widgets.inactive.weak_bg_fill
                            })
                            .stroke(Stroke::new(
                                1.0,
                                if is_security {
                                    Color32::RED
                                } else {
                                    Color32::from_rgb(50, 55, 75)
                                },
                            ))
                            .rounding(CORNER_RADIUS)
                            .inner_margin(12.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&entry.timestamp).monospace().size(11.0));
                                    ui.add_space(8.0);
                                    ui.label(RichText::new(&entry.action).strong().size(12.0));
                                    ui.add_space(8.0);
                                    ui.label(RichText::new(&entry.changed_file).italics().size(12.0));
                                    if is_security {
                                        ui.add_space(8.0);
                                        ui.colored_label(Color32::RED, RichText::new("⚠ SECURITY LOWERING").strong().size(11.0));
                                    }
                                });
                                ui.add_space(6.0);
                                if !entry.old_value.is_empty() {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Previous:").strong().size(11.0).color(Color32::GRAY));
                                        ui.add_space(4.0);
                                        ui.label(RichText::new(&entry.old_value).monospace().size(11.0).color(Color32::LIGHT_GRAY));
                                    });
                                }
                                if !entry.new_value.is_empty() {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("New:").strong().size(11.0).color(Color32::GRAY));
                                        ui.add_space(4.0);
                                        ui.label(RichText::new(&entry.new_value).monospace().size(11.0));
                                    });
                                }
                            });
                        ui.add_space(6.0);
                    }
                });
        });
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn write_with_backup(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    let _ = admin_config::backup_config(path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}
