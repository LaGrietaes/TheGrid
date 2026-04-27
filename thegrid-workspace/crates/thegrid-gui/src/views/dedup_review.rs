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

// ── Sort field ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupSortField {
    /// Wasted bytes = (copies - 1) × size_per_copy
    Waste,
    Name,
    Type,
    Size,
    Date,
}

impl Default for DedupSortField {
    fn default() -> Self { DedupSortField::Waste }
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
    // ── Filters ──────────────────────────────────────────────────────────────
    /// Filename / path substring filter
    pub filter_name:        String,
    /// Comma-separated extension filter, e.g. "rs, py, png"
    pub filter_type:        String,
    /// Minimum file size (KB text input)
    pub filter_min_size_kb: String,
    /// Maximum file size (KB text input)
    pub filter_max_size_kb: String,
    /// Modified-after date "YYYY-MM-DD"
    pub filter_date_after:  String,
    /// Modified-before date "YYYY-MM-DD"
    pub filter_date_before: String,
    // ── Sort ─────────────────────────────────────────────────────────────────
    pub sort_field: DedupSortField,
    pub sort_asc:   bool,
}

impl Default for DedupReviewState {
    fn default() -> Self {
        Self {
            expanded:   HashSet::new(),
            actions:    HashMap::new(),
            confirming: false,
            scanning:   false,
            last_scan:  None,
            filter_name:        String::new(),
            filter_type:        String::new(),
            filter_min_size_kb: String::new(),
            filter_max_size_kb: String::new(),
            filter_date_after:  String::new(),
            filter_date_before: String::new(),
            sort_field: DedupSortField::Waste,
            sort_asc:   false,  // biggest waste first by default
        }
    }
}

impl DedupReviewState {
    /// Seed actions from suggested_anchor for any file that does not already have a
    /// Keep or Delete decision.  Existing decisions are preserved so that re-scanning
    /// does not overwrite what the user has already marked.
    pub fn seed_from_groups(&mut self, groups: &[DuplicateGroup]) {
        for g in groups {
            for file in &g.files {
                let already_decided = matches!(
                    self.actions.get(&file.id),
                    Some(FileAction::Keep) | Some(FileAction::Delete)
                );
                if !already_decided {
                    let is_anchor = g.suggested_anchor.as_deref() == Some(&file.device_id);
                    self.actions.insert(
                        file.id,
                        if is_anchor { FileAction::Keep } else { FileAction::Delete },
                    );
                }
            }
        }
    }

    /// Apply stored actions loaded from the DB (overrides in-memory state).
    /// Called when restoring persisted groups on startup.
    pub fn apply_stored_actions(&mut self, stored: &std::collections::HashMap<i64, String>) {
        for (&file_id, action_str) in stored {
            let action = match action_str.as_str() {
                "keep"   => FileAction::Keep,
                "delete" => FileAction::Delete,
                _        => FileAction::Undecided,
            };
            self.actions.insert(file_id, action);
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

    // ── Filter strip ──────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(Colors::BG_PANEL.gamma_multiply(0.85))
        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
        .inner_margin(egui::Margin::symmetric(10.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                // ── WASTE ────────────────────────────────────────────────────
                let waste_active = state.sort_field == DedupSortField::Waste;
                let waste_arrow  = if waste_active { if state.sort_asc { " ▲" } else { " ▼" } } else { " ·" };
                let waste_lbl    = RichText::new(format!("WASTE{}", waste_arrow))
                    .size(7.5).strong()
                    .color(if waste_active { Colors::AMBER } else { Colors::TEXT_DIM });
                if ui.add(egui::Label::new(waste_lbl).sense(egui::Sense::click())).clicked() {
                    if waste_active { state.sort_asc = !state.sort_asc; }
                    else { state.sort_field = DedupSortField::Waste; state.sort_asc = false; }
                }

                ui.separator();

                // ── NAME ─────────────────────────────────────────────────────
                let name_active = state.sort_field == DedupSortField::Name;
                let name_arrow  = if name_active { if state.sort_asc { " ▲" } else { " ▼" } } else { " ·" };
                let name_lbl    = RichText::new(format!("NAME{}", name_arrow))
                    .size(7.5).strong()
                    .color(if name_active { Colors::AMBER } else { Colors::TEXT_DIM });
                if ui.add(egui::Label::new(name_lbl).sense(egui::Sense::click())).clicked() {
                    if name_active { state.sort_asc = !state.sort_asc; }
                    else { state.sort_field = DedupSortField::Name; state.sort_asc = true; }
                }
                ui.add(egui::TextEdit::singleline(&mut state.filter_name)
                    .desired_width(90.0)
                    .hint_text("path / filename…")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));

                ui.separator();

                // ── TYPE ─────────────────────────────────────────────────────
                let type_active = state.sort_field == DedupSortField::Type;
                let type_arrow  = if type_active { if state.sort_asc { " ▲" } else { " ▼" } } else { " ·" };
                let type_lbl    = RichText::new(format!("TYPE{}", type_arrow))
                    .size(7.5).strong()
                    .color(if type_active { Colors::AMBER } else { Colors::TEXT_DIM });
                if ui.add(egui::Label::new(type_lbl).sense(egui::Sense::click())).clicked() {
                    if type_active { state.sort_asc = !state.sort_asc; }
                    else { state.sort_field = DedupSortField::Type; state.sort_asc = true; }
                }
                ui.add(egui::TextEdit::singleline(&mut state.filter_type)
                    .desired_width(70.0)
                    .hint_text("rs, py, png…")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));

                ui.separator();

                // ── SIZE ─────────────────────────────────────────────────────
                let size_active = state.sort_field == DedupSortField::Size;
                let size_arrow  = if size_active { if state.sort_asc { " ▲" } else { " ▼" } } else { " ·" };
                let size_lbl    = RichText::new(format!("SIZE{}", size_arrow))
                    .size(7.5).strong()
                    .color(if size_active { Colors::AMBER } else { Colors::TEXT_DIM });
                if ui.add(egui::Label::new(size_lbl).sense(egui::Sense::click())).clicked() {
                    if size_active { state.sort_asc = !state.sort_asc; }
                    else { state.sort_field = DedupSortField::Size; state.sort_asc = true; }
                }
                ui.add(egui::TextEdit::singleline(&mut state.filter_min_size_kb)
                    .desired_width(50.0)
                    .hint_text("min KB")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));
                ui.label(RichText::new("–").color(Colors::TEXT_DIM).size(8.0));
                ui.add(egui::TextEdit::singleline(&mut state.filter_max_size_kb)
                    .desired_width(50.0)
                    .hint_text("max KB")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));

                ui.separator();

                // ── DATE ─────────────────────────────────────────────────────
                let date_active = state.sort_field == DedupSortField::Date;
                let date_arrow  = if date_active { if state.sort_asc { " ▲" } else { " ▼" } } else { " ·" };
                let date_lbl    = RichText::new(format!("DATE{}", date_arrow))
                    .size(7.5).strong()
                    .color(if date_active { Colors::AMBER } else { Colors::TEXT_DIM });
                if ui.add(egui::Label::new(date_lbl).sense(egui::Sense::click())).clicked() {
                    if date_active { state.sort_asc = !state.sort_asc; }
                    else { state.sort_field = DedupSortField::Date; state.sort_asc = true; }
                }
                ui.add(egui::TextEdit::singleline(&mut state.filter_date_after)
                    .desired_width(72.0)
                    .hint_text("YYYY-MM-DD")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));
                ui.label(RichText::new("→").color(Colors::TEXT_DIM).size(8.0));
                ui.add(egui::TextEdit::singleline(&mut state.filter_date_before)
                    .desired_width(72.0)
                    .hint_text("YYYY-MM-DD")
                    .font(egui::FontId::new(8.5, egui::FontFamily::Monospace)));

                let any_active = !state.filter_name.is_empty()
                    || !state.filter_type.is_empty()
                    || !state.filter_min_size_kb.is_empty()
                    || !state.filter_max_size_kb.is_empty()
                    || !state.filter_date_after.is_empty()
                    || !state.filter_date_before.is_empty();
                if any_active {
                    ui.add_space(4.0);
                    if ui.button(RichText::new("✕ CLEAR").color(Colors::RED).size(8.0)).clicked() {
                        state.filter_name.clear();
                        state.filter_type.clear();
                        state.filter_min_size_kb.clear();
                        state.filter_max_size_kb.clear();
                        state.filter_date_after.clear();
                        state.filter_date_before.clear();
                    }
                }
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

    // ── Parse filter values once ──────────────────────────────────────────────
    let f_name  = state.filter_name.to_lowercase();
    let f_exts: Vec<String> = state.filter_type
        .split(',').map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty()).collect();
    let f_min_bytes: Option<u64> = state.filter_min_size_kb
        .trim().parse::<u64>().ok().map(|kb| kb * 1024);
    let f_max_bytes: Option<u64> = state.filter_max_size_kb
        .trim().parse::<u64>().ok().map(|kb| kb * 1024);
    let f_after = chrono::NaiveDate::parse_from_str(
        state.filter_date_after.trim(), "%Y-%m-%d").ok()
        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp());
    let f_before = chrono::NaiveDate::parse_from_str(
        state.filter_date_before.trim(), "%Y-%m-%d").ok()
        .map(|d| d.and_hms_opt(23, 59, 59).unwrap().and_utc().timestamp());

    let any_filter = !f_name.is_empty() || !f_exts.is_empty()
        || f_min_bytes.is_some() || f_max_bytes.is_some()
        || f_after.is_some() || f_before.is_some();

    // Filter groups: a group passes if at least one of its files matches all
    // active criteria.
    let filtered_groups: Vec<&DuplicateGroup> = groups.iter().filter(|g| {
        if !any_filter { return true; }
        g.files.iter().any(|file| {
            let path_lower = file.path.to_string_lossy().to_lowercase();
            // Name filter — match against full path
            if !f_name.is_empty() && !path_lower.contains(&f_name) { return false; }
            // Type / extension filter
            if !f_exts.is_empty() {
                let ext = file.path.extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if !f_exts.iter().any(|e| e == &ext) { return false; }
            }
            // Size filter (group.size is the per-copy size)
            if let Some(min) = f_min_bytes { if g.size < min { return false; } }
            if let Some(max) = f_max_bytes { if g.size > max { return false; } }
            // Date filter — match against file.modified (Option<i64> unix ts)
            if let Some(after)  = f_after  {
                match file.modified { Some(m) if m >= after  => {}, _ => { return false; } }
            }
            if let Some(before) = f_before {
                match file.modified { Some(m) if m <= before => {}, _ => { return false; } }
            }
            true
        })
    }).collect();

    // ── Sort filtered groups ──────────────────────────────────────────────────
    let sort_field = state.sort_field;
    let sort_asc   = state.sort_asc;
    let mut filtered_groups = filtered_groups;
    filtered_groups.sort_by(|a, b| {
        let ord = match sort_field {
            DedupSortField::Waste => {
                let wa = (a.file_count.saturating_sub(1) as u64) * a.size;
                let wb = (b.file_count.saturating_sub(1) as u64) * b.size;
                wa.cmp(&wb)
            }
            DedupSortField::Size  => a.size.cmp(&b.size),
            DedupSortField::Name  => {
                let na = a.files.first().map(|f| f.path.to_string_lossy().to_lowercase()).unwrap_or_default();
                let nb = b.files.first().map(|f| f.path.to_string_lossy().to_lowercase()).unwrap_or_default();
                na.cmp(&nb)
            }
            DedupSortField::Type  => {
                let ea = a.files.first().and_then(|f| f.path.extension())
                    .map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let eb = b.files.first().and_then(|f| f.path.extension())
                    .map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                ea.cmp(&eb)
            }
            DedupSortField::Date  => {
                let da = a.files.first().and_then(|f| f.modified).unwrap_or(0);
                let db = b.files.first().and_then(|f| f.modified).unwrap_or(0);
                da.cmp(&db)
            }
        };
        if sort_asc { ord } else { ord.reverse() }
    });


    ScrollArea::vertical()
        .id_source("dedup_review_scroll")
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add_space(4.0);

            if filtered_groups.is_empty() && any_filter {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(RichText::new("NO GROUPS MATCH CURRENT FILTERS")
                        .color(Colors::TEXT_DIM).size(9.0).italics());
                });
            } else {
                let visible = filtered_groups.len();
                let total   = groups.len();
                if any_filter {
                    ui.label(RichText::new(
                        format!("showing {} of {} groups", visible, total))
                        .color(Colors::TEXT_DIM).size(8.0).italics());
                    ui.add_space(4.0);
                }
                for group in filtered_groups {
                    render_group(ui, group, state, local_device);
                    ui.add_space(2.0);
                }
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
