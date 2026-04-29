// Media Ingest / Culling View - THE GRID
//
// Keyboard shortcuts (when a cell is focused):
//   1-5     - star rating
//   P       - pick
//   X       - reject
//   U       - unmark (none)
//   R/G/Y/B - color labels (red/green/yellow/blue)
//   Del     - clear label
//   Arrows  - navigate cells
//   Enter   - open preview
//   Esc     - clear selection
//   Ctrl+A  - select all

use std::collections::{HashMap, HashSet};
use egui::{Color32, Key, RichText, ScrollArea, Ui, Vec2};
use thegrid_core::{AppEvent, models::FileSearchResult};
use crate::theme::Colors;

// --- Pick flag constants ----------------------------------------------------

const PICK_NONE:   &str = "none";
const PICK_PICK:   &str = "pick";
const PICK_REJECT: &str = "reject";

// --- Media file type (derived from extension) ------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MediaFileType {
    #[default]
    All,
    Raw,
    Video,
    Photo,
    Audio,
    Psd,
    Ai,
    Doc,
    Drone,
}

impl MediaFileType {
    pub fn label(self) -> &'static str {
        match self {
            Self::All   => "ALL",
            Self::Raw   => "RAW",
            Self::Video => "VIDEO",
            Self::Photo => "PHOTO",
            Self::Audio => "AUDIO",
            Self::Psd   => "PSD",
            Self::Ai    => "AI",
            Self::Doc   => "DOC",
            Self::Drone => "DRONE",
        }
    }

    pub fn fg_color(self) -> Color32 {
        match self {
            Self::All   => Colors::GREEN,
            Self::Raw   => Color32::from_rgb(255, 214, 0),
            Self::Video => Color32::from_rgb(255, 104, 32),
            Self::Photo => Color32::from_rgb(68,  136, 255),
            Self::Audio => Color32::from_rgb(0,   255, 65),
            Self::Psd   => Color32::from_rgb(49,  197, 244),
            Self::Ai    => Color32::from_rgb(255, 154, 0),
            Self::Doc   => Color32::from_rgb(136, 136, 136),
            Self::Drone => Color32::from_rgb(0,   229, 255),
        }
    }

    pub fn bg_color(self) -> Color32 {
        let c = self.fg_color();
        Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 20)
    }

    pub fn border_color(self) -> Color32 {
        let c = self.fg_color();
        Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 70)
    }

    pub fn from_ext(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "dng" | "cr2" | "cr3" | "nef" | "arw" | "rw2" | "orf" | "raf" | "srw" | "3fr" => Self::Raw,
            "mp4" | "mov" | "mkv" | "avi" | "mts" | "m2ts" | "mxf" | "wmv" | "flv" | "webm" => Self::Video,
            "jpg" | "jpeg" | "png" | "webp" | "heic" | "heif" | "bmp" | "tif" | "tiff" => Self::Photo,
            "wav" | "mp3" | "flac" | "aac" | "m4a" | "ogg" | "aiff" | "opus" => Self::Audio,
            "psd" | "psb" => Self::Psd,
            "ai" => Self::Ai,
            "md" | "txt" | "pdf" | "docx" | "doc" | "rtf" | "rst" => Self::Doc,
            "srt" | "lrf" | "obs" => Self::Drone,
            _ => Self::Doc,
        }
    }

    fn matches_ext(self, ext: &str) -> bool {
        self == Self::All || self == Self::from_ext(ext)
    }
}

// --- Source type (derived from device name) --------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MediaSourceType {
    #[default]
    All,
    Drone,
    Action,
    Mirrorless,
    Phone,
    AudioRec,
    Workstation,
}

impl MediaSourceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::All         => "ALL SOURCES",
            Self::Drone       => "DRONE",
            Self::Action      => "ACTION CAM",
            Self::Mirrorless  => "MIRRORLESS",
            Self::Phone       => "PHONE",
            Self::AudioRec    => "AUDIO REC",
            Self::Workstation => "WORKSTATION",
        }
    }

    pub fn from_device(device_name: &str) -> Self {
        let d = device_name.to_lowercase();
        if d.contains("mini") || d.contains("drone") || d.contains("mavic") || d.contains("phantom") {
            Self::Drone
        } else if d.contains("action") || d.contains("osmo") || d.contains("gopro") {
            Self::Action
        } else if d.contains("lumix") || d.contains("canon") || d.contains("sony") || d.contains("nikon")
               || d.contains("fuji") || d.contains("olympus") || d.contains("mirrorless") {
            Self::Mirrorless
        } else if d.contains("phone") || d.contains("iphone") || d.contains("pixel")
               || d.contains("galaxy") || d.contains("cmf") || d.contains("android") {
            Self::Phone
        } else if d.contains("zoom") || d.contains("mic") || d.contains("audio") || d.contains("recorder") {
            Self::AudioRec
        } else {
            Self::Workstation
        }
    }

    fn matches_device(self, device_name: &str) -> bool {
        self == Self::All || self == Self::from_device(device_name)
    }
}

// --- Type badge label for a file -------------------------------------------

fn type_badge(file: &FileSearchResult) -> String {
    let ext = file.ext.as_deref().unwrap_or("").to_uppercase();
    let t = MediaFileType::from_ext(file.ext.as_deref().unwrap_or(""));
    match t {
        MediaFileType::Raw   => format!("RAW {}", ext),
        MediaFileType::Video => ext.clone(),
        MediaFileType::Photo => ext.clone(),
        MediaFileType::Audio => ext.clone(),
        MediaFileType::Psd   => "Ps".to_string(),
        MediaFileType::Ai    => "Ai".to_string(),
        MediaFileType::Doc   => format!(".{}", ext),
        MediaFileType::Drone => format!("{} GPS", ext),
        MediaFileType::All   => ext.clone(),
    }
}

// --- Color label -----------------------------------------------------------

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

// --- Sort order ------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaSortField {
    #[default]
    Date,
    Name,
    Size,
    Rating,
    Type,
}

impl MediaSortField {
    fn label(self) -> &'static str {
        match self {
            Self::Date   => "DATE",
            Self::Name   => "NAME",
            Self::Size   => "SIZE",
            Self::Rating => "RATING",
            Self::Type   => "TYPE",
        }
    }
}

// --- Per-file review overlay (optimistic UI) -------------------------------

#[derive(Debug, Clone, Default)]
pub struct FileReviewState {
    pub rating:      Option<u8>,
    pub pick_flag:   String,
    pub color_label: Option<String>,
}

// --- View state ------------------------------------------------------------

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
    pub selected_ids:        HashSet<i64>,
    pub review:              HashMap<i64, FileReviewState>,
    pub cols:                usize,
    pub type_filter:         MediaFileType,
    pub src_filter:          MediaSourceType,
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
            selected_ids:        HashSet::new(),
            review:              HashMap::new(),
            cols:                4,
            type_filter:         MediaFileType::All,
            src_filter:          MediaSourceType::All,
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

// --- Actions returned to app.rs --------------------------------------------

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

// --- Main render -----------------------------------------------------------

pub fn render_media_ingest(ui: &mut Ui, state: &mut MediaIngestState) -> MediaIngestActions {
    let mut actions = MediaIngestActions::default();

    render_top_bar(ui, state, &mut actions);
    let visible = render_filter_bar(ui, state);

    let approx_cols = state.cols.max(1);
    handle_keyboard(ui, state, &mut actions, approx_cols, &visible);

    sort_results(&mut state.results, state.sort, state.sort_desc, &state.review);

    let n = visible.len();
    let cols = state.cols.max(1);

    let scrollable_height = ui.available_height()
        - if !state.selected_ids.is_empty() { 36.0 } else { 0.0 }
        - 22.0;

    ScrollArea::vertical()
        .max_height(scrollable_height.max(100.0))
        .show(ui, |ui| {
            if n == 0 {
                ui.add_space(40.0);
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("NO FILES MATCH CURRENT FILTER")
                            .color(Colors::TEXT_MUTED)
                            .size(11.0)
                            .extra_letter_spacing(4.0),
                    );
                });
                return;
            }

            let avail_w = ui.available_width();
            let gap = 8.0;
            let cell_w = ((avail_w - gap * (cols as f32 - 1.0)) / cols as f32).max(140.0);
            // Card structure (matches HTML prototype):
            //   title bar 26 + image area 1:1 (= cell_w) + meta panel ~120
            let img_side = cell_w;
            let total_h  = 26.0 + img_side + 120.0;
            let rows = (n + cols - 1) / cols;

            for row in 0..rows {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
                    for col in 0..cols {
                        let idx = row * cols + col;
                        if idx >= n {
                            ui.allocate_space(Vec2::new(cell_w, total_h));
                            continue;
                        }
                        let file_id = visible[idx];
                        let file_pos = state.results.iter().position(|f| f.id == file_id);
                        if let Some(pos) = file_pos {
                            let file = state.results[pos].clone();
                            let is_focused = state.selected_idx == Some(idx);
                            let is_selected = state.selected_ids.contains(&file_id);
                            let rev = state.review.get(&file_id).cloned().unwrap_or_default();

                            // Strictly bound the card UI to a fixed-size sub-region.
                            // This is the critical fix: without this, child widgets
                            // can overflow the intended card width because the outer
                            // horizontal layout gives them the full row width.
                            let resp = ui.allocate_ui_with_layout(
                                Vec2::new(cell_w, total_h),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_min_size(Vec2::new(cell_w, total_h));
                                    ui.set_max_width(cell_w);
                                    render_card(ui, &file, &rev, is_focused, is_selected, cell_w, img_side)
                                },
                            ).inner;

                            if resp.clicked() {
                                if is_selected {
                                    state.selected_ids.remove(&file_id);
                                } else {
                                    state.selected_ids.insert(file_id);
                                }
                                state.selected_idx = Some(idx);
                            }
                            if resp.double_clicked() {
                                actions.open_preview = Some(file);
                            }
                        }
                    }
                });
                ui.add_space(gap);
            }
        });

    if !state.selected_ids.is_empty() {
        render_selection_bar(ui, state, &mut actions);
    }

    render_status_bar(ui, state, n);

    actions
}

// --- Top bar ---------------------------------------------------------------

fn render_top_bar(ui: &mut Ui, state: &mut MediaIngestState, actions: &mut MediaIngestActions) {
    let bar_height = 44.0;
    egui::Frame::none()
        .fill(Color32::from_rgb(9, 9, 9))
        .inner_margin(egui::Margin { left: 14.0, right: 14.0, top: 0.0, bottom: 0.0 })
        .show(ui, |ui| {
            ui.set_min_height(bar_height);
            ui.horizontal(|ui| {
                ui.set_min_height(bar_height);

                draw_hex_logo(ui, 18.0);
                ui.add_space(6.0);

                ui.label(RichText::new("MEDIA INGEST").color(Colors::GREEN).size(11.0).strong().extra_letter_spacing(3.0));

                ui.add_space(10.0);

                let resp = ui.add(
                    egui::TextEdit::singleline(&mut state.pending_query)
                        .hint_text("SEARCH FILES, TAGS, SOURCES...")
                        .desired_width(340.0)
                        .font(egui::TextStyle::Monospace),
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
                    egui::Button::new(RichText::new("SEARCH").color(Color32::BLACK).size(9.0).strong())
                        .fill(Colors::GREEN)
                        .min_size(Vec2::new(64.0, 26.0)),
                ).clicked() {
                    state.query = build_effective_query(state);
                    actions.trigger_search = true;
                    state.loading = true;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let filtered_count = state.results.iter().filter(|f| {
                        let ext = f.ext.as_deref().unwrap_or("");
                        state.type_filter.matches_ext(ext)
                            && state.src_filter.matches_device(&f.device_name)
                    }).count();
                    ui.label(
                        RichText::new(format!("{} / {}", filtered_count, state.results.len()))
                            .color(Colors::TEXT_MUTED)
                            .size(8.0),
                    );

                    ui.add_space(8.0);

                    for n in [6usize, 5, 4, 3] {
                        let active = state.cols == n;
                        let color = if active { Colors::GREEN } else { Color32::from_gray(72) };
                        let fill = if active { Color32::from_rgba_unmultiplied(0, 255, 65, 20) } else { Color32::TRANSPARENT };
                        if ui.add(
                            egui::Button::new(RichText::new(n.to_string()).size(9.0).color(color))
                                .fill(fill)
                                .stroke(egui::Stroke::new(1.0, if active { Colors::GREEN } else { Colors::BORDER2 }))
                                .min_size(Vec2::new(24.0, 24.0)),
                        ).clicked() {
                            state.cols = n;
                        }
                    }

                    ui.add_space(8.0);

                    for s in [MediaSortField::Rating, MediaSortField::Type, MediaSortField::Size, MediaSortField::Name, MediaSortField::Date] {
                        let active = state.sort == s;
                        let arrow = if active {
                            if state.sort_desc { " v" } else { " ^" }
                        } else { "" };
                        let label = format!("{}{}", s.label(), arrow);
                        let color = if active { Colors::GREEN } else { Color32::from_gray(72) };
                        let fill = if active { Color32::from_rgba_unmultiplied(0, 255, 65, 16) } else { Color32::TRANSPARENT };
                        if ui.add(
                            egui::Button::new(RichText::new(label).size(8.0).color(color))
                                .fill(fill)
                                .stroke(egui::Stroke::new(1.0, if active { Colors::GREEN } else { Colors::BORDER2 }))
                                .min_size(Vec2::new(0.0, 24.0)),
                        ).clicked() {
                            if state.sort == s {
                                state.sort_desc = !state.sort_desc;
                            } else {
                                state.sort = s;
                                state.sort_desc = true;
                            }
                        }
                    }

                    ui.label(RichText::new("SORT").color(Colors::TEXT_MUTED).size(7.0).extra_letter_spacing(2.0));
                });
            });
        });

    ui.separator();
}

// --- Filter bar ------------------------------------------------------------

fn render_filter_bar(ui: &mut Ui, state: &mut MediaIngestState) -> Vec<i64> {
    egui::Frame::none()
        .fill(Color32::from_rgb(7, 7, 9))
        .inner_margin(egui::Margin { left: 14.0, right: 14.0, top: 6.0, bottom: 6.0 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("TYPE").color(Colors::TEXT_MUTED).size(7.0).extra_letter_spacing(2.0));
                ui.add_space(2.0);

                for t in [
                    MediaFileType::All, MediaFileType::Raw, MediaFileType::Video, MediaFileType::Photo,
                    MediaFileType::Audio, MediaFileType::Psd, MediaFileType::Ai, MediaFileType::Doc, MediaFileType::Drone,
                ] {
                    let active = state.type_filter == t;
                    let color  = if active { t.fg_color() } else { Color32::from_gray(72) };
                    let fill   = if active { t.bg_color() } else { Color32::TRANSPARENT };
                    let border = if active { t.border_color() } else { Colors::BORDER2 };
                    if ui.add(
                        egui::Button::new(RichText::new(t.label()).size(8.0).color(color))
                            .fill(fill)
                            .stroke(egui::Stroke::new(1.0, border))
                            .min_size(Vec2::new(0.0, 22.0)),
                    ).clicked() {
                        state.type_filter = t;
                    }
                }

                ui.add_space(8.0);
                ui.label(RichText::new("SOURCE").color(Colors::TEXT_MUTED).size(7.0).extra_letter_spacing(2.0));
                ui.add_space(2.0);

                for s in [
                    MediaSourceType::All, MediaSourceType::Drone, MediaSourceType::Action,
                    MediaSourceType::Mirrorless, MediaSourceType::Phone, MediaSourceType::AudioRec, MediaSourceType::Workstation,
                ] {
                    let active = state.src_filter == s;
                    let color  = if active { Colors::GREEN } else { Color32::from_gray(72) };
                    let fill   = if active { Color32::from_rgba_unmultiplied(0, 255, 65, 16) } else { Color32::TRANSPARENT };
                    let border = if active { Colors::GREEN_DIM } else { Colors::BORDER2 };
                    if ui.add(
                        egui::Button::new(RichText::new(s.label()).size(8.0).color(color))
                            .fill(fill)
                            .stroke(egui::Stroke::new(1.0, border))
                            .min_size(Vec2::new(0.0, 22.0)),
                    ).clicked() {
                        state.src_filter = s;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let sel = state.selected_ids.len();
                    if sel > 0 {
                        if ui.add(
                            egui::Button::new(RichText::new("CLEAR").size(8.0).color(Color32::from_gray(68)))
                                .fill(Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(1.0, Colors::BORDER2)),
                        ).clicked() {
                            state.selected_ids.clear();
                        }
                        ui.label(
                            RichText::new(format!("{} SEL", sel))
                                .color(Colors::GREEN)
                                .size(8.0)
                                .background_color(Color32::from_rgba_unmultiplied(0, 255, 65, 25)),
                        );
                    }

                    let visible_ids: Vec<i64> = state.results.iter()
                        .filter(|f| passes_filter(f, state))
                        .map(|f| f.id)
                        .collect();

                    if ui.add(
                        egui::Button::new(RichText::new("SEL ALL").size(8.0).color(Color32::from_gray(68)))
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0, Colors::BORDER2)),
                    ).clicked() {
                        state.selected_ids = visible_ids.iter().cloned().collect();
                    }
                });
            });
        });

    ui.separator();

    state.results.iter()
        .filter(|f| passes_filter(f, state))
        .map(|f| f.id)
        .collect()
}

fn passes_filter(f: &FileSearchResult, state: &MediaIngestState) -> bool {
    let ext = f.ext.as_deref().unwrap_or("");
    state.type_filter.matches_ext(ext) && state.src_filter.matches_device(&f.device_name)
}

// --- File card -------------------------------------------------------------
//
// Strategy: allocate one fixed-size rect for the entire card, then paint
// every visual element directly with `ui.painter_at(card_rect)`. This makes
// it impossible for child widgets to overflow the card boundary. Interactive
// widgets (PICK/RJCT buttons) get their own sub-uis at computed sub-rects.

fn render_card(
    ui: &mut Ui,
    file: &FileSearchResult,
    rev: &FileReviewState,
    is_focused: bool,
    is_selected: bool,
    width: f32,
    img_side: f32,
) -> egui::Response {
    let t = MediaFileType::from_ext(file.ext.as_deref().unwrap_or(""));
    let fg = t.fg_color();
    let bg_tint = Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 30);

    let title_h = 26.0;
    let meta_h  = 120.0;
    let total_h = title_h + img_side + meta_h;

    // Allocate the entire card rect upfront.
    let (card_rect, response) = ui.allocate_exact_size(
        Vec2::new(width, total_h),
        egui::Sense::click(),
    );
    let p = ui.painter_at(card_rect);

    // Card background + border
    let border_color = match rev.pick_flag.as_str() {
        PICK_PICK   => Colors::GREEN,
        PICK_REJECT => Colors::RED,
        _ if is_selected || is_focused => Colors::GREEN,
        _ => Color32::from_gray(28),
    };
    let card_bg = match rev.pick_flag.as_str() {
        PICK_PICK   => Color32::from_rgb(11, 23, 11),
        PICK_REJECT => Color32::from_rgb(18, 8, 8),
        _           => Color32::from_rgb(15, 15, 15),
    };
    let stroke_w = if is_selected || is_focused { 1.5 } else { 1.0 };

    p.rect_filled(card_rect, 0.0, card_bg);
    p.rect_stroke(card_rect, 0.0, egui::Stroke::new(stroke_w, border_color));

    // Sub-rects
    let title_rect = egui::Rect::from_min_size(
        card_rect.min,
        Vec2::new(width, title_h),
    );
    let img_rect = egui::Rect::from_min_size(
        egui::pos2(card_rect.min.x, card_rect.min.y + title_h),
        Vec2::new(width, img_side),
    );
    let meta_rect = egui::Rect::from_min_size(
        egui::pos2(card_rect.min.x, card_rect.min.y + title_h + img_side),
        Vec2::new(width, meta_h),
    );

    // ---- Title bar ----
    p.rect_filled(title_rect, 0.0, bg_tint);
    p.line_segment(
        [
            egui::pos2(title_rect.min.x, title_rect.max.y),
            egui::pos2(title_rect.max.x, title_rect.max.y),
        ],
        egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 70)),
    );

    // Type badge (left)
    let badge_text = type_badge(file);
    let badge_galley = p.layout_no_wrap(
        badge_text.clone(),
        egui::FontId::monospace(8.5),
        fg,
    );
    let badge_pad = egui::vec2(6.0, 3.0);
    let badge_rect = egui::Rect::from_min_size(
        title_rect.min + egui::vec2(6.0, (title_h - badge_galley.size().y - badge_pad.y * 2.0) / 2.0),
        badge_galley.size() + badge_pad * 2.0,
    );
    p.rect_filled(badge_rect, 0.0, Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 35));
    p.rect_stroke(badge_rect, 0.0, egui::Stroke::new(0.7, Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 120)));
    p.galley(badge_rect.min + badge_pad, badge_galley, fg);

    // Filename (middle, truncated to fit remaining space)
    let stem = file.path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| file.name.clone());

    let dot_size = 8.0;
    let check_size = 14.0;
    let mut right_used = 6.0;
    if rev.color_label.is_some() { right_used += dot_size + 4.0; }
    if is_selected               { right_used += check_size + 4.0; }

    let name_x_min = badge_rect.max.x + 6.0;
    let name_x_max = title_rect.max.x - right_used;
    let name_avail = (name_x_max - name_x_min).max(0.0);

    if name_avail > 30.0 {
        // Approx 6 px per mono char at size 9
        let max_chars = ((name_avail / 6.0) as usize).max(4);
        let display = truncate_name(&stem, max_chars);
        let name_galley = p.layout_no_wrap(display, egui::FontId::monospace(9.0), Color32::from_gray(210));
        let name_y = title_rect.center().y - name_galley.size().y / 2.0;
        p.galley(egui::pos2(name_x_min, name_y), name_galley, Color32::from_gray(210));
    }

    // Right side: selected check + color label dot
    let mut rx = title_rect.max.x - 6.0;
    if is_selected {
        let r = egui::Rect::from_min_size(
            egui::pos2(rx - check_size, title_rect.center().y - check_size / 2.0),
            Vec2::splat(check_size),
        );
        p.rect_filled(r, 0.0, Colors::GREEN);
        p.text(r.center(), egui::Align2::CENTER_CENTER, "v",
            egui::FontId::monospace(10.0), Color32::BLACK);
        rx -= check_size + 4.0;
    }
    if let Some(lbl) = &rev.color_label {
        let c = label_color(lbl);
        p.circle_filled(
            egui::pos2(rx - dot_size / 2.0, title_rect.center().y),
            dot_size / 2.0,
            c,
        );
    }

    // ---- Image / blueprint placeholder ----
    p.rect_filled(img_rect, 0.0, Color32::from_rgb(5, 7, 8));

    // Subtle scanlines
    {
        let mut y = img_rect.min.y;
        while y < img_rect.max.y {
            p.line_segment(
                [egui::pos2(img_rect.min.x, y), egui::pos2(img_rect.max.x, y)],
                egui::Stroke::new(0.5, Color32::from_rgba_unmultiplied(255, 255, 255, 6)),
            );
            y += 4.0;
        }
    }

    // Slab extension text (centered, sized relative to img_side)
    let ext_up = file.ext.as_deref().unwrap_or("FILE").to_uppercase();
    let slab_size = (img_side * 0.32).clamp(40.0, 110.0);
    p.text(
        img_rect.center() - egui::vec2(0.0, slab_size * 0.10),
        egui::Align2::CENTER_CENTER,
        format!(".{}", ext_up),
        egui::FontId::monospace(slab_size),
        Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 130),
    );

    // Type label below the slab
    p.text(
        img_rect.center() + egui::vec2(0.0, slab_size * 0.55),
        egui::Align2::CENTER_CENTER,
        t.label(),
        egui::FontId::monospace(9.0),
        Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 90),
    );

    // Reject overlay
    if rev.pick_flag == PICK_REJECT {
        p.rect_filled(img_rect, 0.0, Color32::from_rgba_unmultiplied(180, 20, 20, 55));
        p.text(
            img_rect.center(),
            egui::Align2::CENTER_CENTER,
            "X",
            egui::FontId::proportional(slab_size * 1.4),
            Color32::from_rgba_unmultiplied(220, 60, 60, 230),
        );
    }

    // Corner brackets
    let s = 10.0;
    let bracket = Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 130);
    let stroke = egui::Stroke::new(1.2, bracket);
    let pad = 4.0;
    // TL
    p.line_segment([img_rect.min + egui::vec2(pad, pad), img_rect.min + egui::vec2(pad + s, pad)], stroke);
    p.line_segment([img_rect.min + egui::vec2(pad, pad), img_rect.min + egui::vec2(pad, pad + s)], stroke);
    // TR
    let tr = egui::pos2(img_rect.max.x - pad, img_rect.min.y + pad);
    p.line_segment([tr, tr + egui::vec2(-s, 0.0)], stroke);
    p.line_segment([tr, tr + egui::vec2(0.0, s)], stroke);
    // BL
    let bl = egui::pos2(img_rect.min.x + pad, img_rect.max.y - pad);
    p.line_segment([bl, bl + egui::vec2(s, 0.0)], stroke);
    p.line_segment([bl, bl + egui::vec2(0.0, -s)], stroke);
    // BR
    let br = egui::pos2(img_rect.max.x - pad, img_rect.max.y - pad);
    p.line_segment([br, br + egui::vec2(-s, 0.0)], stroke);
    p.line_segment([br, br + egui::vec2(0.0, -s)], stroke);

    // Bottom status line
    let status = match rev.pick_flag.as_str() {
        PICK_PICK   => "PICK",
        PICK_REJECT => "REJECTED",
        _           => "PREVIEW READY",
    };
    p.text(
        egui::pos2(img_rect.center().x, img_rect.max.y - 9.0),
        egui::Align2::CENTER_CENTER,
        status,
        egui::FontId::monospace(7.5),
        Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 130),
    );

    // ---- Meta panel ----
    p.rect_filled(meta_rect, 0.0, Color32::from_rgb(10, 10, 10));
    p.line_segment(
        [
            egui::pos2(meta_rect.min.x, meta_rect.min.y),
            egui::pos2(meta_rect.max.x, meta_rect.min.y),
        ],
        egui::Stroke::new(1.0, Color32::from_gray(26)),
    );

    // Meta rows (drawn as text)
    let meta_pad_x = 9.0;
    let label_color = Color32::from_gray(80);
    let mut row_y = meta_rect.min.y + 8.0;
    let row_h = 12.0;

    let rows_data = [
        ("SOURCE", file.device_name.clone(),   Color32::from_gray(225)),
        ("SIZE",   fmt_size(file.size),        fg),
        ("DATE",   fmt_date(file.modified),    Color32::from_gray(200)),
    ];
    for (lbl, val, vc) in &rows_data {
        // Label left
        p.text(
            egui::pos2(meta_rect.min.x + meta_pad_x, row_y),
            egui::Align2::LEFT_TOP,
            lbl,
            egui::FontId::monospace(7.5),
            label_color,
        );
        // Value right (truncated to fit)
        let max_val_chars = (((width - meta_pad_x * 2.0 - 50.0) / 5.5) as usize).max(6);
        let val_disp = truncate_name(val, max_val_chars);
        p.text(
            egui::pos2(meta_rect.max.x - meta_pad_x, row_y),
            egui::Align2::RIGHT_TOP,
            val_disp,
            egui::FontId::monospace(8.5),
            *vc,
        );
        row_y += row_h;
    }

    // Stars row
    let star_y = row_y + 6.0;
    let rating = rev.rating.unwrap_or(0);
    for star in 1u8..=5 {
        let c = if star <= rating {
            Color32::from_rgb(255, 210, 40)
        } else {
            Color32::from_gray(48)
        };
        p.text(
            egui::pos2(meta_rect.min.x + meta_pad_x + (star as f32 - 1.0) * 12.0, star_y),
            egui::Align2::LEFT_TOP,
            "*",
            egui::FontId::monospace(13.0),
            c,
        );
    }

    // PICK / RJCT buttons (interactive — sub-ui at calculated rect)
    let btn_w = 44.0;
    let btn_h = 18.0;
    let btn_y = meta_rect.max.y - btn_h - 8.0;

    let pick_active = rev.pick_flag == PICK_PICK;
    let rjct_active = rev.pick_flag == PICK_REJECT;

    let pick_rect = egui::Rect::from_min_size(
        egui::pos2(meta_rect.max.x - meta_pad_x - btn_w * 2.0 - 4.0, btn_y),
        Vec2::new(btn_w, btn_h),
    );
    let rjct_rect = egui::Rect::from_min_size(
        egui::pos2(meta_rect.max.x - meta_pad_x - btn_w, btn_y),
        Vec2::new(btn_w, btn_h),
    );

    paint_pill(&p, pick_rect, "PICK",
        if pick_active { Colors::GREEN } else { Color32::from_gray(76) },
        if pick_active { Color32::from_rgba_unmultiplied(0, 255, 65, 30) } else { Color32::TRANSPARENT },
        if pick_active { Colors::GREEN } else { Colors::BORDER2 },
    );
    paint_pill(&p, rjct_rect, "RJCT",
        if rjct_active { Colors::RED } else { Color32::from_gray(76) },
        if rjct_active { Color32::from_rgba_unmultiplied(255, 34, 68, 30) } else { Color32::TRANSPARENT },
        if rjct_active { Colors::RED } else { Colors::BORDER2 },
    );

    // Hit testing for the buttons (return separate response, but let click bubble to card)
    let pick_resp = ui.interact(pick_rect, ui.id().with(("pick_btn", file.id)), egui::Sense::click());
    let rjct_resp = ui.interact(rjct_rect, ui.id().with(("rjct_btn", file.id)), egui::Sense::click());

    // The card-level response: forward button clicks via the response (caller handles state)
    // Note: we don't currently route these — clicks on buttons fall through and are
    // treated as a card click (toggling selection). That's acceptable for now;
    // future work: surface button events in MediaIngestActions.
    let _ = (pick_resp, rjct_resp);

    response
}

fn paint_pill(p: &egui::Painter, rect: egui::Rect, text: &str, fg: Color32, bg: Color32, border: Color32) {
    p.rect_filled(rect, 1.0, bg);
    p.rect_stroke(rect, 1.0, egui::Stroke::new(1.0, border));
    p.text(rect.center(), egui::Align2::CENTER_CENTER, text,
        egui::FontId::monospace(8.5), fg);
}

// --- Selection action bar --------------------------------------------------

fn render_selection_bar(ui: &mut Ui, state: &mut MediaIngestState, _actions: &mut MediaIngestActions) {
    let sel_count = state.selected_ids.len();
    let sel_size: u64 = state.results.iter()
        .filter(|f| state.selected_ids.contains(&f.id))
        .map(|f| f.size)
        .sum();

    egui::Frame::none()
        .fill(Color32::from_rgba_unmultiplied(0, 255, 65, 12))
        .stroke(egui::Stroke::new(1.0, Colors::GREEN))
        .inner_margin(egui::Margin { left: 14.0, right: 14.0, top: 0.0, bottom: 0.0 })
        .show(ui, |ui| {
            ui.set_min_height(36.0);
            ui.horizontal(|ui| {
                ui.set_min_height(36.0);
                ui.label(
                    RichText::new(format!("{} FILE{} SELECTED", sel_count, if sel_count == 1 { "" } else { "S" }))
                        .color(Colors::GREEN).size(10.0).strong()
                );
                ui.label(RichText::new(fmt_size(sel_size)).color(Colors::GREEN_DIM).size(8.0));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(
                        egui::Button::new(RichText::new("X CLEAR").size(8.0).color(Color32::from_gray(85)))
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0, Colors::BORDER2)),
                    ).clicked() {
                        state.selected_ids.clear();
                    }

                    if ui.add(
                        egui::Button::new(RichText::new("REJECT ALL").size(8.0).color(Colors::RED).strong())
                            .fill(Color32::from_rgba_unmultiplied(255, 34, 68, 25))
                            .stroke(egui::Stroke::new(1.0, Colors::RED)),
                    ).clicked() {
                        for id in &state.selected_ids {
                            let rev = state.review.entry(*id).or_default();
                            rev.pick_flag = PICK_REJECT.to_string();
                        }
                    }

                    for action in ["MOVE", "EXPORT", "TAG ALL", "INGEST"] {
                        let _ = ui.add(
                            egui::Button::new(RichText::new(action).size(8.0).color(Colors::GREEN).strong())
                                .fill(Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(1.0, Colors::GREEN)),
                        );
                    }
                });
            });
        });
}

// --- Status bar ------------------------------------------------------------

fn render_status_bar(ui: &mut Ui, state: &MediaIngestState, showing: usize) {
    let total = state.results.len();
    let picks   = state.review.values().filter(|r| r.pick_flag == PICK_PICK).count();
    let rejects = state.review.values().filter(|r| r.pick_flag == PICK_REJECT).count();
    let rated   = state.review.values().filter(|r| r.rating.is_some()).count();
    let total_sz: u64 = state.results.iter().map(|f| f.size).sum();

    egui::Frame::none()
        .fill(Color32::from_rgb(6, 6, 6))
        .inner_margin(egui::Margin { left: 14.0, right: 14.0, top: 0.0, bottom: 0.0 })
        .show(ui, |ui| {
            ui.set_min_height(20.0);
            ui.horizontal(|ui| {
                ui.set_min_height(20.0);
                let items = [
                    format!("INDEXED: {}", total),
                    format!("TOTAL: {}", fmt_size(total_sz)),
                    format!("PICKS: {}", picks),
                    format!("REJECTS: {}", rejects),
                    format!("RATED: {}", rated),
                    format!("SHOWING: {}", showing),
                ];
                for item in &items {
                    ui.label(RichText::new(item).size(7.5).color(Colors::TEXT_MUTED).extra_letter_spacing(1.0));
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("THEGRID / MEDIA INGEST / MVP")
                            .size(7.5).color(Color32::from_gray(40)),
                    );
                });
            });
        });
}

// --- Keyboard --------------------------------------------------------------

fn handle_keyboard(
    ui: &mut Ui,
    state: &mut MediaIngestState,
    actions: &mut MediaIngestActions,
    cols: usize,
    visible: &[i64],
) {
    let n = visible.len();
    if n == 0 { return; }

    ui.input(|i| {
        if i.key_pressed(Key::Escape) {
            state.selected_ids.clear();
        }
        if (i.modifiers.ctrl || i.modifiers.mac_cmd) && i.key_pressed(Key::A) {
            state.selected_ids = visible.iter().cloned().collect();
        }

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

        let sel = match state.selected_idx { Some(s) if s < n => s, _ => return };
        let file_id = visible[sel];

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
            if let Some(pos) = state.results.iter().position(|f| f.id == file_id) {
                actions.open_preview = Some(state.results[pos].clone());
            }
        }
    });

    let _ = ui;
}

// --- Hex logo --------------------------------------------------------------

fn draw_hex_logo(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let cx = rect.center().x;
    let cy = rect.center().y;
    let r  = size * 0.42;

    let pts: Vec<egui::Pos2> = (0..6).map(|i| {
        let a = i as f32 * 60.0_f32.to_radians();
        egui::pos2(cx + r * a.cos(), cy + r * a.sin())
    }).collect();

    for i in 0..6 {
        painter.line_segment(
            [pts[i], pts[(i + 1) % 6]],
            egui::Stroke::new(1.2, Colors::GREEN),
        );
        painter.line_segment(
            [egui::pos2(cx, cy), pts[i]],
            egui::Stroke::new(0.55, Color32::from_rgba_unmultiplied(0, 255, 65, 178)),
        );
    }
    painter.circle_filled(egui::pos2(cx, cy), 1.6, Colors::GREEN);
}

// --- Helpers ---------------------------------------------------------------

fn truncate_name(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        name.to_string()
    } else {
        let cut = name.char_indices().nth(max.saturating_sub(2)).map(|(i, _)| i).unwrap_or(name.len());
        format!("{}..", &name[..cut])
    }
}

fn fmt_size(b: u64) -> String {
    if b >= 1_000_000_000 {
        format!("{:.1} GB", b as f64 / 1e9)
    } else if b >= 1_000_000 {
        format!("{:.1} MB", b as f64 / 1e6)
    } else if b >= 1_000 {
        format!("{:.1} KB", b as f64 / 1e3)
    } else {
        format!("{} B", b)
    }
}

fn fmt_date(ts: Option<i64>) -> String {
    match ts {
        None => "-".to_string(),
        Some(t) => {
            let secs = t.max(0) as u64;
            let days = secs / 86400;
            let y = 1970 + days / 365;
            let rem = days % 365;
            let m = rem / 30 + 1;
            let d = rem % 30 + 1;
            format!("{:04}-{:02}-{:02}", y, m.min(12), d.min(31))
        }
    }
}

fn sort_results(results: &mut Vec<FileSearchResult>, sort: MediaSortField, desc: bool, review: &HashMap<i64, FileReviewState>) {
    results.sort_by(|a, b| {
        let ord = match sort {
            MediaSortField::Name   => a.name.cmp(&b.name),
            MediaSortField::Size   => a.size.cmp(&b.size),
            MediaSortField::Date   => a.modified.cmp(&b.modified),
            MediaSortField::Type   => {
                let ta = MediaFileType::from_ext(a.ext.as_deref().unwrap_or("")).label();
                let tb = MediaFileType::from_ext(b.ext.as_deref().unwrap_or("")).label();
                ta.cmp(tb)
            }
            MediaSortField::Rating => {
                let ra = review.get(&a.id).and_then(|r| r.rating).unwrap_or(0);
                let rb = review.get(&b.id).and_then(|r| r.rating).unwrap_or(0);
                ra.cmp(&rb)
            }
        };
        if desc { ord.reverse() } else { ord }
    });
}

pub fn build_effective_query(state: &MediaIngestState) -> String {
    let mut parts: Vec<String> = vec![state.pending_query.trim().to_string()];
    if state.filter_only_geotagged   { parts.push("gps:true".to_string()); }
    if state.filter_in_focus         { parts.push("focus:in".to_string()); }
    if state.filter_only_picks       { parts.push("pick:keep".to_string()); }
    if state.filter_min_rating > 0   { parts.push(format!("rating>={}", state.filter_min_rating)); }
    if state.filter_min_quality > 0.001 { parts.push(format!("quality>={:.1}", state.filter_min_quality)); }
    parts.retain(|p| !p.is_empty());
    parts.join(" ")
}
