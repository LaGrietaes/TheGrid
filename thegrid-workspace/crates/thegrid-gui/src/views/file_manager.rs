// ═══════════════════════════════════════════════════════════════════════════════
// views/file_manager.rs — Brutalist HUD File Explorer  [v0.3 — Phase 3]
//
// Layout:
//   [Drive Bar]  ← Drive picker from telemetry + manual path entry
//   [Breadcrumb / Nav Bar]
//   [Filter + Sort Toolbar]
//   [File List (left 65%)]  |  [Preview / Metadata Panel (right 35%)]
// ═══════════════════════════════════════════════════════════════════════════════

use egui::{Color32, RichText, Ui, ScrollArea, Stroke, Margin};
use crate::theme::{self, Colors};
use crate::views::dashboard::{DetailState, DetailActions};
use crate::app::FileViewMode;

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

pub fn render(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    ui.vertical(|ui| {
        // ── 1. Drive bar ──────────────────────────────────────────────────────
        render_drive_bar(ui, s, actions);
        ui.add_space(4.0);

        // ── 2. Breadcrumb navigation ──────────────────────────────────────────
        render_nav_bar(ui, s, actions);
        ui.add_space(4.0);

        // ── 3. Toolbar (sort, filter, view mode) ──────────────────────────────
        render_toolbar(ui, s, actions);

        ui.add(egui::Separator::default().spacing(0.0));

        // ── 4. Main content: file list + preview ──────────────────────────────
        let avail = ui.available_width();
        let preview_w = (avail * 0.32).min(280.0);
        let list_w    = avail - preview_w - 8.0;

        ui.horizontal_top(|ui| {
            // File list
            ui.vertical(|ui| {
                ui.set_min_width(list_w);
                ui.set_max_width(list_w);
                egui::Frame::none()
                    .fill(Colors::BG_WIDGET)
                    .stroke(Stroke::new(1.0, Colors::BORDER))
                    .show(ui, |ui| {
                        if s.file_manager.view_mode == FileViewMode::Grid {
                            render_grid_view(ui, s, actions);
                        } else {
                            render_list_view(ui, s, actions);
                        }
                    });
            });

            ui.add_space(8.0);

            // Preview panel
            ui.vertical(|ui| {
                ui.set_min_width(preview_w);
                ui.set_max_width(preview_w);
                render_preview_panel(ui, s, actions);
            });
        });
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Drive Bar — shows drives from telemetry as clickable chips
// ─────────────────────────────────────────────────────────────────────────────

fn render_drive_bar(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .stroke(Stroke::new(1.0, Colors::BORDER))
        .inner_margin(Margin::symmetric(12.0, 6.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("DRIVES").color(Colors::TEXT_DIM).size(8.0).strong());
                ui.add_space(8.0);

                if let Some(telem) = s.telemetry {
                    if !telem.capabilities.drives.is_empty() {
                        for drive in &telem.capabilities.drives {
                            let current_root = s.file_manager.current_path.to_string_lossy();
                            let is_active = current_root.starts_with(&drive.name);
                            let label_color = if is_active { Colors::GREEN } else { Colors::TEXT_DIM };
                            let fill = if is_active { Color32::from_rgba_premultiplied(0, 180, 0, 30) } else { Colors::BG_WIDGET };

                            egui::Frame::none()
                                .fill(fill)
                                .stroke(Stroke::new(1.0, if is_active { Colors::GREEN } else { Colors::BORDER }))
                                .inner_margin(Margin::symmetric(8.0, 4.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                        theme::draw_vector_icon(ui, r, theme::IconType::Disk, label_color);
                                        ui.add_space(4.0);
                                        let resp = ui.add(egui::Label::new(
                                            RichText::new(&drive.name).color(label_color).size(9.0).strong()
                                        ).sense(egui::Sense::click()));
                                        if resp.clicked() {
                                            let root = std::path::PathBuf::from(&drive.name);
                                            s.file_manager.current_path = root.clone();
                                            s.file_manager.selected_files.clear();
                                            s.file_manager.preview_file = None;
                                            s.file_manager.preview_content = None;
                                            s.file_manager.preview_texture = None; // clear previous texture
                                            actions.browse_remote = Some(root);
                                        }
                                        // Usage bar
                                        let pct = if drive.total > 0 {
                                            (drive.used as f32 / drive.total as f32).clamp(0.0, 1.0)
                                        } else { 0.0 };
                                        ui.add_space(6.0);
                                        let bar_rect = {
                                            let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 5.0), egui::Sense::hover());
                                            r
                                        };
                                        ui.painter().rect_filled(bar_rect, 1.0, Colors::BG);
                                        let fill_w = bar_rect.width() * pct;
                                        let fill_rect = egui::Rect::from_min_size(bar_rect.min, egui::vec2(fill_w, bar_rect.height()));
                                        let bar_color = if pct > 0.85 { Colors::RED } else if pct > 0.6 { Colors::AMBER } else { Colors::GREEN };
                                        ui.painter().rect_filled(fill_rect, 1.0, bar_color);
                                    });
                                });
                            ui.add_space(4.0);
                        }
                    } else {
                        ui.label(RichText::new("// NO DRIVES — FETCH TELEMETRY FIRST").color(Colors::TEXT_MUTED).size(8.0).italics());
                    }
                } else {
                    ui.label(RichText::new("// NO TELEMETRY — CLICK FETCH TELEMETRY IN ACTIONS").color(Colors::TEXT_MUTED).size(8.0).italics());
                    if theme::micro_button(ui, "REFRESH").clicked() {
                        actions.scan_remote = true;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::micro_button(ui, "SCAN").clicked() {
                        actions.scan_remote = true;
                    }
                    ui.add_space(4.0);
                    if theme::micro_button(ui, "↑ UP").clicked() {
                        if let Some(parent) = s.file_manager.current_path.parent() {
                            let p = parent.to_path_buf();
                            actions.browse_remote = Some(p.clone());
                            s.file_manager.current_path = p;
                        }
                    }
                });
            });
        });
}

// ─────────────────────────────────────────────────────────────────────────────
// Breadcrumb navigation bar
// ─────────────────────────────────────────────────────────────────────────────

fn render_nav_bar(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .stroke(Stroke::new(1.0, Colors::BORDER))
        .inner_margin(Margin::symmetric(12.0, 6.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Back button
                let can_go_back = s.file_manager.current_path.parent().is_some()
                    && s.file_manager.current_path != std::path::Path::new("");
                ui.set_enabled(can_go_back);
                if ui.button(RichText::new("◀").color(Colors::GREEN).size(10.0)).clicked() {
                    let parent = s.file_manager.current_path.parent()
                        .unwrap_or(std::path::Path::new(""))
                        .to_path_buf();
                    actions.browse_remote = Some(parent.clone());
                    s.file_manager.current_path = parent;
                    s.file_manager.selected_files.clear();
                }
                ui.set_enabled(true);
                ui.add_space(6.0);

                // Breadcrumb segments
                let path_str = s.file_manager.current_path.to_string_lossy().to_string();
                if path_str.is_empty() {
                    ui.label(RichText::new("/ root").color(Colors::TEXT_DIM).size(9.0));
                } else {
                    // Clone path to avoid borrow conflict when we later mutate it
                    let path_clone = s.file_manager.current_path.clone();
                    let mut accumulated = std::path::PathBuf::new();
                    let parts: Vec<_> = path_clone.components().collect();
                    for (i, part) in parts.iter().enumerate() {
                        accumulated.push(part);
                        let seg = part.as_os_str().to_string_lossy().to_string();
                        let is_last = i == parts.len() - 1;
                        let color = if is_last { Colors::GREEN } else { Colors::TEXT_DIM };
                        let acc_copy = accumulated.clone();
                        let resp = ui.add(egui::Label::new(
                            RichText::new(format!("{}/", seg)).color(color).size(9.0).strong()
                        ).sense(egui::Sense::click()));
                        if resp.clicked() && !is_last {
                            actions.browse_remote = Some(acc_copy.clone());
                            s.file_manager.current_path = acc_copy;
                            s.file_manager.selected_files.clear();
                        }
                    }
                }

                // File count badge
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let n = s.remote_files.len();
                    let sel = s.file_manager.selected_files.len();
                    let badge = if sel > 0 {
                        format!("{} selected / {} items", sel, n)
                    } else {
                        format!("{} items", n)
                    };
                    ui.label(RichText::new(badge).color(Colors::TEXT_DIM).size(8.0));
                });
            });
        });
}

// ─────────────────────────────────────────────────────────────────────────────
// Toolbar — filter, sort, view mode
// ─────────────────────────────────────────────────────────────────────────────

fn render_toolbar(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    ui.horizontal(|ui| {
        ui.set_height(28.0);
        ui.add_space(8.0);

        // Search/filter
        ui.label(RichText::new("FILTER:").color(Colors::TEXT_DIM).size(8.0));
        ui.add(
            egui::TextEdit::singleline(&mut s.file_manager.filter_query)
                .desired_width(120.0)
                .hint_text("filename...")
                .font(egui::FontId::new(9.0, egui::FontFamily::Monospace))
        );

        ui.add_space(8.0);

        // Sort
        ui.label(RichText::new("SORT:").color(Colors::TEXT_DIM).size(8.0));
        let sort_label = if s.file_manager.sort_ascending { "NAME ↑" } else { "NAME ↓" };
        if ui.button(RichText::new(sort_label).size(8.0)).clicked() {
            s.file_manager.sort_ascending = !s.file_manager.sort_ascending;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            // View mode
            if s.file_manager.view_mode == FileViewMode::List {
                if ui.button(RichText::new("⊞ GRID").size(8.0)).clicked() {
                    s.file_manager.view_mode = FileViewMode::Grid;
                }
            } else {
                if ui.button(RichText::new("☰ LIST").size(8.0)).clicked() {
                    s.file_manager.view_mode = FileViewMode::List;
                }
            }
            ui.add_space(8.0);
            // Selection actions
            if !s.file_manager.selected_files.is_empty() {
                ui.label(RichText::new(format!("[{}]", s.file_manager.selected_files.len())).color(Colors::AMBER).size(9.0).strong());
                if ui.button(RichText::new("✗ DEL").color(Colors::RED).size(8.0)).clicked() {
                    let paths: Vec<String> = s.file_manager.selected_files.iter().cloned().collect();
                    actions.fm_delete = Some(paths);
                    s.file_manager.selected_files.clear();
                }
            }
        });
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// List view
// ─────────────────────────────────────────────────────────────────────────────

fn render_list_view(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    let active_rule = s.file_manager.active_rule.as_ref()
        .and_then(|id| s.smart_rules.iter().find(|r| &r.id == id));

    // Column header
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(Margin::symmetric(8.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("NAME").color(Colors::TEXT_DIM).size(8.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new("SIZE").color(Colors::TEXT_DIM).size(8.0).strong());
                    ui.add_space(40.0);
                    ui.label(RichText::new("TYPE").color(Colors::TEXT_DIM).size(8.0).strong());
                });
            });
        });
    ui.add(egui::Separator::default().spacing(0.0));

    ScrollArea::vertical()
        .id_source("fm_list_v3")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if s.remote_files.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new("◌").color(Colors::TEXT_MUTED).size(20.0));
                    ui.add_space(8.0);
                    ui.label(RichText::new("DIRECTORY EMPTY OR NOT YET LOADED").color(Colors::TEXT_MUTED).size(9.0));
                    ui.add_space(8.0);
                    ui.label(RichText::new("← SELECT A DRIVE ABOVE TO START").color(Colors::TEXT_DIM).size(8.0).italics());
                });
                return;
            }

            let query = s.file_manager.filter_query.to_lowercase();
            let asc = s.file_manager.sort_ascending;

            // Filter & Sort
            let mut sorted: Vec<_> = s.remote_files.iter().filter(|rf| {
                if !query.is_empty() && !rf.name.to_lowercase().contains(&query) { return false; }
                if let Some(rule) = active_rule {
                    for f in &rule.filters {
                        match f {
                            thegrid_core::models::SmartFilterType::Extension(ext) => {
                                if rf.is_dir { return false; }
                                let file_ext = std::path::Path::new(&rf.name)
                                    .extension()
                                    .map(|e| e.to_string_lossy().to_lowercase())
                                    .unwrap_or_default();
                                if file_ext != ext.to_lowercase() { return false; }
                            }
                            thegrid_core::models::SmartFilterType::MinSize(ms) => {
                                if rf.is_dir || rf.size < *ms { return false; }
                            }
                            thegrid_core::models::SmartFilterType::MaxSize(ms) => {
                                if rf.is_dir || rf.size > *ms { return false; }
                            }
                            thegrid_core::models::SmartFilterType::ModifiedAfter(dt) => {
                                if let Some(m) = rf.modified { if m < *dt { return false; } } else { return false; }
                            }
                            thegrid_core::models::SmartFilterType::ModifiedBefore(dt) => {
                                if let Some(m) = rf.modified { if m > *dt { return false; } } else { return false; }
                            }
                            _ => {} // Project/Category tags not implemented yet
                        }
                    }
                }
                true
            }).collect();
            sorted.sort_by(|a, b| {
                if a.is_dir != b.is_dir {
                    // dirs first
                    b.is_dir.cmp(&a.is_dir)
                } else if asc {
                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                } else {
                    b.name.to_lowercase().cmp(&a.name.to_lowercase())
                }
            });

            for rf in sorted {

                let is_selected = s.file_manager.selected_files.contains(&rf.name);
                let is_preview  = s.file_manager.preview_file.as_deref() == Some(&rf.name);
                let bg = if is_selected {
                    Colors::AMBER.gamma_multiply(0.12)
                } else if is_preview {
                    Color32::from_rgba_premultiplied(0, 150, 0, 15)
                } else {
                    Color32::TRANSPARENT
                };

                let resp = egui::Frame::none()
                    .fill(bg)
                    .inner_margin(Margin::symmetric(8.0, 5.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Icon
                            let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                            let (icon, icon_color) = if rf.is_dir {
                                (theme::IconType::Folder, Colors::GREEN)
                            } else {
                                let ext = std::path::Path::new(&rf.name).extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
                                theme::get_file_icon(&ext)
                            };
                            theme::draw_vector_icon(ui, icon_rect, icon, icon_color);
                            ui.add_space(6.0);

                            let label_color = if rf.is_dir { Colors::GREEN } else { Colors::TEXT };
                            let resp = ui.add(egui::Label::new(
                                RichText::new(&rf.name).color(label_color).size(10.0)
                            ).sense(egui::Sense::click()));

                            if resp.clicked() {
                                if ui.input(|i| i.modifiers.ctrl) {
                                    // Multi-select
                                    if is_selected { s.file_manager.selected_files.remove(&rf.name); }
                                    else { s.file_manager.selected_files.insert(rf.name.clone()); }
                                } else if rf.is_dir {
                                    let mut new_path = s.file_manager.current_path.clone();
                                    new_path.push(&rf.name);
                                    actions.browse_remote = Some(new_path.clone());
                                    s.file_manager.current_path = new_path;
                                    s.file_manager.selected_files.clear();
                                    s.file_manager.preview_file = None;
                                    s.file_manager.preview_content = None;
                                    s.file_manager.preview_texture = None;
                                } else {
                                    // Single click: select + show in preview
                                    s.file_manager.selected_files.clear();
                                    s.file_manager.selected_files.insert(rf.name.clone());
                                    s.file_manager.preview_file = Some(rf.name.clone());
                                    let mut p = s.file_manager.current_path.clone();
                                    p.push(&rf.name);
                                    actions.preview_remote = Some(p);
                                    s.file_manager.preview_content = None; // will load via agent
                                    s.file_manager.preview_texture = None;
                                }
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add_space(8.0);
                                if !rf.is_dir {
                                    ui.label(RichText::new(crate::views::dashboard::fmt_bytes(rf.size))
                                        .color(Colors::TEXT_DIM).size(8.0));
                                    if ui.button(RichText::new("↓").size(8.0)).clicked() {
                                        let mut p = s.file_manager.current_path.clone();
                                        p.push(&rf.name);
                                        actions.download_remote_file = Some(p);
                                    }
                                } else {
                                    ui.label(RichText::new("DIR").color(Colors::TEXT_DIM).size(8.0));
                                }
                                // Type badge
                                let ext = std::path::Path::new(&rf.name)
                                    .extension()
                                    .map(|e| e.to_string_lossy().to_uppercase())
                                    .unwrap_or_default();
                                if !ext.is_empty() {
                                    ui.label(RichText::new(format!(".{}", ext)).color(Colors::TEXT_DIM).size(7.0));
                                }
                            });
                        });
                    }).response;

                // Hover glow
                if resp.hovered() && !is_selected {
                    ui.painter().rect_stroke(resp.rect, egui::Rounding::ZERO,
                        Stroke::new(1.0, Colors::BORDER2.gamma_multiply(0.5)));
                }

                ui.add(egui::Separator::default().spacing(0.0));
            }
        });
}

// ─────────────────────────────────────────────────────────────────────────────
// Grid view
// ─────────────────────────────────────────────────────────────────────────────

fn render_grid_view(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    let active_rule = s.file_manager.active_rule.as_ref()
        .and_then(|id| s.smart_rules.iter().find(|r| &r.id == id));

    if s.remote_files.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new("◌").color(Colors::TEXT_MUTED).size(20.0));
            ui.add_space(8.0);
            ui.label(RichText::new("SELECT A DRIVE TO EXPLORE").color(Colors::TEXT_MUTED).size(9.0));
        });
        return;
    }

    let query = s.file_manager.filter_query.to_lowercase();

    ScrollArea::vertical()
        .id_source("fm_grid_v3")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width();
            let cols = ((width / 80.0) as usize).max(1);

            egui::Grid::new("fm_grid_inner_v3")
                .spacing(egui::vec2(8.0, 8.0))
                .show(ui, |ui| {
                    let mut count = 0;
                    for rf in s.remote_files {
                        if !query.is_empty() && !rf.name.to_lowercase().contains(&query) { continue; }
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
                            if !matches { continue; }
                        }


                        let is_selected = s.file_manager.selected_files.contains(&rf.name);

                        ui.vertical_centered(|ui| {
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(70.0, 70.0), egui::Sense::click());
                            let bg = if is_selected { Colors::AMBER.gamma_multiply(0.2) }
                                     else if resp.hovered() { Colors::BORDER.gamma_multiply(0.3) }
                                     else { Colors::BG_WIDGET };
                            ui.painter().rect_filled(rect, 2.0, bg);
                            ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, if is_selected { Colors::AMBER } else { Colors::BORDER }));

                            // Icon
                            let icon_rect = egui::Rect::from_center_size(rect.center() - egui::vec2(0.0, 8.0), egui::vec2(16.0, 16.0));
                            let (icon, icon_color) = if rf.is_dir {
                                (theme::IconType::Folder, Colors::GREEN)
                            } else {
                                let ext = std::path::Path::new(&rf.name).extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
                                theme::get_file_icon(&ext)
                            };
                            theme::draw_vector_icon(ui, icon_rect, icon, icon_color);

                            let mut name = rf.name.clone();
                            if name.len() > 10 { name.truncate(8); name.push_str(".."); }
                            ui.painter().text(rect.center() + egui::vec2(0.0, 18.0),
                                egui::Align2::CENTER_CENTER, name,
                                egui::FontId::proportional(8.0),
                                if rf.is_dir { Colors::GREEN } else { Colors::TEXT });

                            if resp.clicked() {
                                if rf.is_dir {
                                    let mut new_path = s.file_manager.current_path.clone();
                                    new_path.push(&rf.name);
                                    actions.browse_remote = Some(new_path.clone());
                                    s.file_manager.current_path = new_path;
                                    s.file_manager.selected_files.clear();
                                } else {
                                    s.file_manager.selected_files.clear();
                                    s.file_manager.selected_files.insert(rf.name.clone());
                                    s.file_manager.preview_file = Some(rf.name.clone());
                                    let mut p = s.file_manager.current_path.clone();
                                    p.push(&rf.name);
                                    actions.preview_remote = Some(p);
                                    s.file_manager.preview_content = None;
                                    s.file_manager.preview_texture = None;
                                }
                            }
                        });

                        count += 1;
                        if count % cols == 0 { ui.end_row(); }
                    }
                });
        });
}

// ─────────────────────────────────────────────────────────────────────────────
// Preview Panel — metadata + content preview for selected file
// ─────────────────────────────────────────────────────────────────────────────

fn render_preview_panel(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .stroke(Stroke::new(1.0, Colors::BORDER))
        .inner_margin(Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_min_height(300.0);

            // Header
            ui.label(RichText::new("// PREVIEW").color(Colors::GREEN).size(9.0).strong());
            ui.add(egui::Separator::default().spacing(4.0));
            ui.add_space(4.0);

            if let Some(fname) = &s.file_manager.preview_file.clone() {
                // Find the remote file entry
                let rf_opt = s.remote_files.iter().find(|f| &f.name == fname).cloned();

                if let Some(rf) = rf_opt {
                    // File name
                    ui.label(RichText::new(&rf.name).color(Colors::TEXT).size(10.0).strong());
                    ui.add_space(8.0);

                    // Metadata rows
                    let ext = std::path::Path::new(&rf.name)
                        .extension()
                        .map(|e| e.to_string_lossy().to_uppercase())
                        .unwrap_or_else(|| "—".to_string());

                    meta_row(ui, "TYPE",  if rf.is_dir { "DIRECTORY" } else { &ext });
                    if !rf.is_dir {
                        meta_row(ui, "SIZE", &crate::views::dashboard::fmt_bytes(rf.size));
                    }

                    ui.add_space(12.0);

                    if rf.is_dir {
                        ui.label(RichText::new("// FOLDER — CLICK TO OPEN").color(Colors::TEXT_DIM).size(8.0).italics());
                    } else {
                        let ext_lower = std::path::Path::new(&rf.name).extension()
                                .map(|e| e.to_string_lossy().to_lowercase())
                                .unwrap_or_default();

                        let is_text = matches!(
                            ext_lower.as_str(),
                            "txt" | "log" | "md" | "json" | "toml" | "yaml" | "yml" | "rs" | "py" | "js" | "ts" | "sh" | "csv" | "xml" | "html" | "css"
                        );
                        let is_image = matches!(
                            ext_lower.as_str(),
                            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp"
                        );

                        if is_text {
                            if let Some(bytes) = &s.file_manager.preview_content {
                                let content = String::from_utf8_lossy(bytes);
                                ui.label(RichText::new("// CONTENT PREVIEW").color(Colors::TEXT_DIM).size(8.0));
                                ui.add_space(4.0);
                                egui::Frame::none()
                                    .fill(Colors::BG)
                                    .stroke(Stroke::new(1.0, Colors::BORDER))
                                    .inner_margin(Margin::same(8.0))
                                    .show(ui, |ui| {
                                        ScrollArea::vertical()
                                            .id_source("preview_scroll")
                                            .max_height(240.0)
                                            .show(ui, |ui| {
                                                ui.add(egui::Label::new(
                                                    RichText::new(content).
                                                        size(8.0).
                                                        color(Colors::TEXT_DIM).
                                                        monospace()
                                                ).wrap(true));
                                            });
                                    });
                            } else {
                                ui.label(RichText::new("// LOADING TEXT PREVIEW...").color(Colors::TEXT_DIM).size(8.0).italics());
                            }
                        } else if is_image {
                            if let Some(texture) = &s.file_manager.preview_texture {
                                ui.label(RichText::new("// IMAGE PREVIEW").color(Colors::TEXT_DIM).size(8.0));
                                ui.add_space(4.0);
                                ui.add(egui::Image::from_texture(texture).max_width(200.0).maintain_aspect_ratio(true));
                            } else if let Some(bytes) = &s.file_manager.preview_content {
                                // Try to load texture
                                if let Ok(image) = image::load_from_memory(bytes) {
                                    let size = [image.width() as usize, image.height() as usize];
                                    let image_buffer = image.to_rgba8();
                                    let pixels = image_buffer.as_flat_samples();
                                    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
                                    s.file_manager.preview_texture = Some(ui.ctx().load_texture("preview", color_image, Default::default()));
                                } else {
                                    ui.label(RichText::new("// FAILED TO DECODE IMAGE").color(Colors::RED).size(8.0).italics());
                                }
                            } else {
                                ui.label(RichText::new("// LOADING IMAGE PREVIEW...").color(Colors::TEXT_DIM).size(8.0).italics());
                            }
                        } else {
                            // Binary / non-text file
                            let category = categorize_file(&rf.name);
                            ui.label(RichText::new(format!("// {} FILE", category)).color(Colors::TEXT_DIM).size(8.0).italics());
                        }

                        ui.add_space(12.0);
                        // Download button
                        if ui.button(RichText::new("↓ DOWNLOAD TO LOCAL").color(Colors::GREEN).size(9.0)).clicked() {
                            let mut p = s.file_manager.current_path.clone();
                            p.push(&rf.name);
                            actions.download_remote_file = Some(p);
                        }
                    }
                }
            } else {
                // Nothing selected
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new("◌").color(Colors::TEXT_MUTED).size(18.0));
                    ui.add_space(8.0);
                    ui.label(RichText::new("SELECT A FILE\nTO PREVIEW").color(Colors::TEXT_MUTED).size(8.0));
                });
            }
        });
}

fn meta_row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{}:", label)).color(Colors::TEXT_DIM).size(8.0).strong());
        ui.add_space(4.0);
        ui.label(RichText::new(value).color(Colors::TEXT).size(9.0));
    });
    ui.add_space(2.0);
}

fn categorize_file(name: &str) -> &'static str {
    let ext = std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp4" | "mkv" | "avi" | "mov" | "webm" => "VIDEO",
        "mp3" | "flac" | "wav" | "ogg" | "m4a" => "AUDIO",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" => "IMAGE",
        "zip" | "tar" | "gz" | "rar" | "7z" => "ARCHIVE",
        "pdf" => "PDF",
        "exe" | "msi" | "deb" | "apk" => "BINARY",
        _ => "BINARY",
    }
}
