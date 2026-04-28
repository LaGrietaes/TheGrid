/// Media Ingest / Culling View — THE GRID
///
/// Keyboard shortcuts (when a cell is selected):
///   1-5     — star rating
///   P       — pick
///   X       — reject
///   U       — unmark (none)
///   R/G/Y/B — color labels (red/green/yellow/blue)
///   Del     — clear label
///   ←/→/↑/↓ — navigate cells
///   Enter   — open preview

use std::collections::HashMap;
use egui::{Color32, Key, RichText, ScrollArea, Ui, Vec2};
use thegrid_core::{AppEvent, models::FileSearchResult};
use crate::theme::Colors;

// ─── Pick flag constants ──────────────────────────────────────────────────────

const PICK_NONE:   &str = "none";
const PICK_PICK:   &str = "pick";
const PICK_REJECT: &str = "reject";

fn label_color(label: &str) -> Color32 {
    match label {
        "red"    => Color32::from_rgb(220, 50,  50),
        "green"  => Color32::from_rgb(50,  200, 80),
        "yellow" => Color32::from_rgb(220, 180, 30),
        "blue"   => Color32::from_rgb(60,  130, 230),
        "purple" => Color32::from_rgb(160, 60,  200),
        _        => Color32::GRAY,
    }
}

// ─── Sort order ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaSortField {
    #[default]
    Date,
    Name,
    Size,
    Rating,
}

impl MediaSortField {
    fn label(self) -> &'static str {
        match self {
            Self::Date   => "Date",
            Self::Name   => "Name",
            Self::Size   => "Size",
            Self::Rating => "Rating",
        }
    }
}

// ─── Per-file review overlay (optimistic UI) ──────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct FileReviewState {
    pub rating:      Option<u8>,
    pub pick_flag:   String,
    pub color_label: Option<String>,
}

// ─── View state ───────────────────────────────────────────────────────────────

pub struct MediaIngestState {
    pub query:               String,
    pub pending_query:       String,
    pub last_searched:       Option<std::time::Instant>,
    pub debounce_ms:         u64,
    pub results:             Vec<FileSearchResult>,
    pub loading:             bool,
    pub sort:                MediaSortField,
    pub sort_desc:           bool,
    pub selected_idx:        Option<usize>,
    /// Optimistic review overlay — keyed by file_id
    pub review:              HashMap<i64, FileReviewState>,
    pub thumb_size:          f32,
    pub show_filter_help:    bool,
    pub filter_only_picks:   bool,
    pub filter_only_unrated: bool,
    pub filter_min_rating:   u8,
    pub filter_min_quality:  f32,
    pub filter_only_geotagged: bool,
    pub filter_in_focus:     bool,
}

impl Default for MediaIngestState {
    fn default() -> Self {
        Self {
            query:               String::new(),
            pending_query:       String::new(),
            last_searched:       None,
            debounce_ms:         300,
            results:             Vec::new(),
            loading:             false,
            sort:                MediaSortField::Date,
            sort_desc:           true,
            selected_idx:        None,
            review:              HashMap::new(),
            thumb_size:          120.0,
            show_filter_help:    false,
            filter_only_picks:   false,
            filter_only_unrated: false,
            filter_min_rating:   0,
            filter_min_quality:  0.0,
            filter_only_geotagged: false,
            filter_in_focus:     false,
        }
    }
}

// ─── Actions returned to app.rs ───────────────────────────────────────────────

pub struct MediaIngestActions {
    pub events:         Vec<AppEvent>,
    pub trigger_search: bool,
    pub open_preview:   Option<FileSearchResult>,
}

impl Default for MediaIngestActions {
    fn default() -> Self {
        Self { events: Vec::new(), trigger_search: false, open_preview: None }
    }
}

// ─── Main render function ─────────────────────────────────────────────────────

pub fn render_media_ingest(ui: &mut Ui, state: &mut MediaIngestState) -> MediaIngestActions {
    let mut actions = MediaIngestActions::default();

    // ── Top toolbar ──────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label(RichText::new("MEDIA INGEST").strong().color(Colors::GREEN));
        ui.separator();

        let resp = ui.add(
            egui::TextEdit::singleline(&mut state.pending_query)
                .hint_text("Search… focus:in  iso<=3200  camera:r5  rating>=3  pick:none")
                .desired_width(400.0),
        );
        if resp.changed() {
            state.last_searched = Some(std::time::Instant::now());
        }
        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            state.query = build_effective_query(state);
            actions.trigger_search = true;
            state.loading = true;
        }

        if let Some(t) = state.last_searched {
            if t.elapsed().as_millis() > state.debounce_ms as u128 {
                state.last_searched = None;
                state.query = build_effective_query(state);
                actions.trigger_search = true;
                state.loading = true;
            }
        }

        if ui.add(
            egui::Button::new(RichText::new("Search").color(Colors::GREEN).size(10.0))
                .fill(Colors::BG_WIDGET)
                .stroke(egui::Stroke::new(1.0, Colors::GREEN_DIM))
        ).clicked() {
            state.query = build_effective_query(state);
            actions.trigger_search = true;
            state.loading = true;
        }

        ui.separator();

        egui::ComboBox::from_id_source("media_sort")
            .selected_text(state.sort.label())
            .show_ui(ui, |ui| {
                for s in [MediaSortField::Date, MediaSortField::Name, MediaSortField::Size, MediaSortField::Rating] {
                    ui.selectable_value(&mut state.sort, s, s.label());
                }
            });

        let sort_lbl = if state.sort_desc { "▼" } else { "▲" };
        if ui.add(
            egui::Button::new(RichText::new(sort_lbl).size(10.0))
                .fill(Colors::BG_WIDGET)
                .min_size(Vec2::new(24.0, 0.0))
        ).clicked() {
            state.sort_desc = !state.sort_desc;
        }

        ui.separator();
        ui.label(RichText::new("Size").color(Colors::TEXT_DIM).size(10.0));
        ui.add(egui::Slider::new(&mut state.thumb_size, 64.0..=240.0).show_value(false));
        ui.separator();
        ui.toggle_value(&mut state.show_filter_help, "?");
    });

    // ── Quick filter chips ────────────────────────────────────────────────────
    ui.horizontal_wrapped(|ui| {
        toggle_chip(ui, "📍 GPS", &mut state.filter_only_geotagged);
        toggle_chip(ui, "🎯 Focus", &mut state.filter_in_focus);
        toggle_chip(ui, "✓ Picks", &mut state.filter_only_picks);
        toggle_chip(ui, "★ Unrated", &mut state.filter_only_unrated);

        ui.label(RichText::new("Min ★").color(Colors::TEXT_DIM).size(9.0));
        for n in 0u8..=5 {
            let label_str = if n == 0 { "off".to_string() } else { "★".repeat(n as usize) };
            if ui.selectable_label(state.filter_min_rating == n, RichText::new(label_str).size(10.0)).clicked() {
                state.filter_min_rating = n;
            }
        }

        ui.label(RichText::new("Quality≥").color(Colors::TEXT_DIM).size(9.0));
        for (lbl, val) in [("off", 0.0f32), ("0.3", 0.3), ("0.5", 0.5), ("0.7", 0.7)] {
            if ui.selectable_label((state.filter_min_quality - val).abs() < 0.01, RichText::new(lbl).size(10.0)).clicked() {
                state.filter_min_quality = val;
            }
        }
    });

    // ── Filter help ───────────────────────────────────────────────────────────
    if state.show_filter_help {
        egui::Frame::none()
            .fill(Color32::from_black_alpha(180))
            .inner_margin(8.0)
            .rounding(6.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Search filter syntax").strong().color(Colors::TEXT));
                ui.separator();
                for (token, desc) in [
                    ("focus:in / focus:out",                "Sharp or blurry images"),
                    ("quality>=0.7",                        "Quality composite ≥ 0.7"),
                    ("mp>=12",                              "Megapixels ≥ 12"),
                    ("camera:r5",                           "Camera model contains r5"),
                    ("lens:85mm",                           "Lens contains 85mm"),
                    ("iso>=400 iso<=3200",                  "ISO range"),
                    ("aperture>=1.4 aperture<=4.0",         "Aperture range"),
                    ("focal>=50 focal<=200",                "Focal length range"),
                    ("captured>=2024-01-01",                "Capture date range"),
                    ("gps:true",                            "Has GPS coordinates"),
                    ("rating>=3",                           "Rating ≥ 3 stars"),
                    ("pick:keep / pick:reject / pick:none", "Pick status"),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(token).monospace().color(Colors::GREEN_DIM).small());
                        ui.label(RichText::new(desc).small().color(Colors::TEXT_DIM));
                    });
                }
            });
    }

    ui.separator();

    // ── Result count / status bar ─────────────────────────────────────────────
    ui.horizontal(|ui| {
        if state.loading {
            ui.spinner();
            ui.label(RichText::new("Searching…").color(Colors::TEXT_DIM).small());
        } else {
            ui.label(RichText::new(format!("{} files", state.results.len())).color(Colors::TEXT_DIM).small());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !state.results.is_empty() {
                let (mut np, mut nr, mut nrat) = (0usize, 0usize, 0usize);
                for r in &state.results {
                    let rev = state.review.get(&r.id);
                    match rev.map(|rv| rv.pick_flag.as_str()).unwrap_or("none") {
                        "pick"   => np   += 1,
                        "reject" => nr   += 1,
                        _        => {}
                    }
                    if rev.and_then(|rv| rv.rating).is_some() { nrat += 1; }
                }
                ui.label(RichText::new(format!("✓ {np}  ✗ {nr}  ★ {nrat}")).color(Colors::TEXT_DIM).small());
            }
        });
    });

    // ── Sort results ─────────────────────────────────────────────────────────
    sort_results(&mut state.results, state.sort, state.sort_desc, &state.review);

    // ── Keyboard navigation ───────────────────────────────────────────────────
    let approx_cols = ((ui.available_width() / (state.thumb_size + 10.0)) as usize).max(1);
    handle_keyboard(ui, state, &mut actions, approx_cols);

    // ── Grid ─────────────────────────────────────────────────────────────────
    let cell_w = state.thumb_size + 10.0;
    let avail_w = ui.available_width();
    let cols = ((avail_w / cell_w) as usize).max(1);

    ScrollArea::vertical().show(ui, |ui| {
        let n = state.results.len();
        if n == 0 {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("No media files found.  Try searching or adding a watch folder.").color(Colors::TEXT_DIM));
            });
            return;
        }
        let rows = (n + cols - 1) / cols;
        for row in 0..rows {
            ui.horizontal(|ui| {
                for col in 0..cols {
                    let idx = row * cols + col;
                    if idx >= n { break; }
                    let file   = &state.results[idx];
                    let is_sel = state.selected_idx == Some(idx);
                    let rev    = state.review.get(&file.id).cloned().unwrap_or_default();
                    let cell_resp = render_cell(ui, file, &rev, is_sel, state.thumb_size);
                    if cell_resp.clicked()        { state.selected_idx = Some(idx); }
                    if cell_resp.double_clicked() { actions.open_preview = Some(file.clone()); }
                }
            });
        }
    });

    actions
}

// ─── Cell rendering ──────────────────────────────────────────────────────────

fn render_cell(ui: &mut Ui, file: &FileSearchResult, rev: &FileReviewState, selected: bool, size: f32) -> egui::Response {
    let pick_border: Color32 = match rev.pick_flag.as_str() {
        PICK_PICK   => Color32::from_rgb(50, 210, 90),
        PICK_REJECT => Color32::from_rgb(210, 50, 50),
        _           => if selected { Color32::from_rgb(80, 160, 255) } else { Colors::BORDER2 },
    };

    egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .stroke(egui::Stroke::new(if selected { 2.0 } else { 1.0 }, pick_border))
        .rounding(3.0)
        .inner_margin(3.0)
        .show(ui, |ui| {
            ui.set_max_width(size + 6.0);

            let (rect, _) = ui.allocate_exact_size(Vec2::new(size, size), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 3.0, Color32::from_gray(18));

            let ext = std::path::Path::new(&file.name)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let icon = match ext.as_str() {
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tif" | "tiff" => "🖼",
                "mp4" | "mkv" | "mov" | "avi" => "🎬",
                "raw" | "cr2" | "nef" | "arw" | "dng" => "📷",
                _ => "📄",
            };
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, icon,
                egui::FontId::proportional(size * 0.38), Color32::from_gray(90));

            // Reject overlay
            if rev.pick_flag == PICK_REJECT {
                painter.rect_filled(rect, 3.0, Color32::from_rgba_unmultiplied(180, 20, 20, 55));
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, "✗",
                    egui::FontId::proportional(size * 0.45), Color32::from_rgba_unmultiplied(220, 60, 60, 200));
            }

            // Color label dot (top-right)
            if let Some(label) = &rev.color_label {
                painter.circle_filled(rect.right_top() + egui::vec2(-9.0, 9.0), 6.0, label_color(label));
            }

            // Filename
            ui.label(RichText::new(truncate_name(&file.name, 15)).size(8.5).color(Colors::TEXT_DIM));

            // Stars
            ui.horizontal(|ui| {
                let rating = rev.rating.unwrap_or(0);
                for star in 1u8..=5 {
                    let c = if star <= rating { Color32::from_rgb(255, 210, 40) } else { Color32::from_gray(45) };
                    ui.label(RichText::new("★").size(9.0).color(c));
                }
            });
        }).response
}

// ─── Keyboard handling ───────────────────────────────────────────────────────

fn handle_keyboard(ui: &mut Ui, state: &mut MediaIngestState, actions: &mut MediaIngestActions, cols: usize) {
    let n = state.results.len();
    if n == 0 { return; }

    ui.input(|i| {
        if i.key_pressed(Key::ArrowRight) {
            state.selected_idx = Some(state.selected_idx.map(|x| (x + 1).min(n - 1)).unwrap_or(0));
        }
        if i.key_pressed(Key::ArrowLeft) {
            state.selected_idx = Some(state.selected_idx.map(|x| x.saturating_sub(1)).unwrap_or(0));
        }
        if i.key_pressed(Key::ArrowDown) {
            state.selected_idx = Some(state.selected_idx.map(|x| (x + cols).min(n - 1)).unwrap_or(0));
        }
        if i.key_pressed(Key::ArrowUp) {
            state.selected_idx = Some(state.selected_idx.map(|x| x.saturating_sub(cols)).unwrap_or(0));
        }

        let sel = match state.selected_idx { Some(s) => s, None => return };
        let file_id = state.results[sel].id;

        for (key, num) in [(Key::Num1, 1u8), (Key::Num2, 2), (Key::Num3, 3), (Key::Num4, 4), (Key::Num5, 5)] {
            if i.key_pressed(key) {
                state.review.entry(file_id).or_default().rating = Some(num);
                actions.events.push(AppEvent::SetMediaReview { file_id, rating: Some(num), pick_flag: None, color_label: None });
            }
        }

        for (key, flag) in [(Key::P, PICK_PICK), (Key::X, PICK_REJECT), (Key::U, PICK_NONE)] {
            if i.key_pressed(key) {
                state.review.entry(file_id).or_default().pick_flag = flag.to_string();
                actions.events.push(AppEvent::SetMediaReview { file_id, rating: None, pick_flag: Some(flag.to_string()), color_label: None });
            }
        }

        for (key, lbl) in [(Key::R, "red"), (Key::G, "green"), (Key::Y, "yellow"), (Key::B, "blue")] {
            if i.key_pressed(key) {
                state.review.entry(file_id).or_default().color_label = Some(lbl.to_string());
                actions.events.push(AppEvent::SetMediaReview { file_id, rating: None, pick_flag: None, color_label: Some(lbl.to_string()) });
            }
        }

        if i.key_pressed(Key::Delete) {
            state.review.entry(file_id).or_default().color_label = None;
            actions.events.push(AppEvent::SetMediaReview { file_id, rating: None, pick_flag: None, color_label: None });
        }

        if i.key_pressed(Key::Enter) {
            actions.open_preview = Some(state.results[sel].clone());
        }
    });
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn toggle_chip(ui: &mut Ui, label: &str, active: &mut bool) {
    let color = if *active { Colors::GREEN } else { Colors::TEXT_DIM };
    if ui.add(
        egui::Button::new(RichText::new(label).size(9.0).color(color))
            .fill(Color32::from_black_alpha(60))
            .rounding(8.0)
    ).clicked() {
        *active = !*active;
    }
}

fn truncate_name(name: &str, max: usize) -> String {
    let base = std::path::Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string());
    if base.chars().count() <= max {
        base
    } else {
        let cut = base.char_indices().nth(max).map(|(i, _)| i).unwrap_or(base.len());
        format!("{}…", &base[..cut])
    }
}

fn sort_results(results: &mut Vec<FileSearchResult>, sort: MediaSortField, desc: bool, review: &HashMap<i64, FileReviewState>) {
    results.sort_by(|a, b| {
        let ord = match sort {
            MediaSortField::Name   => a.name.cmp(&b.name),
            MediaSortField::Size   => a.size.cmp(&b.size),
            MediaSortField::Date   => a.modified.cmp(&b.modified),
            MediaSortField::Rating => {
                let ra = review.get(&a.id).and_then(|r| r.rating).unwrap_or(0);
                let rb = review.get(&b.id).and_then(|r| r.rating).unwrap_or(0);
                ra.cmp(&rb)
            }
        };
        if desc { ord.reverse() } else { ord }
    });
}

/// Build the effective query string including quick-filter tokens.
pub fn build_effective_query(state: &MediaIngestState) -> String {
    let mut parts: Vec<String> = vec![state.pending_query.trim().to_string()];
    if state.filter_only_geotagged              { parts.push("gps:true".to_string()); }
    if state.filter_in_focus                    { parts.push("focus:in".to_string()); }
    if state.filter_only_picks                  { parts.push("pick:keep".to_string()); }
    if state.filter_min_rating > 0              { parts.push(format!("rating>={}", state.filter_min_rating)); }
    if state.filter_min_quality > 0.001         { parts.push(format!("quality>={:.1}", state.filter_min_quality)); }
    parts.retain(|p| !p.is_empty());
    parts.join(" ")
}
