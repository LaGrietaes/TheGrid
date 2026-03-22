// ═══════════════════════════════════════════════════════════════════════════════
// views/timeline.rs — Temporal View: "The Flow"
//
// Answers: "I was working on something 2 hours ago — where is it?"
//
// Renders a vertical timeline of recently modified files across ALL watched
// devices, sorted newest-first. Each entry shows:
//   - Event kind glyph (⊕ created · ⊙ modified · ⊘ deleted)
//   - File name + extension icon
//   - Device name + relative path
//   - Human-relative timestamp ("2 hours ago", "Yesterday", "3 days ago")
//   - File size
//
// Triggered: clicking the TIMELINE button in the main tab bar.
// Data: loaded via AppEvent::TemporalLoaded, refreshed every time the tab is
// selected or a FileSystemChanged event arrives.
// ═══════════════════════════════════════════════════════════════════════════════

use egui::{Color32, RichText, ScrollArea, Ui};
use chrono::TimeZone;
use thegrid_core::models::{TemporalEntry, TemporalEventKind};
use crate::theme::Colors;

// ─────────────────────────────────────────────────────────────────────────────
// TimelineState — stored in TheGridApp
// ─────────────────────────────────────────────────────────────────────────────

pub struct TimelineState {
    pub entries:      Vec<TemporalEntry>,
    pub loading:      bool,
    pub last_refresh: Option<std::time::Instant>,
    /// Minimum time between auto-refreshes
    pub refresh_interval: std::time::Duration,
    /// Filter text
    pub filter: String,
    /// Device filter (None = all devices)
    pub device_filter: Option<String>,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            entries:          Vec::new(),
            loading:          false,
            last_refresh:     None,
            refresh_interval: std::time::Duration::from_secs(30),
            filter:           String::new(),
            device_filter:    None,
        }
    }
}

impl TimelineState {
    /// Returns true if we should trigger a data refresh.
    pub fn needs_refresh(&self) -> bool {
        match self.last_refresh {
            None    => true,
            Some(t) => t.elapsed() > self.refresh_interval,
        }
    }
    pub fn mark_refreshed(&mut self) {
        self.last_refresh = Some(std::time::Instant::now());
        self.loading = false;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TimelineAction — returned from render()
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct TimelineAction {
    /// User clicked a file entry — navigate to owning device
    pub open_entry: Option<TemporalEntry>,
    /// User wants to manually refresh the timeline
    pub refresh: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// render()
// ─────────────────────────────────────────────────────────────────────────────

pub fn render(ui: &mut Ui, s: &mut TimelineState) -> TimelineAction {
    let mut action = TimelineAction::default();

    // ── Header bar ────────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("// THE FLOW")
                .color(Colors::GREEN).size(9.0).strong()
        );
        ui.add_space(8.0);

        // Filter input
        ui.add(
            egui::TextEdit::singleline(&mut s.filter)
                .hint_text("FILTER...")
                .font(egui::FontId::new(10.0, egui::FontFamily::Monospace))
                .desired_width(180.0)
                .frame(false)
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::theme::micro_button(ui, "REFRESH").clicked() {
                action.refresh = true;
            }
            if s.loading {
                ui.add_space(4.0);
                ui.spinner();
            }
            if let Some(t) = s.last_refresh {
                let secs = t.elapsed().as_secs();
                ui.label(
                    RichText::new(format!("{}s ago", secs))
                        .color(Colors::TEXT_MUTED).size(8.0)
                );
            }
        });
    });

    ui.add_space(12.0);

    // ── Empty / loading states ────────────────────────────────────────────────
    if s.loading && s.entries.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.spinner();
            ui.add_space(8.0);
            ui.label(
                RichText::new("LOADING TEMPORAL INDEX...")
                    .color(Colors::TEXT_MUTED).size(10.0)
            );
        });
        return action;
    }

    if s.entries.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(RichText::new("⊙").color(Colors::BORDER).size(40.0));
            ui.add_space(12.0);
            ui.label(
                RichText::new("NO FILE ACTIVITY YET")
                    .color(Colors::TEXT_MUTED).size(11.0)
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new("Add watch directories in the ACTIONS tab to start tracking")
                    .color(Colors::TEXT_MUTED).size(9.0)
            );
        });
        return action;
    }

    // ── Timeline entries ──────────────────────────────────────────────────────
    let filter_lower = s.filter.to_lowercase();
    let now = chrono::Utc::now().timestamp();

    // Group entries by day for date separators
    let mut last_day: Option<i64> = None;

    ScrollArea::vertical()
        .id_source("timeline_scroll")
        .show(ui, |ui: &mut Ui| {
            for entry in &s.entries {
                // Apply filter
                let matches = filter_lower.is_empty()
                    || entry.name.to_lowercase().contains(&filter_lower)
                    || entry.device_name.to_lowercase().contains(&filter_lower)
                    || entry.path.to_string_lossy().to_lowercase().contains(&filter_lower);

                if !matches { continue; }

                // Date separator
                let entry_day = entry.modified / 86400;  // floor to day
                if last_day != Some(entry_day) {
                    let first_separator = last_day.is_none();
                    last_day = Some(entry_day);
                    if let Some(dt) = chrono::Utc.timestamp_opt(entry.modified, 0).single() {
                        let label = relative_day_label(entry.modified, now);
                        ui.add_space(if first_separator { 0.0 } else { 12.0 });
                        render_day_separator(ui, &label, &dt.format("%A, %B %-d").to_string());
                        ui.add_space(4.0);
                    }
                }

                // Entry row
                let resp = render_entry_row(ui, entry, now);
                if resp.clicked() {
                    action.open_entry = Some(entry.clone());
                }
            }
        });

    action
}

// ─────────────────────────────────────────────────────────────────────────────
// Row renderers
// ─────────────────────────────────────────────────────────────────────────────

fn render_day_separator(ui: &mut Ui, rel_label: &str, full_date: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(rel_label)
                .color(Colors::AMBER).size(9.0).strong()
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(full_date)
                .color(Colors::TEXT_MUTED).size(8.0)
        );
        // Horizontal rule fill
        ui.add(egui::Separator::default().spacing(4.0).horizontal());
    });
}

fn render_entry_row(ui: &mut Ui, e: &TemporalEntry, now: i64) -> egui::Response {
    let (kind_glyph, kind_color) = match e.event_kind {
        TemporalEventKind::Created  => (crate::icons::Glyphs::CREATED, Colors::GREEN),
        TemporalEventKind::Modified => (crate::icons::Glyphs::MODIFIED, Colors::GREEN),
        TemporalEventKind::Deleted  => (crate::icons::Glyphs::DELETED, Colors::RED),
    };

    let ext_color = crate::icons::ext_to_color(e.ext.as_deref());

    let resp = egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Timeline stem + event dot
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(kind_glyph)
                            .color(kind_color).size(12.0)
                    );
                });

                ui.add_space(10.0);

                // File info
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&e.name)
                                .color(Colors::TEXT).size(11.0).strong()
                        );
                        if let Some(ext) = &e.ext {
                            ui.label(
                                RichText::new(ext.to_uppercase())
                                    .color(ext_color).size(8.0)
                            );
                        }
                    });
                    ui.label(
                        RichText::new(format!(
                            "{}  ›  {}",
                            e.device_name.to_uppercase(),
                            e.path.parent()
                                .and_then(|p| p.file_name())
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default()
                        ))
                        .color(Colors::TEXT_DIM).size(8.0)
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(relative_time(e.modified, now))
                            .color(Colors::TEXT_MUTED).size(8.0)
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(crate::telemetry::fmt_bytes(e.size))
                            .color(Colors::TEXT_DIM).size(8.0)
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(e.event_kind.label())
                            .color(kind_color).size(8.0).strong()
                    );
                });
            });
        }).response;

    let interact = ui.interact(
        resp.rect,
        egui::Id::new(("timeline_row", e.file_id)),
        egui::Sense::click(),
    );
    if interact.hovered() {
        ui.painter().rect_filled(
            resp.rect, egui::Rounding::ZERO,
            Color32::from_rgba_premultiplied(255, 255, 255, 4),
        );
    }
    ui.add(egui::Separator::default().spacing(0.0));
    interact
}

// ─────────────────────────────────────────────────────────────────────────────
// Time formatting helpers
// ─────────────────────────────────────────────────────────────────────────────

fn relative_time(ts: i64, now: i64) -> String {
    let age = now - ts;
    if age < 60           { "just now".into() }
    else if age < 3600    { format!("{}m ago", age / 60) }
    else if age < 86400   { format!("{}h ago", age / 3600) }
    else if age < 86400*7 { format!("{}d ago", age / 86400) }
    else {
        chrono::Utc.timestamp_opt(ts, 0)
            .single()
            .map(|dt| dt.format("%b %-d").to_string())
            .unwrap_or_else(|| "—".into())
    }
}

fn relative_day_label(ts: i64, now: i64) -> String {
    let age_days = (now - ts) / 86400;
    match age_days {
        0 => "TODAY".into(),
        1 => "YESTERDAY".into(),
        n => format!("{} DAYS AGO", n),
    }
}

// ext_accent_color removed — now using crate::icons
