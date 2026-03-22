// ═══════════════════════════════════════════════════════════════════════════════
// views/file_manager.rs — Brutalist HUD File Explorer
// ═══════════════════════════════════════════════════════════════════════════════

use egui::{Color32, RichText, Ui, ScrollArea, Stroke, Margin, Pos2, Shape};

use crate::theme::Colors;
use crate::views::dashboard::{DetailState, DetailActions};
use crate::app::FileViewMode;

pub fn render(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    ui.vertical(|ui| {
        // ── HUD Header ────────────────────────────────────────────────────────
        render_hud_header(ui, s, actions);
        
        ui.add_space(8.0);

        // ── Main Content Area ─────────────────────────────────────────────────
        egui::Frame::none()
            .fill(Colors::BG_WIDGET)
            .stroke(Stroke::new(1.0, Colors::BORDER))
            .inner_margin(Margin::same(0.0))
            .show(ui, |ui| {
                // Background Static/Scanline Overlay (Subtle)
                // render_scanlines(ui);

                ui.vertical(|ui| {
                    // Toolbar
                    render_toolbar(ui, s, actions);
                    
                    ui.add(egui::Separator::default().spacing(0.0));

                    // File List/Grid
                    if s.file_manager.view_mode == FileViewMode::Grid {
                        render_grid_view(ui, s, actions);
                    } else {
                        render_list_view(ui, s, actions);
                    }
                });
            });
    });
}

fn render_hud_header(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    let (rect, _response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 32.0),
        egui::Sense::hover()
    );

    let painter = ui.painter();
    
    // Angled background shape
    let points = vec![
        rect.left_top(),
        rect.right_top() + egui::vec2(-10.0, 0.0),
        rect.right_top() + egui::vec2(0.0, 10.0),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    painter.add(Shape::convex_polygon(points, Colors::BG_PANEL, Stroke::new(1.0, Colors::BORDER)));

    // Accent line
    painter.line_segment(
        [rect.left_top() + egui::vec2(2.0, 2.0), rect.left_top() + egui::vec2(30.0, 2.0)],
        Stroke::new(2.0, Colors::GREEN)
    );

    // Breadcrumbs
    ui.put(rect.shrink2(egui::vec2(8.0, 4.0)), |ui: &mut Ui| {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            if ui.button(RichText::new("ᐊ").color(Colors::GREEN).strong()).clicked() {
                let parent = s.file_manager.current_path.parent().unwrap_or(std::path::Path::new(""));
                actions.browse_remote = Some(parent.to_path_buf());
                s.file_manager.current_path = parent.to_path_buf();
            }
            ui.add_space(4.0);
            ui.label(RichText::new("NAV.").color(Colors::TEXT_DIM).size(9.0).strong());
            ui.add_space(4.0);
            
            let path_str = s.file_manager.current_path.display().to_string();
            ui.label(RichText::new(path_str.to_uppercase()).color(Colors::GREEN).size(10.0).strong());
        }).response
    });
}

fn render_toolbar(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    ui.horizontal(|ui| {
        ui.set_height(28.0);
        ui.add_space(8.0);

        // Selection Actions
        if !s.file_manager.selected_files.is_empty() {
             ui.label(RichText::new(format!("[{}] SELECTED", s.file_manager.selected_files.len())).color(Colors::AMBER).size(9.0).strong());
             if ui.button(RichText::new("DELETE").color(Colors::RED).size(9.0).strong()).clicked() {
                 let paths: Vec<String> = s.file_manager.selected_files.iter().cloned().collect();
                 actions.fm_delete = Some(paths);
                 s.file_manager.selected_files.clear();
             }
        } else {
             ui.label(RichText::new("READY").color(Colors::TEXT_MUTED).size(9.0).strong());
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            // View Mode Toggle
            if s.file_manager.view_mode == FileViewMode::List {
                if ui.button("GRID").clicked() { s.file_manager.view_mode = FileViewMode::Grid; }
            } else {
                if ui.button("LIST").clicked() { s.file_manager.view_mode = FileViewMode::List; }
            }
            ui.add_space(8.0);
            if ui.button("REFRESH").clicked() { actions.scan_remote = true; }
        });
    });
}

fn render_list_view(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    ScrollArea::vertical()
        .id_source("fm_list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if s.remote_files.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new("VACUUM DETECTED").color(Colors::TEXT_MUTED).size(12.0).strong());
                    ui.label(RichText::new("INITIALIZE SCAN TO DISCOVER DATA").color(Colors::TEXT_MUTED).size(9.0));
                });
            }

            for rf in s.remote_files {
                let is_selected = s.file_manager.selected_files.contains(&rf.name);
                let bg = if is_selected { Colors::AMBER.gamma_multiply(0.1) } else { Color32::TRANSPARENT };

                egui::Frame::none()
                    .fill(bg)
                    .inner_margin(Margin::symmetric(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let icon = if rf.is_dir { "📁" } else { "📄" };
                            ui.label(RichText::new(icon).color(Colors::TEXT_MUTED).size(11.0));
                            ui.add_space(4.0);

                            let label_color = if rf.is_dir { Colors::GREEN } else { Colors::TEXT };
                            let resp = ui.add(egui::Label::new(RichText::new(&rf.name).color(label_color).size(11.0)).sense(egui::Sense::click()));
                            
                            if resp.clicked() {
                                if ui.input(|i| i.modifiers.ctrl) {
                                    if is_selected { s.file_manager.selected_files.remove(&rf.name); }
                                    else { s.file_manager.selected_files.insert(rf.name.clone()); }
                                } else if rf.is_dir {
                                    let mut new_path = s.file_manager.current_path.clone();
                                    new_path.push(&rf.name);
                                    actions.browse_remote = Some(new_path.clone());
                                    s.file_manager.current_path = new_path;
                                    s.file_manager.selected_files.clear();
                                } else {
                                    // Select single file
                                    s.file_manager.selected_files.clear();
                                    s.file_manager.selected_files.insert(rf.name.clone());
                                }
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if !rf.is_dir {
                                    ui.label(RichText::new(crate::views::dashboard::fmt_bytes(rf.size)).color(Colors::TEXT_DIM).size(9.0));
                                    if ui.button("↓").clicked() {
                                        let mut p = s.file_manager.current_path.clone();
                                        p.push(&rf.name);
                                        actions.download_remote_file = Some(p);
                                    }
                                }
                            });
                        });
                    });
                ui.add(egui::Separator::default().spacing(0.0).grow(0.0));
            }
        });
}

fn render_grid_view(ui: &mut Ui, s: &mut DetailState, actions: &mut DetailActions) {
    ScrollArea::vertical()
        .id_source("fm_grid")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(8.0);
            let width = ui.available_width();
            let cols = (width / 80.0) as usize;
            let cols = cols.max(1);

            egui::Grid::new("fm_grid_inner")
                .spacing(egui::vec2(8.0, 8.0))
                .show(ui, |ui| {
                    let mut count = 0;
                    for rf in s.remote_files {
                        let is_selected = s.file_manager.selected_files.contains(&rf.name);
                        
                        ui.vertical_centered(|ui| {
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(70.0, 70.0), egui::Sense::click());
                            
                            let painter = ui.painter();
                            let bg = if is_selected { Colors::AMBER.gamma_multiply(0.2) } 
                                     else if resp.hovered() { Colors::BORDER.gamma_multiply(0.3) }
                                     else { Colors::BG_WIDGET };
                            
                            painter.rect_filled(rect, 2.0, bg);
                            if is_selected {
                                painter.rect_stroke(rect, 2.0, Stroke::new(1.0, Colors::AMBER));
                            } else {
                                painter.rect_stroke(rect, 2.0, Stroke::new(1.0, Colors::BORDER));
                            }

                            // Icon
                            let icon = if rf.is_dir { "📁" } else { "📄" };
                            painter.text(
                                rect.center() + egui::vec2(0.0, -8.0),
                                egui::Align2::CENTER_CENTER,
                                icon,
                                egui::FontId::proportional(24.0),
                                Colors::TEXT_MUTED
                            );

                            // Label (truncated)
                            let mut display_name = rf.name.clone();
                            if display_name.len() > 10 { display_name.truncate(8); display_name.push_str(".."); }
                            painter.text(
                                rect.center() + egui::vec2(0.0, 18.0),
                                egui::Align2::CENTER_CENTER,
                                display_name,
                                egui::FontId::proportional(9.0),
                                if rf.is_dir { Colors::GREEN } else { Colors::TEXT }
                            );

                            if resp.clicked() {
                                if ui.input(|i| i.modifiers.ctrl) {
                                    if is_selected { s.file_manager.selected_files.remove(&rf.name); }
                                    else { s.file_manager.selected_files.insert(rf.name.clone()); }
                                } else if rf.is_dir {
                                    let mut new_path = s.file_manager.current_path.clone();
                                    new_path.push(&rf.name);
                                    actions.browse_remote = Some(new_path.clone());
                                    s.file_manager.current_path = new_path;
                                    s.file_manager.selected_files.clear();
                                } else {
                                    s.file_manager.selected_files.clear();
                                    s.file_manager.selected_files.insert(rf.name.clone());
                                }
                            }
                        });

                        count += 1;
                        if count % cols == 0 { ui.end_row(); }
                    }
                });
        });
}

fn render_scanlines(ui: &mut Ui) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    
    let color = Color32::from_rgba_premultiplied(0, 255, 65, 3); // Very subtle green (alpha 3)
    let spacing = 8.0; // Increased spacing
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, color)
        );
        y += spacing;
    }
}
