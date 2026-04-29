// ═══════════════════════════════════════════════════════════════════════════════
// views/planner.rs — AI-Enhanced Planner & Workflow System
//
// Layout:
//   [Left column]  Project selector list
//   [Center]       Kanban board: TODO / IN PROGRESS / DONE / BLOCKED columns
//   [Right column] Task detail + add-task form + AI suggestions
// ═══════════════════════════════════════════════════════════════════════════════

use std::collections::HashMap;
use egui::{Color32, RichText, Ui, ScrollArea};
use thegrid_core::models::Project;

use crate::app::{PlannerTask, PlannerTaskStatus, ProjectStatus};
use crate::theme::{self, Colors, IconType};

// ─────────────────────────────────────────────────────────────────────────────
// AI suggestion templates keyed by project-name keywords
// ─────────────────────────────────────────────────────────────────────────────

fn ai_suggestions_for(project_name: &str) -> Vec<&'static str> {
    let n = project_name.to_lowercase();
    if n.contains("brand") || n.contains("logo") {
        vec![
            "Define brand color palette and typography",
            "Create logo variations (light/dark/icon)",
            "Write brand voice & tone guidelines",
            "Produce brand style guide PDF",
        ]
    } else if n.contains("web") || n.contains("site") {
        vec![
            "Wireframe key pages (Home, About, Contact)",
            "Set up repository and CI/CD pipeline",
            "Implement responsive layout skeleton",
            "Write SEO meta tags and sitemap",
            "Cross-browser QA pass",
        ]
    } else if n.contains("media") || n.contains("video") || n.contains("podcast") {
        vec![
            "Draft content calendar for next 4 weeks",
            "Record raw footage / audio sessions",
            "Edit and export master file",
            "Create thumbnails and artwork",
            "Schedule and publish across platforms",
        ]
    } else if n.contains("design") || n.contains("ui") || n.contains("ux") {
        vec![
            "Conduct user research interviews",
            "Create low-fidelity wireframes",
            "Build interactive prototype in Figma",
            "Run usability testing session",
            "Hand off specs to development",
        ]
    } else {
        vec![
            "Define project scope and deliverables",
            "Break work into milestones",
            "Assign owners for each task",
            "Set up progress tracking",
            "Schedule review checkpoints",
        ]
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Main render entry point
// ─────────────────────────────────────────────────────────────────────────────

pub fn render(
    ui:            &mut Ui,
    projects:      &[Project],
    selected:      &mut Option<String>,
    tasks:         &mut HashMap<String, Vec<PlannerTask>>,
    new_task:      &mut String,
    edit_idx:      &mut Option<(String, usize)>,
    statuses:      &HashMap<String, ProjectStatus>,
) {
    if projects.is_empty() {
        empty_state(ui);
        return;
    }

    // ── Header ────────────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(20.0, 10.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("// WORKFLOW PLANNER").color(Colors::GREEN).size(11.0).strong());

                if let Some(sel) = selected.as_ref() {
                    if let Some(p) = projects.iter().find(|p| &p.id == sel) {
                        ui.add_space(8.0);
                        ui.label(RichText::new("→").color(Colors::TEXT_MUTED).size(10.0));
                        ui.add_space(4.0);
                        ui.label(RichText::new(p.name.to_uppercase()).color(Colors::TEXT).size(10.0).strong());
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(sel_id) = selected.as_ref() {
                        let task_list = tasks.entry(sel_id.clone()).or_default();
                        let done  = task_list.iter().filter(|t| t.status == PlannerTaskStatus::Done).count();
                        let total = task_list.len();
                        if total > 0 {
                            let pct = (done as f32 / total as f32 * 100.0) as u32;
                            ui.label(RichText::new(format!("{done}/{total} TASKS ({pct}%)"))
                                .color(Colors::TEXT_DIM).size(9.0));
                        }
                    }
                });
            });
        });

    ui.add(egui::Separator::default().spacing(0.0));

    // ── Three-column layout: Project list | Kanban | Detail ───────────────────
    let avail = ui.available_size();

    // Clamp column widths
    let left_w  = 160.0f32;
    let right_w = 240.0f32;
    let center_w = (avail.x - left_w - right_w).max(320.0);

    // We use Strip for a precise horizontal split
    egui_extras::StripBuilder::new(ui)
        .size(egui_extras::Size::exact(left_w))
        .size(egui_extras::Size::exact(1.0))          // divider
        .size(egui_extras::Size::exact(center_w))
        .size(egui_extras::Size::exact(1.0))          // divider
        .size(egui_extras::Size::remainder())
        .horizontal(|mut strip| {
            // ── Left: Project list ─────────────────────────────────────────────
            strip.cell(|ui| {
                render_project_list(ui, projects, selected, tasks, statuses);
            });

            // ── Divider ───────────────────────────────────────────────────────
            strip.cell(|ui| {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    egui::Rounding::ZERO,
                    Colors::BORDER,
                );
            });

            // ── Center: Kanban board ───────────────────────────────────────────
            strip.cell(|ui| {
                if let Some(proj_id) = selected.clone() {
                    let task_list = tasks.entry(proj_id.clone()).or_default();
                    render_kanban(ui, task_list);
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.label(RichText::new("SELECT A PROJECT")
                            .color(Colors::TEXT_MUTED).size(10.0).strong());
                    });
                }
            });

            // ── Divider ───────────────────────────────────────────────────────
            strip.cell(|ui| {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    egui::Rounding::ZERO,
                    Colors::BORDER,
                );
            });

            // ── Right: Task detail + add task ──────────────────────────────────
            strip.cell(|ui| {
                if let Some(proj_id) = selected.clone() {
                    let proj_name = projects.iter().find(|p| p.id == proj_id)
                        .map(|p| p.name.as_str()).unwrap_or("");
                    render_task_panel(ui, &proj_id, proj_name, tasks, new_task, edit_idx);
                }
            });
        });
}

// ─────────────────────────────────────────────────────────────────────────────
// Left: project selector
// ─────────────────────────────────────────────────────────────────────────────

fn render_project_list(
    ui:       &mut Ui,
    projects: &[Project],
    selected: &mut Option<String>,
    tasks:    &HashMap<String, Vec<PlannerTask>>,
    statuses: &HashMap<String, ProjectStatus>,
) {
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(0.0, 0.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // Section header
            egui::Frame::none()
                .fill(Colors::BG_PANEL)
                .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("// PROJECTS").color(Colors::GREEN).size(9.0).strong());
                });

            ui.add(egui::Separator::default().spacing(0.0));

            ScrollArea::vertical()
                .id_source("planner_proj_list")
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());

                    for project in projects {
                        let proj_tasks = tasks.get(&project.id).map(|v| v.as_slice()).unwrap_or(&[]);
                        let total    = proj_tasks.len();
                        let done     = proj_tasks.iter().filter(|t| t.status == PlannerTaskStatus::Done).count();
                        let blocked  = proj_tasks.iter().any(|t| t.status == PlannerTaskStatus::Blocked);
                        let is_sel   = selected.as_deref() == Some(&project.id);

                        let eff_status = if let Some(s) = statuses.get(&project.id) {
                            s.clone()
                        } else if total == 0 { ProjectStatus::Planned }
                        else if proj_tasks.iter().all(|t| t.status == PlannerTaskStatus::Done) { ProjectStatus::Complete }
                        else if blocked { ProjectStatus::OnHold }
                        else { ProjectStatus::Active };

                        let bg = if is_sel { Colors::BG_ACTIVE } else { Color32::TRANSPARENT };
                        let resp = egui::Frame::none()
                            .fill(bg)
                            .inner_margin(egui::Margin { left: 12.0, right: 8.0, top: 7.0, bottom: 7.0 })
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    // Status dot
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                    ui.painter().circle_filled(r.center(), 3.0, eff_status.color());
                                    ui.add_space(6.0);

                                    ui.vertical(|ui| {
                                        let short = if project.name.len() > 16 {
                                            format!("{}…", &project.name[..16])
                                        } else {
                                            project.name.clone()
                                        };
                                        ui.label(
                                            RichText::new(short.to_uppercase())
                                                .color(if is_sel { Colors::GREEN } else { Colors::TEXT })
                                                .size(9.5).strong()
                                        );
                                        if total > 0 {
                                            ui.label(
                                                RichText::new(format!("{done}/{total}"))
                                                    .color(Colors::TEXT_MUTED).size(8.0)
                                            );
                                        }
                                    });
                                });
                            }).response;

                        if is_sel {
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    resp.rect.min,
                                    egui::vec2(2.0, resp.rect.height()),
                                ),
                                egui::Rounding::ZERO,
                                Colors::GREEN,
                            );
                        }

                        if ui.interact(resp.rect, egui::Id::new(("plan_proj", &project.id)), egui::Sense::click()).clicked() {
                            *selected = Some(project.id.clone());
                        }
                    }
                });
        });
}

// ─────────────────────────────────────────────────────────────────────────────
// Center: Kanban board
// ─────────────────────────────────────────────────────────────────────────────

fn render_kanban(ui: &mut Ui, tasks: &mut Vec<PlannerTask>) {
    let col_width = (ui.available_width() / 4.0).max(80.0);

    ScrollArea::horizontal()
        .id_source("kanban_h_scroll")
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for (col_status, col_label) in [
                    (PlannerTaskStatus::Todo,       "TODO"),
                    (PlannerTaskStatus::InProgress, "IN PROGRESS"),
                    (PlannerTaskStatus::Done,       "DONE"),
                    (PlannerTaskStatus::Blocked,    "BLOCKED"),
                ] {
                    let col_color = col_status.color();
                    let col_tasks: Vec<usize> = tasks
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| t.status == col_status)
                        .map(|(i, _)| i)
                        .collect();

                    egui::Frame::none()
                        .fill(Colors::BG_PANEL)
                        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
                        .inner_margin(egui::Margin::symmetric(0.0, 0.0))
                        .outer_margin(egui::Margin::symmetric(4.0, 4.0))
                        .show(ui, |ui| {
                            ui.set_width(col_width);

                            // Column header
                            egui::Frame::none()
                                .fill(Color32::from_rgba_premultiplied(
                                    col_color.r(), col_color.g(), col_color.b(), 18
                                ))
                                .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(col_label).color(col_color).size(9.0).strong()
                                        );
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(
                                                RichText::new(format!("{}", col_tasks.len()))
                                                    .color(Colors::TEXT_MUTED).size(9.0)
                                            );
                                        });
                                    });
                                });

                            ui.add(egui::Separator::default().spacing(0.0));

                            // Task cards in this column
                            ScrollArea::vertical()
                                .id_source(format!("kanban_{:?}", col_status))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.add_space(4.0);

                                    // Collect promotions/demotions to apply after render
                                    let mut advance: Option<usize> = None;
                                    let mut revert:  Option<usize> = None;

                                    for &task_idx in &col_tasks {
                                        if let Some(task) = tasks.get(task_idx) {
                                            let (adv, rev) = render_task_card(ui, task);
                                            if adv { advance = Some(task_idx); }
                                            if rev { revert  = Some(task_idx); }
                                        }
                                        ui.add_space(4.0);
                                    }

                                    // Apply status changes outside the loop
                                    if let Some(idx) = advance {
                                        if let Some(t) = tasks.get_mut(idx) {
                                            t.status = next_status(&t.status);
                                        }
                                    }
                                    if let Some(idx) = revert {
                                        if let Some(t) = tasks.get_mut(idx) {
                                            t.status = prev_status(&t.status);
                                        }
                                    }

                                    if col_tasks.is_empty() {
                                        ui.add_space(20.0);
                                        ui.vertical_centered(|ui| {
                                            ui.label(RichText::new("EMPTY").color(Colors::TEXT_MUTED).size(8.5));
                                        });
                                    }

                                    ui.add_space(8.0);
                                });
                        });
                }
            });
        });
}

fn next_status(s: &PlannerTaskStatus) -> PlannerTaskStatus {
    match s {
        PlannerTaskStatus::Todo       => PlannerTaskStatus::InProgress,
        PlannerTaskStatus::InProgress => PlannerTaskStatus::Done,
        PlannerTaskStatus::Done       => PlannerTaskStatus::Done,
        PlannerTaskStatus::Blocked    => PlannerTaskStatus::Todo,
    }
}

fn prev_status(s: &PlannerTaskStatus) -> PlannerTaskStatus {
    match s {
        PlannerTaskStatus::Todo       => PlannerTaskStatus::Todo,
        PlannerTaskStatus::InProgress => PlannerTaskStatus::Todo,
        PlannerTaskStatus::Done       => PlannerTaskStatus::InProgress,
        PlannerTaskStatus::Blocked    => PlannerTaskStatus::Todo,
    }
}

/// Returns (advance_clicked, revert_clicked)
fn render_task_card(ui: &mut Ui, task: &PlannerTask) -> (bool, bool) {
    let mut advance = false;
    let mut revert  = false;

    let is_ai = task.ai_suggested || task.assignee == "AI";
    let ai_color = if is_ai { Colors::STATE_COMPUTE_PROVIDE } else { Colors::TEXT_DIM };

    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .stroke(egui::Stroke::new(1.0, if is_ai { Color32::from_rgb(20, 40, 80) } else { Colors::BORDER }))
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
        .outer_margin(egui::Margin::symmetric(4.0, 0.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // Title row
            ui.horizontal(|ui| {
                if is_ai {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    theme::draw_vector_icon(ui, r, IconType::Ai, ai_color);
                    ui.add_space(4.0);
                }
                ui.label(RichText::new(&task.title).color(Colors::TEXT).size(9.5));
            });

            ui.add_space(4.0);

            // Assignee + advance buttons
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&task.assignee).color(ai_color).size(8.0).strong()
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if task.status != PlannerTaskStatus::Done {
                        if ui.add(
                            egui::Button::new(RichText::new("→").color(Colors::GREEN).size(9.0))
                                .fill(Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(1.0, Colors::BORDER))
                                .min_size(egui::vec2(18.0, 14.0))
                        ).on_hover_text("Advance status").clicked() {
                            advance = true;
                        }
                    }
                    if task.status != PlannerTaskStatus::Todo {
                        if ui.add(
                            egui::Button::new(RichText::new("←").color(Colors::TEXT_DIM).size(9.0))
                                .fill(Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(1.0, Colors::BORDER))
                                .min_size(egui::vec2(18.0, 14.0))
                        ).on_hover_text("Revert status").clicked() {
                            revert = true;
                        }
                    }
                });
            });

            // Dependencies
            if !task.depends_on.is_empty() {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("NEEDS: {}", task.depends_on.join(", ")))
                        .color(Colors::TEXT_MUTED).size(7.5)
                );
            }
        });

    (advance, revert)
}

// ─────────────────────────────────────────────────────────────────────────────
// Right: task detail panel + AI suggestions + add-task form
// ─────────────────────────────────────────────────────────────────────────────

fn render_task_panel(
    ui:        &mut Ui,
    proj_id:   &str,
    proj_name: &str,
    tasks:     &mut HashMap<String, Vec<PlannerTask>>,
    new_task:  &mut String,
    _edit_idx: &mut Option<(String, usize)>,
) {
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(0.0, 0.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // ── Add task form ──────────────────────────────────────────────────
            egui::Frame::none()
                .fill(Colors::BG_PANEL)
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("// ADD TASK").color(Colors::GREEN).size(9.0).strong());
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        let te = egui::TextEdit::singleline(new_task)
                            .hint_text("Task title...")
                            .font(egui::FontId::new(10.0, egui::FontFamily::Monospace))
                            .desired_width(ui.available_width() - 120.0)
                            .frame(true);
                        let te_resp = ui.add(te);
                        let enter   = te_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                        let add_human = ui.add(
                            egui::Button::new(RichText::new("+ HUMAN").color(Colors::TEXT_DIM).size(9.0))
                                .fill(Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(1.0, Colors::BORDER2))
                                .min_size(egui::vec2(0.0, 22.0))
                        );
                        let add_ai = ui.add(
                            egui::Button::new(RichText::new("+ AI").color(Colors::STATE_COMPUTE_PROVIDE).size(9.0))
                                .fill(Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(1.0, Color32::from_rgb(20, 40, 80)))
                                .min_size(egui::vec2(0.0, 22.0))
                        );

                        let title = new_task.trim().to_string();
                        if !title.is_empty() && (enter || add_human.clicked()) {
                            let id = format!("task-{}", chrono::Utc::now().timestamp_millis());
                            tasks.entry(proj_id.to_string()).or_default()
                                .push(PlannerTask::human(id, title.clone()));
                            new_task.clear();
                        } else if !title.is_empty() && add_ai.clicked() {
                            let id = format!("task-{}", chrono::Utc::now().timestamp_millis());
                            tasks.entry(proj_id.to_string()).or_default()
                                .push(PlannerTask::ai(id, title.clone()));
                            new_task.clear();
                        }
                    });
                });

            ui.add(egui::Separator::default().spacing(0.0));

            // ── AI suggestions ─────────────────────────────────────────────────
            egui::Frame::none()
                .fill(Colors::BG_PANEL)
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("// AI SUGGESTIONS").color(Colors::STATE_COMPUTE_PROVIDE).size(9.0).strong()
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Click + to add a suggested task").color(Colors::TEXT_MUTED).size(8.5)
                    );
                    ui.add_space(6.0);

                    for suggestion in ai_suggestions_for(proj_name) {
                        // Check if already in task list
                        let already = tasks.get(proj_id)
                            .map(|t| t.iter().any(|x| x.title == suggestion))
                            .unwrap_or(false);

                        ui.horizontal(|ui| {
                            let col = if already { Colors::TEXT_MUTED } else { Colors::TEXT_DIM };
                            if !already {
                                if theme::micro_button(ui, "+").clicked() {
                                    let id = format!("ai-{}", chrono::Utc::now().timestamp_millis());
                                    tasks.entry(proj_id.to_string()).or_default()
                                        .push(PlannerTask::ai(id, suggestion));
                                }
                            } else {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                                let c = r.center();
                                ui.painter().line_segment(
                                    [c + egui::vec2(-3.5, 0.0), c + egui::vec2(-0.5, 3.0)],
                                    egui::Stroke::new(1.2, Colors::GREEN),
                                );
                                ui.painter().line_segment(
                                    [c + egui::vec2(-0.5, 3.0), c + egui::vec2(3.5, -2.5)],
                                    egui::Stroke::new(1.2, Colors::GREEN),
                                );
                            }
                            ui.add_space(4.0);
                            ui.label(RichText::new(suggestion).color(col).size(9.0));
                        });
                        ui.add_space(2.0);
                    }
                });

            ui.add(egui::Separator::default().spacing(0.0));

            // ── Task detail (selected task) ────────────────────────────────────
            // Clone to avoid holding an immutable borrow while the closure below needs mutable access.
            let task_list: Vec<crate::app::PlannerTask> = tasks.get(proj_id).cloned().unwrap_or_default();
            let has_tasks = !task_list.is_empty();

            egui::Frame::none()
                .fill(Colors::BG_PANEL)
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("// TASK LIST").color(Colors::GREEN).size(9.0).strong());
                    ui.add_space(6.0);

                    if !has_tasks {
                        ui.label(RichText::new("No tasks yet. Add one above or pick an AI suggestion.")
                            .color(Colors::TEXT_MUTED).size(9.0));
                        return;
                    }

                    ScrollArea::vertical()
                        .id_source("planner_task_detail_list")
                        .max_height(220.0)
                        .show(ui, |ui| {
                            // Collect deletions to apply after iter
                            let mut to_delete: Option<usize> = None;

                            for (i, task) in task_list.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    let col = task.status.color();
                                    // Status mini-badge
                                    egui::Frame::none()
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::new(1.0, col))
                                        .inner_margin(egui::Margin::symmetric(3.0, 1.0))
                                        .show(ui, |ui| {
                                            let short = match task.status {
                                                PlannerTaskStatus::Todo       => "TODO",
                                                PlannerTaskStatus::InProgress => "WIP",
                                                PlannerTaskStatus::Done       => "DONE",
                                                PlannerTaskStatus::Blocked    => "BLK",
                                            };
                                            ui.label(RichText::new(short).color(col).size(7.5));
                                        });
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(&task.title).color(Colors::TEXT).size(9.0));

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.add(
                                            egui::Button::new(RichText::new("✕").color(Colors::RED).size(8.0))
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(egui::Stroke::NONE)
                                                .min_size(egui::vec2(16.0, 14.0))
                                        ).clicked() {
                                            to_delete = Some(i);
                                        }
                                    });
                                });
                                ui.add_space(2.0);
                            }

                            if let Some(idx) = to_delete {
                                if let Some(v) = tasks.get_mut(proj_id) {
                                    if idx < v.len() {
                                        v.remove(idx);
                                    }
                                }
                            }
                        });
                });
        });
}

// ─────────────────────────────────────────────────────────────────────────────
// Empty state
// ─────────────────────────────────────────────────────────────────────────────

fn empty_state(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(100.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
        theme::draw_vector_icon(ui, rect, IconType::Folder, Colors::TEXT_MUTED);
        ui.add_space(12.0);
        ui.label(RichText::new("NO PROJECTS").color(Colors::TEXT_MUTED).size(11.0).strong());
        ui.add_space(4.0);
        ui.label(RichText::new("Add projects in config.toml or settings to start planning")
            .color(Colors::TEXT_MUTED).size(9.0));
    });
}
