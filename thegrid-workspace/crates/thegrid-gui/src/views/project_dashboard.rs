// ═══════════════════════════════════════════════════════════════════════════════
// views/project_dashboard.rs — Projects Overview Dashboard
//
// Renders the main Projects screen with:
//   - Search / filter / sort toolbar
//   - Project cards: name, status badge, progress bar, tags, description
//   - Quick actions: open planner, pin to quick-view, set status
// ═══════════════════════════════════════════════════════════════════════════════

use std::collections::HashMap;
use egui::{Color32, RichText, Ui, ScrollArea};
use thegrid_core::models::Project;

use crate::app::{
    PlannerTask, PlannerTaskStatus, ProjectStatus, ProjectsSort,
};
use crate::theme::{self, Colors, IconType};

// ─────────────────────────────────────────────────────────────────────────────
// Action returned to app.rs
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ProjectDashboardAction {
    /// Navigate to Planner for this project
    pub open_planner:   Option<String>,
    /// Override status for a project
    pub set_status:     Option<(String, ProjectStatus)>,
    /// Pin project to quick-view slot
    pub pin_to_slot:    Option<usize>,
    pub pin_project_id: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Compute project progress from planner tasks
// ─────────────────────────────────────────────────────────────────────────────

fn compute_progress(tasks: &[PlannerTask]) -> f32 {
    if tasks.is_empty() { return 0.0; }
    let done = tasks.iter().filter(|t| t.status == PlannerTaskStatus::Done).count();
    done as f32 / tasks.len() as f32
}

fn infer_status<'a>(
    proj_id: &str,
    overrides: &'a HashMap<String, ProjectStatus>,
    tasks: &[PlannerTask],
) -> &'a ProjectStatus {
    if let Some(s) = overrides.get(proj_id) {
        return s;
    }
    // Static fallback — we can't return a temporary, but the caller uses a
    // local variable branch below, so this function only returns from `overrides`.
    let _ = tasks; // suppress unused-var warning
    // Return a const ref via a static
    static PLANNED: ProjectStatus  = ProjectStatus::Planned;
    static ACTIVE: ProjectStatus   = ProjectStatus::Active;
    static COMPLETE: ProjectStatus = ProjectStatus::Complete;
    static BLOCKED: ProjectStatus  = ProjectStatus::OnHold;

    if tasks.is_empty() {
        return &PLANNED;
    }
    if tasks.iter().all(|t| t.status == PlannerTaskStatus::Done) {
        return &COMPLETE;
    }
    if tasks.iter().any(|t| t.status == PlannerTaskStatus::Blocked) {
        return &BLOCKED;
    }
    if tasks.iter().any(|t| t.status == PlannerTaskStatus::InProgress) {
        return &ACTIVE;
    }
    &PLANNED
}

// ─────────────────────────────────────────────────────────────────────────────
// Main render entry point
// ─────────────────────────────────────────────────────────────────────────────

pub fn render(
    ui: &mut Ui,
    projects: &[Project],
    filter:   &mut String,
    sort:     &mut ProjectsSort,
    statuses: &mut HashMap<String, ProjectStatus>,
    tasks:    &HashMap<String, Vec<PlannerTask>>,
) -> ProjectDashboardAction {
    let mut action = ProjectDashboardAction::default();

    // ── Header ────────────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(20.0, 12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("// PROJECTS DASHBOARD")
                        .color(Colors::GREEN).size(11.0).strong()
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Sort selector
                    egui::ComboBox::from_id_source("proj_sort")
                        .width(100.0)
                        .selected_text(
                            RichText::new(match sort {
                                ProjectsSort::Name     => "SORT: NAME",
                                ProjectsSort::Status   => "SORT: STATUS",
                                ProjectsSort::Progress => "SORT: PROGRESS",
                            }).color(Colors::TEXT_DIM).size(9.0)
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(sort, ProjectsSort::Name,     "NAME");
                            ui.selectable_value(sort, ProjectsSort::Status,   "STATUS");
                            ui.selectable_value(sort, ProjectsSort::Progress, "PROGRESS");
                        });
                    ui.add_space(8.0);
                });
            });

            ui.add_space(8.0);

            // Stats bar
            let total     = projects.len();
            let active    = projects.iter().filter(|p| {
                matches!(statuses.get(&p.id), Some(ProjectStatus::Active))
                    || (!statuses.contains_key(&p.id)
                        && tasks.get(&p.id).map(|t| !t.is_empty() && !t.iter().all(|x| x.status == PlannerTaskStatus::Done)).unwrap_or(false))
            }).count();
            let complete  = projects.iter().filter(|p| {
                matches!(statuses.get(&p.id), Some(ProjectStatus::Complete))
                    || (!statuses.contains_key(&p.id)
                        && tasks.get(&p.id).map(|t| !t.is_empty() && t.iter().all(|x| x.status == PlannerTaskStatus::Done)).unwrap_or(false))
            }).count();

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                stat_chip(ui, &format!("{total}"), "TOTAL");
                stat_chip(ui, &format!("{active}"), "ACTIVE");
                stat_chip(ui, &format!("{complete}"), "COMPLETE");
                stat_chip(ui, &format!("{}", total - active - complete), "PLANNED");
            });
        });

    ui.add(egui::Separator::default().spacing(0.0));

    // ── Search bar ────────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(20.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                let c = rect.center() - egui::vec2(1.0, 1.0);
                ui.painter().circle_stroke(c, 3.5, egui::Stroke::new(1.0, Colors::TEXT_MUTED));
                ui.painter().line_segment(
                    [c + egui::vec2(2.5, 2.5), c + egui::vec2(5.0, 5.0)],
                    egui::Stroke::new(1.2, Colors::TEXT_MUTED),
                );
                ui.add(
                    egui::TextEdit::singleline(filter)
                        .hint_text("SEARCH PROJECTS...")
                        .font(egui::FontId::new(10.0, egui::FontFamily::Monospace))
                        .desired_width(f32::INFINITY)
                        .frame(false),
                );
            });
        });

    ui.add(egui::Separator::default().spacing(0.0));
    ui.add_space(4.0);

    // ── Build sorted / filtered list ──────────────────────────────────────────
    let filter_lower = filter.to_lowercase();
    let mut sorted: Vec<&Project> = projects
        .iter()
        .filter(|p| {
            filter_lower.is_empty()
                || p.name.to_lowercase().contains(&filter_lower)
                || p.description.to_lowercase().contains(&filter_lower)
                || p.tags.iter().any(|t| t.to_lowercase().contains(&filter_lower))
        })
        .collect();

    match sort {
        ProjectsSort::Name => sorted.sort_by(|a, b| a.name.cmp(&b.name)),
        ProjectsSort::Status => sorted.sort_by_key(|p| {
            match statuses.get(&p.id) {
                Some(ProjectStatus::Active)   => 0,
                Some(ProjectStatus::OnHold)   => 1,
                Some(ProjectStatus::Planned)  => 2,
                Some(ProjectStatus::Complete) => 3,
                None => 2,
            }
        }),
        ProjectsSort::Progress => sorted.sort_by(|a, b| {
            let pa = tasks.get(&a.id).map(|t| compute_progress(t)).unwrap_or(0.0);
            let pb = tasks.get(&b.id).map(|t| compute_progress(t)).unwrap_or(0.0);
            pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
        }),
    }

    // ── Project cards ─────────────────────────────────────────────────────────
    ScrollArea::vertical()
        .id_source("proj_list_scroll")
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            if sorted.is_empty() {
                ui.add_space(60.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("NO PROJECTS")
                            .color(Colors::TEXT_MUTED).size(10.0).strong()
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Add projects in config.toml or via settings")
                            .color(Colors::TEXT_MUTED).size(9.0)
                    );
                });
                return;
            }

            ui.add_space(4.0);

            for project in sorted {
                let proj_tasks = tasks.get(&project.id).map(|v| v.as_slice()).unwrap_or(&[]);
                let progress   = compute_progress(proj_tasks);

                // Determine effective status (check overrides map first, then infer)
                let eff_status: ProjectStatus = if let Some(s) = statuses.get(&project.id) {
                    s.clone()
                } else if proj_tasks.is_empty() {
                    ProjectStatus::Planned
                } else if proj_tasks.iter().all(|t| t.status == PlannerTaskStatus::Done) {
                    ProjectStatus::Complete
                } else if proj_tasks.iter().any(|t| t.status == PlannerTaskStatus::Blocked) {
                    ProjectStatus::OnHold
                } else {
                    ProjectStatus::Active
                };

                let card_response = render_project_card(
                    ui,
                    project,
                    &eff_status,
                    progress,
                    proj_tasks.len(),
                    proj_tasks.iter().filter(|t| t.status == PlannerTaskStatus::Done).count(),
                );

                if card_response.open_planner {
                    action.open_planner = Some(project.id.clone());
                }
                if let Some(new_status) = card_response.set_status {
                    action.set_status = Some((project.id.clone(), new_status));
                    statuses.insert(project.id.clone(), action.set_status.as_ref().unwrap().1.clone());
                }
                if card_response.pin_slot_0 { action.pin_to_slot = Some(0); action.pin_project_id = Some(project.id.clone()); }
                if card_response.pin_slot_1 { action.pin_to_slot = Some(1); action.pin_project_id = Some(project.id.clone()); }
                if card_response.pin_slot_2 { action.pin_to_slot = Some(2); action.pin_project_id = Some(project.id.clone()); }
                if card_response.pin_slot_3 { action.pin_to_slot = Some(3); action.pin_project_id = Some(project.id.clone()); }

                ui.add_space(4.0);
            }
        });

    action
}

// ─────────────────────────────────────────────────────────────────────────────
// Project card
// ─────────────────────────────────────────────────────────────────────────────

struct CardResponse {
    open_planner: bool,
    set_status:   Option<ProjectStatus>,
    pin_slot_0:   bool,
    pin_slot_1:   bool,
    pin_slot_2:   bool,
    pin_slot_3:   bool,
}

fn render_project_card(
    ui: &mut Ui,
    project:    &Project,
    status:     &ProjectStatus,
    progress:   f32,
    total_tasks: usize,
    done_tasks:  usize,
) -> CardResponse {
    let mut resp = CardResponse {
        open_planner: false,
        set_status:   None,
        pin_slot_0:   false,
        pin_slot_1:   false,
        pin_slot_2:   false,
        pin_slot_3:   false,
    };

    let avail_w = ui.available_width();

    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .stroke(egui::Stroke::new(1.0, Colors::BORDER2))
        .inner_margin(egui::Margin::symmetric(16.0, 12.0))
        .outer_margin(egui::Margin { left: 12.0, right: 12.0, top: 0.0, bottom: 4.0 })
        .show(ui, |ui| {
            // ── Row 1: Name + status + actions ────────────────────────────────
            ui.horizontal(|ui| {
                // Project icon
                let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                theme::draw_vector_icon(ui, icon_rect, IconType::Folder, Colors::GREEN_DIM);

                ui.add_space(8.0);

                ui.label(
                    RichText::new(project.name.to_uppercase())
                        .color(Colors::TEXT)
                        .size(11.5)
                        .strong()
                );

                ui.add_space(6.0);

                // Status badge
                egui::Frame::none()
                    .fill(Color32::TRANSPARENT)
                    .stroke(egui::Stroke::new(1.0, status.color()))
                    .inner_margin(egui::Margin::symmetric(5.0, 2.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(status.label()).color(status.color()).size(8.0).strong());
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Status toggle buttons
                    ui.spacing_mut().item_spacing.x = 4.0;

                    if theme::micro_button(ui, "PLAN").clicked() {
                        resp.open_planner = true;
                    }

                    egui::menu::menu_button(ui, RichText::new("PIN").color(Colors::TEXT_DIM).size(9.0), |ui| {
                        ui.set_min_width(100.0);
                        if ui.selectable_label(false, "SLOT 1").clicked() { resp.pin_slot_0 = true; }
                        if ui.selectable_label(false, "SLOT 2").clicked() { resp.pin_slot_1 = true; }
                        if ui.selectable_label(false, "SLOT 3").clicked() { resp.pin_slot_2 = true; }
                        if ui.selectable_label(false, "SLOT 4").clicked() { resp.pin_slot_3 = true; }
                    });

                    egui::menu::menu_button(ui, RichText::new("STATUS").color(Colors::TEXT_DIM).size(9.0), |ui| {
                        ui.set_min_width(100.0);
                        for s in [ProjectStatus::Active, ProjectStatus::Planned, ProjectStatus::OnHold, ProjectStatus::Complete] {
                            let lbl = s.label();
                            let col = s.color();
                            if ui.add(egui::Button::new(RichText::new(lbl).color(col).size(9.0)).fill(Color32::TRANSPARENT)).clicked() {
                                resp.set_status = Some(s);
                                ui.close_menu();
                            }
                        }
                    });
                });
            });

            ui.add_space(6.0);

            // ── Row 2: Progress bar ────────────────────────────────────────────
            let bar_w = avail_w - 32.0;
            let bar_rect = ui.allocate_rect(
                egui::Rect::from_min_size(ui.cursor().min, egui::vec2(bar_w, 4.0)),
                egui::Sense::hover()
            ).rect;
            ui.painter().rect_filled(bar_rect, egui::Rounding::ZERO, Colors::BORDER2);
            if progress > 0.0 {
                let fill_rect = egui::Rect::from_min_size(
                    bar_rect.min,
                    egui::vec2(bar_rect.width() * progress, bar_rect.height()),
                );
                ui.painter().rect_filled(fill_rect, egui::Rounding::ZERO, status.color());
            }

            ui.add_space(6.0);

            // ── Row 3: Metadata row ────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;

                // Progress %
                let pct = (progress * 100.0) as u32;
                ui.label(RichText::new(format!("{pct}%")).color(Colors::TEXT_DIM).size(9.0).strong());

                // Task count
                if total_tasks > 0 {
                    ui.label(RichText::new(format!("{done_tasks}/{total_tasks} TASKS")).color(Colors::TEXT_MUTED).size(9.0));
                } else {
                    ui.label(RichText::new("NO TASKS").color(Colors::TEXT_MUTED).size(9.0));
                }

                // Tags
                for tag in &project.tags {
                    egui::Frame::none()
                        .fill(Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(1.0, Colors::BORDER2))
                        .inner_margin(egui::Margin::symmetric(4.0, 1.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(tag.to_uppercase()).color(Colors::TEXT_MUTED).size(8.0));
                        });
                }
            });

            // ── Row 4: Description ─────────────────────────────────────────────
            if !project.description.is_empty() {
                ui.add_space(4.0);
                let desc = if project.description.len() > 120 {
                    format!("{}…", &project.description[..120])
                } else {
                    project.description.clone()
                };
                ui.label(RichText::new(desc).color(Colors::TEXT_MUTED).size(9.0));
            }
        });

    resp
}

// ─────────────────────────────────────────────────────────────────────────────
// Stat chip helper
// ─────────────────────────────────────────────────────────────────────────────

fn stat_chip(ui: &mut Ui, value: &str, label: &str) {
    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .stroke(egui::Stroke::new(1.0, Colors::BORDER2))
        .inner_margin(egui::Margin::symmetric(10.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(value).color(Colors::GREEN).size(12.0).strong());
                ui.label(RichText::new(label).color(Colors::TEXT_DIM).size(8.0));
            });
        });
}
