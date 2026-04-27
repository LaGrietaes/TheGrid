// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// views/dashboard.rs â€” Main Application Dashboard  [v0.2 â€” Phase 2]
//
// FIXES from v0.1:
//   âœ“ render_actions/files/clipboard_tab now take `s: &mut DetailState`
//     (was `&DetailState` â€” can't get &mut String fields through & reference)
//   âœ“ egui::ComboBox::from_id_source â†’ from_id_source  (egui 0.27 deprecation)
//   âœ“ FileTransferStatus::Failed(e) â†’ Failed(_)  (unused variable warning)
//   âœ“ DetailActions::download_file: Option<String> for per-file downloads
//   âœ“ id_source â†’ id_source on all ScrollAreas
//
// NEW in v0.2:
//   + SettingsState + render_settings_modal  â€” in-app config modal
//   + watch_paths field in DetailState       â€” Phase 2 watcher UI
//   + Refresh button moved into device panel â€” removes it from &self titlebar
//   + Remote file list: each â†“ button returns the filename via DetailActions
//   + Clipboard inbox items: click to populate clip_out for inspection
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

use std::collections::HashMap;
use std::path::PathBuf;
use egui::{Color32, RichText, Ui, ScrollArea};
use egui_extras::{Size, StripBuilder};
use egui_tiles::{Behavior as TileBehavior, Container as TileContainer, Tile as EguiTile, TileId, Tiles, Tree, UiResponse};
use thegrid_core::models::*;
use crate::theme::{self, Colors};

const ENABLE_AI_RIGHT_PANEL: bool = false;

/// Which tab is active in the detail panel
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DashTab {
    #[default] Actions,
    Files,
    Clipboard,
    /// Phase 3: The Flow â€” temporal file activity view
    Timeline,
    /// New in Node Enhancement: Remote Terminal
    Terminal,
    /// New in Dashboard Optimization: Detailed Storage Breakdown
    Storage,
    /// Phase 6: Cross-source duplicate review
    DedupReview,
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// DetailState â€” all mutable references the detail panel needs
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
    /// Phase 3: Smart Rules for filtering
    pub smart_rules:    &'a [thegrid_core::models::SmartRule],
    /// New in Node Enhancement: tracks the current directory being browsed
    #[allow(dead_code)]
    pub _current_remote_path: &'a mut std::path::PathBuf,
    /// Phase 2: File Manager State
    pub file_manager: &'a mut crate::app::FileManagerState,
    /// New in Node Enhancement: tracks the model name being typed in the UI
    /// New in Node Enhancement: tracks the model name being typed in the UI
    pub remote_model_edit: &'a mut String,
    /// New in Node Enhancement: tracks the provider URL being typed in the UI
    pub remote_url_edit:  &'a mut String,
    /// New in Node Enhancement: the terminal view object
    pub terminal_view:      Option<&'a mut crate::views::terminal::TerminalView>,
    /// New in Dashboard Optimization: Name of the local device to detect (LOCAL)
    pub local_device_name:  &'a str,
    /// New in Connectivity Fixes: Agent status
    pub status:             crate::app::NodeStatus,
    pub duplicate_groups:   &'a [(String, u64, Vec<FileSearchResult>)],
    pub duplicate_last_scan: Option<i64>,
    pub hashing_progress: (usize, usize),
    pub drive_last_manifest: Option<&'a PathBuf>,
    pub grid_scan_progress: &'a std::collections::HashMap<String, crate::app::GridScanProgress>,
    pub cloud_pipeline_progress: &'a crate::app::CloudPipelineProgress,
    pub node_crosscheck: &'a std::collections::HashMap<String, crate::app::NodeCrosscheckSummary>,
    /// Phase 6: Rich cross-source duplicate groups
    pub rich_duplicate_groups: &'a [thegrid_core::models::DuplicateGroup],
    pub dedup_review_state: &'a mut crate::views::dedup_review::DedupReviewState,
    pub local_device_id: &'a str,
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// DetailActions â€” returned from render_detail_panel each frame
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
    /// Some(filename) when user clicks â†“ on a specific remote file
    pub download_file:   Option<String>,
    /// Phase 3: request telemetry fetch for this device
    pub fetch_telemetry: bool,
    /// Phase 3: wake sleeping device via WoL
    pub wake_device:     bool,
    /// Phase 3: load timeline data
    pub load_timeline:   bool,
    /// New in Node Enhancement: browse a specific remote path
    pub browse_remote:   Option<std::path::PathBuf>,
    pub preview_remote:  Option<std::path::PathBuf>,
    /// New in Node Enhancement: download a file from any path
    pub download_remote_file: Option<std::path::PathBuf>,
    /// New in Node Enhancement: update remote AI model config (device_type, model, url)
    pub update_remote_config: Option<(Option<String>, Option<String>, Option<String>)>,
    /// New in Node Enhancement: terminal actions
    pub create_terminal:      bool,
    pub send_terminal_input:  Option<Vec<u8>>,
    pub launch_scrcpy:        bool,
    pub enable_rdp:           bool,
    pub launch_ssh:           bool,
    pub fm_delete:            Option<Vec<String>>,
    pub run_duplicate_scan:   Option<DuplicateScanFilter>,
    pub delete_duplicate_files: Option<Vec<(i64, std::path::PathBuf, String)>>,
    /// Phase 6: Rich dedup delete action
    pub dedup_delete_files: Option<Vec<thegrid_core::models::FileSearchResult>>,
    /// Phase 6: trigger cross-source dedup scan
    pub run_cross_source_scan: bool,
    pub export_drive_buffer:  bool,
    pub upload_drive_buffer:  bool,
    #[allow(dead_code)]
    pub _fm_rename:            Option<(String, String)>,
    #[allow(dead_code)]
    pub _fm_move:              Option<(Vec<String>, std::path::PathBuf)>,
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Device panel (left sidebar)
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Returned by render_device_panel each frame.
#[derive(Default)]
pub struct NavPanelResult {
    pub clicked_device:    Option<usize>,
    pub navigate_to:       Option<crate::app::Screen>,
    pub open_planner_add:  bool,
    pub open_project_add:  bool,
    pub ai_load_model:     Option<String>,
    pub ai_start_agent:    Option<String>,
}
#[allow(clippy::too_many_arguments)]
pub fn render_device_panel(
    ui: &mut Ui,
    devices_with_status: &[(TailscaleDevice, crate::app::NodeStatus)],
    display_states: &std::collections::HashMap<String, thegrid_core::models::DeviceDisplayState>,
    telemetries: &std::collections::HashMap<String, thegrid_core::models::NodeTelemetry>,
    selected_idx: Option<usize>,
    selected_node_ids: &mut Vec<String>,
    projects: &[Project],
    categories: &[Category],
    smart_rules: &[thegrid_core::models::SmartRule],
    active_rule: &mut Option<String>,
    filter: &mut String,
    needs_refresh: &mut bool,
    local_device_name: &str,
    // â”€â”€ new navigation state â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    project_nav_tab: &mut crate::app::ProjectNavTab,
    nav_nodes_collapsed: &mut bool,
    quick_view: &mut crate::app::QuickViewState,
    project_statuses: &std::collections::HashMap<String, crate::app::ProjectStatus>,
    // â”€â”€ context for cross-section navigation / previews â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    active_screen: crate::app::Screen,
    planner_tasks: &std::collections::HashMap<String, Vec<crate::app::PlannerTask>>,
    planner_selected: Option<&str>,
    planner_add_open: &mut bool,
    // -- local AI panel state --
    ai_panel: &mut crate::app::AiPanelState,
) -> NavPanelResult {
    let mut result = NavPanelResult::default();

    fn is_local_device(device: &TailscaleDevice, local_device_name: &str) -> bool {
        device.hostname.eq_ignore_ascii_case(local_device_name)
            || device.name.eq_ignore_ascii_case(local_device_name)
            || device.display_name().eq_ignore_ascii_case(local_device_name)
    }


    // -- Logo header --
    egui::Frame::none()
        .fill(Color32::from_rgb(0, 8, 2))
        .inner_margin(egui::Margin::symmetric(0.0, 8.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add(
                    egui::Image::new(egui::include_image!("../../assets/TheGridLogo.svg"))
                        .fit_to_exact_size(egui::vec2(140.0, 140.0))
                        .maintain_aspect_ratio(true),
                );
            });
        });
    ui.add(egui::Separator::default().spacing(0.0));
    // â”€â”€ Filter + Refresh row â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let _ = nav_nodes_collapsed; // collapse replaced by nav tabs
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(8.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Search icon
                let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                let c = rect.center() - egui::vec2(1.0, 1.0);
                ui.painter().circle_stroke(c, 3.0, egui::Stroke::new(1.0, Colors::TEXT_MUTED));
                ui.painter().line_segment(
                    [c + egui::vec2(2.0, 2.0), c + egui::vec2(4.5, 4.5)],
                    egui::Stroke::new(1.2, Colors::TEXT_MUTED)
                );
                ui.add(
                    egui::TextEdit::singleline(filter)
                        .hint_text("FILTER NODES...")
                        .font(egui::FontId::new(9.5, egui::FontFamily::Monospace))
                        .desired_width(f32::INFINITY)
                        .frame(false)
                );
                // Refresh + cluster count on right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
                    let color = if resp.hovered() { Colors::GREEN } else { Colors::TEXT_DIM };
                    let c = rect.center();
                    ui.painter().circle_stroke(c, 3.5, egui::Stroke::new(1.1, color));
                    ui.painter().line_segment([c + egui::vec2(2.0, -2.0), c + egui::vec2(4.5, -4.0)], egui::Stroke::new(1.1, color));
                    ui.painter().line_segment([c + egui::vec2(2.0, -2.0), c + egui::vec2(4.0, -0.2)], egui::Stroke::new(1.1, color));
                    if resp.clicked() { *needs_refresh = true; }
                    resp.on_hover_text("Refresh nodes");
                    if !selected_node_ids.is_empty() {
                        ui.add_space(2.0);
                        ui.label(RichText::new(format!("CLU:{}", selected_node_ids.len()))
                            .color(Colors::GREEN).size(7.5).strong());
                    }
                });
            });
        });

    ui.add(egui::Separator::default().spacing(0.0));

    let filter_lower = filter.to_lowercase();
    ScrollArea::vertical()
        .id_source("device_list_scroll")
        .max_height(205.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // â”€â”€ Nodes Section â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            ui.add_space(4.0);
            for (idx, (device, status)) in devices_with_status.iter().enumerate() {
                let matches = filter_lower.is_empty()
                    || device.hostname.to_lowercase().contains(&filter_lower)
                    || device.name.to_lowercase().contains(&filter_lower)
                    || device.addresses.iter().any(|ip| ip.contains(&filter_lower));

                if !matches { continue; }

                let is_local = is_local_device(device, local_device_name);

                let is_selected = selected_idx == Some(idx);
                let is_in_cluster = selected_node_ids.contains(&device.id);
                
                let bg = if is_selected { Colors::BG_ACTIVE }
                         else if is_in_cluster { Color32::from_rgba_premultiplied(0, 80, 20, 18) }
                         else { Color32::TRANSPARENT };
                
                let status_color = if let Some(ds) = display_states.get(&device.id) {
                    theme::device_state_color(ds)
                } else {
                    match status {
                        crate::app::NodeStatus::GridActive => Colors::GREEN,
                        crate::app::NodeStatus::Reachable  => Colors::AMBER,
                        crate::app::NodeStatus::Offline    => Colors::TEXT_MUTED,
                    }
                };

                let telem = telemetries.get(&device.id);
                let is_ai = telem.map(|t| t.is_ai_capable).unwrap_or(false);
                let ai_model: Option<&str> = telem.and_then(|t| {
                    t.capabilities.ai_models.first().map(|m| m.as_str())
                });
                let device_type_str = telem.map(|t| t.device_type.as_str()).unwrap_or("Desktop");

                let ts_name: &str = if !device.name.is_empty() { &device.name } else { &device.hostname };
                let ts_ip: &str = device.primary_ip().unwrap_or("");

                let mut copy_ts_ip = false;
                let resp = egui::Frame::none()
                    .fill(bg)
                    .inner_margin(egui::Margin { left: 8.0, right: 6.0, top: 4.0, bottom: 4.0 })
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        // â”€â”€ Compact single-area layout â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
                        ui.horizontal(|ui| {
                            // Status dot (replaces cluster toggle as primary left glyph)
                            let (dot_r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                            ui.painter().circle_filled(dot_r.center(), 3.5, status_color);
                            ui.add_space(5.0);

                            // Device icon
                            let h_lower = device.hostname.to_lowercase();
                            let d_lower = device.display_name().to_lowercase();
                            let icon_type = if h_lower.contains("nubia") || d_lower.contains("nubia") || device_type_str == "Tablet" {
                                theme::IconType::Tablet
                            } else if h_lower.contains("nothing") || d_lower.contains("nothing") || device_type_str == "Smartphone" || device_type_str == "Phone" {
                                theme::IconType::Smartphone
                            } else {
                                match device_type_str {
                                    "Laptop"               => theme::IconType::Laptop,
                                    "Server"               => theme::IconType::Server,
                                    "Tablet"               => theme::IconType::Tablet,
                                    "Smartphone" | "Phone" => theme::IconType::Smartphone,
                                    "Chromebook"           => theme::IconType::Chromebook,
                                    _                      => theme::IconType::Desktop,
                                }
                            };
                            let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                            theme::draw_vector_icon(ui, icon_rect, icon_type, status_color);
                            ui.add_space(6.0);

                            // Text column: name row + info row
                            ui.vertical(|ui| {
                                // Row 1: short hostname + [LOCAL] + AI badge
                                let short_name = device.display_name()
                                    .split('.').next()
                                    .unwrap_or(device.display_name());
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    ui.label(
                                        RichText::new(short_name.to_uppercase())
                                            .color(if is_selected { Colors::GREEN } else { Colors::TEXT })
                                            .size(9.0).strong()
                                    );
                                    if is_local {
                                        ui.label(RichText::new("[LOCAL]").color(Colors::GREEN).size(7.0));
                                    }
                                    if is_ai {
                                        let ai_lbl = if let Some(m) = ai_model {
                                            format!("âŸ{}", m)
                                        } else {
                                            "âŸAI".to_string()
                                        };
                                        ui.label(RichText::new(ai_lbl).color(Colors::STATE_COMPUTE_PROVIDE).size(6.5));
                                    }
                                });
                                // Row 2: IP (clickable to copy) + status text
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    if !ts_ip.is_empty() {
                                        let ip_resp = ui.add(
                                            egui::Label::new(
                                                RichText::new(ts_ip).color(Colors::TEXT_MUTED).size(7.5)
                                            ).sense(egui::Sense::click())
                                        );
                                        if ip_resp.clicked() { copy_ts_ip = true; }
                                        ip_resp.on_hover_text(format!("{ts_name}  â€”  click to copy IP"));
                                    }
                                    let (badge_text, badge_color) = match status {
                                        crate::app::NodeStatus::GridActive => ("â¬¡ ONLINE", Colors::GREEN),
                                        crate::app::NodeStatus::Reachable  => ("â—Œ UP",     Colors::AMBER),
                                        crate::app::NodeStatus::Offline    => ("â—¯ OFF",    Colors::TEXT_MUTED),
                                    };
                                    ui.label(RichText::new(badge_text).color(badge_color).size(7.0));
                                });
                            });

                            // Cluster indicator dot (far right) â€” Ctrl+click card to toggle
                            if is_in_cluster {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                                    ui.painter().circle_filled(r.center(), 2.5, Colors::GREEN);
                                });
                            }
                        });
                    }).response;

                if copy_ts_ip {
                    ui.output_mut(|o| o.copied_text = ts_ip.to_string());
                }

                // Left accent bar for selected row
                if is_selected {
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(resp.rect.min.x, resp.rect.min.y),
                            egui::vec2(2.0, resp.rect.height()),
                        ),
                        egui::Rounding::ZERO,
                        Colors::GREEN,
                    );
                }

                let interact = ui.interact(resp.rect, egui::Id::new(("dev_row", idx)), egui::Sense::click());
                if interact.clicked() {
                    let ctrl = ui.input(|i| i.modifiers.ctrl);
                    if ctrl {
                        if is_in_cluster {
                            selected_node_ids.retain(|id| id != &device.id);
                        } else if !selected_node_ids.contains(&device.id) {
                            selected_node_ids.push(device.id.clone());
                        }
                    } else {
                        result.clicked_device = Some(idx);
                    }
                }
            }

        }); // end device list scroll

    ui.add(egui::Separator::default().spacing(0.0));

    // ════════════════════════════════════════════════════════════════════
    // // AI  —  Local model commands (1/3 of mesh area)
    // ════════════════════════════════════════════════════════════════════
    {
        let panel_h = 108.0;
        egui::Frame::none()
            .fill(Color32::from_rgb(0, 10, 4))
            .inner_margin(egui::Margin { left: 10.0, right: 8.0, top: 5.0, bottom: 5.0 })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                // Header row: "// AI" + probe spinner + [PROBE] refresh
                ui.horizontal(|ui| {
                    ui.label(RichText::new("// AI").color(Colors::STATE_COMPUTE_PROVIDE).size(9.0).strong());
                    ui.add_space(4.0);
                    if ai_panel.probing {
                        ui.label(RichText::new("...").color(Colors::TEXT_MUTED).size(8.0));
                    } else if ai_panel.detected_models.is_empty() {
                        ui.label(RichText::new("NO LOCAL MODELS").color(Colors::TEXT_DIM).size(7.5));
                    } else {
                        ui.label(
                            RichText::new(format!("{} MODEL{}", ai_panel.detected_models.len(),
                                if ai_panel.detected_models.len() == 1 { "" } else { "S" }))
                            .color(Colors::TEXT_MUTED).size(7.5)
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let probe_btn = egui::Button::new(
                            RichText::new("PROBE").color(Colors::TEXT_MUTED).size(7.0)
                        ).fill(Color32::TRANSPARENT).stroke(egui::Stroke::new(0.6, Colors::BORDER2));
                        if ui.add(probe_btn).clicked() {
                            ai_panel.probing    = true;
                            ai_panel.last_probe = None; // force re-probe next frame
                        }
                        if ai_panel.agent_running {
                            ui.add_space(4.0);
                            ui.label(RichText::new("AGENT ON").color(Colors::GREEN).size(7.0).strong());
                        }
                    });
                });
                ui.add_space(3.0);
                // Model list (scrollable inside the fixed panel height)
                ScrollArea::vertical()
                    .id_source("ai_model_list")
                    .max_height(panel_h - 54.0)
                    .show(ui, |ui| {
                        if ai_panel.detected_models.is_empty() {
                            ui.label(RichText::new("  Ollama not running or no models installed.")
                                .color(Colors::TEXT_DIM).size(7.5).italics());
                        }
                        for (idx, model_name) in ai_panel.detected_models.iter().enumerate() {
                            let is_sel = ai_panel.selected_model == Some(idx);
                            let row_bg = if is_sel { Color32::from_rgb(0, 24, 8) } else { Color32::TRANSPARENT };
                            let row_col = if is_sel { Colors::GREEN } else { Colors::TEXT };
                            egui::Frame::none()
                                .fill(row_bg)
                                .inner_margin(egui::Margin::symmetric(2.0, 1.0))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        // Selector dot
                                        let (dot_r, _) = ui.allocate_exact_size(egui::vec2(8.0,8.0), egui::Sense::hover());
                                        if is_sel { ui.painter().circle_filled(dot_r.center(), 2.5, Colors::STATE_COMPUTE_PROVIDE); }
                                        let i = model_name.char_indices().nth(22).map(|(i,_)| i).unwrap_or(model_name.len());
                                        let short = if model_name.len() > 22 { &model_name[..i] } else { model_name.as_str() };
                                        ui.label(RichText::new(short).color(row_col).size(8.0));
                                    });
                                });
                            let resp_i = ui.interact(
                                ui.min_rect(),
                                egui::Id::new(("ai_model", idx)),
                                egui::Sense::click(),
                            );
                            if resp_i.clicked() { ai_panel.selected_model = Some(idx); }
                        }
                    });
                ui.add_space(3.0);
                // Action buttons row
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let sel_model = ai_panel.selected_model
                        .and_then(|i| ai_panel.detected_models.get(i).cloned());
                    // LOAD button
                    let load_active = sel_model.is_some();
                    let load_col = if load_active { Colors::GREEN } else { Colors::TEXT_DIM };
                    let load_btn = egui::Button::new(RichText::new("LOAD").color(load_col).size(7.5))
                        .fill(Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(0.8, if load_active { Colors::GREEN_DIM } else { Colors::BORDER2 }))
                        .min_size(egui::vec2(36.0, 16.0));
                    if ui.add(load_btn).clicked() {
                        if let Some(m) = sel_model.clone() { result.ai_load_model = Some(m); }
                    }
                    // START AGENT button
                    let agent_col = if ai_panel.agent_running { Colors::AMBER } else { Colors::TEXT_DIM };
                    let agent_label = if ai_panel.agent_running { "STOP" } else { "AGENT" };
                    let agent_btn = egui::Button::new(RichText::new(agent_label).color(agent_col).size(7.5))
                        .fill(Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(0.8, if load_active { agent_col } else { Colors::BORDER2 }))
                        .min_size(egui::vec2(36.0, 16.0));
                    if ui.add(agent_btn).clicked() {
                        if ai_panel.agent_running {
                            ai_panel.agent_running = false;
                            ai_panel.status_msg = "Agent stopped.".to_string();
                        } else if let Some(m) = sel_model.clone() {
                            result.ai_start_agent = Some(m);
                        }
                    }
                    // Status message
                    if !ai_panel.status_msg.is_empty() {
                        let i = ai_panel.status_msg.char_indices().nth(18)
                            .map(|(i,_)| i).unwrap_or(ai_panel.status_msg.len());
                        let s = &ai_panel.status_msg[..i];
                        ui.label(RichText::new(s).color(Colors::TEXT_MUTED).size(6.5).italics());
                    }
                });
            });
    }

    ui.add(egui::Separator::default().spacing(0.0));

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // WORKSPACE â€” unified Projects + Quick View + Planner preview block
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    ScrollArea::vertical()
        .id_source("nav_workspace_scroll")
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // â”€â”€ WORKSPACE section header (navigates to Projects screen) â”€â”€â”€â”€â”€â”€â”€â”€
            egui::Frame::none()
                .fill(Colors::BG_PANEL)
                .inner_margin(egui::Margin { left: 10.0, right: 8.0, top: 7.0, bottom: 4.0 })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let proj_active = active_screen == crate::app::Screen::Projects;
                        let hdr_btn = egui::Button::new(
                            RichText::new("// PROJECTS").color(
                                if proj_active { Colors::GREEN } else { Colors::TEXT_DIM }
                            ).size(9.0).strong()
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE)
                        .min_size(egui::vec2(0.0, 16.0));
                        if ui.add(hdr_btn).clicked() && !proj_active {
                            result.navigate_to = Some(crate::app::Screen::Projects);
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // [+ NEW] Add project button
                            let new_resp = ui.add(
                                egui::Button::new(RichText::new("+ NEW").color(Colors::GREEN).size(8.0))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::new(1.0, Colors::GREEN_DIM))
                                    .min_size(egui::vec2(0.0, 16.0))
                            );
                            if new_resp.clicked() { result.open_project_add = true; }
                            ui.add_space(4.0);
                            // SWAP menu for quick-view slots
                            egui::menu::menu_button(ui,
                                RichText::new("PIN").color(Colors::TEXT_MUTED).size(8.0), |ui| {
                                    ui.set_min_width(150.0);
                                    ui.label(RichText::new("PIN TO SLOT").color(Colors::GREEN).size(8.5).strong());
                                    ui.add(egui::Separator::default().spacing(0.0));
                                    for slot_i in 0..4usize {
                                        let slot_lbl = format!("SLOT {}", slot_i + 1);
                                        egui::menu::menu_button(ui,
                                            RichText::new(slot_lbl).color(Colors::TEXT_DIM).size(8.5), |ui| {
                                            ui.set_min_width(130.0);
                                            if ui.selectable_label(false, "CLEAR").clicked() {
                                                quick_view.slots[slot_i] = None;
                                                ui.close_menu();
                                            }
                                            ui.add(egui::Separator::default().spacing(0.0));
                                            for proj in projects {
                                                if ui.selectable_label(false, proj.name.to_uppercase()).clicked() {
                                                    quick_view.slots[slot_i] = Some(proj.id.clone());
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                    }
                                }
                            );
                        });
                    });

                    ui.add_space(5.0);
                    // Tab row: [BRAND] [WEB] [MEDIA] [DESIGN]
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for tab in crate::app::ProjectNavTab::all() {
                            let active = *project_nav_tab == tab;
                            let fill   = if active { Color32::from_rgb(0, 20, 6) } else { Color32::TRANSPARENT };
                            let border = if active { Colors::GREEN_DIM } else { Colors::BORDER };
                            let color  = if active { Colors::GREEN } else { Colors::TEXT_MUTED };
                            let btn = egui::Button::new(RichText::new(tab.label()).color(color).size(8.0))
                                .fill(fill)
                                .stroke(egui::Stroke::new(1.0, border))
                                .min_size(egui::vec2(0.0, 17.0));
                            if ui.add(btn).clicked() { *project_nav_tab = tab; }
                        }
                    });
                    ui.add_space(3.0);

                    // Project list (compact single-line rows)
                    let kws = project_nav_tab.keywords();
                    let tab_projects: Vec<&thegrid_core::models::Project> = projects.iter().filter(|p| {
                        let n = p.name.to_lowercase();
                        kws.iter().any(|k| n.contains(k))
                            || p.tags.iter().any(|t| kws.iter().any(|k| t.to_lowercase().contains(k)))
                    }).collect();

                    if tab_projects.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            ui.label(RichText::new("NO PROJECTS").color(Colors::TEXT_MUTED).size(8.0).italics());
                        });
                    } else {
                        for proj in &tab_projects {
                            let eff_status = project_statuses.get(&proj.id).cloned()
                                .unwrap_or(crate::app::ProjectStatus::Planned);
                            let row_resp = ui.add(
                                egui::Button::new(
                                    egui::RichText::new({
                                        let s = if proj.name.len() > 18 {
                                            format!("{}\u{2026}", proj.name.char_indices().nth(18).map(|(i,_)| &proj.name[..i]).unwrap_or(&proj.name))
                                        } else { proj.name.clone() };
                                        s.to_uppercase()
                                    }).color(Colors::TEXT).size(8.5)
                                )
                                .fill(Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .min_size(egui::vec2(0.0, 20.0))
                            );
                            // Draw status dot manually to the left of the button rect
                            let dot_center = egui::pos2(
                                row_resp.rect.min.x - 6.0,
                                row_resp.rect.center().y,
                            );
                            ui.painter().circle_filled(dot_center, 3.0, eff_status.color());

                            if row_resp.clicked() {
                                result.navigate_to = Some(crate::app::Screen::Projects);
                            }
                        }
                    }
                });

            ui.add(egui::Separator::default().spacing(0.0));

            // â”€â”€ Quick-View 2Ã—2 grid â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            egui::Frame::none()
                .fill(Colors::BG_PANEL)
                .inner_margin(egui::Margin { left: 6.0, right: 6.0, top: 5.0, bottom: 6.0 })
                .show(ui, |ui| {
                    let avail = ui.available_width();
                    // slot_inner_w: accounts for 4px gap, 2Ã—4px inner_margin, 2Ã—1px border per slot
                    let slot_inner_w = ((avail - 4.0) / 2.0 - 10.0).max(10.0);
                    let slot_h = 40.0;

                    for row in 0..2usize {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for col in 0..2usize {
                                let slot_i = row * 2 + col;
                                let slot_proj = quick_view.slots[slot_i].as_ref()
                                    .and_then(|id| projects.iter().find(|p| &p.id == id));
                                let slot_filled = slot_proj.is_some();
                                let frame_resp = egui::Frame::none()
                                    .fill(if slot_filled { Color32::from_rgb(0, 18, 5) } else { Colors::BG_WIDGET })
                                    .stroke(egui::Stroke::new(1.0, if slot_filled { Colors::GREEN_DIM } else { Colors::BORDER2 }))
                                    .inner_margin(egui::Margin::symmetric(4.0, 3.0))
                                    .show(ui, |ui| {
                                        ui.set_min_size(egui::vec2(slot_inner_w, slot_h));
                                        if let Some(proj) = slot_proj {
                                            let eff_status = project_statuses.get(&proj.id).cloned()
                                                .unwrap_or(crate::app::ProjectStatus::Planned);
                                            let short = if proj.name.len() > 9 {
                                                let i = proj.name.char_indices().nth(9).map(|(i,_)| i).unwrap_or(proj.name.len());
                                                format!("{}…", &proj.name[..i])
                                            } else { proj.name.clone() };
                                            ui.vertical(|ui| {
                                                ui.horizontal(|ui| {
                                                    let (r, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                                    ui.painter().circle_filled(r.center(), 3.0, eff_status.color());
                                                    ui.add_space(2.0);
                                                    ui.label(RichText::new(short.to_uppercase()).color(Colors::TEXT).size(7.5).strong());
                                                });
                                                ui.label(RichText::new(eff_status.label()).color(eff_status.color()).size(6.5));
                                            });
                                        } else {
                                            ui.vertical_centered(|ui| {
                                                ui.add_space(11.0);
                                                ui.label(RichText::new(format!("SLOT {}", slot_i + 1)).color(Colors::TEXT_MUTED).size(7.5));
                                            });
                                        }
                                    });
                                if ui.interact(
                                    frame_resp.response.rect,
                                    egui::Id::new(("qslot", slot_i)),
                                    egui::Sense::click(),
                                ).clicked() {
                                    result.navigate_to = Some(crate::app::Screen::Projects);
                                }
                            }
                        });
                        if row == 0 { ui.add_space(4.0); }
                    }
                });

            ui.add(egui::Separator::default().spacing(0.0));

            // â”€â”€ Categories (compact collapsible) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            if !categories.is_empty() {
                egui::Frame::none()
                    .fill(Colors::BG_PANEL)
                    .inner_margin(egui::Margin { left: 10.0, right: 8.0, top: 5.0, bottom: 4.0 })
                    .show(ui, |ui| {
                        ui.label(RichText::new("// CATEGORIES").color(Colors::TEXT_MUTED).size(8.5).strong());
                        ui.add_space(2.0);
                        for cat in categories {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(RichText::new(
                                    format!("{} {}", cat.icon, cat.name.to_uppercase())
                                ).color(Colors::TEXT_MUTED).size(8.0));
                            });
                        }
                    });
                ui.add(egui::Separator::default().spacing(0.0));
            }

            // â”€â”€ Modes / Smart Rules (compact) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            if !smart_rules.is_empty() {
                egui::Frame::none()
                    .fill(Colors::BG_PANEL)
                    .inner_margin(egui::Margin { left: 10.0, right: 8.0, top: 5.0, bottom: 4.0 })
                    .show(ui, |ui| {
                        ui.label(RichText::new("// MODES").color(Colors::TEXT_MUTED).size(8.5).strong());
                        ui.add_space(2.0);
                        for rule in smart_rules {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                let is_active = *active_rule == Some(rule.id.clone());
                                let color = if is_active { Colors::GREEN } else { Colors::TEXT_MUTED };
                                let btn = egui::Button::new(
                                    RichText::new(format!("â­ {}", rule.name.to_uppercase())).color(color).size(8.0)
                                ).fill(Color32::TRANSPARENT).stroke(egui::Stroke::NONE)
                                 .min_size(egui::vec2(0.0, 16.0));
                                if ui.add(btn).clicked() {
                                    if is_active { *active_rule = None; } else { *active_rule = Some(rule.id.clone()); }
                                }
                            });
                        }
                    });
                ui.add(egui::Separator::default().spacing(0.0));
            }

            // â”€â”€ PLANNER mini-preview â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            egui::Frame::none()
                .fill(Colors::BG_PANEL)
                .inner_margin(egui::Margin { left: 10.0, right: 8.0, top: 7.0, bottom: 8.0 })
                .show(ui, |ui| {
                    // Section header â€” navigates to Planner screen
                    ui.horizontal(|ui| {
                        let plan_active = active_screen == crate::app::Screen::Planner;
                        let hdr_btn = egui::Button::new(
                            RichText::new("// PLANNER").color(
                                if plan_active { Colors::GREEN } else { Colors::TEXT_DIM }
                            ).size(9.0).strong()
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE)
                        .min_size(egui::vec2(0.0, 16.0));
                        if ui.add(hdr_btn).clicked() && !plan_active {
                            result.navigate_to = Some(crate::app::Screen::Planner);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // [+] Add task button
                            let add_resp = ui.add(
                                egui::Button::new(RichText::new("+ ADD").color(Colors::GREEN).size(8.0))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::new(1.0, Colors::GREEN_DIM))
                                    .min_size(egui::vec2(0.0, 16.0))
                            );
                            if add_resp.clicked() {
                                result.open_planner_add = true;
                                // Pre-fill the project if one is selected
                                if let Some(proj_id) = planner_selected {
                                    *planner_add_open = true;
                                    let _ = proj_id; // the caller will set the project_id
                                }
                            }
                        });
                    });
                    ui.add_space(4.0);

                    // Show tasks for the selected project (up to 5, any status)
                    let preview_tasks: Vec<&crate::app::PlannerTask> = planner_selected
                        .and_then(|pid| planner_tasks.get(pid))
                        .map(|v| v.iter().collect())
                        .unwrap_or_default();

                    if preview_tasks.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            if let Some(pid) = planner_selected {
                                let proj_name = projects.iter()
                                    .find(|p| p.id == pid)
                                    .map(|p| p.name.as_str())
                                    .unwrap_or("?");
                                ui.label(RichText::new(format!("NO TASKS  ({})", proj_name.to_uppercase()))
                                    .color(Colors::TEXT_MUTED).size(8.0).italics());
                            } else {
                                ui.label(RichText::new("SELECT A PROJECT")
                                    .color(Colors::TEXT_MUTED).size(8.0).italics());
                            }
                        });
                    } else {
                        for task in preview_tasks.iter().take(5) {
                            ui.horizontal(|ui| {
                                ui.add_space(6.0);
                                let col = task.status.color();
                                // Mini status badge
                                egui::Frame::none()
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::new(1.0, col))
                                    .inner_margin(egui::Margin::symmetric(2.0, 1.0))
                                    .show(ui, |ui| {
                                        let s = match task.status {
                                            crate::app::PlannerTaskStatus::Todo       => "Â·",
                                            crate::app::PlannerTaskStatus::InProgress => "â–¶",
                                            crate::app::PlannerTaskStatus::Done       => "âœ“",
                                            crate::app::PlannerTaskStatus::Blocked    => "âœ•",
                                        };
                                        ui.label(RichText::new(s).color(col).size(7.5));
                                    });
                                ui.add_space(3.0);
                                let title_short = if task.title.len() > 22 {
                                    let i = task.title.char_indices().nth(22).map(|(i,_)| i).unwrap_or(task.title.len());
                                    format!("{}…", &task.title[..i])
                                } else { task.title.clone() };
                                ui.label(RichText::new(title_short).color(Colors::TEXT).size(8.5));
                                if task.ai_suggested {
                                    ui.label(RichText::new("âŸ").color(Colors::STATE_COMPUTE_PROVIDE).size(7.5));
                                }
                            });
                            ui.add_space(2.0);
                        }
                        if preview_tasks.len() > 5 {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(RichText::new(format!("â€¦+{} more", preview_tasks.len() - 5))
                                    .color(Colors::TEXT_MUTED).size(7.5).italics());
                            });
                        }
                    }
                });
        }); // end nav workspace scroll

    result
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// ACTIONS tab
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

// FIX: was `fn render_actions_tab(ui, s: &DetailState, ...)` â€” &DetailState
// cannot hand out &mut to its inner &'a mut String fields.
// All three tab functions now take `s: &mut DetailState`.
fn render_actions_tab(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    let is_android = s.device.os.to_lowercase().contains("android");

    ui.spacing_mut().item_spacing = egui::vec2(10.0, MainScreenUiRules::BLOCK_GAP);

    ui.columns(3, |cols| {
        if is_android {
            if action_card(&mut cols[0], theme::IconType::Tablet, "SCREEN MIRROR", "Launch scrcpy") {
                actions.launch_scrcpy = true;
            }
        } else if action_card(&mut cols[0], theme::IconType::RDP, "REMOTE DESKTOP", "Launch RDP session") {
            actions.launch_rdp = true;
        }
        if action_card(&mut cols[1], theme::IconType::Folder, "BROWSE FILES", "Open network share") {
            actions.browse_share = true;
        }
        if action_card(&mut cols[2], theme::IconType::Pulse, "PING NODE", "Check THE GRID agent") {
            actions.ping = true;
        }
    });

    ui.add_space(MainScreenUiRules::BLOCK_GAP);
    ui.columns(3, |cols| {
        if action_card(&mut cols[0], theme::IconType::Disk, "INDEX DRIVES", "Full local disk scan") {
            actions.scan_remote = true;
        }
        if action_card(&mut cols[1], theme::IconType::Globe, "GLOBAL SYNC", "Pull from remote mesh") {
            // This button triggers mesh sync immediately (implicit in app.rs if scan_remote is false?)
            // For now, let's keep it simple.
        }
        if action_card(&mut cols[2], theme::IconType::Power, "WOL", "Send Wake-on-LAN") {
            actions.wake_device = true;
        }
    });

    // â”€â”€ Grid Reachability Banner â”€â”€
    if s.status == crate::app::NodeStatus::Reachable {
        ui.add_space(MainScreenUiRules::BLOCK_GAP);
        egui::Frame::none()
            .fill(Colors::BG_WIDGET)
            .stroke(egui::Stroke::new(1.0, Colors::AMBER))
            .inner_margin(egui::Margin::symmetric(16.0, 12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("âš ").color(Colors::AMBER).size(14.0));
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("AGENT UNREACHABLE").color(Colors::TEXT).size(10.0).strong());
                        ui.label(RichText::new("The machine is online but THE GRID agent isn't responding.").color(Colors::TEXT_DIM).size(9.0));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.horizontal(|ui| {
                            if s.device.os.to_lowercase().contains("windows") {
                                if theme::micro_button(ui, "LAUNCH RDP").clicked() {
                                    actions.launch_rdp = true;
                                }
                            }
                            if theme::micro_button(ui, "TRY SSH").clicked() {
                                actions.launch_ssh = true;
                            }
                        });
                    });
                });
            });
    }

    // â”€â”€ RDP Enablement Banner â”€â”€
    let has_rdp = s.telemetry.map(|t| t.capabilities.has_rdp).unwrap_or(false);
    if !has_rdp && s.device.os.to_lowercase().contains("windows") {
        ui.add_space(MainScreenUiRules::BLOCK_GAP);
        egui::Frame::none()
            .fill(Colors::BG_WIDGET)
            .stroke(egui::Stroke::new(1.0, Colors::AMBER))
            .inner_margin(egui::Margin::symmetric(16.0, 12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("âš ").color(Colors::AMBER).size(14.0));
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("REMOTE DESKTOP IS DISABLED").color(Colors::TEXT).size(10.0).strong());
                        ui.label(RichText::new("RDP must be enabled on the target Windows machine to connect.").color(Colors::TEXT_DIM).size(9.0));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::primary_button(ui, "ENABLE RDP").clicked() {
                            actions.enable_rdp = true;
                        }
                    });
                });
            });
    }

    if !is_android {
        ui.add_space(MainScreenUiRules::SECTION_GAP);
        ui.add(egui::Separator::default().spacing(0.0));
        ui.add_space(MainScreenUiRules::SECTION_GAP);

        // â”€â”€ RDP Options â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        theme::section_title(ui, "// RDP OPTIONS");
        ui.add_space(MainScreenUiRules::BLOCK_GAP);

        ui.columns(2, |cols| {
            cols[0].label(
                RichText::new("USERNAME").color(Colors::TEXT_DIM).size(MainScreenUiRules::FIELD_LABEL_SIZE).strong()
            );
            cols[0].add_space(4.0);
            cols[0].add(
                egui::TextEdit::singleline(s.rdp_username)
                    .font(egui::FontId::new(11.0, egui::FontFamily::Monospace))
                    .hint_text("Administrator")
                    .desired_width(f32::INFINITY)
            );

            cols[1].label(
                RichText::new("RESOLUTION").color(Colors::TEXT_DIM).size(MainScreenUiRules::FIELD_LABEL_SIZE).strong()
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
                    for opt in ["FULLSCREEN", "1920Ã—1080", "2560Ã—1440", "1280Ã—800"] {
                        ui.selectable_value(s.rdp_resolution, opt.to_string(), opt);
                    }
                });
        });
    }

    ui.add_space(MainScreenUiRules::SECTION_GAP);
    ui.add(egui::Separator::default().spacing(0.0));
    ui.add_space(MainScreenUiRules::SECTION_GAP);

    if ui.available_width() >= MainScreenUiRules::SIDE_BY_SIDE_BREAKPOINT {
        StripBuilder::new(ui)
            .size(Size::relative(0.46))
            .size(Size::remainder())
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    render_node_info_section(ui, s.device);
                });
                strip.cell(|ui| {
                    render_watched_paths_section(ui, s.watch_paths, actions);
                });
            });
    } else {
        render_node_info_section(ui, s.device);
        ui.add_space(MainScreenUiRules::SECTION_GAP);
        ui.add(egui::Separator::default().spacing(0.0));
        ui.add_space(MainScreenUiRules::SECTION_GAP);
        render_watched_paths_section(ui, s.watch_paths, actions);
    }

    // â”€â”€ AI Model Setup â”€â”€
    if s.is_tg_agent {
        ui.add_space(MainScreenUiRules::SECTION_GAP);
        ui.add(egui::Separator::default().spacing(0.0));
        ui.add_space(MainScreenUiRules::SECTION_GAP);
        theme::section_title(ui, "// REMOTE AI SETUP");
        ui.add_space(MainScreenUiRules::BLOCK_GAP);

        // Auto-fill placeholders from telemetry if empty
        if s.remote_model_edit.is_empty() {
            if let Some(_t) = s.telemetry {
                if let Some(m) = _t.capabilities.ai_models.first() {
                    *s.remote_model_edit = m.clone();
                }
            }
        }
        if s.remote_url_edit.is_empty() {
            if let Some(_t) = s.telemetry {
                // If the remote node has a provider URL already, we could show it, 
                // but telemetry currently doesn't include it.
            }
        }
        
        ui.columns(2, |cols| {
            cols[0].label(RichText::new("AI MODEL").color(Colors::TEXT_DIM).size(MainScreenUiRules::FIELD_LABEL_SIZE).strong());
            cols[0].add_space(4.0);
            cols[0].add(egui::TextEdit::singleline(s.remote_model_edit).hint_text("e.g. llama3").desired_width(f32::INFINITY));

            cols[1].label(RichText::new("PROVIDER URL").color(Colors::TEXT_DIM).size(MainScreenUiRules::FIELD_LABEL_SIZE).strong());
            cols[1].add_space(4.0);
            cols[1].add(egui::TextEdit::singleline(s.remote_url_edit).hint_text("http://localhost:8080").desired_width(f32::INFINITY));
        });

        ui.add_space(MainScreenUiRules::BLOCK_GAP);
        if theme::secondary_button(ui, "UPDATE REMOTE CONFIG").clicked() {
            actions.update_remote_config = Some((
                None, // device_type (no change from this UI)
                Some(s.remote_model_edit.clone()), 
                Some(s.remote_url_edit.clone())
            ));
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

fn render_node_info_section(ui: &mut Ui, device: &TailscaleDevice) {
    theme::section_title(ui, "// NODE INFO");
    ui.add_space(MainScreenUiRules::BLOCK_GAP);

    egui::Grid::new("node_info_grid")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            for (k, v) in [
                ("HOSTNAME", device.hostname.as_str()),
                ("IP", device.primary_ip().unwrap_or("â€”")),
                ("OS", device.os.as_str()),
                ("CLIENT", device.client_version.as_str()),
                ("USER", device.user.as_str()),
                ("AUTHORIZED", if device.authorized { "YES" } else { "NO" }),
            ] {
                ui.label(RichText::new(k).color(Colors::TEXT_DIM).size(MainScreenUiRules::INFO_LABEL_SIZE));
                ui.label(RichText::new(v.to_uppercase()).color(Colors::TEXT).size(MainScreenUiRules::INFO_VALUE_SIZE).strong());
                ui.end_row();
            }

            ui.label(RichText::new("LAST SEEN").color(Colors::TEXT_DIM).size(MainScreenUiRules::INFO_LABEL_SIZE));
            ui.label(
                RichText::new(
                    device
                        .last_seen
                        .map(|t| t.with_timezone(&chrono::Local).format("%d/%m/%y %H:%M").to_string())
                        .unwrap_or_else(|| "â€”".into())
                )
                .color(Colors::TEXT)
                .size(MainScreenUiRules::INFO_VALUE_SIZE)
                .strong()
            );
            ui.end_row();
        });
}

fn render_watched_paths_section(ui: &mut Ui, watch_paths: &[PathBuf], actions: &mut DetailActions) {
    theme::section_title(ui, "// WATCHED PATHS");
    ui.add_space(MainScreenUiRules::BLOCK_GAP);

    if watch_paths.is_empty() {
        ui.label(RichText::new("No directories watched yet").color(Colors::TEXT_MUTED).size(9.0));
    } else {
        for path in watch_paths {
            ui.horizontal(|ui| {
                ui.label(RichText::new("â—ˆ").color(Colors::GREEN).size(9.0));
                ui.add_space(4.0);
                ui.label(RichText::new(path.display().to_string()).color(Colors::TEXT_DIM).size(9.0));
            });
        }
    }

    ui.add_space(MainScreenUiRules::BLOCK_GAP);
    if theme::secondary_button(ui, "+ ADD WATCH DIRECTORY").clicked() {
        actions.add_watch_path = true;
    }
}

fn action_card(ui: &mut Ui, icon: theme::IconType, label: &str, sub: &str) -> bool {
    let desired_size = egui::vec2(ui.available_width(), MainScreenUiRules::ACTION_CARD_H);
    let (rect, resp) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    let hovered = resp.hovered();
    let time = ui.input(|i| i.time);

    // Sci-fi pad background + borders
    theme::Fx::action_pad(ui.painter(), rect, Colors::GREEN, hovered, time);

    // Icon (centred in upper half)
    let icon_size = 20.0;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + rect.height() * 0.38),
        egui::vec2(icon_size, icon_size),
    );
    let icon_color = if hovered { Colors::GREEN } else { Color32::from_rgba_unmultiplied(0, 255, 65, 180) };
    theme::draw_vector_icon(ui, icon_rect, icon, icon_color);

    // Label
    let label_y = rect.top() + rect.height() * 0.64;
    ui.painter().text(
        egui::pos2(rect.center().x, label_y),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::new(MainScreenUiRules::ACTION_LABEL_SIZE, egui::FontFamily::Monospace),
        if hovered { Colors::GREEN } else { Colors::TEXT },
    );

    // Sub-label
    let sub_y = rect.top() + rect.height() * 0.84;
    ui.painter().text(
        egui::pos2(rect.center().x, sub_y),
        egui::Align2::CENTER_CENTER,
        sub,
        egui::FontId::new(MainScreenUiRules::ACTION_SUB_SIZE, egui::FontFamily::Monospace),
        Colors::TEXT_DIM,
    );

    if hovered {
        ui.ctx().request_repaint();
    }
    resp.clicked()
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// FILES tab
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn render_files_tab(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    // Weighted layout: 30% Send, 70% File Manager
    let total_width = ui.available_width();
    let left_w = (total_width * 0.3).max(200.0);
    let right_w = total_width - left_w - 12.0;

    ui.horizontal_top(|ui| {
        // â”€â”€ Left: Send column â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        ui.vertical(|ui| {
            ui.set_min_width(left_w);
            ui.set_max_width(left_w);

            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                let c = rect.center();
                ui.painter().line_segment([c + egui::vec2(0.0, 5.0), c + egui::vec2(0.0, -5.0)], egui::Stroke::new(1.2, Colors::TEXT_DIM));
                ui.painter().line_segment([c + egui::vec2(-3.0, -2.0), c + egui::vec2(0.0, -5.0)], egui::Stroke::new(1.2, Colors::TEXT_DIM));
                ui.painter().line_segment([c + egui::vec2(3.0, -2.0), c + egui::vec2(0.0, -5.0)], egui::Stroke::new(1.2, Colors::TEXT_DIM));
                ui.add_space(4.0);
                ui.label(RichText::new("SEND TO NODE").color(Colors::TEXT_DIM).size(9.0).strong());
            });
            ui.add_space(8.0);

            let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
            let dz_stroke = if hovering {
                egui::Stroke::new(1.0, Colors::GREEN)
            } else {
                egui::Stroke::new(1.0, Colors::BORDER)
            };

            egui::Frame::none()
                .fill(Colors::BG_WIDGET)
                .stroke(dz_stroke)
                .inner_margin(egui::Margin::same(16.0))
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(ui.available_width(), 80.0));
                    ui.vertical_centered(|ui| {
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
                        ui.label(RichText::new("DROP FILES HERE").color(Colors::TEXT_MUTED).size(9.0));
                        ui.label(RichText::new("or SELECT FILES below").color(Colors::TEXT_MUTED).size(8.0));
                    });
                });

            ui.add_space(8.0);
            for item in s.file_queue {
                let (label, color) = match &item.status {
                    FileTransferStatus::Pending   => ("PENDING", Colors::TEXT_MUTED),
                    FileTransferStatus::Sending   => ("SENDING", Colors::AMBER),
                    FileTransferStatus::Done      => ("âœ“ DONE",  Colors::GREEN),
                    FileTransferStatus::Failed(_) => ("âœ— FAIL",  Colors::RED),
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&item.name).color(Colors::TEXT).size(9.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(label).color(color).size(8.0));
                        ui.label(RichText::new(fmt_bytes(item.size)).color(Colors::TEXT_DIM).size(8.0));
                    });
                });
            }

            ui.add_space(8.0);
            if theme::secondary_button(ui, "SELECT FILES").clicked() {
                actions.select_files = true;
            }
        });

        ui.add_space(12.0);

        // â”€â”€ Right: HUD File Manager â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        ui.vertical(|ui| {
            ui.set_min_width(right_w);
            ui.set_max_width(right_w);
            
            crate::views::file_manager::render(ui, s, actions);
            
            ui.add_space(8.0);
            if theme::secondary_button(ui, "OPEN INBOX FOLDER").clicked() {
                actions.open_inbox = true;
            }
        });
    });

    // â”€â”€ Transfer log â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
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

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// CLIPBOARD tab
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
        if theme::secondary_button(ui, "â†‘ LOAD MY CLIPBOARD").clicked() {
            actions.load_clipboard = true;
        }
        ui.add_space(8.0);
        ui.set_enabled(!s.clip_out.trim().is_empty());
        if theme::primary_button(ui, "â‡’ TRANSMIT").clicked() {
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
                                        .color(Colors::GREEN).size(8.0).strong()
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
                                let i = entry.content.char_indices().nth(180).map(|(i,_)| i).unwrap_or(entry.content.len());
                                format!("{}…", &entry.content[..i])
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

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Settings modal
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

pub struct SettingsState {
    pub api_key:      String,
    pub device_name:  String,
    pub rdp_username: String,
    pub agent_port:   String,
    pub watch_paths:  Vec<String>,
    pub ai_model:     String,
    pub device_type:  String,
    pub open:         bool,
    pub google_client_id:     String,
    pub google_client_secret: String,
    /// Set to true for one frame when user clicks "Connect Drive"
    pub connect_drive: bool,
    /// Set to true for one frame when user clicks "Index Drive"
    pub index_drive: bool,
    /// If set, "PUSH TO NODE & RESTART" button is enabled; cleared after app reads it.
    pub push_and_restart: bool,
    /// IP of the remote node to push config to (set by app before opening modal for a remote device).
    pub target_device_ip: Option<String>,
    /// Device ID of the remote node.
    pub target_device_id: Option<String>,
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
            google_client_id:     cfg.google_client_id.clone().unwrap_or_default(),
            google_client_secret: cfg.google_client_secret.clone().unwrap_or_default(),
            connect_drive: false,
            index_drive:   false,
            push_and_restart: false,
            target_device_ip: None,
            target_device_id: None,
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
            ui.painter().rect_filled(bar, egui::Rounding::ZERO, Colors::GREEN);
            ui.add_space(2.0);

            // Header row
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(20.0, 14.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("// CONFIGURATION")
                                .color(Colors::GREEN).size(10.0).strong()
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui.add(
                                    egui::Button::new(
                                        RichText::new("âœ•").color(Colors::TEXT_DIM)
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
                    ui.add(egui::Separator::default().spacing(0.0));
                    ui.add_space(10.0);
                    ui.label(RichText::new("GOOGLE DRIVE").color(Colors::TEXT_DIM).size(9.0).strong());
                    ui.add_space(6.0);
                    modal_field(ui, "OAUTH CLIENT ID",     &mut s.google_client_id,     false, "paste from Google Cloud Console");
                    ui.add_space(8.0);
                    modal_field(ui, "OAUTH CLIENT SECRET", &mut s.google_client_secret, true,  "paste from Google Cloud Console");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if theme::primary_button(ui, "CONNECT DRIVE").clicked() {
                            s.connect_drive = true;
                        }
                        ui.add_space(8.0);
                        if theme::micro_button(ui, "INDEX DRIVE").clicked() {
                            s.index_drive = true;
                        }
                    });
                    ui.add_space(10.0);
                    ui.add(egui::Separator::default().spacing(0.0));
                    ui.add_space(14.0);
                    ui.label(RichText::new("WATCHED DIRECTORIES").color(Colors::TEXT_DIM).size(9.0).strong());
                    ui.add_space(4.0);
                    
                    let mut to_remove = None;
                    for (i, path) in s.watch_paths.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(path).desired_width(ui.available_width() - 30.0));
                            if ui.button("âœ•").clicked() {
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
                    ui.horizontal(|ui| {
                        if theme::primary_button(ui, "SAVE & REFRESH").clicked() {
                            s.open = false;
                            saved = true;
                        }
                        if let Some(ref ip) = s.target_device_ip.clone() {
                            let label = format!("PUSH TO {} & RESTART", ip);
                            ui.add_space(8.0);
                            if theme::primary_button(ui, &label).clicked() {
                                s.open = false;
                                saved = true;
                                s.push_and_restart = true;
                            }
                        }
                    });
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

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Empty state
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Helpers
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

struct TelemetryUiRules;

impl TelemetryUiRules {
    const WIDE_BREAKPOINT: f32 = 980.0;
    const BAND_HEIGHT: f32 = 92.0;
    const BAND_MIN_HEIGHT: f32 = 72.0;
    const BAND_MAX_HEIGHT: f32 = 180.0;
    const BAND_BOTTOM_GAP: f32 = 8.0;
    const RESIZE_HANDLE_H: f32 = 6.0;
    const PANE_MIN_SIZE: f32 = 96.0;
    const CARD_MIN_HEIGHT: f32 = 78.0;
    const CPU_HEATMAP_CELL_H: f32 = 11.0;
    const RAM_SLOT_CELL_H: f32 = 14.0;
    const CARD_PAD_Y: f32 = 3.0;
    const CARD_ITEM_GAP_X: f32 = 4.0;
    const CARD_ITEM_GAP_Y: f32 = 2.0;

    const HEADER_TITLE_SIZE: f32 = 8.5;
    const HEADER_VALUE_SIZE: f32 = 8.0;
    const META_TEXT_SIZE: f32 = 7.5;

    const BAR_CHAR_PX: f32 = 6.2;
    const DISK_ROW_H: f32 = 13.0;
    const NET_BAR_H: f32 = 10.0;
    const ROW_GAP: f32 = 2.0;
    const SECTION_GAP: f32 = 3.0;

    const TILE_SHARE_ID: f32 = 2.0;
    const TILE_SHARE_CENTER: f32 = 7.2;
    const TILE_SHARE_PERF: f32 = 1.8;

    const TILE_SHARE_CPU: f32 = 1.0;
    const TILE_SHARE_RAM_GPU: f32 = 1.05;
    const TILE_SHARE_DISK: f32 = 1.35;
    const TILE_SHARE_NET: f32 = 0.95;
    const TILE_SHARE_TASKS: f32 = 1.0;
}

pub fn default_telemetry_band_height() -> f32 {
    TelemetryUiRules::BAND_HEIGHT
}

struct MainScreenUiRules;

impl MainScreenUiRules {
    const SECTION_GAP: f32 = 16.0;
    const BLOCK_GAP: f32 = 8.0;
    const ACTION_CARD_H: f32 = 86.0;
    const ACTION_CARD_PAD: f32 = 16.0;
    const ACTION_LABEL_SIZE: f32 = 9.0;
    const ACTION_SUB_SIZE: f32 = 8.0;
    const FIELD_LABEL_SIZE: f32 = 8.0;
    const INFO_LABEL_SIZE: f32 = 10.0;
    const INFO_VALUE_SIZE: f32 = 10.0;
    const SIDE_BY_SIDE_BREAKPOINT: f32 = 900.0;
}

pub fn fmt_bytes(b: u64) -> String {
    const K: u64 = 1024;
    if b < K           { format!("{} B", b) }
    else if b < K * K  { format!("{:.1} KB", b as f64 / K as f64) }
    else if b < K*K*K  { format!("{:.1} MB", b as f64 / (K * K) as f64) }
    else               { format!("{:.2} GB", b as f64 / (K * K * K) as f64) }
}

fn telemetry_severity_color(pct: f32) -> Color32 {
    let p = pct.clamp(0.0, 100.0);
    if p < 55.0 {
        Color32::from_rgb(0, 255, 96)
    } else if p < 82.0 {
        Color32::from_rgb(255, 176, 32)
    } else {
        Color32::from_rgb(255, 64, 64)
    }
}

fn telemetry_severity_color_alpha(pct: f32, alpha: u8) -> Color32 {
    let c = telemetry_severity_color(pct);
    Color32::from_rgba_premultiplied(c.r(), c.g(), c.b(), alpha)
}

fn ai_meter_state(ai_status: Option<&str>) -> (f32, String) {
    match ai_status.map(|s| s.to_uppercase()) {
        Some(s) if s.contains("GENERAT") => (90.0, "GENERATING".to_string()),
        Some(s) if s.contains("PROCESS") => (74.0, "PROCESSING".to_string()),
        Some(s) if s.contains("IDLE") => (18.0, "IDLE".to_string()),
        Some(s) => (52.0, s),
        None => (10.0, "IDLE".to_string()),
    }
}

fn render_telemetry_chip(ui: &mut Ui, icon: theme::IconType, label: &str, pct: f32, value: &str) {
    let clamped = pct.clamp(0.0, 100.0);
    let color = telemetry_severity_color(clamped);

    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .stroke(egui::Stroke::new(1.0, Colors::BORDER2))
        .inner_margin(egui::Margin::symmetric(4.0, 2.0))
        .show(ui, |ui| {
            // Force chip to fill full available column width so the bar stretches
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(11.0, 11.0), egui::Sense::hover());
                theme::draw_vector_icon(ui, icon_rect, icon, color);
                ui.add_space(3.0);
                ui.label(RichText::new(label).color(Colors::TEXT_DIM).size(9.0).strong());
                ui.add_space(4.0);

                // Responsive bar: consumes all remaining width minus the value label reserve
                let reserve = (value.len() as f32 * 6.2 + 8.0).clamp(28.0, 100.0);
                let bar_w = (ui.available_width() - reserve).max(10.0);
                let (bar_rect, _) = ui.allocate_exact_size(egui::vec2(bar_w, 4.0), egui::Sense::hover());
                ui.painter().rect_filled(bar_rect, egui::Rounding::ZERO, Colors::BORDER);
                let fill_w = bar_rect.width() * clamped / 100.0;
                if fill_w > 0.0 {
                    let fill = egui::Rect::from_min_size(bar_rect.min, egui::vec2(fill_w, bar_rect.height()));
                    ui.painter().rect_filled(fill, egui::Rounding::ZERO, color);
                    let glow = egui::Rect::from_min_size(fill.min, egui::vec2(fill_w, 1.0));
                    ui.painter().rect_filled(glow, egui::Rounding::ZERO, Colors::TEXT);
                }

                ui.add_space(4.0);
                ui.label(RichText::new(value).color(color).size(9.0).strong());
            });
        });
}

// Draws a single-line section header with an inline progress bar:
//   // TITLE [â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–‘â–‘] value
fn render_section_header_bar(ui: &mut Ui, title: &str, pct: f32, value: &str) {
    let clamped = pct.clamp(0.0, 100.0);
    let color = telemetry_severity_color(clamped);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(RichText::new(title).color(Colors::GREEN).size(TelemetryUiRules::HEADER_TITLE_SIZE).strong());
        ui.add_space(5.0);
        // Reserve proportional to value text length, based on shared typography scale.
        let reserve = (value.len() as f32 * TelemetryUiRules::BAR_CHAR_PX + 10.0).clamp(50.0, 160.0);
        let bar_w = (ui.available_width() - reserve).max(10.0);
        let (bar_rect, _) = ui.allocate_exact_size(egui::vec2(bar_w, 3.0), egui::Sense::hover());
        ui.painter().rect_filled(bar_rect, egui::Rounding::ZERO, Colors::BORDER);
        let fill_w = bar_rect.width() * clamped / 100.0;
        if fill_w > 0.0 {
            let fill = egui::Rect::from_min_size(bar_rect.min, egui::vec2(fill_w, bar_rect.height()));
            ui.painter().rect_filled(fill, egui::Rounding::ZERO, color);
            let glow = egui::Rect::from_min_size(fill.min, egui::vec2(fill_w, 1.0));
            ui.painter().rect_filled(glow, egui::Rounding::ZERO, Colors::TEXT);
        }
        ui.add_space(5.0);
        ui.label(RichText::new(value).color(color).size(TelemetryUiRules::HEADER_VALUE_SIZE).strong());
    });
}

fn render_telemetry_card(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    // No fill/background â€” keeps the strip flat. Padding gives breathing room.
    // A bottom hairline acts as the section separator instead of a full box.
    egui::Frame::none().show(ui, |ui| {
        let card_target_h = ui.available_height().max(TelemetryUiRules::CARD_MIN_HEIGHT);
        ui.set_min_height(card_target_h);
        ui.add_space(TelemetryUiRules::CARD_PAD_Y);
        let old_spacing = ui.spacing().item_spacing;
        ui.spacing_mut().item_spacing = egui::vec2(TelemetryUiRules::CARD_ITEM_GAP_X, TelemetryUiRules::CARD_ITEM_GAP_Y);
        add_contents(ui);
        ui.spacing_mut().item_spacing = old_spacing;
        // Bottom hairline separator
        ui.add_space(TelemetryUiRules::CARD_PAD_Y);
        let width = ui.available_width().max(1.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, egui::Rounding::ZERO, Colors::BORDER2);
        ui.add_space(TelemetryUiRules::CARD_PAD_Y);
    });
}

fn render_section_divider(ui: &mut Ui) {
    let width = ui.available_width().max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::Rounding::ZERO, Colors::BORDER2);
}

fn render_compact_status_row(
    ui: &mut Ui,
    icon: theme::IconType,
    label: &str,
    right: &str,
    right_color: Color32,
) {
    ui.horizontal(|ui| {
        let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        theme::draw_vector_icon(ui, icon_rect, icon, right_color);
        ui.add_space(4.0);

        let reserve = (right.len() as f32 * 6.2 + 18.0).clamp(46.0, 92.0);
        let label_w = (ui.available_width() - reserve).max(28.0);
        let max_chars = ((label_w / 6.1).floor() as usize).max(8);
        ui.add_sized(
            egui::vec2(label_w, 10.0),
            egui::Label::new(RichText::new(elide(label, max_chars)).color(Colors::TEXT_DIM).size(8.0)),
        );
        ui.label(RichText::new(right).color(right_color).size(8.0).strong());
    });
}

#[allow(dead_code)]
fn render_disk_row(ui: &mut Ui, drive: &thegrid_core::models::DriveInfo) {
    let pct = if drive.total > 0 {
        drive.used as f32 / drive.total as f32 * 100.0
    } else {
        0.0
    };
    let color = telemetry_severity_color(pct);
    let right = format!("{} / {}", compact_gib(drive.used), compact_gib(drive.total));

    let drive_name = drive
        .name
        .split_whitespace()
        .next()
        .unwrap_or(drive.name.as_str());
    let short_name = elide(drive_name, 8);
    let label = format!("{} {}", drive.kind.as_deref().unwrap_or("DISK"), short_name);

    ui.horizontal(|ui| {
        let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        theme::draw_vector_icon(ui, icon_rect, disk_icon(drive.kind.as_deref()), color);
        ui.add_space(4.0);

        let reserve = (right.len() as f32 * 6.2 + 24.0).clamp(76.0, 140.0);
        let label_w = (ui.available_width() - reserve).max(26.0);
        let max_chars = ((label_w / 5.9).floor() as usize).max(6);
        ui.add_sized(
            egui::vec2(label_w, 10.0),
            egui::Label::new(RichText::new(elide(&label, max_chars)).color(Colors::TEXT_DIM).size(7.8)),
        );

        let bar_w = (ui.available_width() - (right.len() as f32 * 6.2 + 12.0)).max(10.0);
        let (bar_rect, _) = ui.allocate_exact_size(egui::vec2(bar_w, 3.0), egui::Sense::hover());
        ui.painter().rect_filled(bar_rect, egui::Rounding::ZERO, Colors::BORDER);
        let fill_w = bar_rect.width() * pct.clamp(0.0, 100.0) / 100.0;
        if fill_w > 0.0 {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(bar_rect.min, egui::vec2(fill_w, bar_rect.height())),
                egui::Rounding::ZERO,
                color,
            );
        }

        ui.add_space(5.0);
        ui.label(RichText::new(right).color(color).size(8.0).strong());
    });
}

fn render_cpu_core_strip(
    ui: &mut Ui,
    cores: &[f32],
    physical_cores: Option<u32>,
    logical_processors: Option<u32>,
) {
    if cores.is_empty() { return; }

    let n = cores.len().min(64);
    let logical: Vec<f32> = cores.iter().copied().take(n).collect();
    let logical_count = logical_processors.unwrap_or(n as u32).max(1) as usize;

    let physical_target = physical_cores
        .map(|v| v as usize)
        .filter(|v| *v > 0)
        .unwrap_or_else(|| (logical_count / 2).max(1));

    // Aggregate logical samples to physical-core buckets for a closer topology view.
    let chunk = ((logical.len() as f32) / (physical_target as f32)).ceil() as usize;
    let chunk = chunk.max(1);
    let mut physical = Vec::with_capacity(physical_target);
    for part in logical.chunks(chunk).take(physical_target) {
        let avg = part.iter().copied().sum::<f32>() / part.len().max(1) as f32;
        physical.push(avg);
    }
    if physical.is_empty() {
        physical.push(logical.iter().copied().sum::<f32>() / logical.len().max(1) as f32);
    }

    let logical_avg = logical.iter().copied().sum::<f32>() / logical.len().max(1) as f32;
    let physical_avg = physical.iter().copied().sum::<f32>() / physical.len().max(1) as f32;

    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("THREADS {}", logical_count)).color(Colors::TEXT_DIM).size(8.2).strong());
        ui.add_space(8.0);
        ui.label(RichText::new(format!("AVG {:>2.0}%", logical_avg.round())).color(Colors::TEXT_MUTED).size(7.9));
        ui.add_space(10.0);
        ui.label(RichText::new(format!("PHYSICAL {}", physical_target)).color(Colors::TEXT_DIM).size(8.2).strong());
        ui.add_space(8.0);
        ui.label(RichText::new(format!("AVG {:>2.0}%", physical_avg.round())).color(Colors::TEXT_MUTED).size(7.9));
    });

    let area_w = ui.available_width().max(80.0);
    let area_h = (TelemetryUiRules::CPU_HEATMAP_CELL_H * 4.5).max(42.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(area_w, area_h), egui::Sense::hover());
    let inner = rect.shrink2(egui::vec2(2.0, 2.0));

    let origin = egui::pos2(inner.min.x + 12.0, inner.max.y - 2.0);
    let vx = egui::vec2((inner.width() - 24.0).max(24.0), 0.0);        // x axis (samples)
    let vz = egui::vec2(-inner.width() * 0.12, -inner.height() * 0.20); // depth axis (lanes)
    let vy = egui::vec2(0.0, -inner.height() * 0.70);                   // height axis (load)

    let proj = |t: f32, lane: f32, load: f32| -> egui::Pos2 {
        origin + vx * t + vz * lane + vy * load.clamp(0.0, 1.0)
    };

    // Main platform.
    let base_poly = vec![proj(0.0, 0.0, 0.0), proj(1.0, 0.0, 0.0), proj(1.0, 1.0, 0.0), proj(0.0, 1.0, 0.0)];
    ui.painter().add(egui::Shape::convex_polygon(
        base_poly,
        Color32::from_rgba_premultiplied(10, 16, 13, 220),
        egui::Stroke::new(1.0, Colors::BORDER2),
    ));

    // Subtle grid in the platform.
    for i in 1..=8 {
        let t = i as f32 / 9.0;
        ui.painter().line_segment(
            [proj(t, 0.0, 0.0), proj(t, 1.0, 0.0)],
            egui::Stroke::new(1.0, Color32::from_rgba_premultiplied(0, 255, 96, 15)),
        );
    }
    for i in 1..=2 {
        let l = i as f32 / 3.0;
        ui.painter().line_segment(
            [proj(0.0, l, 0.0), proj(1.0, l, 0.0)],
            egui::Stroke::new(1.0, Color32::from_rgba_premultiplied(0, 255, 96, 12)),
        );
    }

    let draw_iso_blocks = |ui: &mut Ui, arr: &[f32], lane: f32, label: &str, reverse_x: bool| {
        if arr.is_empty() { return; }
        let n = arr.len();
        let step = 1.0 / n as f32;
        let bar_w = step * 0.78;

        for (i, v) in arr.iter().copied().enumerate() {
            let base_t = i as f32 * step + (step - bar_w) * 0.5;
            let t0 = if reverse_x { 1.0 - (base_t + bar_w) } else { base_t };
            let t1 = t0 + bar_w;
            let h = (v / 100.0).clamp(0.0, 1.0);

            let a = proj(t0, lane, 0.0);
            let b = proj(t1, lane, 0.0);
            let c = proj(t1, lane + 0.09, 0.0);

            let a_top = proj(t0, lane, h);
            let b_top = proj(t1, lane, h);
            let c_top = proj(t1, lane + 0.09, h);
            let d_top = proj(t0, lane + 0.09, h);

            let color = telemetry_severity_color(v);
            let top_fill = telemetry_severity_color_alpha(v, 160);
            let side_fill = telemetry_severity_color_alpha(v, 88);
            let front_fill = telemetry_severity_color_alpha(v, 118);

            // left/front face
            ui.painter().add(egui::Shape::convex_polygon(
                vec![a, b, b_top, a_top],
                front_fill,
                egui::Stroke::new(0.7, telemetry_severity_color_alpha(v, 120)),
            ));
            // depth side face
            ui.painter().add(egui::Shape::convex_polygon(
                vec![b, c, c_top, b_top],
                side_fill,
                egui::Stroke::new(0.7, telemetry_severity_color_alpha(v, 110)),
            ));
            // top face
            ui.painter().add(egui::Shape::convex_polygon(
                vec![a_top, b_top, c_top, d_top],
                top_fill,
                egui::Stroke::new(0.8, color),
            ));

            // tiny bloom on hot bars
            if v >= 70.0 {
                let center = egui::pos2((a_top.x + c_top.x) * 0.5, (a_top.y + c_top.y) * 0.5);
                ui.painter().circle_filled(center, 1.4, telemetry_severity_color_alpha(v, 180));
            }
        }

        ui.painter().text(
            proj(0.0, lane, 0.02),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::monospace(7.1),
            Colors::TEXT_DIM,
        );
    };

    // Crossing direction: threads forward, physical reversed.
    draw_iso_blocks(ui, &logical, 0.14, "THREADS", false);
    draw_iso_blocks(ui, &physical, 0.82, "PHYSICAL", true);
}

fn compact_gib(bytes: u64) -> String {
    let gib = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    if gib >= 10.0 {
        format!("{:.0}G", gib)
    } else {
        format!("{:.1}G", gib)
    }
}

fn elide(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        return s.to_string();
    }
    if max_chars <= 3 {
        return "...".to_string();
    }
    let take = max_chars - 3;
    format!("{}...", chars[..take].iter().collect::<String>())
}

/// Shorten GPU vendor names to a compact model identifier.
/// "NVIDIA GeForce RTX 3080 Ti" â†’ "RTX 3080 Ti"
/// "Intel(R) UHD Graphics 630"  â†’ "UHD 630"
/// "AMD Radeon RX 7900 XTX"     â†’ "RX 7900 XTX"
fn short_gpu_name(name: &str) -> String {
    let stripped = name
        .replace("NVIDIA GeForce ", "")
        .replace("NVIDIA ", "")
        .replace("AMD Radeon ", "")
        .replace("AMD ", "")
        .replace("Intel(R) ", "")
        .replace("Intel ", "");
    // Take at most the first 4 words of what remains
    stripped.split_whitespace().take(4).collect::<Vec<_>>().join(" ")
}

#[allow(dead_code)]
fn disk_icon(kind: Option<&str>) -> theme::IconType {
    match kind.map(|k| k.to_ascii_uppercase()) {
        Some(k) if k.contains("NVME") => theme::IconType::Database,
        Some(k) if k.contains("SSD") => theme::IconType::Disk,
        _ => theme::IconType::Disk,
    }
}

fn perf_scores(telem: &NodeTelemetry) -> (f32, f32, f32) {
    let ram_gb = telem.ram_total as f32 / (1024.0 * 1024.0 * 1024.0);
    let cores = telem
        .cpu_physical_cores
        .map(|v| v as f32)
        .or_else(|| telem.cpu_cores_pct.as_ref().map(|c| (c.len().max(2) as f32) * 0.5))
        .unwrap_or(4.0);
    let gpu_bonus = if telem.gpu_name.is_some() || !telem.gpu_devices.is_empty() { 22.0 } else { 0.0 };
    let gpu_mem_bonus = telem.gpu_mem_total.map(|m| (m as f32 / (1024.0 * 1024.0 * 1024.0)).min(24.0)).unwrap_or(0.0);
    let freq = telem.cpu_freq_ghz.unwrap_or(2.0);

    let image = (ram_gb * 3.2 + cores * 1.8 + gpu_bonus + gpu_mem_bonus).clamp(0.0, 100.0);
    let text = (ram_gb * 4.8 + cores * 2.4 + freq * 8.0).clamp(0.0, 100.0);
    let coding = (ram_gb * 3.8 + cores * 3.6 + freq * 10.0).clamp(0.0, 100.0);
    (image, text, coding)
}

fn render_perf_hex(ui: &mut Ui, telem: &NodeTelemetry) {
    let (img, txt, code) = perf_scores(telem);
    // No outer frame â€” sits flat inside the right panel column.
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 36.0), egui::Sense::hover());
        let c = rect.center();
        let r = 13.0;
        let mut outer = Vec::new();
        for i in 0..6 {
            let a = std::f32::consts::TAU * (i as f32) / 6.0 + std::f32::consts::PI / 6.0;
            outer.push(c + egui::vec2(r * a.cos(), r * a.sin()));
        }
        ui.painter().add(egui::Shape::closed_line(outer.clone(), egui::Stroke::new(1.0, Colors::BORDER2)));

        let points = [img, txt, code, img * 0.6, txt * 0.6, code * 0.6];
        let mut poly = Vec::new();
        for (i, pct) in points.iter().enumerate() {
            let a = std::f32::consts::TAU * (i as f32) / 6.0 + std::f32::consts::PI / 6.0;
            let rr = r * (*pct / 100.0);
            poly.push(c + egui::vec2(rr * a.cos(), rr * a.sin()));
        }
        ui.painter().add(egui::Shape::convex_polygon(poly, Color32::from_rgba_premultiplied(0, 255, 120, 36), egui::Stroke::new(1.0, Colors::GREEN)));

        ui.vertical(|ui| {
            ui.label(RichText::new("MODEL PERF").color(Colors::GREEN).size(7.5).strong());
            ui.label(RichText::new(format!("IMG {:>3.0}%", img)).color(Colors::TEXT_DIM).size(7.0));
            ui.label(RichText::new(format!("TXT {:>3.0}%", txt)).color(Colors::TEXT_DIM).size(7.0));
            ui.label(RichText::new(format!("CODE {:>3.0}%", code)).color(Colors::TEXT_DIM).size(7.0));
        });
    });
}

fn render_cpu_section(ui: &mut Ui, telem: &NodeTelemetry) {
    render_telemetry_card(ui, |ui| {
        let cpu = telem.cpu_pct;
        let cpu_value = if let Some(freq) = telem.cpu_freq_ghz {
            format!("{:.0}%  {:.2}GHz", cpu.round(), freq)
        } else {
            format!("{:.0}%", cpu.round())
        };
        render_section_header_bar(ui, "// CPU", cpu, &cpu_value);

        // Temperature only â€” LP count is shown inside the core strip label
        if let Some(temp) = telem.cpu_temp {
            ui.label(RichText::new(format!("TEMP {:.0}C", temp)).color(Colors::TEXT_DIM).size(TelemetryUiRules::META_TEXT_SIZE));
        }

        if let (Some(pc), Some(lp)) = (telem.cpu_physical_cores, telem.cpu_logical_processors) {
            ui.label(
                RichText::new(format!("TOPO {}C / {}T", pc, lp))
                    .color(Colors::TEXT_DIM)
                    .size(TelemetryUiRules::META_TEXT_SIZE)
            );
        }
        if let Some(model) = &telem.cpu_model {
            ui.label(
                RichText::new(elide(model, 34))
                    .color(Colors::TEXT_MUTED)
                    .size(TelemetryUiRules::META_TEXT_SIZE)
            );
        }

        if let Some(cores) = &telem.cpu_cores_pct {
            render_cpu_core_strip(
                ui,
                cores,
                telem.cpu_physical_cores,
                telem.cpu_logical_processors,
            );
        }
    });
}

// Draws all RAM slots in a single horizontal row: [16G][16G][FREE][FREE]
fn render_ram_slot_chips(
    ui: &mut Ui,
    modules: &[thegrid_core::models::RamModule],
    slots_used: Option<u32>,
    slots_total: Option<u32>,
    ram_total: u64,
) {
    let inferred_used = slots_used.unwrap_or(modules.len() as u32) as usize;
    let populated = modules.len().max(inferred_used);
    let mut total = slots_total.unwrap_or(populated as u32) as usize;
    if total < populated { total = populated; }
    if total == 0 { return; }

    let free = total.saturating_sub(populated);
    let gap  = 3.0_f32;
    let area_w = ui.available_width().max(40.0);
    let cell_w = ((area_w - gap * total.saturating_sub(1) as f32) / total as f32)
        .clamp(16.0, 48.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        // Populated slots
        for i in 0..populated {
            let label = if let Some(m) = modules.get(i) {
                compact_gib(m.capacity)
            } else {
                let cap_per = if populated > 0 && ram_total > 0 { ram_total / populated as u64 } else { 0 };
                if cap_per > 0 { compact_gib(cap_per) } else { "USED".to_string() }
            };
            egui::Frame::none()
                .fill(Colors::BG_WIDGET)
                .stroke(egui::Stroke::new(1.0, Colors::GREEN_DIM))
                .inner_margin(egui::Margin::symmetric(2.0, 2.0))
                .show(ui, |ui| {
                    ui.set_min_width(cell_w);
                    ui.set_min_height(TelemetryUiRules::RAM_SLOT_CELL_H);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.label(RichText::new(label).color(Colors::GREEN).size(7.5).strong());
                    });
                });
        }
        // Free slots
        for _ in 0..free {
            egui::Frame::none()
                .fill(Colors::BG_WIDGET)
                .stroke(egui::Stroke::new(1.0, Colors::BORDER2))
                .inner_margin(egui::Margin::symmetric(2.0, 2.0))
                .show(ui, |ui| {
                    ui.set_min_width(cell_w);
                    ui.set_min_height(TelemetryUiRules::RAM_SLOT_CELL_H);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.label(RichText::new("FREE").color(Colors::TEXT_MUTED).size(7.0));
                    });
                });
        }
    });
}

fn render_ram_gpu_section(ui: &mut Ui, telem: &NodeTelemetry) {
    render_telemetry_card(ui, |ui| {
        let ram = telem.ram_pct();
        let ram_value = format!("{} / {}",
            crate::telemetry::fmt_bytes(telem.ram_used),
            crate::telemetry::fmt_bytes(telem.ram_total));
        render_section_header_bar(ui, "// RAM", ram, &ram_value);

        if telem.ram_slots_total.unwrap_or(0) > 0 || !telem.ram_modules.is_empty() || telem.ram_slots_used.unwrap_or(0) > 0 {
            render_ram_slot_chips(
                ui,
                &telem.ram_modules,
                telem.ram_slots_used,
                telem.ram_slots_total,
                telem.ram_total,
            );
        }

        let mut ram_meta = Vec::<String>::new();
        if let Some(ff) = &telem.ram_form_factor { ram_meta.push(ff.clone()); }
        if let Some((used, total)) = telem.ram_slots_used.zip(telem.ram_slots_total) {
            ram_meta.push(format!("{}/{} SLOTS", used, total));
        }
        if let Some(mhz) = telem.ram_speed_mhz { ram_meta.push(format!("{}MHz", mhz)); }
        if !ram_meta.is_empty() {
            ui.label(RichText::new(ram_meta.join("  â€¢  ")).color(Colors::TEXT_DIM).size(TelemetryUiRules::META_TEXT_SIZE));
        }

        render_section_divider(ui);

        let discrete = telem.gpu_devices.iter().find(|d| d.is_discrete);
        let integrated = telem.gpu_devices.iter().find(|d| d.is_integrated || !d.is_discrete);
        let primary = discrete.or(integrated);

        if let Some(dev) = primary {
            let gpu_pct = dev.gpu_pct.or(telem.gpu_pct).unwrap_or(0.0);
            let gpu_value = match (dev.mem_used, dev.mem_total) {
                (Some(u), Some(t)) => format!("{} / {}", compact_gib(u), compact_gib(t)),
                (None, Some(t)) => compact_gib(t),
                _ => format!("{:.0}%", gpu_pct.round()),
            };
            render_section_header_bar(ui, "// GPU", gpu_pct, &gpu_value);

            let primary_state = if discrete.is_some() { "ACTIVE" } else { "ONLINE" };
            let primary_right = format!("{} {:.0}%", primary_state, gpu_pct.round());
            render_compact_status_row(ui, theme::IconType::Gpu, &short_gpu_name(&dev.name), &primary_right, telemetry_severity_color(gpu_pct));

            if let Some(igpu) = integrated {
                if igpu.name != dev.name {
                    let igpu_pct = igpu.gpu_pct.unwrap_or(0.0);
                    let igpu_right = if igpu_pct > 0.0 {
                        format!("READY {:.0}%", igpu_pct.round())
                    } else {
                        "IDLE".to_string()
                    };
                    render_compact_status_row(ui, theme::IconType::Laptop, &format!("iGPU {}", short_gpu_name(&igpu.name)), &igpu_right, Colors::TEXT_DIM);
                }
            }

            let mut gpu_meta = Vec::<String>::new();
            if let Some(mem_t) = &dev.vram_type { gpu_meta.push(mem_t.clone()); }
            if dev.is_rtx { gpu_meta.push("RTX".to_string()); }
            if dev.ai_capable { gpu_meta.push("AI".to_string()); }
            if let Some(bus) = &dev.bus_type { gpu_meta.push(bus.clone()); }
            if !gpu_meta.is_empty() {
                ui.label(RichText::new(gpu_meta.join("  â€¢  ")).color(Colors::TEXT_DIM).size(TelemetryUiRules::META_TEXT_SIZE));
            }
        } else {
            ui.label(RichText::new("// GPU").color(Colors::GREEN).size(TelemetryUiRules::HEADER_TITLE_SIZE).strong());
            ui.label(RichText::new("NO GPU DETECTED").color(Colors::TEXT_DIM).size(8.0));
        }
    });
}

fn render_disks_section(ui: &mut Ui, telem: &NodeTelemetry, _scroll_id: &str) {
    render_telemetry_card(ui, |ui| {
        let drives = &telem.capabilities.drives;
        let (used_total, disk_total) = if drives.is_empty() {
            (telem.disk_used, telem.disk_total)
        } else {
            (
                drives.iter().map(|d| d.used).sum::<u64>(),
                drives.iter().map(|d| d.total).sum::<u64>(),
            )
        };
        let free_total = disk_total.saturating_sub(used_total);
        let disk_pct = if disk_total == 0 {
            0.0
        } else {
            used_total as f32 / disk_total as f32 * 100.0
        };

        let disk_value = format!(
            "USED {}  FREE {}  TOT {}",
            crate::telemetry::fmt_bytes(used_total),
            crate::telemetry::fmt_bytes(free_total),
            crate::telemetry::fmt_bytes(disk_total)
        );
        render_section_header_bar(ui, "// DISKS", disk_pct, &disk_value);

        if drives.is_empty() {
            ui.label(RichText::new("NO DRIVE DETAILS").color(Colors::TEXT_DIM).size(TelemetryUiRules::META_TEXT_SIZE));
            return;
        }

        // â”€â”€ Uniform fixed-height rows: one bar per drive, fill = used%
        let area_w = ui.available_width().max(40.0);
        let row_h = TelemetryUiRules::DISK_ROW_H;
        let gap = TelemetryUiRules::ROW_GAP;

        for (i, drive) in drives.iter().enumerate() {
            let pct = if drive.total > 0 { drive.used as f32 / drive.total as f32 } else { 0.0 };
            let color = telemetry_severity_color(pct * 100.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(area_w, row_h), egui::Sense::hover());

            // Track + fill
            ui.painter().rect_filled(rect, egui::Rounding::ZERO, Colors::BORDER);
            if pct > 0.0 {
                let fill_w = (rect.width() * pct).max(2.0);
                let fill = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
                ui.painter().rect_filled(fill, egui::Rounding::ZERO, color);
            }

            // Drive name + used/free/total painted over bar for quick auditing.
            let drive_name = drive.name.split_whitespace().next().unwrap_or(&drive.name);
            let drive_free = drive.total.saturating_sub(drive.used);
            let label = format!(
                "{} U:{} F:{} T:{}",
                drive_name,
                compact_gib(drive.used),
                compact_gib(drive_free),
                compact_gib(drive.total)
            );
            ui.painter().text(
                egui::pos2(rect.min.x + 4.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                &label,
                egui::FontId::monospace(TelemetryUiRules::META_TEXT_SIZE),
                Color32::WHITE,
            );

            if i + 1 < drives.len() { ui.add_space(gap); }
        }
    });
}

fn render_tasks_sw_section(ui: &mut Ui, telem: &NodeTelemetry) {
    render_telemetry_card(ui, |ui| {
        let proc_count = telem.running_processes.unwrap_or(0);
        let proc_pct = (proc_count as f32 / 300.0 * 100.0).clamp(0.0, 100.0);
        render_section_header_bar(ui, "// TASKS", proc_pct, &format!("{} RUN", proc_count));
        for name in telem.top_processes.iter().take(5) {
            ui.label(RichText::new(elide(name, 16)).color(Colors::TEXT_DIM).size(TelemetryUiRules::META_TEXT_SIZE));
        }
    });
}

/// Format bytes/sec as a compact human-readable string: "1.2 MB/s", "340 KB/s"
fn fmt_bps(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{:.1} MB/s", bps as f64 / 1_000_000.0)
    } else if bps >= 1_000 {
        format!("{:.0} KB/s", bps as f64 / 1_000.0)
    } else {
        format!("{} B/s", bps)
    }
}

/// Logarithmic scale 0..1 over [1 B/s .. 100 MB/s] so low-traffic bars stay visible.
fn net_log_pct(bps: u64) -> f32 {
    if bps == 0 { return 0.0; }
    let v = (bps as f64).log10().clamp(0.0, 8.0); // 10^0=1B/s .. 10^8=100MB/s
    (v / 8.0) as f32
}

fn render_net_section(ui: &mut Ui, telem: &NodeTelemetry) {
    render_telemetry_card(ui, |ui| {
        let rx = telem.net_rx_bps.unwrap_or(0);
        let tx = telem.net_tx_bps.unwrap_or(0);
        let net_pct = ((rx.max(tx)) as f32 / 100_000_000_f32 * 100.0).clamp(0.0, 100.0);
        let header_val = format!("{} / {}", fmt_bps(rx), fmt_bps(tx));
        render_section_header_bar(ui, "// NET", net_pct, &header_val);

        let area_w = ui.available_width().max(40.0);
        let bar_h  = TelemetryUiRules::NET_BAR_H;
        let gap    = TelemetryUiRules::SECTION_GAP;
        let rx_color = telemetry_severity_color_alpha(28.0, 220);
        let tx_color = telemetry_severity_color_alpha(58.0, 210);

        // â”€â”€ DL bar (log scale so even idle traffic shows a bar)
        let (rx_rect, _) = ui.allocate_exact_size(egui::vec2(area_w, bar_h), egui::Sense::hover());
        ui.painter().rect_filled(rx_rect, egui::Rounding::ZERO, Colors::BORDER);
        let rx_fill = net_log_pct(rx);
        if rx_fill > 0.0 {
            let fill = egui::Rect::from_min_size(rx_rect.min, egui::vec2(rx_rect.width() * rx_fill, bar_h));
            ui.painter().rect_filled(fill, egui::Rounding::ZERO, rx_color);
        }
        ui.painter().text(
            egui::pos2(rx_rect.min.x + 4.0, rx_rect.center().y),
            egui::Align2::LEFT_CENTER,
            "DL",
            egui::FontId::monospace(TelemetryUiRules::META_TEXT_SIZE),
            Color32::WHITE,
        );
        ui.painter().text(
            egui::pos2(rx_rect.max.x - 4.0, rx_rect.center().y),
            egui::Align2::RIGHT_CENTER,
            &fmt_bps(rx),
            egui::FontId::monospace(TelemetryUiRules::META_TEXT_SIZE),
            Color32::WHITE,
        );

        ui.add_space(gap);

        // â”€â”€ UL bar
        let (tx_rect, _) = ui.allocate_exact_size(egui::vec2(area_w, bar_h), egui::Sense::hover());
        ui.painter().rect_filled(tx_rect, egui::Rounding::ZERO, Colors::BORDER);
        let tx_fill = net_log_pct(tx);
        if tx_fill > 0.0 {
            let fill = egui::Rect::from_min_size(tx_rect.min, egui::vec2(tx_rect.width() * tx_fill, bar_h));
            ui.painter().rect_filled(fill, egui::Rounding::ZERO, tx_color);
        }
        ui.painter().text(
            egui::pos2(tx_rect.min.x + 4.0, tx_rect.center().y),
            egui::Align2::LEFT_CENTER,
            "UL",
            egui::FontId::monospace(TelemetryUiRules::META_TEXT_SIZE),
            Color32::WHITE,
        );
        ui.painter().text(
            egui::pos2(tx_rect.max.x - 4.0, tx_rect.center().y),
            egui::Align2::RIGHT_CENTER,
            &fmt_bps(tx),
            egui::FontId::monospace(TelemetryUiRules::META_TEXT_SIZE),
            Color32::WHITE,
        );
    });
}

fn agent_load_pct(telem: &NodeTelemetry) -> f32 {
    telem.ai_tokens_per_sec
        .map(|t| (t * 8.0).clamp(0.0, 100.0))
        .unwrap_or_else(|| {
            let status = telem.ai_status.as_deref().unwrap_or("").to_ascii_lowercase();
            if status.contains("process") || status.contains("generat") {
                (telem.cpu_pct * 0.7).clamp(0.0, 100.0)
            } else {
                0.0
            }
        })
}

// Compact AI+AGENT chips used in both narrow mode and as filling in wide columns.
fn render_ai_section(ui: &mut Ui, telem: &NodeTelemetry) {
    let (ai_pct, ai_text) = ai_meter_state(telem.ai_status.as_deref());
    let agent_load = agent_load_pct(telem);

    ui.label(RichText::new("// AI").color(Colors::GREEN).size(9.0).strong());
    render_telemetry_chip(ui, theme::IconType::Ai, "AI", ai_pct, &ai_text);

    ui.label(RichText::new("// AGENT").color(Colors::GREEN).size(9.0).strong());
    render_telemetry_chip(
        ui,
        theme::IconType::Pulse,
        "AGENT",
        agent_load,
        &telem.ai_status.clone().unwrap_or_else(|| "IDLE".to_string()).to_uppercase(),
    );
    if !telem.capabilities.ai_models.is_empty() {
        ui.label(RichText::new(telem.capabilities.ai_models.iter().take(2).cloned().collect::<Vec<_>>().join(" | ")).color(Colors::TEXT_DIM).size(8.0));
    }
    let mut io_caps = Vec::new();
    if telem.capabilities.has_camera    { io_caps.push("CAM");   }
    if telem.capabilities.has_microphone { io_caps.push("MIC");   }
    if telem.capabilities.has_speakers  { io_caps.push("SPK");   }
    if telem.capabilities.has_rdp       { io_caps.push("RDP");   }
    if telem.capabilities.has_file_access { io_caps.push("FILES"); }
    if !io_caps.is_empty() {
        ui.label(RichText::new(io_caps.join(" â€¢ ")).color(Colors::TEXT_MUTED).size(8.0));
    }
}

// Dedicated right-zone panel: overall perf chart + AI + AGENT (wide mode only).
fn render_stat_right_panel(ui: &mut Ui, telem: &NodeTelemetry) {
    let (ai_pct, ai_text) = ai_meter_state(telem.ai_status.as_deref());
    let agent_load = agent_load_pct(telem);
    let (img, txt, code) = perf_scores(telem);
    let perf_pct = ((img + txt + code) / 3.0).clamp(0.0, 100.0);

    ui.label(RichText::new("// PERF AI AGENT").color(Colors::GREEN).size(9.0).strong());
    render_telemetry_chip(ui, theme::IconType::Ai, "PERF", perf_pct, &format!("P {:>3.0}", perf_pct.round()));
    render_telemetry_chip(ui, theme::IconType::Ai, "AI", ai_pct, &ai_text);
    render_telemetry_chip(
        ui,
        theme::IconType::Pulse,
        "AGENT",
        agent_load,
        &telem.ai_status.clone().unwrap_or_else(|| "IDLE".to_string()).to_uppercase(),
    );

    // Perf hex chart fills vertical space below chips
    ui.add_space(3.0);
    render_perf_hex(ui, telem);

    // AI models (up to 2)
    if !telem.capabilities.ai_models.is_empty() {
        ui.add_space(2.0);
        ui.label(
            RichText::new(
                telem.capabilities.ai_models.iter().take(2).cloned().collect::<Vec<_>>().join(" | "),
            )
            .color(Colors::TEXT_DIM)
            .size(7.5),
        );
    }
}

fn render_perf_only_panel(ui: &mut Ui, telem: &NodeTelemetry) {
    let (img, txt, code) = perf_scores(telem);
    let perf_pct = ((img + txt + code) / 3.0).clamp(0.0, 100.0);
    ui.label(RichText::new("// PERF").color(Colors::GREEN).size(9.0).strong());
    render_telemetry_chip(ui, theme::IconType::Ai, "PERF", perf_pct, &format!("P {:>3.0}", perf_pct.round()));
    ui.add_space(3.0);
    render_perf_hex(ui, telem);
}

fn render_identity_section(
    ui: &mut Ui,
    s: &DetailState,
    icon_type: theme::IconType,
    is_local_node: bool,
    is_online: bool,
) {
    ui.horizontal(|ui| {
        crate::theme::render_crt_icon(ui, icon_type, 22.0, Colors::GREEN);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                let short_name = s.device.display_name()
                    .split('.')
                    .next()
                    .unwrap_or(s.device.display_name());
                ui.label(
                    RichText::new(short_name.to_uppercase())
                        .color(Colors::TEXT).size(14.0).strong()
                );
                if is_local_node {
                    ui.label(
                        RichText::new("(LOCAL)")
                            .color(Colors::GREEN).size(9.5).strong()
                    );
                }
            });
            ui.label(
                RichText::new(s.device.primary_ip().unwrap_or("No Tailscale IP"))
                    .color(Colors::GREEN).size(10.5)
            );
            ui.label(
                RichText::new(format!("{} Â· {}", s.device.os.to_uppercase(), s.device.client_version))
                    .color(Colors::TEXT_DIM).size(9.0)
            );
        });
    });
    let auth_txt    = if s.device.authorized { "AUTH YES" } else { "AUTH NO" };
    let inbound_txt = if s.device.blocks_incoming { "BLOCKED" } else { "OPEN" };
    let seen_txt    = s.device.last_seen
        .map(|t| t.with_timezone(&chrono::Local).format("%d/%m/%y %H:%M").to_string())
        .unwrap_or_else(|| "N/A".to_string());
    ui.label(RichText::new(format!("{} â€¢ INBOUND {}", auth_txt, inbound_txt)).color(Colors::TEXT_MUTED).size(8.0));
    ui.label(RichText::new(format!("SEEN {}  {}", seen_txt, s.device.user)).color(Colors::TEXT_MUTED).size(8.0));
    ui.horizontal(|ui| {
        let (dot_r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        if is_online {
            ui.painter().circle_filled(dot_r.center(), 4.0, Colors::GREEN);
            ui.painter().circle_stroke(dot_r.center(), 5.5, egui::Stroke::new(1.0, Colors::GREEN_DIM));
        } else {
            ui.painter().circle_stroke(dot_r.center(), 3.5, egui::Stroke::new(1.0, Colors::TEXT_MUTED));
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new(if is_online { "ONLINE" } else { "OFFLINE" })
            .color(if is_online { Colors::GREEN } else { Colors::TEXT_MUTED })
            .size(9.0).strong());
    });
}

#[derive(Clone, Debug, PartialEq)]
pub enum TelemetryPane {
    Identity,
    Cpu,
    RamGpu,
    Disks,
    Net,
    Tasks,
    Perf,
}

pub fn build_default_telemetry_tree() -> Tree<TelemetryPane> {
    let mut tiles = Tiles::default();

    let id_pane = tiles.insert_pane(TelemetryPane::Identity);
    let cpu_pane = tiles.insert_pane(TelemetryPane::Cpu);
    let ram_pane = tiles.insert_pane(TelemetryPane::RamGpu);
    let disk_pane = tiles.insert_pane(TelemetryPane::Disks);
    let net_pane = tiles.insert_pane(TelemetryPane::Net);
    let tasks_pane = tiles.insert_pane(TelemetryPane::Tasks);
    let perf_pane = tiles.insert_pane(TelemetryPane::Perf);

    let center = tiles.insert_horizontal_tile(vec![cpu_pane, ram_pane, disk_pane, net_pane, tasks_pane]);
    if let Some(EguiTile::Container(TileContainer::Linear(linear))) = tiles.get_mut(center) {
        linear.shares.set_share(cpu_pane, TelemetryUiRules::TILE_SHARE_CPU);
        linear.shares.set_share(ram_pane, TelemetryUiRules::TILE_SHARE_RAM_GPU);
        linear.shares.set_share(disk_pane, TelemetryUiRules::TILE_SHARE_DISK);
        linear.shares.set_share(net_pane, TelemetryUiRules::TILE_SHARE_NET);
        linear.shares.set_share(tasks_pane, TelemetryUiRules::TILE_SHARE_TASKS);
    }

    let root = tiles.insert_horizontal_tile(vec![id_pane, center, perf_pane]);
    if let Some(EguiTile::Container(TileContainer::Linear(linear))) = tiles.get_mut(root) {
        linear.shares.set_share(id_pane, TelemetryUiRules::TILE_SHARE_ID);
        linear.shares.set_share(center, TelemetryUiRules::TILE_SHARE_CENTER);
        linear.shares.set_share(perf_pane, TelemetryUiRules::TILE_SHARE_PERF);
    }

    Tree::new("telemetry_tiles", root, tiles)
}

struct TelemetryTileBehavior<'a, 'b> {
    detail: &'a DetailState<'a>,
    telem: Option<&'a NodeTelemetry>,
    actions: &'b mut DetailActions,
    is_local_node: bool,
    is_online: bool,
}

impl<'a, 'b> TileBehavior<TelemetryPane> for TelemetryTileBehavior<'a, 'b> {
    fn tab_title_for_pane(&mut self, pane: &TelemetryPane) -> egui::WidgetText {
        match pane {
            TelemetryPane::Identity => "ID".into(),
            TelemetryPane::Cpu => "CPU".into(),
            TelemetryPane::RamGpu => "RAM+GPU".into(),
            TelemetryPane::Disks => "DISK".into(),
            TelemetryPane::Net => "NET".into(),
            TelemetryPane::Tasks => "TASKS".into(),
            TelemetryPane::Perf => "PERF".into(),
        }
    }

    fn pane_ui(&mut self, ui: &mut Ui, _tile_id: TileId, pane: &mut TelemetryPane) -> UiResponse {
        match pane {
            TelemetryPane::Identity => {
                let h_lower = self.detail.device.hostname.to_lowercase();
                let d_lower = self.detail.device.display_name().to_lowercase();
                let device_type = self.telem.map(|t| t.device_type.as_str()).unwrap_or("Desktop");
                let icon_type = if h_lower.contains("nubia") {
                    theme::IconType::Tablet
                } else if h_lower.contains("nothing") || d_lower.contains("nothing") {
                    theme::IconType::Smartphone
                } else {
                    match device_type {
                        "Laptop" => theme::IconType::Laptop,
                        "Server" => theme::IconType::Server,
                        "Tablet" => theme::IconType::Tablet,
                        "Smartphone" => theme::IconType::Smartphone,
                        "Phone" => theme::IconType::Smartphone,
                        "Chromebook" => theme::IconType::Chromebook,
                        _ => theme::IconType::Desktop,
                    }
                };
                render_identity_section(ui, self.detail, icon_type, self.is_local_node, self.is_online);
            }
            TelemetryPane::Cpu => {
                if let Some(telem) = self.telem { render_cpu_section(ui, telem); }
            }
            TelemetryPane::RamGpu => {
                if let Some(telem) = self.telem { render_ram_gpu_section(ui, telem); }
            }
            TelemetryPane::Disks => {
                if let Some(telem) = self.telem { render_disks_section(ui, telem, "telemetry_disks_tiles"); }
            }
            TelemetryPane::Net => {
                if let Some(telem) = self.telem { render_net_section(ui, telem); }
            }
            TelemetryPane::Tasks => {
                if let Some(telem) = self.telem { render_tasks_sw_section(ui, telem); }
            }
            TelemetryPane::Perf => {
                if let Some(telem) = self.telem {
                    if ENABLE_AI_RIGHT_PANEL {
                        render_stat_right_panel(ui, telem);
                    } else {
                        render_perf_only_panel(ui, telem);
                        ui.label(RichText::new("AI PANEL MUTED (FLAG)").color(Colors::TEXT_MUTED).size(7.5));
                    }
                }
            }
        }

        if self.telem.is_none() && !matches!(pane, TelemetryPane::Identity) {
            ui.label(RichText::new("NO TELEMETRY").color(Colors::TEXT_MUTED).size(8.0));
            if crate::theme::micro_button(ui, "FETCH").clicked() {
                self.actions.fetch_telemetry = true;
            }
        }

        UiResponse::None
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        1.0
    }

    fn min_size(&self) -> f32 {
        TelemetryUiRules::PANE_MIN_SIZE
    }
}

fn render_wide_telemetry_tiles(
    ui: &mut Ui,
    s: &DetailState,
    actions: &mut DetailActions,
    is_local_node: bool,
    is_online: bool,
    tree: &mut Tree<TelemetryPane>,
) {
    let mut behavior = TelemetryTileBehavior {
        detail: s,
        telem: s.telemetry,
        actions,
        is_local_node,
        is_online,
    };
    tree.ui(&mut behavior, ui);
}

// Three-column center zone (wide mode). The AI/AGENT/perf right panel is rendered separately.
// Five-column wide layout: CPU | RAM+GPU | DISKS treemap | NET | TASKS
#[allow(dead_code)]
fn fill_telemetry_columns(cols: &mut [Ui], telem: &NodeTelemetry) {
    if cols.len() < 5 { return; }
    render_cpu_section(&mut cols[0], telem);
    render_ram_gpu_section(&mut cols[1], telem);
    render_disks_section(&mut cols[2], telem, "telemetry_disks_scroll");
    render_net_section(&mut cols[3], telem);
    render_tasks_sw_section(&mut cols[4], telem);
}

// Two-column compact layout (narrow viewports). Perf hex is embedded here since there's no right panel.
fn fill_telemetry_columns_compact(cols: &mut [Ui], telem: &NodeTelemetry) {
    if cols.len() < 2 {
        return;
    }
    render_cpu_section(&mut cols[0], telem);
    cols[0].add_space(6.0);
    render_ram_gpu_section(&mut cols[0], telem);

    render_disks_section(&mut cols[1], telem, "telemetry_disks_scroll_compact");
    cols[1].add_space(6.0);
    render_ai_section(&mut cols[1], telem);
    cols[1].add_space(4.0);
    render_perf_hex(&mut cols[1], telem);
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// render_detail_panel_with_timeline â€” Phase 3 entry point
//
// Extends render_detail_panel with the Timeline tab and telemetry gauges.
// Called from app.rs instead of render_detail_panel when Phase 3 is active.
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

pub fn render_detail_panel_with_timeline(
    ui:            &mut egui::Ui,
    s:             &mut DetailState,
    timeline:      &mut crate::views::timeline::TimelineState,
    _index_stats:   &thegrid_core::models::IndexStats,
    telemetry_tree: &mut Tree<TelemetryPane>,
    telemetry_band_height: &mut f32,
) -> DetailActions {
    let mut actions = DetailActions::default();
    let is_online = s.device.is_likely_online();
    let is_local_node = s.device.hostname.eq_ignore_ascii_case(s.local_device_name)
        || s.device.name.eq_ignore_ascii_case(s.local_device_name)
        || s.device.display_name().eq_ignore_ascii_case(s.local_device_name);

    // â”€â”€ Device header â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(0.0, 0.0))
        .show(ui, |ui| {
            let h_lower = s.device.hostname.to_lowercase();
            let d_lower = s.device.display_name().to_lowercase();
            let device_type = s.telemetry.map(|t| t.device_type.as_str()).unwrap_or("Desktop");

            let icon_type = if h_lower.contains("nubia") {
                theme::IconType::Tablet
            } else if h_lower.contains("nothing") || d_lower.contains("nothing") {
                theme::IconType::Smartphone
            } else {
                match device_type {
                    "Laptop" => theme::IconType::Laptop,
                    "Server" => theme::IconType::Server,
                    "Tablet" => theme::IconType::Tablet,
                    "Smartphone" => theme::IconType::Smartphone,
                    "Phone" => theme::IconType::Smartphone,
                    "Chromebook" => theme::IconType::Chromebook,
                    _ => theme::IconType::Desktop,
                }
            };

            egui::Frame::none()
                .fill(Colors::BG_WIDGET)
                .stroke(egui::Stroke::new(1.0, Colors::BORDER2))
                .inner_margin(egui::Margin::symmetric(0.0, 3.0))
                .show(ui, |ui| {
                    let avail_w = ui.available_width();
                    let wide_layout = avail_w >= TelemetryUiRules::WIDE_BREAKPOINT;
                    let band_h = telemetry_band_height
                        .clamp(TelemetryUiRules::BAND_MIN_HEIGHT, TelemetryUiRules::BAND_MAX_HEIGHT);

                    if wide_layout {
                        ui.allocate_ui_with_layout(
                            egui::vec2(avail_w, band_h),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_height(band_h);
                                render_wide_telemetry_tiles(ui, s, &mut actions, is_local_node, is_online, telemetry_tree);
                            },
                        );
                    } else {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                crate::theme::render_crt_icon(ui, icon_type, 24.0, Colors::GREEN);
                                ui.add_space(10.0);
                                ui.vertical(|ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(
                                            RichText::new(s.device.display_name().to_uppercase())
                                                .color(Colors::TEXT).size(15.0).strong()
                                        );
                                        if is_local_node {
                                            ui.add_space(6.0);
                                            ui.label(
                                                RichText::new("(LOCAL)")
                                                    .color(Colors::GREEN).size(9.0).strong()
                                            );
                                        }
                                    });
                                    ui.label(
                                        RichText::new(s.device.primary_ip().unwrap_or("No Tailscale IP"))
                                            .color(Colors::GREEN).size(10.0)
                                    );
                                });
                            });
                            ui.add_space(6.0);
                            if let Some(telem) = s.telemetry {
                                ui.columns(2, |cols| {
                                    fill_telemetry_columns_compact(cols, telem);
                                });
                            } else {
                                ui.label(RichText::new("NO TELEMETRY").color(Colors::TEXT_MUTED).size(8.0));
                                if crate::theme::micro_button(ui, "FETCH").clicked() {
                                    actions.fetch_telemetry = true;
                                }
                            }
                        });
                    }
                });

            // â”€â”€ Device Classification (Android) â”€â”€
            if s.device.os.to_lowercase().contains("android") {
                ui.add_space(8.0);
                egui::Frame::none()
                    .fill(Colors::BG_WIDGET)
                    .inner_margin(egui::Margin::symmetric(24.0, 10.0))
                    .stroke(egui::Stroke::new(1.0, Colors::BORDER))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("CLASSIFY DEVICE:").color(Colors::TEXT_DIM).size(8.0).strong());
                            ui.add_space(12.0);
                            if theme::micro_button(ui, "PHONE").clicked() {
                                actions.update_remote_config = Some((Some("Phone".into()), None, None));
                            }
                            ui.add_space(8.0);
                            if theme::micro_button(ui, "TABLET").clicked() {
                                actions.update_remote_config = Some((Some("Tablet".into()), None, None));
                            }
                            ui.add_space(8.0);
                            if theme::micro_button(ui, "CHROMEBOOK").clicked() {
                                actions.update_remote_config = Some((Some("Chromebook".into()), None, None));
                            }
                            
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new("// SETS ICON REMOTELY").color(Colors::TEXT_DIM).size(8.0).italics());
                            });
                        });
                    });
            }

        });

    // Draggable handle between telemetry band and main body (wide layout only).
    if ui.available_width() >= TelemetryUiRules::WIDE_BREAKPOINT {
        let (handle_rect, handle_resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), TelemetryUiRules::RESIZE_HANDLE_H),
            egui::Sense::click_and_drag(),
        );
        let stroke_color = if handle_resp.hovered() || handle_resp.dragged() {
            Colors::GREEN
        } else {
            Colors::BORDER2
        };
        ui.painter().line_segment(
            [
                egui::pos2(handle_rect.min.x + 10.0, handle_rect.center().y),
                egui::pos2(handle_rect.max.x - 10.0, handle_rect.center().y),
            ],
            egui::Stroke::new(1.0, stroke_color),
        );

        if handle_resp.dragged() {
            let dy = ui.ctx().input(|i| i.pointer.delta().y);
            *telemetry_band_height = (*telemetry_band_height + dy)
                .clamp(TelemetryUiRules::BAND_MIN_HEIGHT, TelemetryUiRules::BAND_MAX_HEIGHT);
        }
    }

    ui.add_space(TelemetryUiRules::BAND_BOTTOM_GAP);
    ui.add(egui::Separator::default().spacing(0.0));

    // â”€â”€ Tab bar (4 tabs) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        for (label, tab_variant) in [
            ("ACTIONS",   DashTab::Actions),
            ("FILES",     DashTab::Files),
            ("CLIPBOARD", DashTab::Clipboard),
            ("TIMELINE",  DashTab::Timeline),
            ("TERMINAL",  DashTab::Terminal),
            ("STORAGE",   DashTab::Storage),
            ("DEDUP",     DashTab::DedupReview),
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

    // â”€â”€ Tab content â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Capture correct content width before the scroll area is entered.
    // Inside ScrollArea the inner Ui can diverge from the panel width.
    let content_w = ui.available_width();
    egui::ScrollArea::vertical()
        .id_source("detail_v3_scroll")
        .show(ui, |ui| {
            ui.set_max_width(content_w);
            ui.add_space(16.0);
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(24.0, 0.0))
                .show(ui, |ui| {
                    let tab = s.active_tab.clone();
                    match tab {
                        DashTab::Actions   => render_actions_tab(ui, s, &mut actions),
                        DashTab::Files     => render_files_tab(ui, s, &mut actions),
                        DashTab::Clipboard => render_clipboard_tab(ui, s, &mut actions),
                        DashTab::Storage   => render_storage_tab(ui, s, &mut actions),
                        DashTab::Timeline  => {
                            // Trigger data load if needed
                            if timeline.needs_refresh() {
                                actions.load_timeline = true;
                            }
                            let tl_action = crate::views::timeline::render(ui, timeline);
                            if tl_action.refresh { actions.load_timeline = true; }
                            if tl_action.open_entry.is_some() {
                                // Navigation handled in app.rs via actions
                                // For now just show a toast â€” Phase 4 can deep-link to the file
                            }
                        }
                        DashTab::Terminal  => render_terminal_tab(ui, s, &mut actions),
                        DashTab::DedupReview => {
                            let mut scan_req = false;
                            if let Some(to_delete) = crate::views::dedup_review::render_dedup_review(
                                ui,
                                s.rich_duplicate_groups,
                                s.dedup_review_state,
                                s.local_device_id,
                                &mut scan_req,
                            ) {
                                actions.dedup_delete_files = Some(to_delete);
                            }
                            if scan_req {
                                actions.run_cross_source_scan = true;
                            }
                        }
                    }
                });
            ui.add_space(16.0);
        });

    actions
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Cluster View (Phase 3)
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Cluster View (Phase 3)
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Default)]
pub struct ClusterActions {
    pub load_node_path: Option<(String, PathBuf)>,
}

pub fn render_cluster_view(
    ui: &mut Ui,
    devices: &[TailscaleDevice],
    telemetries: &HashMap<String, NodeTelemetry>,
    cluster_paths: &mut HashMap<String, PathBuf>,
    cluster_files: &HashMap<String, Vec<RemoteFile>>,
    local_device_name: &str,
    active_rule: Option<&thegrid_core::models::SmartRule>,
) -> ClusterActions {
    let mut actions = ClusterActions::default();
    
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("// CLUSTER VIEW").color(Colors::GREEN).size(10.0).strong());
        ui.add_space(8.0);
        ui.label(RichText::new(format!("{} NODES SYNCED", devices.len())).color(Colors::TEXT_DIM).size(9.0));
    });
    ui.add_space(16.0);

    let num_nodes = devices.len();
    let num_cols = if num_nodes > 2 { 2 } else { 1 };
    
    egui::Grid::new("cluster_grid")
        .num_columns(num_cols)
        .spacing([12.0, 12.0])
        .min_col_width(ui.available_width() / num_cols as f32 - 12.0)
        .show(ui, |ui| {
            for (i, dev) in devices.iter().enumerate() {
                ui.vertical(|ui| {
                    if let Some(act) = render_cluster_node_explorer(
                        ui, 
                        dev, 
                        telemetries.get(&dev.id), 
                        cluster_paths.get(&dev.id),
                        cluster_files.get(&dev.id).unwrap_or(&vec![]),
                        local_device_name,
                        active_rule
                    ) {
                        actions.load_node_path = Some((dev.id.clone(), act));
                    }
                });
                if (i + 1) % num_cols == 0 { ui.end_row(); }
            }
        });

    actions
}

fn render_cluster_node_explorer(
    ui: &mut Ui,
    dev: &TailscaleDevice,
    telem: Option<&NodeTelemetry>,
    current_path: Option<&PathBuf>,
    files: &[RemoteFile],
    local_device_name: &str,
    active_rule: Option<&thegrid_core::models::SmartRule>,
) -> Option<PathBuf> {
    let mut next_path = None;
    let is_local = dev.hostname.eq_ignore_ascii_case(local_device_name)
        || dev.name.eq_ignore_ascii_case(local_device_name)
        || dev.display_name().eq_ignore_ascii_case(local_device_name);
    let bg = if is_local { Color32::from_rgba_premultiplied(0, 100, 0, 20) } else { Colors::BG_WIDGET };

    egui::Frame::none()
        .fill(bg)
        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_min_height(300.0);
            
            // Node Header
            ui.horizontal(|ui| {
                let status_color = if dev.is_likely_online() { Colors::GREEN } else { Colors::TEXT_DIM };
                theme::status_dot(ui, status_color);
                ui.label(RichText::new(dev.display_name().to_uppercase()).color(Colors::TEXT).size(10.0).strong());
                if is_local {
                    ui.label(RichText::new("[MASTER]").color(Colors::GREEN).size(8.0).strong());
                }
            });
            ui.add_space(8.0);

            // Drive Navigation
            if let Some(t) = telem {
                if !t.capabilities.drives.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("DRIVE:").color(Colors::TEXT_DIM).size(8.0));
                        for drive in &t.capabilities.drives {
                            if ui.selectable_label(false, &drive.name).clicked() {
                                next_path = Some(PathBuf::from(&drive.name));
                            }
                        }
                    });
                    ui.add_space(8.0);
                }
            }

            // Path Bar
            ui.horizontal(|ui| {
                ui.label(RichText::new("PATH:").color(Colors::TEXT_DIM).size(8.0));
                let path_str = current_path.map(|p: &PathBuf| p.to_string_lossy().to_string()).unwrap_or_else(|| "/".to_string());
                ui.label(RichText::new(path_str).color(Colors::TEXT).size(8.0));
            });
            ui.add_space(8.0);

            // File List
            ScrollArea::vertical()
                .id_source(format!("scroll_{}", dev.id))
                .max_height(200.0)
                .show(ui, |ui| {
                    if files.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);
                            ui.label(RichText::new("// EMPTY OR LOADING").color(Colors::TEXT_DIM).size(7.0).italics());
                        });
                        let mut filtered_files: Vec<_> = files.iter().filter(|rf| {
                            if let Some(rule) = active_rule {
                                let mut matches = true;
                                for f in &rule.filters {
                                    match f {
                                        thegrid_core::models::SmartFilterType::Extension(ext) => {
                                            if rf.is_dir { matches = false; break; }
                                            let file_ext = std::path::Path::new(&rf.name)
                                                .extension()
                                                .map(|e| e.to_string_lossy().to_lowercase())
                                                .unwrap_or_default();
                                            if file_ext != ext.to_lowercase() { matches = false; break; }
                                        }
                                        thegrid_core::models::SmartFilterType::MinSize(ms) => {
                                            if rf.is_dir || rf.size < *ms { matches = false; break; }
                                        }
                                        thegrid_core::models::SmartFilterType::MaxSize(ms) => {
                                            if rf.is_dir || rf.size > *ms { matches = false; break; }
                                        }
                                        thegrid_core::models::SmartFilterType::ModifiedAfter(dt) => {
                                            if let Some(m) = rf.modified { if m < *dt { matches = false; break; } } else { matches = false; break; }
                                        }
                                        thegrid_core::models::SmartFilterType::ModifiedBefore(dt) => {
                                            if let Some(m) = rf.modified { if m > *dt { matches = false; break; } } else { matches = false; break; }
                                        }
                                        _ => {} 
                                    }
                                }
                                matches
                            } else {
                                true
                            }
                        }).collect();
                        
                        // Sort: dirs first
                        filtered_files.sort_by(|a, b| {
                            if a.is_dir != b.is_dir {
                                b.is_dir.cmp(&a.is_dir)
                            } else {
                                a.name.to_lowercase().cmp(&b.name.to_lowercase())
                            }
                        });

                        for file in filtered_files {
                            ui.horizontal(|ui| {
                                let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                let (icon, color) = if file.is_dir { 
                                    (theme::IconType::Folder, Colors::GREEN)
                                } else { 
                                    let ext = std::path::Path::new(&file.name).extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
                                    theme::get_file_icon(&ext)
                                };
                                theme::draw_vector_icon(ui, icon_rect, icon, color);
                                ui.add_space(4.0);
                                if ui.selectable_label(false, RichText::new(&file.name).size(9.0)).clicked() {
                                    if file.is_dir {
                                        let mut p = current_path.cloned().unwrap_or_else(|| PathBuf::from("/"));
                                        p.push(&file.name);
                                        next_path = Some(p);
                                    }
                                }
                            });
                        }
                    }
                });
        });

    next_path
}

fn render_storage_tab(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    ui.label(RichText::new("DATA PIPELINE").color(Colors::GREEN).size(10.0).strong());
    ui.add_space(10.0);

    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
        .show(ui, |ui| {
            ui.label(RichText::new("GRID SCAN PROGRESS").color(Colors::TEXT_DIM).size(8.0).strong());
            ui.add_space(6.0);

            if s.grid_scan_progress.is_empty() {
                ui.label(RichText::new("No active machine scan status yet.").color(Colors::TEXT_MUTED).size(8.0));
            } else {
                let mut rows: Vec<_> = s.grid_scan_progress.values().collect();
                rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

                ScrollArea::vertical().id_source("storage_grid_scan_progress").max_height(140.0).show(ui, |ui| {
                    for p in rows {
                        let pct = if p.total > 0 {
                            (p.scanned as f32 / p.total as f32).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let label = if p.machine_id.is_empty() { "unknown" } else { &p.machine_id };
                        ui.label(
                            RichText::new(format!(
                                "{} | {} | drive={} | sector={} | {}/{} | pending={}",
                                label,
                                p.step,
                                p.current_drive,
                                p.current_sector,
                                p.scanned,
                                p.total,
                                p.pending_sectors
                            ))
                            .color(Colors::TEXT)
                            .size(8.0)
                        );
                        ui.add(egui::ProgressBar::new(pct).desired_width(ui.available_width() - 12.0));
                        if let Some(err) = &p.last_error {
                            ui.label(RichText::new(format!("warn: {}", err)).color(Colors::AMBER).size(8.0));
                        }
                        ui.add_space(4.0);
                    }
                });
            }
        });

    ui.add_space(8.0);
    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
        .show(ui, |ui| {
            ui.label(RichText::new("MESH CROSSCHECK (OFFLINE-CAPABLE)").color(Colors::TEXT_DIM).size(8.0).strong());
            ui.add_space(6.0);
            if s.node_crosscheck.is_empty() {
                ui.label(RichText::new("No node vector crosscheck results yet. Results appear after sync. ").color(Colors::TEXT_MUTED).size(8.0));
            } else {
                let mut items: Vec<_> = s.node_crosscheck.values().collect();
                items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                ScrollArea::vertical().id_source("storage_crosscheck_summary").max_height(120.0).show(ui, |ui| {
                    for c in items {
                        ui.label(
                            RichText::new(format!(
                                "{} | groups={} | files={} | bytes={} | vectors={}",
                                c.node_id,
                                c.groups,
                                c.files,
                                crate::telemetry::fmt_bytes(c.bytes),
                                c.known_devices
                            ))
                            .color(Colors::TEXT)
                            .size(8.0)
                        );
                    }
                });
            }
        });

    ui.add_space(8.0);
    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
        .show(ui, |ui| {
            ui.label(RichText::new("CLOUD PIPELINE PROGRESS").color(Colors::TEXT_DIM).size(8.0).strong());
            ui.add_space(6.0);
            let cp = s.cloud_pipeline_progress;
            ui.label(
                RichText::new(format!(
                    "stage={} | step={} | target={}",
                    cp.stage,
                    cp.step,
                    cp.target
                ))
                .color(Colors::TEXT)
                .size(8.0)
            );
            if cp.total > 0 {
                let pct = (cp.done as f32 / cp.total as f32).clamp(0.0, 1.0);
                ui.label(RichText::new(format!("items: {}/{}", cp.done, cp.total)).color(Colors::TEXT_DIM).size(8.0));
                ui.add(egui::ProgressBar::new(pct).desired_width(ui.available_width() - 12.0));
            } else {
                ui.label(RichText::new("items: tracking step state").color(Colors::TEXT_DIM).size(8.0));
            }
            if cp.bytes_total > 0 {
                ui.label(RichText::new(format!(
                    "bytes: {} / {}",
                    crate::telemetry::fmt_bytes(cp.bytes_done),
                    crate::telemetry::fmt_bytes(cp.bytes_total)
                )).color(Colors::TEXT_DIM).size(8.0));
            } else if cp.bytes_done > 0 {
                ui.label(RichText::new(format!(
                    "bytes processed: {}",
                    crate::telemetry::fmt_bytes(cp.bytes_done)
                )).color(Colors::TEXT_DIM).size(8.0));
            }
            if let Some(err) = &cp.last_error {
                ui.label(RichText::new(format!("error: {}", err)).color(Colors::RED).size(8.0));
            }
        });

    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
        .show(ui, |ui| {
            ui.label(RichText::new("DUPLICATE FINDER").color(Colors::GREEN).size(10.0).strong());
            ui.add_space(4.0);

            // Hashing status warning
            let (hashed, hash_total) = s.hashing_progress;
            if hash_total > 0 && hashed < hash_total {
                let remaining = hash_total - hashed;
                let pct = (hashed as f32 / hash_total as f32 * 100.0).min(100.0);
                ui.label(
                    RichText::new(format!(
                        "âš   Hashing in progress: {}/{} files ({:.1}%) â€” {} still pending. Scan now for partial results or wait for completion.",
                        hashed, hash_total, pct, remaining
                    ))
                    .color(Colors::AMBER).size(8.0)
                );
                ui.add_space(4.0);
            } else if hash_total == 0 {
                ui.label(
                    RichText::new("// No files indexed yet. Wait for initial scan to complete before running duplicate analysis.")
                        .color(Colors::TEXT_MUTED).size(8.0).italics()
                );
                ui.add_space(4.0);
            }

            ui.label(RichText::new("Grid-wide scan: all indexed drives and synced nodes. OS/software/system paths are excluded automatically.").color(Colors::TEXT_MUTED).size(8.0));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("MIN MB").color(Colors::TEXT_DIM).size(8.0));
                ui.add(egui::DragValue::new(&mut s.file_manager.duplicate_min_size_mb).clamp_range(0..=1024 * 16));
                ui.add_space(10.0);
                ui.label(RichText::new("MAX GROUPS").color(Colors::TEXT_DIM).size(8.0));
                ui.add(egui::DragValue::new(&mut s.file_manager.duplicate_max_groups).clamp_range(10..=5000));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("EXT").color(Colors::TEXT_DIM).size(8.0));
                ui.add(
                    egui::TextEdit::singleline(&mut s.file_manager.duplicate_ext_filter)
                        .desired_width(180.0)
                        .hint_text("optional: jpg,png,pdf,mov")
                );
                ui.add_space(8.0);
                ui.label(RichText::new("PATH PREFIX").color(Colors::TEXT_DIM).size(8.0));
                ui.add(
                    egui::TextEdit::singleline(&mut s.file_manager.duplicate_path_filter)
                        .desired_width(260.0)
                        .hint_text("optional: C:/Users/me")
                );
            });

            ui.add_space(8.0);
            if theme::primary_button(ui, "RUN DUPLICATE ANALYSIS").clicked() {
                let include_extensions = s.file_manager
                    .duplicate_ext_filter
                    .split(',')
                    .map(|e| e.trim().trim_start_matches('.').to_lowercase())
                    .filter(|e| !e.is_empty())
                    .collect::<Vec<_>>();
                actions.run_duplicate_scan = Some(DuplicateScanFilter {
                    min_size_bytes: s.file_manager.duplicate_min_size_mb.saturating_mul(1_048_576),
                    include_extensions,
                    path_prefix: if s.file_manager.duplicate_path_filter.trim().is_empty() {
                        None
                    } else {
                        Some(s.file_manager.duplicate_path_filter.trim().to_string())
                    },
                    device_id: None,
                    exclude_system_paths: true,
                    max_groups: s.file_manager.duplicate_max_groups,
                });
            }
        });

    ui.add_space(10.0);
    let total_wasted: u64 = s.duplicate_groups
        .iter()
        .map(|(_, size, files)| size.saturating_mul(files.len().saturating_sub(1) as u64))
        .sum();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!(
            "GROUPS: {}  |  RECOVERABLE: {:.2} GB",
            s.duplicate_groups.len(),
            total_wasted as f64 / 1_073_741_824.0
        )).color(Colors::TEXT).size(9.0));
        if let Some(ts) = s.duplicate_last_scan {
            ui.add_space(8.0);
            let formatted = chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt: chrono::DateTime<chrono::Utc>| dt.with_timezone(&chrono::Local).format("%d/%m/%y %H:%M").to_string())
                .unwrap_or_else(|| ts.to_string());
            ui.label(RichText::new(format!("LAST SCAN: {}", formatted)).color(Colors::TEXT_DIM).size(8.0));
        }
    });

    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
        .show(ui, |ui| {
        if s.duplicate_groups.is_empty() {
            let msg = if s.duplicate_last_scan.is_none() {
                "No scan run yet. Set filters above and click RUN DUPLICATE ANALYSIS.".to_string()
            } else {
                let (hashed, total) = s.hashing_progress;
                if total > 0 && hashed < total {
                    format!(
                        "No duplicates found in last scan. Note: {}/{} files still unhashed â€” more duplicates may appear once hashing is complete.",
                        total - hashed, total
                    )
                } else {
                    "No duplicates found in last scan. All hashed files are unique.".to_string()
                }
            };
            ui.label(RichText::new("DUPLICATE GROUPS").color(Colors::TEXT_DIM).size(8.0).strong());
            ui.add_space(6.0);
            ui.label(RichText::new(msg).color(Colors::TEXT_MUTED).size(8.0).italics());
        } else {
            ui.label(RichText::new("DUPLICATE GROUPS â€” SELECT FILES TO DELETE").color(Colors::TEXT_DIM).size(8.0).strong());
                ui.add_space(6.0);

                // Snapshot display data so closures below don't borrow s
                let groups: &[(String, u64, Vec<FileSearchResult>)] = s.duplicate_groups;
                let max_show = s.file_manager.duplicate_max_groups.min(groups.len());

                struct DupFileRow {
                    id: i64,
                    path: std::path::PathBuf,
                    device_id: String,
                    label: String,
                    selected: bool,
                }
                struct DupGroupRow {
                    hash: String,
                    hash_short: String,
                    size: u64,
                    wasted: u64,
                    file_count: usize,
                    expanded: bool,
                    files: Vec<DupFileRow>,
                    selected_count: usize,
                }

                let display: Vec<DupGroupRow> = groups.iter().take(max_show).map(|(hash, size, files)| {
                    let expanded = s.file_manager.duplicate_expanded_groups.contains(hash.as_str());
                    let file_rows: Vec<DupFileRow> = files.iter().map(|f| {
                        let selected = s.file_manager.duplicate_selected_files.contains(&f.id);
                        DupFileRow {
                            id: f.id,
                            path: f.path.clone(),
                            device_id: f.device_id.clone(),
                            label: format!("[{}]  {}", f.device_name, f.path.display()),
                            selected,
                        }
                    }).collect();
                    let selected_count = file_rows.iter().filter(|f| f.selected).count();
                    DupGroupRow {
                        hash: hash.clone(),
                        hash_short: hash[..std::cmp::min(8, hash.len())].to_string(),
                        size: *size,
                        wasted: size.saturating_mul(files.len().saturating_sub(1) as u64),
                        file_count: files.len(),
                        expanded,
                        files: file_rows,
                        selected_count,
                    }
                }).collect();

                // Accumulate at most one state change per frame (single click per frame)
                let mut toggle_hash: Option<String> = None;
                let mut toggle_file: Option<(i64, bool)> = None;
                let mut delete_files: Vec<(i64, std::path::PathBuf, String)> = Vec::new();

                ScrollArea::vertical().id_source("storage_dup_groups").max_height(360.0).show(ui, |ui| {
                    for g in &display {
                        // Header row: expand arrow + summary
                        let arrow_clicked = ui.horizontal(|ui| {
                            let arrow = if g.expanded { "â–¼" } else { "â–¶" };
                            let clicked = theme::micro_button(ui, arrow).clicked();
                            let header = format!(
                                "[{}]  {} copies  |  {} each  |  wasted {:.1} MB",
                                g.hash_short, g.file_count,
                                crate::telemetry::fmt_bytes(g.size),
                                g.wasted as f64 / 1_048_576.0
                            );
                            ui.label(RichText::new(&header).color(Colors::TEXT).size(8.0));
                            clicked
                        }).inner;

                        if arrow_clicked {
                            toggle_hash = Some(g.hash.clone());
                        }

                        // Per-file rows with checkboxes (shown when expanded)
                        if g.expanded {
                            for file in &g.files {
                                let (fid, new_state) = ui.horizontal(|ui| {
                                    ui.add_space(18.0);
                                    let mut checked = file.selected;
                                    let changed = ui.checkbox(&mut checked, RichText::new(&file.label).size(7.5).color(Colors::TEXT_DIM)).changed();
                                    (file.id, if changed { Some(checked) } else { None })
                                }).inner;
                                if let Some(state) = new_state {
                                    toggle_file = Some((fid, state));
                                }
                            }

                            if g.selected_count > 0 {
                                ui.add_space(3.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(18.0);
                                    let del_label = format!("DELETE {} SELECTED", g.selected_count);
                                    if theme::danger_button(ui, &del_label).clicked() {
                                        delete_files.extend(
                                            g.files.iter().filter(|f| f.selected)
                                                .map(|f| (f.id, f.path.clone(), f.device_id.clone()))
                                        );
                                    }
                                });
                                ui.add_space(3.0);
                            }
                        }
                        ui.add_space(2.0);
                    }
                });

                // Apply mutations after rendering (borrow checker: s is fully free here)
                if let Some(hash) = toggle_hash {
                    if s.file_manager.duplicate_expanded_groups.contains(&hash) {
                        s.file_manager.duplicate_expanded_groups.remove(&hash);
                    } else {
                        s.file_manager.duplicate_expanded_groups.insert(hash);
                    }
                }
                if let Some((id, sel)) = toggle_file {
                    if sel {
                        s.file_manager.duplicate_selected_files.insert(id);
                    } else {
                        s.file_manager.duplicate_selected_files.remove(&id);
                    }
                }
                if !delete_files.is_empty() {
                    actions.delete_duplicate_files = Some(delete_files);
                }
        } // else
    }); // Frame

    ui.add_space(12.0);
    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
        .show(ui, |ui| {
            ui.label(RichText::new("GOOGLE DRIVE BUFFER PIPELINE").color(Colors::TEXT_DIM).size(8.0).strong());
            ui.add_space(6.0);
            ui.label(RichText::new(
                "Exports one canonical original per duplicate group into a structured staging tree with metadata sidecars."
            ).color(Colors::TEXT_MUTED).size(8.0));

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("REMOTE").color(Colors::TEXT_DIM).size(8.0));
                ui.add(
                    egui::TextEdit::singleline(&mut s.file_manager.drive_remote)
                        .desired_width(260.0)
                        .hint_text("gdrive:THEGRID-BUFFER")
                );
            });

            if let Some(path) = s.drive_last_manifest {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("MANIFEST: {}", path.display())).color(Colors::GREEN).size(8.0));
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if theme::secondary_button(ui, "EXPORT CANONICALS TO BUFFER").clicked() {
                    actions.export_drive_buffer = true;
                }
                if theme::primary_button(ui, "UPLOAD BUFFER TO DRIVE").clicked() {
                    actions.upload_drive_buffer = true;
                }
            });
        });

    if let Some(telem) = s.telemetry {
        ui.label(RichText::new("STORAGE SNAPSHOT").color(Colors::GREEN).size(10.0).strong());
        ui.add_space(12.0);

        if telem.capabilities.drives.is_empty() {
            ui.label(RichText::new("// NO STORAGE UNITS DETECTED").color(Colors::TEXT_DIM).size(9.0).italics());
        } else {
            for drive in &telem.capabilities.drives {
                let pct = if drive.total > 0 { (drive.used as f64 / drive.total as f64) * 100.0 } else { 0.0 };
                
                egui::Frame::none()
                    .fill(Colors::BG_WIDGET)
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0))
                    .stroke(egui::Stroke::new(1.0, Colors::BORDER))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                                theme::draw_vector_icon(ui, rect, theme::IconType::Disk, Colors::GREEN);
                                ui.add_space(6.0);
                                ui.label(RichText::new(&drive.name).color(Colors::TEXT).size(10.0).strong());
                            });
                            ui.add_space(8.0);
                            
                            crate::telemetry::render_gauge(
                                ui, 
                                "CAPACITY", 
                                None, 
                                pct as f32, 
                                &format!("{} / {}", 
                                    crate::telemetry::fmt_bytes(drive.used), 
                                    crate::telemetry::fmt_bytes(drive.total)
                                )
                            );

                            ui.add_space(8.0);
                            if theme::secondary_button(ui, "â¬¡ SCAN DRIVE").clicked() {
                                actions.scan_remote = true;
                            }
                        });
                    });
                ui.add_space(8.0);
            }
        }
    } else {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new("// WAITING FOR TELEMETRY DATA").color(Colors::TEXT_DIM).size(9.0).italics());
        });
    }
}
