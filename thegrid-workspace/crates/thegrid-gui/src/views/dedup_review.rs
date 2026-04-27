/// Dedup Review Panel — Phase 6
///
/// Renders rich `DuplicateGroup` data collected from the cross-source dedup
/// query.  Each group shows all copies (local + Drive + NAS) with per-file
/// action toggles (Keep / Delete).  On "Execute" it fires `AppEvent::DeleteFiles`
/// for remote devices and uses `std::fs::remove_file` (via a background thread)
/// for local files.  Every deletion is audited via `db.log_deletion()`.
use std::collections::{HashMap, HashSet};

use egui::{Color32, RichText, ScrollArea, Ui};

use thegrid_core::models::{DuplicateGroup, FileSearchResult, SourceType};

use crate::theme::{Colors, danger_button, micro_button, primary_button, section_title};

// ── Action per file ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAction {
    Keep,
    Delete,
    Undecided,
}

// ── Review state ───────────────────────────────────────────────────────────────

pub struct DedupReviewState {
    /// Expanded group hashes
    pub expanded:    HashSet<String>,
    /// Per-file (id) action
    pub actions:     HashMap<i64, FileAction>,
    /// Confirmation modal visible
    pub confirming:  bool,
    /// Scan in progress
    pub scanning:    bool,
    /// Last scan timestamp
    pub last_scan:   Option<std::time::Instant>,
}

impl Default for DedupReviewState {
    fn default() -> Self {
        Self {
            expanded:   HashSet::new(),
            actions:    HashMap::new(),
            confirming: false,
            scanning:   false,
            last_scan:  None,
        }
    }
}

impl DedupReviewState {
    /// Auto-seed actions from suggested_anchor: anchor copies → Keep, others → Delete.
    pub fn seed_from_groups(&mut self, groups: &[DuplicateGroup]) {
        self.actions.clear();
        for g in groups {
            for file in &g.files {
                let is_anchor = g.suggested_anchor.as_deref() == Some(&file.device_id);
                self.actions.insert(
                    file.id,
                    if is_anchor { FileAction::Keep } else { FileAction::Delete },
                );
            }
        }
    }

    pub fn pending_deletes<'a>(&self, groups: &'a [DuplicateGroup]) -> Vec<&'a FileSearchResult> {
        groups.iter().flat_map(|g| &g.files)
            .filter(|f| self.actions.get(&f.id) == Some(&FileAction::Delete))
            .collect()
    }
}

// ── Render ─────────────────────────────────────────────────────────────────────

/// Returns `Some(files_to_delete)` when the user clicks Execute.
/// Sets `*scan_requested = true` when the user clicks SCAN.
pub fn render_dedup_review(
    ui:            &mut Ui,
    groups:        &[DuplicateGroup],
    state:         &mut DedupReviewState,
    local_device:  &str,
    scan_requested: &mut bool,
) -> Option<Vec<FileSearchResult>> {
    let mut execute_result: Option<Vec<FileSearchResult>> = None;

    // ── Header bar ────────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(Colors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(16.0, 12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                section_title(ui, "// DUPLICATE REVIEW");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let pending = state.pending_deletes(groups).len();
                    if pending > 0 {
                        let lbl = format!("EXECUTE ({} files)", pending);
                        if danger_button(ui, &lbl).clicked() {
                            state.confirming = true;
                        }
                    }
                    if micro_button(ui, "AUTO-MARK").clicked() {
                        state.seed_from_groups(groups);
                    }
                    if micro_button(ui, "EXPAND ALL").clicked() {
                        for g in groups {
                            state.expanded.insert(g.hash.clone());
                        }
                    }
                    if micro_button(ui, "COLLAPSE").clicked() {
                        state.expanded.clear();
                    }
                    let scan_label = if state.scanning { "SCANNING…" } else { "SCAN" };
                    if micro_button(ui, scan_label).clicked() && !state.scanning {
                        state.scanning = true;
                        *scan_requested = true;
                    }
                    ui.label(
                        RichText::new(format!("{} groups", groups.len()))
                            .color(Colors::TEXT_DIM).size(9.0)
                    );
                });
            });
        });

    ui.add(egui::Separator::default().spacing(0.0));

    if groups.is_empty() {
        egui::Frame::none()
            .fill(Colors::BG_PANEL)
            .inner_margin(egui::Margin::symmetric(16.0, 24.0))
            .show(ui, |ui| {
                ui.label(RichText::new("No duplicate groups found.").color(Colors::TEXT_DIM).size(11.0));
            });
        return None;
    }

    // ── Group list ────────────────────────────────────────────────────────────
    ScrollArea::vertical()
        .id_source("dedup_review_scroll")
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add_space(4.0);

            for group in groups {
                render_group(ui, group, state, local_device);
                ui.add_space(2.0);
            }
        });

    // ── Confirmation modal ────────────────────────────────────────────────────
    if state.confirming {
        let mut do_execute = false;
        let mut cancel = false;

        egui::Window::new("CONFIRM DELETION")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                let pending = state.pending_deletes(groups);
                ui.label(
                    RichText::new(format!(
                        "You are about to permanently delete {} file(s).\nThis action cannot be undone.",
                        pending.len()
                    ))
                    .color(Colors::AMBER)
                    .size(11.0)
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if danger_button(ui, "DELETE").clicked() {
                        do_execute = true;
                    }
                    ui.add_space(8.0);
                    if primary_button(ui, "CANCEL").clicked() {
                        cancel = true;
                    }
                });
            });

        if do_execute {
            let to_delete: Vec<FileSearchResult> = state
                .pending_deletes(groups)
                .into_iter()
                .cloned()
                .collect();
            state.confirming = false;
            execute_result = Some(to_delete);
        }
        if cancel {
            state.confirming = false;
        }
    }

    execute_result
}

// ── Group card ────────────────────────────────────────────────────────────────

fn render_group(
    ui:           &mut Ui,
    group:        &DuplicateGroup,
    state:        &mut DedupReviewState,
    local_device: &str,
) {
    let is_expanded = state.expanded.contains(&group.hash);
    let size_mb = group.size as f64 / 1_048_576.0;

    // Wasted = (copies - 1) × size
    let copies = group.file_count.max(1) as u64;
    let wasted_mb = (copies - 1) as f64 * size_mb;

    let header_color = if wasted_mb > 100.0 { Colors::RED }
                       else if wasted_mb > 10.0 { Colors::AMBER }
                       else { Colors::TEXT };

    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // Group header row
            ui.horizontal(|ui| {
                // Expand/collapse toggle
                let toggle = if is_expanded { "▼" } else { "▶" };
                if ui.button(RichText::new(toggle).color(Colors::GREEN_DIM).size(9.0)).clicked() {
                    if is_expanded {
                        state.expanded.remove(&group.hash);
                    } else {
                        state.expanded.insert(group.hash.clone());
                    }
                }

                ui.label(
                    RichText::new(format!("{}×  {:>8.2} MB  →  waste {:>6.1} MB",
                        copies, size_mb, wasted_mb))
                        .color(header_color)
                        .size(10.0)
                        .strong()
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Source summary badges
                    for src in &group.sources {
                        let label = match src.source_type {
                            SourceType::Local       => format!("LOCAL({})", src.file_count),
                            SourceType::GoogleDrive => format!("DRIVE({})", src.file_count),
                            SourceType::Nas         => format!("NAS({})", src.file_count),
                        };
                        let color = match src.source_type {
                            SourceType::Local       => Colors::GREEN_DIM,
                            SourceType::GoogleDrive => Color32::from_rgb(66, 133, 244),
                            SourceType::Nas         => Colors::AMBER,
                        };
                        ui.label(RichText::new(label).color(color).size(8.0));
                        ui.add_space(4.0);
                    }
                });
            });

            // Expanded file list
            if is_expanded {
                ui.add_space(6.0);
                for file in &group.files {
                    render_file_row(ui, file, group, state, local_device);
                }
            }
        });
}

// ── File row ──────────────────────────────────────────────────────────────────

fn render_file_row(
    ui:           &mut Ui,
    file:         &FileSearchResult,
    group:        &DuplicateGroup,
    state:        &mut DedupReviewState,
    _local_device: &str,
) {
    let action = state.actions.get(&file.id).cloned().unwrap_or(FileAction::Undecided);
    let is_anchor = group.suggested_anchor.as_deref() == Some(&file.device_id);

    let row_bg = match action {
        FileAction::Keep     => Color32::from_rgba_premultiplied(0, 40, 10, 200),
        FileAction::Delete   => Color32::from_rgba_premultiplied(60, 0, 0, 200),
        FileAction::Undecided => Color32::TRANSPARENT,
    };

    egui::Frame::none()
        .fill(row_bg)
        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Action toggle
                let keep_color   = if action == FileAction::Keep   { Colors::GREEN } else { Colors::TEXT_MUTED };
                let delete_color = if action == FileAction::Delete { Colors::RED }   else { Colors::TEXT_MUTED };

                if ui.button(RichText::new("KEEP").color(keep_color).size(8.0)).clicked() {
                    state.actions.insert(file.id, FileAction::Keep);
                }
                if ui.button(RichText::new("DEL").color(delete_color).size(8.0)).clicked() {
                    state.actions.insert(file.id, FileAction::Delete);
                }

                ui.add_space(4.0);

                // Anchor badge
                if is_anchor {
                    ui.label(RichText::new("★").color(Colors::AMBER).size(9.0));
                    ui.add_space(2.0);
                }

                // Path (truncated)
                let path_str = file.path.to_string_lossy();
                let display = if path_str.len() > 70 {
                    format!("…{}", &path_str[path_str.len().saturating_sub(67)..])
                } else {
                    path_str.into_owned()
                };

                ui.label(
                    RichText::new(&display)
                        .color(Colors::TEXT_DIM)
                        .size(9.0)
                        .monospace()
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Device label
                    ui.label(
                        RichText::new(&file.device_id)
                            .color(Colors::TEXT_MUTED)
                            .size(8.0)
                    );
                });
            });
        });
}
