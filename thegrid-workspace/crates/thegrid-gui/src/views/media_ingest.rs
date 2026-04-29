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
    /// Width of the right-side detail inspector (0 = hidden)
    pub detail_panel_width:  f32,
    pub show_filter_help:    bool,
    pub filter_only_picks:   bool,
    pub filter_only_unrated: bool,
    pub filter_min_rating:   u8,
    pub filter_min_quality:  f32,
    pub filter_only_geotagged: bool,
    pub filter_in_focus:     bool,
    pub result_limit:        usize,
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
            detail_panel_width:  260.0,
            show_filter_help:    false,
            filter_only_picks:   false,
            filter_only_unrated: false,
            filter_min_rating:   0,
            filter_min_quality:  0.0,
            filter_only_geotagged: false,
            filter_in_focus:     false,
            result_limit:        24,
        }
    }
}

// ─── Actions returned to app.rs ───────────────────────────────────────────────

pub struct MediaIngestActions {
    pub events:         Vec<AppEvent>,
    pub trigger_search: bool,
    pub open_preview:   Option<FileSearchResult>,
    pub search_limit:   usize,
}

impl Default for MediaIngestActions {
    fn default() -> Self {
        Self { events: Vec::new(), trigger_search: false, open_preview: None, search_limit: 50 }
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
            let q = build_effective_query(state);
            if q != state.query {
                state.result_limit = 24;
            }
            state.query = q;
            actions.trigger_search = true;
            actions.search_limit = state.result_limit;
            state.loading = true;
        }

        if let Some(t) = state.last_searched {
            if t.elapsed().as_millis() > state.debounce_ms as u128 {
                state.last_searched = None;
                let q = build_effective_query(state);
                if q != state.query {
                    state.result_limit = 24;
                }
                state.query = q;
                actions.trigger_search = true;
                actions.search_limit = state.result_limit;
                state.loading = true;
            }
        }

        if ui.add(
            egui::Button::new(RichText::new("Search").color(Colors::GREEN).size(10.0))
                .fill(Colors::BG_WIDGET)
                .stroke(egui::Stroke::new(1.0, Colors::GREEN_DIM))
        ).clicked() {
            let q = build_effective_query(state);
            if q != state.query {
                state.result_limit = 24;
            }
            state.query = q;
            actions.trigger_search = true;
            actions.search_limit = state.result_limit;
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
            ui.label(
                RichText::new(format!("{} files (limit {})", state.results.len(), state.result_limit))
                    .color(Colors::TEXT_DIM)
                    .small()
            );
            if state.results.len() >= state.result_limit {
                if ui.add(
                    egui::Button::new(RichText::new("Load More").size(9.0).color(Colors::TEXT))
                        .fill(Colors::BG_WIDGET)
                ).clicked() {
                    state.result_limit = (state.result_limit + 24).min(500);
                    actions.trigger_search = true;
                    actions.search_limit = state.result_limit;
                    state.loading = true;
                }
            }
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
    let card_info_w = 170.0;
    let card_w = state.thumb_size + card_info_w + 18.0;
    let grid_avail = ui.available_width();
    let item_sp = ui.spacing().item_spacing.x.max(8.0);
    let approx_cols = (((grid_avail + item_sp) / (card_w + item_sp)) as usize).max(1);
    handle_keyboard(ui, state, &mut actions, approx_cols);

    // ── Split layout: grid left, detail inspector right ───────────────────────
    let has_selection = state.selected_idx.is_some();
    let panel_w = if has_selection { state.detail_panel_width } else { 0.0 };

    // Detail panel (right side)
    if has_selection {
        egui::SidePanel::right("media_detail_panel")
            .exact_width(panel_w)
            .resizable(true)
            .show_inside(ui, |ui| {
                render_detail_panel(ui, state);
            });
    }

    // ── Grid ─────────────────────────────────────────────────────────────────
    let avail_w = ui.available_width();
    let cols = (((avail_w + item_sp) / (card_w + item_sp)) as usize).max(1);
    let n = state.results.len();

    if n == 0 {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new("No media files found.  Try searching or adding a watch folder.").color(Colors::TEXT_DIM));
        });
        return actions;
    }

    let rows = (n + cols - 1) / cols;
    let row_height = state.thumb_size + 20.0;
    ui.spacing_mut().item_spacing.y = 8.0;
    ScrollArea::vertical().max_width(avail_w).show_rows(ui, row_height, rows, |ui, row_range| {
        for row in row_range {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = item_sp;
                for col in 0..cols {
                    let idx = row * cols + col;
                    if idx >= n { break; }

                    let file   = &state.results[idx];
                    let is_sel = state.selected_idx == Some(idx);
                    let rev    = state.review.get(&file.id).cloned().unwrap_or_default();
                    let cell_resp = render_cell(ui, file, &rev, is_sel, state.thumb_size, card_info_w);
                    if cell_resp.clicked()        { state.selected_idx = Some(idx); }
                    if cell_resp.double_clicked() { actions.open_preview = Some(file.clone()); }
                }
            });
        }
    });

    actions
}

// ─── Cell rendering ──────────────────────────────────────────────────────────

fn render_cell(
    ui: &mut Ui,
    file: &FileSearchResult,
    rev: &FileReviewState,
    selected: bool,
    thumb_size: f32,
    info_w: f32,
) -> egui::Response {
    let pick_border: Color32 = match rev.pick_flag.as_str() {
        PICK_PICK   => Color32::from_rgb(50, 210, 90),
        PICK_REJECT => Color32::from_rgb(210, 50, 50),
        _           => if selected { Color32::from_rgb(80, 160, 255) } else { Colors::BORDER2 },
    };

    let ext = file.ext.as_deref()
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    let is_raster = matches!(ext.as_str(),
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tif" | "tiff");
    let fallback_label = match ext.as_str() {
        "mp4" | "mkv" | "mov" | "avi" => "VIDEO",
        "raw" | "cr2" | "nef" | "arw" | "dng" => "RAW",
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tif" | "tiff" => "IMAGE",
        _ => "FILE",
    };

    let card_h = thumb_size + 8.0;
    let card_w = thumb_size + info_w + 18.0;

    let outer = egui::Frame::none()
        .fill(Colors::BG_WIDGET)
        .stroke(egui::Stroke::new(if selected { 2.0 } else { 1.0 }, pick_border))
        .rounding(3.0)
        .inner_margin(3.0)
        .show(ui, |ui| {
            ui.set_min_size(Vec2::new(card_w, card_h));
            ui.set_width(card_w);

            ui.horizontal(|ui| {
                let (thumb_rect, _) = ui.allocate_exact_size(Vec2::new(thumb_size, thumb_size), egui::Sense::hover());
                let painter = ui.painter_at(thumb_rect);
                painter.rect_filled(thumb_rect, 3.0, Color32::from_gray(18));

                if is_raster {
                    let path_fwd = file.path.to_string_lossy().replace('\\', "/");
                    let uri = format!("file:///{}", path_fwd.trim_start_matches('/'));
                    let img = egui::Image::new(uri)
                        .fit_to_exact_size(Vec2::new(thumb_size, thumb_size))
                        .rounding(egui::Rounding::same(3.0));
                    ui.put(thumb_rect, img);
                } else {
                    painter.text(
                        thumb_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        fallback_label,
                        egui::FontId::proportional((thumb_size * 0.14).clamp(10.0, 14.0)),
                        Color32::from_gray(120),
                    );
                }

                if rev.pick_flag == PICK_REJECT {
                    painter.rect_filled(thumb_rect, 3.0, Color32::from_rgba_unmultiplied(180, 20, 20, 55));
                    painter.text(
                        thumb_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "✗",
                        egui::FontId::proportional(thumb_size * 0.45),
                        Color32::from_rgba_unmultiplied(220, 60, 60, 200),
                    );
                }

                if let Some(label) = &rev.color_label {
                    painter.circle_filled(thumb_rect.right_top() + egui::vec2(-9.0, 9.0), 6.0, label_color(label));
                }

                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.set_width(info_w);

                    let max_chars = ((info_w / 6.4) as usize).max(20);
                    ui.label(RichText::new(truncate_name(&file.name, max_chars)).size(9.5).color(Colors::TEXT).strong());

                    let ext_str = file.ext.as_deref().map(|e| e.to_uppercase()).unwrap_or_else(|| "?".to_string());
                    let badge_color = ext_badge_color(&ext_str);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        egui::Frame::none()
                            .fill(badge_color.linear_multiply(0.25))
                            .rounding(2.0)
                            .inner_margin(egui::Margin::symmetric(4.0, 1.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(&ext_str).size(8.0).color(badge_color).strong());
                            });
                        ui.label(RichText::new(format_size(file.size)).size(8.5).color(Colors::TEXT_DIM));
                    });

                    ui.label(RichText::new(format_modified(file.modified)).size(8.5).color(Colors::TEXT_DIM));
                    ui.label(RichText::new(truncate_name(&file.device_name, 22)).size(8.0).color(Color32::from_gray(125)));

                    ui.horizontal(|ui| {
                        let rating = rev.rating.unwrap_or(0);
                        for star in 1u8..=5 {
                            let c = if star <= rating { Color32::from_rgb(255, 210, 40) } else { Color32::from_gray(45) };
                            ui.label(RichText::new("★").size(9.0).color(c));
                        }
                    });
                });
            });
        });

    // Full-path tooltip on hover
    outer.response.clone().on_hover_ui(|ui| {
        ui.label(RichText::new(file.path.to_string_lossy().as_ref()).monospace().small().color(Colors::TEXT));
        ui.label(
            RichText::new(format!("{} · {}",
                format_size(file.size),
                format_modified(file.modified)
            )).small().color(Colors::TEXT_DIM)
        );
    });

    outer.response
}

// ─── Detail inspector panel ───────────────────────────────────────────────────

fn render_detail_panel(ui: &mut Ui, state: &MediaIngestState) {
    let idx = match state.selected_idx { Some(i) => i, None => return };
    let file = match state.results.get(idx) { Some(f) => f, None => return };
    let rev  = state.review.get(&file.id).cloned().unwrap_or_default();

    ui.add_space(4.0);
    ui.label(RichText::new("FILE INFO").strong().size(10.0).color(Colors::GREEN));
    ui.separator();

    // Thumbnail preview (larger)
    let ext = file.ext.as_deref().map(|e| e.to_lowercase()).unwrap_or_default();
    let is_raster = matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tif" | "tiff");
    if is_raster {
        let preview_w = ui.available_width();
        let path_fwd = file.path.to_string_lossy().replace('\\', "/");
        let uri = format!("file:///{}", path_fwd.trim_start_matches('/'));
        ui.add(egui::Image::new(uri)
            .fit_to_exact_size(Vec2::new(preview_w, preview_w * 0.67))
            .rounding(egui::Rounding::same(3.0)));
        ui.add_space(6.0);
    }

    // Full filename
    egui::Frame::none()
        .fill(Color32::from_black_alpha(60))
        .rounding(3.0)
        .inner_margin(6.0)
        .show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(&file.name).strong().size(10.5).color(Colors::TEXT)).wrap(true));
        });
    ui.add_space(4.0);

    // Metadata rows
    let rows: &[(&str, String)] = &[
        ("Path",     file.path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()),
        ("Device",   file.device_name.clone()),
        ("Type",     ext.to_uppercase()),
        ("Size",     format_size(file.size)),
        ("Modified", format_modified(file.modified)),
        ("Indexed",  {
            let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(file.indexed_at, 0)
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
            dt
        }),
    ];

    // AI metadata extras (resolution, duration, quality)
    if let Some(json_str) = &file.ai_metadata {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let (Some(w), Some(h)) = (v.get("width").and_then(|x| x.as_u64()), v.get("height").and_then(|x| x.as_u64())) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Resolution").size(9.0).color(Colors::TEXT_DIM));
                    ui.label(RichText::new(format!("{w}×{h}")).size(9.5).color(Colors::TEXT));
                });
            }
            if let Some(d) = v.get("duration_secs").and_then(|x| x.as_f64()) {
                if d > 0.0 {
                    let mins = (d / 60.0) as u64;
                    let secs = (d % 60.0) as u64;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Duration").size(9.0).color(Colors::TEXT_DIM));
                        ui.label(RichText::new(format!("{mins}:{secs:02}")).size(9.5).color(Colors::TEXT));
                    });
                }
            }
            if let Some(q) = v.get("quality_score").and_then(|x| x.as_f64()) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Quality").size(9.0).color(Colors::TEXT_DIM));
                    let color = if q >= 0.7 { Color32::from_rgb(50, 210, 90) }
                                else if q >= 0.4 { Color32::from_rgb(220, 180, 30) }
                                else { Color32::from_rgb(200, 80, 80) };
                    ui.label(RichText::new(format!("{:.0}%", q * 100.0)).size(9.5).color(color));
                });
            }
            if let Some(cam) = v.get("camera_model").and_then(|x| x.as_str()) {
                if !cam.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Camera").size(9.0).color(Colors::TEXT_DIM));
                        ui.label(RichText::new(cam).size(9.5).color(Colors::TEXT));
                    });
                }
            }
        }
    }

    egui::Grid::new("file_detail_grid")
        .num_columns(2)
        .spacing([8.0, 3.0])
        .show(ui, |ui| {
            for (label, value) in rows {
                ui.label(RichText::new(*label).size(9.0).color(Colors::TEXT_DIM));
                ui.add(egui::Label::new(RichText::new(value).size(9.5).color(Colors::TEXT)).wrap(true));
                ui.end_row();
            }
        });

    ui.add_space(6.0);
    ui.separator();
    ui.label(RichText::new("REVIEW").strong().size(9.0).color(Colors::TEXT_DIM));
    ui.add_space(3.0);

    // Stars display
    ui.horizontal(|ui| {
        let rating = rev.rating.unwrap_or(0);
        for star in 1u8..=5 {
            let c = if star <= rating { Color32::from_rgb(255, 210, 40) } else { Color32::from_gray(45) };
            ui.label(RichText::new("★").size(14.0).color(c));
        }
        if rating > 0 {
            ui.label(RichText::new(format!("({rating})")).size(9.0).color(Colors::TEXT_DIM));
        }
    });

    // Pick flag
    let pick_color = match rev.pick_flag.as_str() {
        PICK_PICK   => Color32::from_rgb(50, 210, 90),
        PICK_REJECT => Color32::from_rgb(210, 50, 50),
        _           => Colors::TEXT_DIM,
    };
    let pick_label = match rev.pick_flag.as_str() {
        PICK_PICK   => "✓ PICK",
        PICK_REJECT => "✗ REJECT",
        _           => "— unrated",
    };
    ui.label(RichText::new(pick_label).size(10.0).color(pick_color));

    if let Some(lbl) = &rev.color_label {
        ui.horizontal(|ui| {
            let dot_color = label_color(lbl);
            ui.painter().circle_filled(
                ui.cursor().min + egui::vec2(6.0, 6.0), 5.0, dot_color
            );
            ui.add_space(16.0);
            ui.label(RichText::new(lbl.to_uppercase()).size(9.5).color(dot_color));
        });
    }

    ui.add_space(6.0);
    ui.label(RichText::new("[1-5] rate  [P] pick  [X] reject").size(8.5).color(Colors::TEXT_DIM));
    ui.label(RichText::new("[Enter] preview  [Del] clear label").size(8.5).color(Colors::TEXT_DIM));
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

fn ext_badge_color(ext: &str) -> Color32 {
    match ext {
        "JPG" | "JPEG" => Color32::from_rgb(80, 160, 255),
        "PNG"          => Color32::from_rgb(100, 200, 100),
        "WEBP"         => Color32::from_rgb(60, 200, 180),
        "GIF"          => Color32::from_rgb(200, 140, 60),
        "TIF" | "TIFF" => Color32::from_rgb(160, 100, 220),
        "RAW" | "CR2" | "NEF" | "ARW" | "DNG" => Color32::from_rgb(220, 160, 40),
        "MP4" | "MOV"  => Color32::from_rgb(220, 80, 80),
        "MKV" | "AVI"  => Color32::from_rgb(200, 60, 140),
        _              => Color32::from_gray(120),
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 { format!("{:.1} GB", bytes as f64 / 1_073_741_824.0) }
    else if bytes >= 1_048_576  { format!("{:.1} MB", bytes as f64 / 1_048_576.0) }
    else if bytes >= 1_024       { format!("{:.1} KB", bytes as f64 / 1_024.0) }
    else                         { format!("{bytes} B") }
}

fn format_modified(ts: Option<i64>) -> String {
    ts.and_then(|t| chrono::DateTime::<chrono::Utc>::from_timestamp(t, 0))
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "—".to_string())
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
