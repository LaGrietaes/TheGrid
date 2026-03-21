// ═══════════════════════════════════════════════════════════════════════════════
// views/dashboard.rs — Main Application Dashboard  [v0.2 — Phase 2]
//
// FIXES from v0.1:
//   ✓ render_actions/files/clipboard_tab now take `s: &mut DetailState`
//     (was `&DetailState` — can't get &mut String fields through & reference)
//   ✓ egui::ComboBox::from_id_source → from_id_source  (egui 0.27 deprecation)
//   ✓ FileTransferStatus::Failed(e) → Failed(_)  (unused variable warning)
//   ✓ DetailActions::download_file: Option<String> for per-file downloads
//   ✓ id_source → id_source on all ScrollAreas
//
// NEW in v0.2:
//   + SettingsState + render_settings_modal  — in-app config modal
//   + watch_paths field in DetailState       — Phase 2 watcher UI
//   + Refresh button moved into device panel — removes it from &self titlebar
//   + Remote file list: each ↓ button returns the filename via DetailActions
//   + Clipboard inbox items: click to populate clip_out for inspection
// ═══════════════════════════════════════════════════════════════════════════════

use egui::{Color32, RichText, Ui, ScrollArea};
use thegrid_core::models::*;
use crate::theme::{self, Colors};

/// Which tab is active in the detail panel
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DashTab {
    #[default] Actions,
    Files,
    Clipboard,
    /// Phase 3: The Flow — temporal file activity view
    Timeline,
    /// New in Node Enhancement: Remote Terminal
    Terminal,
}

// ─────────────────────────────────────────────────────────────────────────────
// DetailState — all mutable references the detail panel needs
// ─────────────────────────────────────────────────────────────────────────────

pub struct DetailState<'a> {
    pub device:         &'a TailscaleDevice,
    pub active_tab:     &'a mut DashTab,
    pub rdp_username:   &'a mut String,
    pub rdp_resolution: &'a mut String,
    pub clip_out:       &'a mut String,
    pub clip_inbox:     &'a [ClipboardEntry],
    pub file_queue:     &'a [FileQueueItem],
    pub remote_files:   &'a [RemoteFile],
    pub transfer_log:   &'a [TransferLogEntry],
    pub is_tg_agent:    bool,
    /// Phase 2: directories this machine is currently watching
    pub watch_paths:    &'a [std::path::PathBuf],
    /// Phase 3: live telemetry for this device (None = not yet fetched)
    pub telemetry:      Option<&'a thegrid_core::models::NodeTelemetry>,
    /// New in Node Enhancement: tracks the current directory being browsed
    pub current_remote_path: &'a mut std::path::PathBuf,
    /// New in Node Enhancement: tracks the model name being typed in the UI
    /// New in Node Enhancement: tracks the model name being typed in the UI
    pub remote_model_edit: &'a mut String,
    /// New in Node Enhancement: the terminal view object
    pub terminal_view:      Option<&'a mut crate::views::terminal::TerminalView>,
}

// ─────────────────────────────────────────────────────────────────────────────
// DetailActions — returned from render_detail_panel each frame
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct DetailActions {
    pub launch_rdp:      bool,
    pub browse_share:    bool,
    pub ping:            bool,
    pub send_clipboard:  bool,
    pub load_clipboard:  bool,
    pub select_files:    bool,
    pub scan_remote:     bool,
    pub open_inbox:      bool,
    pub add_watch_path:  bool,
    /// Some(filename) when user clicks ↓ on a specific remote file
    pub download_file:   Option<String>,
    /// Phase 3: request telemetry fetch for this device
    pub fetch_telemetry: bool,
    /// Phase 3: wake sleeping device via WoL
    pub wake_device:     bool,
    /// Phase 3: load timeline data
    pub load_timeline:   bool,
    /// New in Node Enhancement: browse a specific remote path
    pub browse_remote:   Option<std::path::PathBuf>,
    /// New in Node Enhancement: download a file from any path
    pub download_remote_file: Option<std::path::PathBuf>,
    /// New in Node Enhancement: update remote AI model config (model, url)
    pub update_remote_config: Option<(Option<String>, Option<String>)>,
    /// New in Node Enhancement: terminal actions
    pub create_terminal:      bool,
    pub send_terminal_input:  Option<Vec<u8>>,
    pub launch_scrcpy:        bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Device panel (left sidebar)
// ─────────────────────────────────────────────────────────────────────────────

/// Render the left device panel.
/// Returns: (clicked device index, refresh requested)
pub fn render_device_panel(
    ui: &mut Ui,
    devices: &[TailscaleDevice],
    telemetries: &std::collections::HashMap<String, thegrid_core::models::NodeTelemetry>,
    selected_idx: Option<usize>,
    filter: &mut String,
    needs_refresh: &mut bool,
) -> Option<usize> {
    let mut clicked = None;

    // ── Header ────────────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(16.0, 12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("// NODES")
                        .color(Colors::GREEN).size(9.0).strong()
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Refresh button lives here — avoids the &self / &mut self conflict
                    // that plagued v0.1's titlebar implementation
                    // Refresh Vector Button
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                    let color = if resp.hovered() { Colors::TEXT } else { Colors::TEXT_DIM };
                    let c = rect.center();
                    ui.painter().circle_stroke(c, 4.5, egui::Stroke::new(1.2, color));
                    // Arrow tip
                    ui.painter().line_segment([c + egui::vec2(3.0, -3.0), c + egui::vec2(6.0, -5.0)], egui::Stroke::new(1.2, color));
                    ui.painter().line_segment([c + egui::vec2(3.0, -3.0), c + egui::vec2(5.0, -1.0)], egui::Stroke::new(1.2, color));
                    if resp.clicked() {
                        *needs_refresh = true;
                    }
                    ui.label(
                        RichText::new(format!("{}", devices.len()))
                            .color(Colors::TEXT_DIM).size(9.0)
                    );
                });
            });
        });

    ui.add(egui::Separator::default().spacing(0.0));

    // ── Search ────────────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Vector Search Magnifier
                let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                let c = rect.center() - egui::vec2(1.0, 1.0);
                ui.painter().circle_stroke(c, 3.5, egui::Stroke::new(1.0, Colors::TEXT_MUTED));
                ui.painter().line_segment(
                    [c + egui::vec2(2.5, 2.5), c + egui::vec2(5.0, 5.0)],
                    egui::Stroke::new(1.2, Colors::TEXT_MUTED)
                );
                ui.add(
                    egui::TextEdit::singleline(filter)
                        .hint_text("FILTER NODES...")
                        .font(egui::FontId::new(10.0, egui::FontFamily::Monospace))
                        .desired_width(f32::INFINITY)
                        .frame(false)
                );
            });
        });

    ui.add(egui::Separator::default().spacing(0.0));

    // ── Device list ───────────────────────────────────────────────────────────
    let filter_lower = filter.to_lowercase();
    ScrollArea::vertical()
        .id_source("device_list_scroll") // FIX: id_source replaces deprecated id_source
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            if devices.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("◌").color(Colors::TEXT_MUTED).size(24.0));
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("SCANNING NETWORK...")
                            .color(Colors::TEXT_MUTED).size(10.0)
                    );
                });
                return;
            }

            for (idx, device) in devices.iter().enumerate() {
                let matches = filter_lower.is_empty()
                    || device.hostname.to_lowercase().contains(&filter_lower)
                    || device.name.to_lowercase().contains(&filter_lower)
                    || device.addresses.iter().any(|ip| ip.contains(&filter_lower));

                if !matches { continue; }

                let is_selected = selected_idx == Some(idx);
                let is_online   = device.is_likely_online();
                let bg          = if is_selected { Colors::BG_ACTIVE } else { Color32::TRANSPARENT };

                let resp = egui::Frame::none()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(16.0, 10.0))
                    .show(ui, |ui| {
                        if is_selected {
                            // Left green selection stripe
                            let r = ui.min_rect();
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    egui::pos2(r.min.x - 16.0, r.min.y - 10.0),
                                    egui::vec2(2.0, r.height() + 20.0),
                                ),
                                egui::Rounding::ZERO,
                                Colors::GREEN,
                            );
                        }
                        ui.horizontal(|ui| {
                            theme::status_dot(ui, is_online);
                            ui.add_space(2.0);
                            
                            // Sidebar Row Vector Icon (using telemetry lookup)
                            let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                            let device_type = telemetries.get(&device.id).map(|t| t.device_type.as_str()).unwrap_or("Desktop");
                            let icon_type = match device_type {
                                "Laptop" => theme::IconType::Laptop,
                                "Server" => theme::IconType::Server,
                                _ => theme::IconType::Desktop,
                            };
                            theme::draw_vector_icon(ui, icon_rect, icon_type, if is_online { Colors::GREEN } else { Colors::TEXT_MUTED });
                            
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(device.display_name().to_uppercase())
                                        .color(Colors::TEXT).size(11.0).strong()
                                );
                                ui.label(
                                    RichText::new(device.primary_ip().unwrap_or("—"))
                                        .color(Colors::TEXT_DIM).size(9.0)
                                );
                            });
                        });
                    }).response;

                let interact = ui.interact(
                    resp.rect,
                    egui::Id::new(("dev_row", idx)),
                    egui::Sense::click(),
                );
                if interact.hovered() && !is_selected {
                    ui.painter().rect_stroke(
                        resp.rect, egui::Rounding::ZERO,
                        egui::Stroke::new(1.0, Colors::BORDER2),
                    );
                }
                if interact.clicked() { clicked = Some(idx); }
            }
        });

    clicked
}

// ─────────────────────────────────────────────────────────────────────────────
// Detail panel
// ─────────────────────────────────────────────────────────────────────────────

pub fn render_detail_panel(ui: &mut Ui, s: &mut DetailState) -> DetailActions {
    let mut actions = DetailActions::default();
    let is_online = s.device.is_likely_online();

    // ── Device header ─────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(24.0, 16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let device_type = s.telemetry.map(|t| t.device_type.as_str()).unwrap_or("Desktop");
                let icon_type = match device_type {
                    "Laptop" => theme::IconType::Laptop,
                    "Server" => theme::IconType::Server,
                    _ => theme::IconType::Desktop,
                };
                crate::theme::render_crt_icon(ui, icon_type, 28.0, Colors::GREEN);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(s.device.display_name().to_uppercase())
                            .color(Colors::TEXT).size(16.0).strong()
                    );
                    ui.label(
                        RichText::new(s.device.primary_ip().unwrap_or("No Tailscale IP"))
                            .color(Colors::CYAN).size(10.0)
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            s.device.os.to_uppercase(),
                            s.device.client_version
                        ))
                        .color(Colors::TEXT_DIM).size(9.0)
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme::status_badge(
                        ui,
                        if is_online { "ONLINE" } else { "OFFLINE" },
                        is_online,
                    );
                    if s.is_tg_agent {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("⬡ AGENT").color(Colors::CYAN).size(9.0)
                        );
                    }
                    if !is_online {
                        ui.add_space(8.0);
                        if ui.button(
                            RichText::new("WAKE NODE").color(Colors::CYAN).size(9.0).strong()
                        ).clicked() {
                            actions.wake_device = true;
                        }
                    }
                });
            });
        });

    ui.add(egui::Separator::default().spacing(0.0));

    // ── Tab bar ───────────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        for (label, tab_variant) in [
            ("ACTIONS",   DashTab::Actions),
            ("FILES",     DashTab::Files),
            ("CLIPBOARD", DashTab::Clipboard),
            ("TIMELINE",  DashTab::Timeline),
        ] {
            let is_active = *s.active_tab == tab_variant;
            let color = if is_active { Colors::GREEN } else { Colors::TEXT_DIM };
            let resp = ui.add(
                egui::Button::new(RichText::new(label).color(color).size(9.0).strong())
                    .fill(Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE)
                    .min_size(egui::vec2(88.0, 36.0))
            );
            if is_active {
                let r = resp.rect;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(r.min.x, r.max.y - 2.0),
                        egui::vec2(r.width(), 2.0),
                    ),
                    egui::Rounding::ZERO,
                    Colors::GREEN,
                );
            }
            if resp.clicked() { *s.active_tab = tab_variant; }
        }
    });

    ui.add(egui::Separator::default().spacing(0.0));

    // ── Tab content ───────────────────────────────────────────────────────────
    ScrollArea::vertical()
        .id_source("detail_content_scroll")
        .show(ui, |ui| {
            ui.add_space(16.0);
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(24.0, 0.0))
                .show(ui, |ui| {
                    // Clone the tab to avoid simultaneous borrow of s.active_tab
                    // while we pass `s` mutably into the sub-render functions.
                    let tab = s.active_tab.clone();
                    match tab {
                        DashTab::Actions   => render_actions_tab(ui, s, &mut actions),
                        DashTab::Files     => render_files_tab(ui, s, &mut actions),
                        DashTab::Clipboard => render_clipboard_tab(ui, s, &mut actions),
                        DashTab::Timeline  => {
                            // Minimal placeholder for the non-timeline version of the dashboard
                            ui.label("Timeline is not available in this view configuration.");
                        }
                        DashTab::Terminal  => render_terminal_tab(ui, s, &mut actions),
                    }
                });
            ui.add_space(16.0);
        });

    actions
}

// ─────────────────────────────────────────────────────────────────────────────
// ACTIONS tab
// ─────────────────────────────────────────────────────────────────────────────

// FIX: was `fn render_actions_tab(ui, s: &DetailState, ...)` — &DetailState
// cannot hand out &mut to its inner &'a mut String fields.
// All three tab functions now take `s: &mut DetailState`.
fn render_actions_tab(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    ui.columns(3, |cols| {
        if action_card(&mut cols[0], theme::IconType::RDP, "REMOTE DESKTOP", "Launch RDP session") {
            actions.launch_rdp = true;
        }
        if action_card(&mut cols[1], theme::IconType::Folder, "BROWSE FILES", "Open network share") {
            actions.browse_share = true;
        }
        if action_card(&mut cols[2], theme::IconType::Pulse, "PING NODE", "Check THE GRID agent") {
            actions.ping = true;
        }
    });

    if s.is_tg_agent {
        ui.add_space(8.0);
        ui.columns(3, |cols| {
            if action_card(&mut cols[0], theme::IconType::Desktop, "SCREEN MIRROR", "Launch Scrcpy (ADB over IP)") {
                actions.launch_scrcpy = true;
            }
            // Future slots
        });
    }

    ui.add_space(20.0);
    ui.add(egui::Separator::default().spacing(0.0));
    ui.add_space(16.0);

    // ── RDP Options ───────────────────────────────────────────────────────────
    theme::section_title(ui, "// RDP OPTIONS");
    ui.add_space(8.0);

    ui.columns(2, |cols| {
        cols[0].label(
            RichText::new("USERNAME").color(Colors::TEXT_DIM).size(8.0).strong()
        );
        cols[0].add_space(4.0);
        cols[0].add(
            egui::TextEdit::singleline(s.rdp_username)
                .font(egui::FontId::new(11.0, egui::FontFamily::Monospace))
                .hint_text("Administrator")
                .desired_width(f32::INFINITY)
        );

        cols[1].label(
            RichText::new("RESOLUTION").color(Colors::TEXT_DIM).size(8.0).strong()
        );
        cols[1].add_space(4.0);
        // FIX: from_id_source replaces deprecated from_id_source (egui 0.27)
        egui::ComboBox::from_id_source("rdp_resolution_combo")
            .width(cols[1].available_width())
            .selected_text(
                RichText::new(s.rdp_resolution.as_str())
                    .font(egui::FontId::new(11.0, egui::FontFamily::Monospace))
            )
            .show_ui(&mut cols[1], |ui| {
                for opt in ["FULLSCREEN", "1920×1080", "2560×1440", "1280×800"] {
                    ui.selectable_value(s.rdp_resolution, opt.to_string(), opt);
                }
            });
    });

    ui.add_space(20.0);
    ui.add(egui::Separator::default().spacing(0.0));
    ui.add_space(16.0);

    // ── Node info table ───────────────────────────────────────────────────────
    theme::section_title(ui, "// NODE INFO");
    ui.add_space(8.0);

    let d = s.device;
    egui::Grid::new("node_info_grid")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            for (k, v) in [
                ("HOSTNAME",    d.hostname.as_str()),
                ("IP",          d.primary_ip().unwrap_or("—")),
                ("OS",          d.os.as_str()),
                ("CLIENT",      d.client_version.as_str()),
                ("USER",        d.user.as_str()),
                ("AUTHORIZED",  if d.authorized { "YES" } else { "NO" }),
            ] {
                ui.label(RichText::new(k).color(Colors::TEXT_DIM).size(10.0));
                ui.label(RichText::new(v.to_uppercase()).color(Colors::TEXT).size(10.0).strong());
                ui.end_row();
            }
            // Last seen formatted separately (needs owned String)
            ui.label(RichText::new("LAST SEEN").color(Colors::TEXT_DIM).size(10.0));
            ui.label(
                RichText::new(
                    d.last_seen
                        .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                        .unwrap_or_else(|| "—".into())
                )
                .color(Colors::TEXT).size(10.0).strong()
            );
            ui.end_row();
        });

    // ── Phase 2: Watched paths ────────────────────────────────────────────────
    ui.add_space(20.0);
    ui.add(egui::Separator::default().spacing(0.0));
    ui.add_space(16.0);
    theme::section_title(ui, "// WATCHED PATHS");
    ui.add_space(8.0);

    if s.watch_paths.is_empty() {
        ui.label(
            RichText::new("No directories watched yet")
                .color(Colors::TEXT_MUTED).size(9.0)
        );
    } else {
        for path in s.watch_paths {
            ui.horizontal(|ui| {
                ui.label(RichText::new("◈").color(Colors::GREEN).size(9.0));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(path.display().to_string())
                        .color(Colors::TEXT_DIM).size(9.0)
                );
            });
        }
    }
    ui.add_space(8.0);
    if theme::secondary_button(ui, "+ ADD WATCH DIRECTORY").clicked() {
        actions.add_watch_path = true;
    }

    // ── AI Model Setup ──
    if s.is_tg_agent {
        ui.add_space(20.0);
        ui.add(egui::Separator::default().spacing(0.0));
        ui.add_space(16.0);
        theme::section_title(ui, "// REMOTE AI SETUP");
        ui.add_space(8.0);

        // Auto-fill placeholder from telemetry if empty
        if s.remote_model_edit.is_empty() {
            if let Some(t) = s.telemetry {
                if let Some(m) = t.capabilities.ai_models.first() {
                    *s.remote_model_edit = m.clone();
                }
            }
        }
        
        ui.columns(2, |cols| {
            cols[0].label(RichText::new("AI MODEL").color(Colors::TEXT_DIM).size(8.0).strong());
            cols[0].add_space(4.0);
            cols[0].add(egui::TextEdit::singleline(s.remote_model_edit).hint_text("e.g. llama3").desired_width(f32::INFINITY));

            cols[1].label(RichText::new("PROVIDER URL").color(Colors::TEXT_DIM).size(8.0).strong());
            cols[1].add_space(4.0);
            cols[1].label(RichText::new("Uses remote node's local config.").color(Colors::TEXT_MUTED).size(8.0));
        });

        ui.add_space(8.0);
        if theme::secondary_button(ui, "UPDATE REMOTE CONFIG").clicked() {
            actions.update_remote_config = Some((Some(s.remote_model_edit.clone()), None));
        }
    }
}

fn render_terminal_tab(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    if let Some(terminal) = s.terminal_view.as_deref_mut() {
        // If the terminal doesn't have a session, show a "Create" button
        if terminal.session_id.is_none() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(RichText::new("TERMINAL SESSION NOT ACTIVE").color(Colors::TEXT_DIM).size(10.0));
                ui.add_space(12.0);
                if theme::primary_button(ui, "SPAWN REMOTE SHELL").clicked() {
                    actions.create_terminal = true;
                }
            });
        } else {
            // Render the terminal view and capture input
            if let Some(input) = terminal.ui(ui) {
                actions.send_terminal_input = Some(input.into_bytes());
            }
        }
    } else {
        ui.label(RichText::new("TERMINAL INITIALIZING...").color(Colors::TEXT_DIM));
    }
}

fn action_card(ui: &mut Ui, icon: theme::IconType, label: &str, sub: &str) -> bool {
    let resp = egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(ui.available_width(), 80.0));
            ui.vertical_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                theme::draw_vector_icon(ui, rect, icon, Colors::GREEN);
                ui.add_space(6.0);
                ui.label(RichText::new(label).color(Colors::TEXT).size(9.0).strong());
                ui.label(RichText::new(sub).color(Colors::TEXT_DIM).size(8.0));
            });
        }).response;

    let interact = ui.interact(
        resp.rect,
        egui::Id::new(("action_card", label)),
        egui::Sense::click(),
    );
    if interact.hovered() {
        ui.painter().rect_stroke(
            resp.rect, egui::Rounding::ZERO,
            egui::Stroke::new(1.0, Colors::GREEN),
        );
    }
    interact.clicked()
}

// ─────────────────────────────────────────────────────────────────────────────
// FILES tab
// ─────────────────────────────────────────────────────────────────────────────

fn render_files_tab(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    ui.columns(2, |cols| {

        // ── Send column ───────────────────────────────────────────────────────
        // ── Send column (Vector Arrow) ─────────────────────────────────────────
        cols[0].horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            let c = rect.center();
            ui.painter().line_segment([c + egui::vec2(0.0, 5.0), c + egui::vec2(0.0, -5.0)], egui::Stroke::new(1.2, Colors::TEXT_DIM));
            ui.painter().line_segment([c + egui::vec2(-3.0, -2.0), c + egui::vec2(0.0, -5.0)], egui::Stroke::new(1.2, Colors::TEXT_DIM));
            ui.painter().line_segment([c + egui::vec2(3.0, -2.0), c + egui::vec2(0.0, -5.0)], egui::Stroke::new(1.2, Colors::TEXT_DIM));
            ui.add_space(4.0);
            ui.label(RichText::new("SEND TO NODE").color(Colors::TEXT_DIM).size(9.0).strong());
        });
        cols[0].add_space(8.0);

        let hovering = cols[0].ctx().input(|i| !i.raw.hovered_files.is_empty());
        let dz_stroke = if hovering {
            egui::Stroke::new(1.0, Colors::GREEN)
        } else {
            egui::Stroke::new(1.0, Colors::BORDER)
        };

        egui::Frame::none()
            .fill(Colors::BG_WIDGET)
            .stroke(dz_stroke)
            .inner_margin(egui::Margin::same(16.0))
            .show(&mut cols[0], |ui| {
                ui.set_min_size(egui::vec2(ui.available_width(), 80.0));
                ui.vertical_centered(|ui| {
                    // Vector Hexagon Placeholder
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                    let c = rect.center();
                    let r = 10.0;
                    let mut points = vec![];
                    for i in 0..6 {
                        let angle = std::f32::consts::PI / 3.0 * i as f32 + std::f32::consts::PI / 2.0;
                        points.push(c + egui::vec2(r * angle.cos(), r * angle.sin()));
                    }
                    ui.painter().add(egui::Shape::convex_polygon(points, Color32::TRANSPARENT, egui::Stroke::new(1.5, Colors::TEXT_MUTED)));
                    
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("DROP FILES HERE")
                            .color(Colors::TEXT_MUTED).size(9.0)
                    );
                    ui.label(
                        RichText::new("or SELECT FILES below")
                            .color(Colors::TEXT_MUTED).size(8.0)
                    );
                });
            });

        cols[0].add_space(8.0);

        // File queue status
        for item in s.file_queue {
            let (label, color) = match &item.status {
                FileTransferStatus::Pending   => ("PENDING", Colors::TEXT_MUTED),
                FileTransferStatus::Sending   => ("SENDING", Colors::AMBER),
                FileTransferStatus::Done      => ("✓ DONE",  Colors::GREEN),
                // FIX: was Failed(e) — 'e' unused, suppress warning with _
                FileTransferStatus::Failed(_) => ("✗ FAIL",  Colors::RED),
            };
            cols[0].horizontal(|ui| {
                ui.label(RichText::new(&item.name).color(Colors::TEXT).size(9.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(label).color(color).size(8.0));
                    ui.label(
                        RichText::new(fmt_bytes(item.size))
                            .color(Colors::TEXT_DIM).size(8.0)
                    );
                });
            });
        }

        cols[0].add_space(8.0);
        if theme::secondary_button(&mut cols[0], "SELECT FILES").clicked() {
            actions.select_files = true;
        }

        // ── Receive column ────────────────────────────────────────────────────
        // ── Receive column (Vector Arrow) ──────────────────────────────────────
        cols[1].horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            let c = rect.center();
            ui.painter().line_segment([c + egui::vec2(0.0, -5.0), c + egui::vec2(0.0, 5.0)], egui::Stroke::new(1.2, Colors::TEXT_DIM));
            ui.painter().line_segment([c + egui::vec2(-3.0, 2.0), c + egui::vec2(0.0, 5.0)], egui::Stroke::new(1.2, Colors::TEXT_DIM));
            ui.painter().line_segment([c + egui::vec2(3.0, 2.0), c + egui::vec2(0.0, 5.0)], egui::Stroke::new(1.2, Colors::TEXT_DIM));
            ui.add_space(4.0);
            ui.label(
                RichText::new("RECEIVE FROM NODE")
                    .color(Colors::TEXT_DIM).size(9.0).strong()
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::micro_button(ui, "SCAN").clicked() {
                    actions.scan_remote = true;
                }
            });
        });
        cols[1].add_space(8.0);

        // Remote file list
        egui::Frame::none()
            .fill(Colors::BG_WIDGET)
            .stroke(egui::Stroke::new(1.0, Colors::BORDER))
            .show(&mut cols[1], |ui| {
                ui.set_min_height(120.0);
                
                // ── Path breadcrumb / Back button ──
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    if s.current_remote_path.as_os_str().is_empty() {
                        ui.label(RichText::new("NOT BROWSING REMOTE").color(Colors::TEXT_MUTED).size(9.0));
                    } else {
                        if theme::micro_button(ui, "ᐊ BACK").clicked() {
                            let parent = s.current_remote_path.parent().unwrap_or(std::path::Path::new(""));
                            actions.browse_remote = Some(parent.to_path_buf());
                        }
                        ui.add_space(4.0);
                        ui.label(RichText::new(s.current_remote_path.display().to_string()).color(Colors::CYAN).size(9.0).strong());
                    }
                });
                ui.add(egui::Separator::default().spacing(4.0));

                if s.remote_files.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(16.0);
                        ui.label(RichText::new("NO FILES OR NOT SCANNED").color(Colors::TEXT_MUTED).size(9.0));
                        if theme::secondary_button(ui, "BROWSE ROOT").clicked() {
                            actions.browse_remote = Some(std::path::PathBuf::new());
                        }
                    });
                } else {
                    ScrollArea::vertical()
                        .id_source("remote_file_list")
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for rf in s.remote_files {
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    let icon = if rf.is_dir { "📁" } else { "📄" };
                                    ui.label(RichText::new(icon).color(Colors::TEXT_MUTED).size(9.0));
                                    ui.add_space(4.0);
                                    
                                    let label_color = if rf.is_dir { Colors::CYAN } else { Colors::TEXT };
                                    let resp = ui.add(egui::Label::new(RichText::new(&rf.name).color(label_color).size(10.0)).sense(egui::Sense::click()));
                                    
                                    if resp.clicked() {
                                        if rf.is_dir {
                                            let mut new_path = s.current_remote_path.clone();
                                            new_path.push(&rf.name);
                                            actions.browse_remote = Some(new_path);
                                        }
                                    }

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if !rf.is_dir {
                                            // FIX: DL button for files
                                            let dl_btn = ui.add(
                                                egui::Button::new(RichText::new(format!("{} DL", crate::icons::Glyphs::DOWNLOAD)).color(Colors::TEXT_DIM).size(8.0))
                                                .fill(Color32::TRANSPARENT).stroke(egui::Stroke::new(1.0, Colors::BORDER)).min_size(egui::vec2(0.0, 20.0))
                                            );
                                            if dl_btn.clicked() {
                                                if s.current_remote_path.as_os_str().is_empty() {
                                                    // Transfers dir fallback
                                                    actions.download_file = Some(rf.name.clone());
                                                } else {
                                                    let mut full_path = s.current_remote_path.clone();
                                                    full_path.push(&rf.name);
                                                    actions.download_remote_file = Some(full_path);
                                                }
                                            }
                                            ui.label(RichText::new(fmt_bytes(rf.size)).color(Colors::TEXT_DIM).size(8.0));
                                        }
                                    });
                                });
                                ui.add(egui::Separator::default().spacing(2.0));
                            }
                        });
                }
            });

        cols[1].add_space(8.0);
        if theme::secondary_button(&mut cols[1], "OPEN INBOX FOLDER").clicked() {
            actions.open_inbox = true;
        }
    });

    // ── Transfer log ──────────────────────────────────────────────────────────
    ui.add_space(12.0);
    theme::section_title(ui, "// TRANSFER LOG");
    ui.add_space(6.0);

    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_source("transfer_log_scroll")
                .max_height(80.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if s.transfer_log.is_empty() {
                        ui.label(
                            RichText::new("No transfers yet")
                                .color(Colors::TEXT_MUTED).size(9.0)
                        );
                    }
                    for entry in s.transfer_log {
                        let color = match entry.level {
                            TransferLogLevel::Ok    => Colors::GREEN,
                            TransferLogLevel::Error => Colors::RED,
                            TransferLogLevel::Info  => Colors::TEXT_DIM,
                        };
                        ui.label(
                            RichText::new(format!(
                                "[{}] {}",
                                entry.timestamp.format("%H:%M:%S"),
                                entry.message
                            ))
                            .color(color).size(9.0)
                        );
                    }
                });
        });
}

// ─────────────────────────────────────────────────────────────────────────────
// CLIPBOARD tab
// ─────────────────────────────────────────────────────────────────────────────

fn render_clipboard_tab(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    theme::section_title(ui, "// SEND CLIPBOARD TO NODE");
    ui.add_space(8.0);

    ui.add(
        egui::TextEdit::multiline(s.clip_out)
            .hint_text("Type or paste content to send...")
            .font(egui::FontId::new(10.0, egui::FontFamily::Monospace))
            .desired_width(f32::INFINITY)
            .desired_rows(5)
    );

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if theme::secondary_button(ui, "↑ LOAD MY CLIPBOARD").clicked() {
            actions.load_clipboard = true;
        }
        ui.add_space(8.0);
        ui.set_enabled(!s.clip_out.trim().is_empty());
        if theme::primary_button(ui, "⇒ TRANSMIT").clicked() {
            actions.send_clipboard = true;
        }
    });

    ui.add_space(16.0);
    ui.add(egui::Separator::default().spacing(0.0));
    ui.add_space(16.0);

    theme::section_title(ui, "// RECEIVED FROM NODES");
    ui.add_space(8.0);

    if s.clip_inbox.is_empty() {
        egui::Frame::none()
            .fill(Colors::BG_WIDGET)
            .stroke(egui::Stroke::new(1.0, Colors::BORDER))
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("WAITING FOR INCOMING CLIPBOARD DATA...")
                            .color(Colors::TEXT_MUTED).size(9.0)
                    );
                });
            });
    } else {
        ScrollArea::vertical()
            .id_source("clipboard_inbox_scroll")
            .max_height(220.0)
            .show(ui, |ui| {
                // Newest first
                let entries: Vec<_> = s.clip_inbox.iter().rev().collect();
                for entry in entries {
                    let resp = egui::Frame::none()
                        .fill(Colors::BG_WIDGET)
                        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
                        .inner_margin(egui::Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&entry.sender)
                                        .color(Colors::CYAN).size(8.0).strong()
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(
                                                entry.received_at.format("%H:%M:%S").to_string()
                                            )
                                            .color(Colors::TEXT_MUTED).size(8.0)
                                        );
                                    }
                                );
                            });
                            ui.add_space(4.0);
                            let preview = if entry.content.len() > 180 {
                                format!("{}…", &entry.content[..180])
                            } else {
                                entry.content.clone()
                            };
                            ui.label(RichText::new(&preview).color(Colors::TEXT).size(10.0));
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!("{} click to load into editor above", crate::icons::Glyphs::ARROW_L))
                                    .color(Colors::TEXT_MUTED).size(8.0)
                            );
                        }).response;

                    if ui.interact(
                        resp.rect,
                        egui::Id::new(("clip_entry", entry.received_at.timestamp_millis())),
                        egui::Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked() {
                        // Populate the outbound text area so the user can inspect
                        // and optionally transmit onwards. The LOAD MY CLIPBOARD
                        // button doesn't overwrite this automatically.
                        *s.clip_out = entry.content.clone();
                    }

                    ui.add_space(4.0);
                }
            });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Settings modal
// ─────────────────────────────────────────────────────────────────────────────

pub struct SettingsState {
    pub api_key:      String,
    pub device_name:  String,
    pub rdp_username: String,
    pub agent_port:   String,
    pub watch_paths:  Vec<String>,
    pub ai_model:     String,
    pub device_type:  String,
    pub open:         bool,
}

impl SettingsState {
    pub fn from_config(cfg: &thegrid_core::Config) -> Self {
        Self {
            api_key:      cfg.api_key.clone(),
            device_name:  cfg.device_name.clone(),
            rdp_username: cfg.rdp_username.clone(),
            agent_port:   cfg.agent_port.to_string(),
            watch_paths:  cfg.watch_paths.iter().map(|p| p.to_string_lossy().to_string()).collect(),
            ai_model:     cfg.ai_model.clone().unwrap_or_default(),
            device_type:  cfg.device_type.clone(),
            open:         false,
        }
    }
}

/// Renders the settings modal. Returns true when user clicks SAVE.
/// Overlay + window are drawn above everything else via egui::Area/Window ordering.
pub fn render_settings_modal(ctx: &egui::Context, s: &mut SettingsState) -> bool {
    if !s.open { return false; }
    let mut saved = false;

    // Semi-transparent backdrop
    egui::Area::new(egui::Id::new("settings_backdrop"))
        .fixed_pos(egui::Pos2::ZERO)
        .order(egui::Order::Background)
        .interactable(false)
        .show(ctx, |ui| {
            ui.painter().rect_filled(
                ctx.screen_rect(),
                egui::Rounding::ZERO,
                Color32::from_rgba_premultiplied(0, 0, 0, 180),
            );
        });

    // Modal window
    egui::Window::new(format!("{} CONFIGURATION", crate::icons::Glyphs::BRAND_HEX))
        .id(egui::Id::new("settings_modal_window"))
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .fixed_size(egui::vec2(480.0, 0.0))
        .frame(
            egui::Frame::none()
                .fill(Colors::BG_PANEL)
                .stroke(egui::Stroke::new(1.0, Colors::BORDER2))
        )
        .show(ctx, |ui| {
            // Cyan top accent bar
            let top = ui.next_widget_position();
            let bar = egui::Rect::from_min_size(top, egui::vec2(480.0, 2.0));
            ui.painter().rect_filled(bar, egui::Rounding::ZERO, Colors::CYAN);
            ui.add_space(2.0);

            // Header row
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(20.0, 14.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("// CONFIGURATION")
                                .color(Colors::CYAN).size(10.0).strong()
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui.add(
                                    egui::Button::new(
                                        RichText::new("✕").color(Colors::TEXT_DIM)
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .frame(false)
                                ).clicked() {
                                    s.open = false;
                                }
                            }
                        );
                    });
                });

            ui.add(egui::Separator::default().spacing(0.0));

            egui::Frame::none()
                .inner_margin(egui::Margin::same(24.0))
                .show(ui, |ui| {
                    modal_field(ui, "TAILSCALE API KEY",    &mut s.api_key,      true,  "tskey-api-...");
                    ui.add_space(14.0);
                    modal_field(ui, "THIS DEVICE LABEL",    &mut s.device_name,  false, "e.g. WORKSTATION-MAIN");
                    ui.add_space(14.0);
                    modal_field(ui, "DEFAULT RDP USERNAME", &mut s.rdp_username, false, "e.g. Administrator");
                    ui.add_space(14.0);
                    modal_field(ui, "AGENT PORT",           &mut s.agent_port,   false, "47731");
                    ui.add_space(14.0);
                    modal_field(ui, "LOCAL AI MODEL",       &mut s.ai_model,     false, "e.g. llama3:8b");

                    ui.add_space(14.0);
                    ui.label(RichText::new("DEVICE TYPE").color(Colors::TEXT_DIM).size(9.0).strong());
                    ui.add_space(4.0);
                    egui::ComboBox::from_id_source("device_type_combo")
                        .width(ui.available_width())
                        .selected_text(s.device_type.clone())
                        .show_ui(ui, |ui| {
                            for typ in ["Desktop", "Laptop", "Tablet", "Smartphone", "Server", "NAS", "Board"] {
                                ui.selectable_value(&mut s.device_type, typ.to_string(), typ);
                            }
                        });

                    ui.add_space(14.0);
                    ui.label(RichText::new("WATCHED DIRECTORIES").color(Colors::TEXT_DIM).size(9.0).strong());
                    ui.add_space(4.0);
                    
                    let mut to_remove = None;
                    for (i, path) in s.watch_paths.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(path).desired_width(ui.available_width() - 30.0));
                            if ui.button("✕").clicked() {
                                to_remove = Some(i);
                            }
                        });
                        ui.add_space(4.0);
                    }
                    if let Some(i) = to_remove {
                        s.watch_paths.remove(i);
                    }
                    if ui.button("+ ADD DIRECTORY").clicked() {
                        s.watch_paths.push(String::new());
                    }
                });

            ui.add(egui::Separator::default().spacing(0.0));

            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(24.0, 16.0))
                .show(ui, |ui| {
                    if theme::primary_button(ui, "SAVE & REFRESH").clicked() {
                        s.open = false;
                        saved = true;
                    }
                });
        });

    saved
}

fn modal_field(ui: &mut Ui, label: &str, value: &mut String, password: bool, hint: &str) {
    ui.label(
        RichText::new(label).color(Colors::TEXT_DIM).size(9.0).strong()
    );
    ui.add_space(4.0);
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .password(password)
            .font(egui::FontId::new(11.0, egui::FontFamily::Monospace))
            .desired_width(f32::INFINITY)
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Empty state
// ─────────────────────────────────────────────────────────────────────────────

pub fn render_empty_state(ui: &mut Ui) {
    let h = ui.available_height();
    ui.vertical_centered(|ui| {
        ui.add_space(h * 0.3);
        // Vector Hexagon
        let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
        let c = rect.center();
        let r = 20.0;
        let mut points = vec![];
        for i in 0..6 {
            let angle = std::f32::consts::PI / 3.0 * i as f32 + std::f32::consts::PI / 2.0;
            points.push(c + egui::vec2(r * angle.cos(), r * angle.sin()));
        }
        ui.painter().add(egui::Shape::convex_polygon(points, Color32::TRANSPARENT, egui::Stroke::new(2.0, Colors::BORDER)));
        ui.add_space(16.0);
        ui.label(
            RichText::new("SELECT A NODE TO ESTABLISH LINK")
                .color(Colors::TEXT_MUTED).size(11.0)
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("All connections are end-to-end encrypted via Tailscale")
                .color(Colors::TEXT_MUTED).size(9.0)
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn fmt_bytes(b: u64) -> String {
    const K: u64 = 1024;
    if b < K           { format!("{} B", b) }
    else if b < K * K  { format!("{:.1} KB", b as f64 / K as f64) }
    else if b < K*K*K  { format!("{:.1} MB", b as f64 / (K * K) as f64) }
    else               { format!("{:.2} GB", b as f64 / (K * K * K) as f64) }
}

// ─────────────────────────────────────────────────────────────────────────────
// render_detail_panel_with_timeline — Phase 3 entry point
//
// Extends render_detail_panel with the Timeline tab and telemetry gauges.
// Called from app.rs instead of render_detail_panel when Phase 3 is active.
// ─────────────────────────────────────────────────────────────────────────────

pub fn render_detail_panel_with_timeline(
    ui:            &mut egui::Ui,
    s:             &mut DetailState,
    timeline:      &mut crate::views::timeline::TimelineState,
    _index_stats:   &thegrid_core::models::IndexStats,
) -> DetailActions {
    let mut actions = DetailActions::default();
    let is_online = s.device.is_likely_online();

    // ── Device header ─────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(24.0, 14.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let device_type = s.telemetry.map(|t| t.device_type.as_str()).unwrap_or("Desktop");
                let icon_type = match device_type {
                    "Laptop" => theme::IconType::Laptop,
                    "Server" => theme::IconType::Server,
                    _ => theme::IconType::Desktop,
                };
                crate::theme::render_crt_icon(ui, icon_type, 28.0, Colors::GREEN);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(s.device.display_name().to_uppercase())
                            .color(Colors::TEXT).size(16.0).strong()
                    );
                    ui.label(
                        RichText::new(s.device.primary_ip().unwrap_or("No Tailscale IP"))
                            .color(Colors::CYAN).size(10.0)
                    );
                    ui.label(
                        RichText::new(format!("{} · {}", s.device.os.to_uppercase(), s.device.client_version))
                            .color(Colors::TEXT_DIM).size(9.0)
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Wake button for offline devices
                    if !is_online {
                        if crate::theme::micro_button(ui, "⚡ WAKE").clicked() {
                            actions.wake_device = true;
                        }
                        ui.add_space(6.0);
                    }
                    theme::status_badge(ui, if is_online { "ONLINE" } else { "OFFLINE" }, is_online);
                    if s.is_tg_agent {
                        ui.add_space(8.0);
                        ui.label(RichText::new("⬡ AGENT").color(Colors::CYAN).size(9.0));
                    }
                });
            });

            // ── Telemetry gauges (Phase 3) ─────────────────────────────────
            if let Some(telem) = s.telemetry {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.set_min_width(ui.available_width());
                    // CPU gauge
                    crate::telemetry::render_gauge(
                        ui, "CPU",
                        telem.cpu_pct,
                        &format!("{:.0}%", telem.cpu_pct),
                    );
                    ui.add_space(16.0);
                    // RAM gauge
                    crate::telemetry::render_gauge(
                        ui, "RAM",
                        telem.ram_pct(),
                        &format!("{} / {}",
                            crate::telemetry::fmt_bytes(telem.ram_used),
                            crate::telemetry::fmt_bytes(telem.ram_total)
                        ),
                    );
                    ui.add_space(16.0);
                    // Disk gauge
                    crate::telemetry::render_gauge(
                        ui, "DISK",
                        telem.disk_pct(),
                        &format!("{} / {}",
                            crate::telemetry::fmt_bytes(telem.disk_used),
                            crate::telemetry::fmt_bytes(telem.disk_total)
                        ),
                    );
                    // AI badge
                    if telem.is_ai_capable {
                        ui.add_space(12.0);
                        ui.label(RichText::new(format!("AI NODE {}", crate::icons::Glyphs::BRAND_HEX_F)).color(Colors::GREEN).size(8.0).strong());
                    }
                });

                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    if telem.capabilities.has_rdp {
                        theme::status_badge(ui, "RDP", true);
                        ui.add_space(4.0);
                    }
                    if telem.capabilities.has_file_access {
                        theme::status_badge(ui, "FILE SHARES", true);
                        ui.add_space(4.0);
                    }
                    if telem.capabilities.has_camera {
                        theme::status_badge(ui, "CAMERA", true);
                        ui.add_space(4.0);
                    }
                    if telem.capabilities.has_microphone {
                        theme::status_badge(ui, "MIC", true);
                        ui.add_space(4.0);
                    }
                    if telem.capabilities.has_speakers {
                        theme::status_badge(ui, "AUDIO", true);
                        ui.add_space(4.0);
                    }
                    for model in &telem.capabilities.ai_models {
                        theme::status_badge(ui, &format!("AI: {}", model.to_uppercase()), true);
                        ui.add_space(4.0);
                    }
                    if !telem.capabilities.drives.is_empty() {
                        let drives_str = telem.capabilities.drives.join(", ");
                        theme::status_badge(ui, &format!("DRIVES: {}", drives_str), true);
                    }
                });
            } else {
                // No telemetry yet — show fetch button
                ui.add_space(6.0);
                if crate::theme::micro_button(ui, "FETCH TELEMETRY").clicked() {
                    actions.fetch_telemetry = true;
                }
            }
        });

    ui.add(egui::Separator::default().spacing(0.0));

    // ── Tab bar (4 tabs) ──────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        for (label, tab_variant) in [
            ("ACTIONS",   DashTab::Actions),
            ("FILES",     DashTab::Files),
            ("CLIPBOARD", DashTab::Clipboard),
            ("TIMELINE",  DashTab::Timeline),
            ("TERMINAL",  DashTab::Terminal),
        ] {
            if tab_variant == DashTab::Terminal && !s.is_tg_agent { continue; }
            let is_active = *s.active_tab == tab_variant;
            let color = if is_active { Colors::GREEN } else { Colors::TEXT_DIM };
            let resp = ui.add(
                egui::Button::new(RichText::new(label).color(color).size(9.0).strong())
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE)
                    .min_size(egui::vec2(88.0, 36.0))
            );
            if is_active {
                let r = resp.rect;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(egui::pos2(r.min.x, r.max.y - 2.0), egui::vec2(r.width(), 2.0)),
                    egui::Rounding::ZERO, Colors::GREEN,
                );
            }
            if resp.clicked() { *s.active_tab = tab_variant; }
        }
    });

    ui.add(egui::Separator::default().spacing(0.0));

    // ── Tab content ───────────────────────────────────────────────────────────
    egui::ScrollArea::vertical()
        .id_source("detail_v3_scroll")
        .show(ui, |ui| {
            ui.add_space(16.0);
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(24.0, 0.0))
                .show(ui, |ui| {
                    let tab = s.active_tab.clone();
                    match tab {
                        DashTab::Actions   => render_actions_tab(ui, s, &mut actions),
                        DashTab::Files     => render_files_tab(ui, s, &mut actions),
                        DashTab::Clipboard => render_clipboard_tab(ui, s, &mut actions),
                        DashTab::Timeline  => {
                            // Trigger data load if needed
                            if timeline.needs_refresh() {
                                actions.load_timeline = true;
                            }
                            let tl_action = crate::views::timeline::render(ui, timeline);
                            if tl_action.refresh { actions.load_timeline = true; }
                            if tl_action.open_entry.is_some() {
                                // Navigation handled in app.rs via actions
                                // For now just show a toast — Phase 4 can deep-link to the file
                            }
                        }
                        DashTab::Terminal  => render_terminal_tab(ui, s, &mut actions),
                    }
                });
            ui.add_space(16.0);
        });

    actions
}
