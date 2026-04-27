// ═══════════════════════════════════════════════════════════════════════════════
// views/file_manager.rs — Brutalist HUD File Explorer  [v0.4 — Phase 4]
//
// Layout:
//   [Drive Bar]
//   [Breadcrumb / Nav Bar]
//   [Sort + View Toolbar]
//   [Filter Strip]  ← name / type / size / date filter boxes
//   [File List (left ~75%)]  |  [Meta Panel (right ~25%)]
//   [▼ Big Preview Drop-down]  ← expands when file selected
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

        // ── 3. Sort / view toolbar ────────────────────────────────────────────
        render_toolbar(ui, s, actions);

        // ── 4. Filter strip (name / type / size / date) ───────────────────────
        render_filter_strip(ui, s);

        ui.add(egui::Separator::default().spacing(0.0));

        // ── 5. Main content: file list + meta panel ───────────────────────────
        let avail_w = ui.available_width();
        let meta_w  = (avail_w * 0.24).min(200.0).max(140.0);
        let list_w  = avail_w - meta_w - 8.0;

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

            // Metadata panel (narrow — TYPE / SIZE / DATE, no content)
            ui.vertical(|ui| {
                ui.set_min_width(meta_w);
                ui.set_max_width(meta_w);
                render_meta_panel(ui, s, actions);
            });
        });

        // ── 6. Big collapsible content preview ────────────────────────────────
        if s.file_manager.preview_file.is_some() {
            ui.add_space(4.0);
            ui.add(egui::Separator::default().spacing(0.0));
            render_big_preview_panel(ui, s, actions);
        }
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
        // (sort is now on the filter strip label buttons)
        let sort_label = match s.file_manager.sort_field {
            crate::app::FileSortField::Name => if s.file_manager.sort_ascending { "NAME ▲" } else { "NAME ▼" },
            crate::app::FileSortField::Type => if s.file_manager.sort_ascending { "TYPE ▲" } else { "TYPE ▼" },
            crate::app::FileSortField::Size => if s.file_manager.sort_ascending { "SIZE ▲" } else { "SIZE ▼" },
            crate::app::FileSortField::Date => if s.file_manager.sort_ascending { "DATE ▲" } else { "DATE ▼" },
        };
        ui.label(RichText::new(sort_label).color(Colors::AMBER).size(8.0).strong());

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
            let sel_count = s.file_manager.selected_files.len();
            if sel_count > 0 {
                ui.label(RichText::new(format!("[{}]", sel_count)).color(Colors::AMBER).size(9.0).strong());
                if ui.button(RichText::new("✗ DEL").color(Colors::RED).size(8.0)).clicked() {
                    let paths: Vec<String> = s.file_manager.selected_files.iter().cloned().collect();
                    actions.fm_delete = Some(paths);
                    s.file_manager.selected_files.clear();
                    s.file_manager.rename_target = None;
                }
                // Rename — only for single selection
                if sel_count == 1 {
                    let current_name = s.file_manager.selected_files.iter().next().cloned().unwrap_or_default();
                    let is_renaming = s.file_manager.rename_target.as_deref() == Some(&current_name);
                    if is_renaming {
                        // Inline rename field (right-to-left layout, so show in order)
                        ui.add_space(4.0);
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut s.file_manager.rename_buffer)
                                .desired_width(100.0)
                                .font(egui::FontId::new(8.5, egui::FontFamily::Monospace))
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let new_name = s.file_manager.rename_buffer.trim().to_string();
                            if !new_name.is_empty() && new_name != current_name {
                                actions.fm_rename = Some((current_name.clone(), new_name));
                            }
                            s.file_manager.rename_target = None;
                            s.file_manager.rename_buffer.clear();
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            s.file_manager.rename_target = None;
                            s.file_manager.rename_buffer.clear();
                        }
                        if theme::micro_button(ui, "✓").clicked() {
                            let new_name = s.file_manager.rename_buffer.trim().to_string();
                            if !new_name.is_empty() && new_name != current_name {
                                actions.fm_rename = Some((current_name.clone(), new_name));
                            }
                            s.file_manager.rename_target = None;
                            s.file_manager.rename_buffer.clear();
                        }
                        if theme::micro_button(ui, "✗").clicked() {
                            s.file_manager.rename_target = None;
                            s.file_manager.rename_buffer.clear();
                        }
                    } else if theme::micro_button(ui, "✎ REN").clicked() {
                        s.file_manager.rename_buffer = current_name.clone();
                        s.file_manager.rename_target = Some(current_name);
                    }
                }
                // Move — available for any selection
                if theme::micro_button(ui, "→ MOVE").clicked() {
                    if let Some(dest) = rfd::FileDialog::new()
                        .set_title("Move to folder")
                        .pick_folder()
                    {
                        let paths: Vec<String> = s.file_manager.selected_files.iter().cloned().collect();
                        actions.fm_move = Some((paths, dest));
                        s.file_manager.selected_files.clear();
                    }
                }
            }
        });
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Filter strip — name / type / size / date filter boxes
// ─────────────────────────────────────────────────────────────────────────────

fn render_filter_strip(ui: &mut Ui, s: &mut DetailState) {
    egui::Frame::none()
        .fill(Colors::BG_PANEL.gamma_multiply(0.85))
        .stroke(Stroke::new(1.0, Colors::BORDER))
        .inner_margin(Margin::symmetric(10.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                // ── NAME ─────────────────────────────────────────────────────
                let name_active = s.file_manager.sort_field == crate::app::FileSortField::Name;
                let name_arrow  = if name_active { if s.file_manager.sort_ascending { " ▲" } else { " ▼" } } else { " ·" };
                let name_label  = RichText::new(format!("NAME{}", name_arrow))
                    .size(7.5).strong()
                    .color(if name_active { Colors::AMBER } else { Colors::TEXT_DIM });
                if ui.add(egui::Label::new(name_label).sense(egui::Sense::click())).clicked() {
                    if name_active { s.file_manager.sort_ascending = !s.file_manager.sort_ascending; }
                    else { s.file_manager.sort_field = crate::app::FileSortField::Name; s.file_manager.sort_ascending = true; }
                }
                ui.add(egui::TextEdit::singleline(&mut s.file_manager.filter_query)
                    .desired_width(90.0)
                    .hint_text("filename…")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));

                ui.separator();

                // ── TYPE ─────────────────────────────────────────────────────
                let type_active = s.file_manager.sort_field == crate::app::FileSortField::Type;
                let type_arrow  = if type_active { if s.file_manager.sort_ascending { " ▲" } else { " ▼" } } else { " ·" };
                let type_label  = RichText::new(format!("TYPE{}", type_arrow))
                    .size(7.5).strong()
                    .color(if type_active { Colors::AMBER } else { Colors::TEXT_DIM });
                if ui.add(egui::Label::new(type_label).sense(egui::Sense::click())).clicked() {
                    if type_active { s.file_manager.sort_ascending = !s.file_manager.sort_ascending; }
                    else { s.file_manager.sort_field = crate::app::FileSortField::Type; s.file_manager.sort_ascending = true; }
                }
                ui.add(egui::TextEdit::singleline(&mut s.file_manager.filter_type)
                    .desired_width(70.0)
                    .hint_text("rs, py, png…")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));

                ui.separator();

                // ── SIZE ─────────────────────────────────────────────────────
                let size_active = s.file_manager.sort_field == crate::app::FileSortField::Size;
                let size_arrow  = if size_active { if s.file_manager.sort_ascending { " ▲" } else { " ▼" } } else { " ·" };
                let size_label  = RichText::new(format!("SIZE{}", size_arrow))
                    .size(7.5).strong()
                    .color(if size_active { Colors::AMBER } else { Colors::TEXT_DIM });
                if ui.add(egui::Label::new(size_label).sense(egui::Sense::click())).clicked() {
                    if size_active { s.file_manager.sort_ascending = !s.file_manager.sort_ascending; }
                    else { s.file_manager.sort_field = crate::app::FileSortField::Size; s.file_manager.sort_ascending = true; }
                }
                ui.add(egui::TextEdit::singleline(&mut s.file_manager.filter_min_size_kb)
                    .desired_width(50.0)
                    .hint_text("min KB")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));
                ui.label(RichText::new("–").color(Colors::TEXT_DIM).size(8.0));
                ui.add(egui::TextEdit::singleline(&mut s.file_manager.filter_max_size_kb)
                    .desired_width(50.0)
                    .hint_text("max KB")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));

                ui.separator();

                // ── DATE ─────────────────────────────────────────────────────
                let date_active = s.file_manager.sort_field == crate::app::FileSortField::Date;
                let date_arrow  = if date_active { if s.file_manager.sort_ascending { " ▲" } else { " ▼" } } else { " ·" };
                let date_label  = RichText::new(format!("DATE{}", date_arrow))
                    .size(7.5).strong()
                    .color(if date_active { Colors::AMBER } else { Colors::TEXT_DIM });
                if ui.add(egui::Label::new(date_label).sense(egui::Sense::click())).clicked() {
                    if date_active { s.file_manager.sort_ascending = !s.file_manager.sort_ascending; }
                    else { s.file_manager.sort_field = crate::app::FileSortField::Date; s.file_manager.sort_ascending = true; }
                }
                ui.add(egui::TextEdit::singleline(&mut s.file_manager.filter_date_after)
                    .desired_width(72.0)
                    .hint_text("YYYY-MM-DD")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));
                ui.label(RichText::new("→").color(Colors::TEXT_DIM).size(8.0));
                ui.add(egui::TextEdit::singleline(&mut s.file_manager.filter_date_before)
                    .desired_width(72.0)
                    .hint_text("YYYY-MM-DD")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));

                // ── Clear button ──────────────────────────────────────────────
                let any_active = !s.file_manager.filter_query.is_empty()
                    || !s.file_manager.filter_type.is_empty()
                    || !s.file_manager.filter_min_size_kb.is_empty()
                    || !s.file_manager.filter_max_size_kb.is_empty()
                    || !s.file_manager.filter_date_after.is_empty()
                    || !s.file_manager.filter_date_before.is_empty();

                if any_active {
                    ui.add_space(4.0);
                    if ui.button(RichText::new("✕ CLEAR").color(Colors::RED).size(8.0)).clicked() {
                        s.file_manager.filter_query.clear();
                        s.file_manager.filter_type.clear();
                        s.file_manager.filter_min_size_kb.clear();
                        s.file_manager.filter_max_size_kb.clear();
                        s.file_manager.filter_date_after.clear();
                        s.file_manager.filter_date_before.clear();
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
            let asc        = s.file_manager.sort_ascending;
            let sort_field = s.file_manager.sort_field;

            // Parse toolbar filter strip values once
            let type_exts: Vec<String> = s.file_manager.filter_type
                .split(',').map(|e| e.trim().to_lowercase())
                .filter(|e| !e.is_empty()).collect();
            let min_bytes: Option<u64> = s.file_manager.filter_min_size_kb
                .trim().parse::<u64>().ok().map(|kb| kb * 1024);
            let max_bytes: Option<u64> = s.file_manager.filter_max_size_kb
                .trim().parse::<u64>().ok().map(|kb| kb * 1024);
            let date_after = chrono::NaiveDate::parse_from_str(
                s.file_manager.filter_date_after.trim(), "%Y-%m-%d").ok()
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc());
            let date_before = chrono::NaiveDate::parse_from_str(
                s.file_manager.filter_date_before.trim(), "%Y-%m-%d").ok()
                .map(|d| d.and_hms_opt(23, 59, 59).unwrap().and_utc());

            // Filter & Sort
            let mut sorted: Vec<_> = s.remote_files.iter().filter(|rf| {
                // Name filter
                if !query.is_empty() && !rf.name.to_lowercase().contains(&query) { return false; }
                // Type/extension filter
                if !type_exts.is_empty() && !rf.is_dir {
                    let file_ext = std::path::Path::new(&rf.name)
                        .extension().map(|e| e.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    if !type_exts.iter().any(|e| e == &file_ext) { return false; }
                }
                // Size filters (skip dirs)
                if !rf.is_dir {
                    if let Some(min) = min_bytes { if rf.size < min { return false; } }
                    if let Some(max) = max_bytes { if rf.size > max { return false; } }
                }
                // Date filters
                if let Some(after) = date_after {
                    match rf.modified { Some(m) if m >= after => {}, _ => { return false; } }
                }
                if let Some(before) = date_before {
                    match rf.modified { Some(m) if m <= before => {}, _ => { return false; } }
                }
                // SmartRule filters
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
                            thegrid_core::models::SmartFilterType::Project(_) => {}
                            thegrid_core::models::SmartFilterType::Category(_) => {}
                        }
                    }
                }
                true
            }).collect();
            sorted.sort_by(|a, b| {
                // Directories always float to the top regardless of sort field
                if a.is_dir != b.is_dir { return b.is_dir.cmp(&a.is_dir); }
                let ord = match sort_field {
                    crate::app::FileSortField::Name => {
                        a.name.to_lowercase().cmp(&b.name.to_lowercase())
                    }
                    crate::app::FileSortField::Type => {
                        let ea = std::path::Path::new(&a.name).extension()
                            .map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                        let eb = std::path::Path::new(&b.name).extension()
                            .map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                        ea.cmp(&eb)
                    }
                    crate::app::FileSortField::Size => a.size.cmp(&b.size),
                    crate::app::FileSortField::Date => {
                        a.modified.cmp(&b.modified)
                    }
                };
                if asc { ord } else { ord.reverse() }
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
                                    s.file_manager.inline_preview_open = true;
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

    let query      = s.file_manager.filter_query.to_lowercase();
    let asc        = s.file_manager.sort_ascending;
    let sort_field = s.file_manager.sort_field;

    // Parse toolbar filter strip values once
    let type_exts: Vec<String> = s.file_manager.filter_type
        .split(',').map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty()).collect();
    let min_bytes: Option<u64> = s.file_manager.filter_min_size_kb
        .trim().parse::<u64>().ok().map(|kb| kb * 1024);
    let max_bytes: Option<u64> = s.file_manager.filter_max_size_kb
        .trim().parse::<u64>().ok().map(|kb| kb * 1024);
    let date_after = chrono::NaiveDate::parse_from_str(
        s.file_manager.filter_date_after.trim(), "%Y-%m-%d").ok()
        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc());
    let date_before = chrono::NaiveDate::parse_from_str(
        s.file_manager.filter_date_before.trim(), "%Y-%m-%d").ok()
        .map(|d| d.and_hms_opt(23, 59, 59).unwrap().and_utc());

    // Filter then sort into a local vec so the grid can iterate in order
    let mut grid_files: Vec<_> = s.remote_files.iter().filter(|rf| {
        if !query.is_empty() && !rf.name.to_lowercase().contains(&query) { return false; }
        if !type_exts.is_empty() && !rf.is_dir {
            let file_ext = std::path::Path::new(&rf.name)
                .extension().map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if !type_exts.iter().any(|e| e == &file_ext) { return false; }
        }
        if !rf.is_dir {
            if let Some(min) = min_bytes { if rf.size < min { return false; } }
            if let Some(max) = max_bytes { if rf.size > max { return false; } }
        }
        if let Some(after) = date_after {
            match rf.modified { Some(m) if m >= after => {}, _ => { return false; } }
        }
        if let Some(before) = date_before {
            match rf.modified { Some(m) if m <= before => {}, _ => { return false; } }
        }
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
                    thegrid_core::models::SmartFilterType::Project(_) => {}
                    thegrid_core::models::SmartFilterType::Category(_) => {}
                }
            }
            if !matches { return false; }
        }
        true
    }).collect();
    grid_files.sort_by(|a, b| {
        if a.is_dir != b.is_dir { return b.is_dir.cmp(&a.is_dir); }
        let ord = match sort_field {
            crate::app::FileSortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            crate::app::FileSortField::Type => {
                let ea = std::path::Path::new(&a.name).extension()
                    .map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let eb = std::path::Path::new(&b.name).extension()
                    .map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                ea.cmp(&eb)
            }
            crate::app::FileSortField::Size => a.size.cmp(&b.size),
            crate::app::FileSortField::Date => a.modified.cmp(&b.modified),
        };
        if asc { ord } else { ord.reverse() }
    });

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
                    for rf in &grid_files {


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
                                    s.file_manager.inline_preview_open = true;
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
// Meta Panel (right sidebar) — TYPE / SIZE / DATE / DOWNLOAD only
// ─────────────────────────────────────────────────────────────────────────────

fn render_meta_panel(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .stroke(Stroke::new(1.0, Colors::BORDER))
        .inner_margin(Margin::same(10.0))
        .show(ui, |ui| {
            ui.label(RichText::new("// INFO").color(Colors::GREEN).size(9.0).strong());
            ui.add(egui::Separator::default().spacing(4.0));
            ui.add_space(4.0);

            if let Some(fname) = s.file_manager.preview_file.clone() {
                let rf_opt = s.remote_files.iter().find(|f| f.name == fname).cloned();
                if let Some(rf) = rf_opt {
                    let ext = std::path::Path::new(&rf.name)
                        .extension()
                        .map(|e| e.to_string_lossy().to_uppercase())
                        .unwrap_or_else(|| "—".into());

                    // Truncated filename
                    let disp = if rf.name.len() > 18 {
                        format!("{}…", &rf.name[..16])
                    } else {
                        rf.name.clone()
                    };
                    ui.label(RichText::new(disp).color(Colors::TEXT).size(9.5).strong());
                    ui.add_space(6.0);

                    meta_row(ui, "TYPE", if rf.is_dir { "DIR" } else { &ext });
                    if !rf.is_dir {
                        meta_row(ui, "SIZE", &crate::views::dashboard::fmt_bytes(rf.size));
                    }
                    if let Some(m) = rf.modified {
                        meta_row(ui, "DATE", &m.format("%Y-%m-%d").to_string());
                        meta_row(ui, "TIME", &m.format("%H:%M").to_string());
                    }

                    ui.add_space(10.0);

                    // Preview toggle
                    let toggle_label = if s.file_manager.inline_preview_open {
                        "▲ HIDE PREVIEW"
                    } else {
                        "▼ SHOW PREVIEW"
                    };
                    if ui.button(RichText::new(toggle_label).color(Colors::GREEN).size(8.5)).clicked() {
                        s.file_manager.inline_preview_open = !s.file_manager.inline_preview_open;
                    }

                    ui.add_space(6.0);

                    if !rf.is_dir {
                        if ui.button(RichText::new("↓ DOWNLOAD").color(Colors::AMBER).size(8.5)).clicked() {
                            let mut p = s.file_manager.current_path.clone();
                            p.push(&rf.name);
                            actions.download_remote_file = Some(p);
                        }
                    }
                } else {
                    ui.label(RichText::new("◌").color(Colors::TEXT_MUTED).size(18.0));
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(30.0);
                    ui.label(RichText::new("◌").color(Colors::TEXT_MUTED).size(18.0));
                    ui.add_space(6.0);
                    ui.label(RichText::new("SELECT A FILE").color(Colors::TEXT_MUTED).size(8.0));
                });
            }
        });
}

// ─────────────────────────────────────────────────────────────────────────────
// Big Drop-Down Preview Panel — full content preview, collapsible
// ─────────────────────────────────────────────────────────────────────────────

fn render_big_preview_panel(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    let fname = match s.file_manager.preview_file.clone() {
        Some(f) => f,
        None    => return,
    };

    // ── Header bar (always visible) ───────────────────────────────────────────
    egui::Frame::none()
        .fill(Colors::BG_PANEL.gamma_multiply(1.1))
        .stroke(Stroke::new(1.0, Colors::BORDER))
        .inner_margin(Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let arrow = if s.file_manager.inline_preview_open { "▼" } else { "▶" };
                let header_text = format!("{} PREVIEW  ·  {}", arrow, fname);
                let resp = ui.add(egui::Label::new(
                    RichText::new(&header_text).color(Colors::GREEN).size(9.5).strong()
                ).sense(egui::Sense::click()));
                if resp.clicked() {
                    s.file_manager.inline_preview_open = !s.file_manager.inline_preview_open;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::micro_button(ui, "✕").clicked() {
                        s.file_manager.inline_preview_open = false;
                        s.file_manager.preview_file = None;
                        s.file_manager.preview_content = None;
                        s.file_manager.preview_texture = None;
                    }
                });
            });
        });

    if !s.file_manager.inline_preview_open { return; }

    // ── Content area ──────────────────────────────────────────────────────────
    let rf_opt = s.remote_files.iter().find(|f| f.name == fname).cloned();

    egui::Frame::none()
        .fill(Colors::BG)
        .stroke(Stroke::new(1.0, Colors::BORDER))
        .inner_margin(Margin::same(12.0))
        .show(ui, |ui| {
            if let Some(rf) = rf_opt {
                if rf.is_dir {
                    ui.label(RichText::new("// FOLDER — CLICK IN LIST TO OPEN")
                        .color(Colors::TEXT_DIM).size(9.0).italics());
                    return;
                }

                let ext_lower = std::path::Path::new(&rf.name).extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();

                let is_text = matches!(ext_lower.as_str(),
                    "txt"|"log"|"md"|"json"|"toml"|"yaml"|"yml"|"rs"|"py"|"js"|"ts"|"sh"
                    |"csv"|"xml"|"html"|"css"|"c"|"cpp"|"h"|"ini"|"cfg"|"conf"|"ps1"|"bat"|"iss"
                );
                let is_raster = matches!(ext_lower.as_str(),
                    "jpg"|"jpeg"|"png"|"gif"|"bmp"|"webp"|"tiff"|"tif"|"ico"
                );
                let is_svg = ext_lower == "svg";
                let is_psd = ext_lower == "psd";

                if is_text {
                    if let Some(bytes) = &s.file_manager.preview_content {
                        let content = String::from_utf8_lossy(bytes);
                        ScrollArea::vertical()
                            .id_source("big_preview_text")
                            .max_height(360.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Frame::none()
                                    .fill(Colors::BG_WIDGET)
                                    .stroke(Stroke::new(1.0, Colors::BORDER))
                                    .inner_margin(Margin::same(10.0))
                                    .show(ui, |ui| {
                                        ui.add(egui::Label::new(
                                            RichText::new(content.as_ref())
                                                .size(10.0)
                                                .color(Colors::TEXT)
                                                .monospace()
                                        ).wrap(false));
                                    });
                            });
                    } else {
                        preview_loading_indicator(ui, "TEXT");
                    }

                } else if is_raster {
                    if let Some(texture) = &s.file_manager.preview_texture {
                        let avail_w = ui.available_width();
                        ui.centered_and_justified(|ui| {
                            ui.add(egui::Image::from_texture(texture)
                                .max_width(avail_w - 24.0)
                                .maintain_aspect_ratio(true));
                        });
                    } else if let Some(bytes) = s.file_manager.preview_content.as_ref() {
                        match image::load_from_memory(bytes) {
                            Ok(img) => {
                                let size = [img.width() as usize, img.height() as usize];
                                let rgba = img.to_rgba8();
                                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                    size, rgba.as_flat_samples().as_slice()
                                );
                                s.file_manager.preview_texture = Some(
                                    ui.ctx().load_texture("fm_image_big", color_image, Default::default())
                                );
                                ui.ctx().request_repaint();
                            }
                            Err(_) => {
                                ui.label(RichText::new("// FAILED TO DECODE IMAGE")
                                    .color(Color32::RED).size(9.0).italics());
                            }
                        }
                    } else {
                        preview_loading_indicator(ui, "IMAGE");
                    }

                } else if is_svg {
                    if let Some(bytes) = &s.file_manager.preview_content {
                        let uri: std::borrow::Cow<'static, str> =
                            std::borrow::Cow::Owned(format!("bytes://fm_svg_big_{}", rf.name));
                        let src = egui::ImageSource::Bytes {
                            uri,
                            bytes: egui::load::Bytes::Shared(
                                std::sync::Arc::from(bytes.as_slice())
                            ),
                        };
                        let avail_w = ui.available_width();
                        ui.add(egui::Image::new(src)
                            .max_width(avail_w - 24.0)
                            .maintain_aspect_ratio(true));
                    } else {
                        preview_loading_indicator(ui, "SVG");
                    }

                } else if is_psd {
                    if let Some(texture) = &s.file_manager.preview_texture {
                        let avail_w = ui.available_width();
                        ui.centered_and_justified(|ui| {
                            ui.add(egui::Image::from_texture(texture)
                                .max_width(avail_w - 24.0)
                                .maintain_aspect_ratio(true));
                        });
                    } else if let Some(bytes) = s.file_manager.preview_content.as_ref() {
                        match psd::Psd::from_bytes(bytes) {
                            Ok(psd_doc) => {
                                let w = psd_doc.width() as usize;
                                let h = psd_doc.height() as usize;
                                let rgba = psd_doc.rgba();
                                let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                                s.file_manager.preview_texture = Some(
                                    ui.ctx().load_texture("fm_psd_big", color_image, Default::default())
                                );
                                ui.ctx().request_repaint();
                            }
                            Err(_) => {
                                preview_open_external_card(ui, "PSD PHOTOSHOP",
                                    "Could not decode PSD composite.",
                                    &rf.name, &s.file_manager.current_path, actions);
                            }
                        }
                    } else {
                        preview_loading_indicator(ui, "PSD");
                    }

                } else {
                    // Non-renderable format
                    let kind = match ext_lower.as_str() {
                        "ai"|"eps"  => "ADOBE ILLUSTRATOR / EPS",
                        "pdf"       => "PDF DOCUMENT",
                        "mp4"|"mkv"|"avi"|"mov"|"webm"|"flv" => "VIDEO FILE",
                        "mp3"|"flac"|"wav"|"ogg"|"m4a"|"aac" => "AUDIO FILE",
                        _           => "BINARY FILE",
                    };
                    let note = match ext_lower.as_str() {
                        "ai"|"eps"  => "Vector files cannot be rendered inline.",
                        "pdf"       => "Inline PDF rendering is not supported.",
                        "mp4"|"mkv"|"avi"|"mov"|"webm"|"flv" => "Video playback is not supported inline.",
                        "mp3"|"flac"|"wav"|"ogg"|"m4a"|"aac" => "Audio playback is not supported inline.",
                        _           => "No inline preview for this format.",
                    };
                    preview_open_external_card(ui, kind,
                        &format!("{}\nDownload to open locally.", note),
                        &rf.name, &s.file_manager.current_path, actions);
                }

                // Download button
                ui.add_space(10.0);
                if ui.button(RichText::new("↓ DOWNLOAD TO LOCAL").color(Colors::GREEN).size(9.0)).clicked() {
                    let mut p = s.file_manager.current_path.clone();
                    p.push(&rf.name);
                    actions.download_remote_file = Some(p);
                }
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

fn preview_loading_indicator(ui: &mut Ui, kind: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(RichText::new(format!("// LOADING {} PREVIEW...", kind))
            .color(Colors::TEXT_DIM).size(8.0).italics());
    });
}

/// Shows an info card for formats that can't be rendered inline,
/// with a "Download" shortcut action.
fn preview_open_external_card(
    ui:      &mut Ui,
    kind:    &str,
    note:    &str,
    fname:   &str,
    dir:     &std::path::Path,
    actions: &mut DetailActions,
) {
    egui::Frame::none()
        .fill(Colors::BG.gamma_multiply(0.6))
        .stroke(Stroke::new(1.0, Colors::BORDER))
        .inner_margin(Margin::same(8.0))
        .show(ui, |ui| {
            ui.label(RichText::new(format!("// {}", kind))
                .color(Colors::TEXT_DIM).size(8.0).strong());
            ui.add_space(4.0);
            for line in note.lines() {
                ui.label(RichText::new(line).color(Colors::TEXT_MUTED).size(8.0).italics());
            }
            ui.add_space(6.0);
            if theme::micro_button(ui, "↓ DOWNLOAD").clicked() {
                let mut p = dir.to_path_buf();
                p.push(fname);
                actions.download_remote_file = Some(p);
            }
        });
}
