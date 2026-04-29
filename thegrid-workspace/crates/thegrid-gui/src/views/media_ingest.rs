/// Media Ingest / Culling View â€” THE GRID
///
/// Keyboard shortcuts (when a cell is focused):
///   1-5     â€” star rating
///   P       â€” pick
///   X       â€” reject
///   U       â€” unmark (none)
///   R/G/Y/B â€” color labels (red/green/yellow/blue)
///   Del     â€” clear label
///   â†/â†’/â†‘/â†“ â€” navigate cells
///   Enter   â€” open preview
///   Esc     â€” clear selection
///   Ctrl+A  â€” select all

use std::collections::{HashMap, HashSet};
use egui::{Color32, Key, RichText, ScrollArea, Ui, Vec2};
use thegrid_core::{AppEvent, models::FileSearchResult};
use crate::theme::Colors;

// â”€â”€â”€ Pick flag constants â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const PICK_NONE:   &str = "none";
const PICK_PICK:   &str = "pick";
const PICK_REJECT: &str = "reject";

// â”€â”€â”€ Media file type (derived from extension) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

    /// Primary foreground color matching the HTML prototype's TC config
    pub fn fg_color(self) -> Color32 {
        match self {
            Self::All   => Colors::GREEN,
            Self::Raw   => Color32::from_rgb(255, 214, 0),   // #ffd600 amber
            Self::Video => Color32::from_rgb(255, 104, 32),  // #ff6820 orange
            Self::Photo => Color32::from_rgb(68,  136, 255), // #4488ff blue
            Self::Audio => Color32::from_rgb(0,   255, 65),  // #00ff41 green
            Self::Psd   => Color32::from_rgb(49,  197, 244), // #31c5f4 cyan
            Self::Ai    => Color32::from_rgb(255, 154, 0),   // #ff9a00 amber-orange
            Self::Doc   => Color32::from_rgb(136, 136, 136), // #888 gray
            Self::Drone => Color32::from_rgb(0,   229, 255), // #00e5ff electric blue
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

// â”€â”€â”€ Source type (derived from device name) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€â”€ Type badge label for a file â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn type_badge(file: &FileSearchResult) -> String {
    let ext = file.ext.as_deref().unwrap_or("").to_uppercase();
    let t = MediaFileType::from_ext(file.ext.as_deref().unwrap_or(""));
    match t {
        MediaFileType::Raw   => format!("RAWÂ·{}", ext),
        MediaFileType::Video => ext.clone(),
        MediaFileType::Photo => ext.clone(),
        MediaFileType::Audio => ext.clone(),
        MediaFileType::Psd   => "Ps".to_string(),
        MediaFileType::Ai    => "Ai".to_string(),
        MediaFileType::Doc   => format!(".{}", ext),
        MediaFileType::Drone => format!("{}Â·GPS", ext),
        MediaFileType::All   => ext.clone(),
    }
}

// â”€â”€â”€ Color label â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€â”€ Sort order â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€â”€ Per-file review overlay (optimistic UI) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Debug, Clone, Default)]
pub struct FileReviewState {
    pub rating:      Option<u8>,
    pub pick_flag:   String,
    pub color_label: Option<String>,
}

// â”€â”€â”€ View state â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
    /// Optimistic review overlay â€” keyed by file_id
    pub review:              HashMap<i64, FileReviewState>,
    pub cols:                usize,
    pub type_filter:         MediaFileType,
    pub src_filter:          MediaSourceType,
    // legacy quick-filter fields (kept for backward compat with app.rs search queries)
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

// â”€â”€â”€ Actions returned to app.rs â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€â”€ Main render function â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

pub fn render_media_ingest(ui: &mut Ui, state: &mut MediaIngestState) -> MediaIngestActions {
    let mut actions = MediaIngestActions::default();

    // â”€â”€ Top bar â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    render_top_bar(ui, state, &mut actions);

    // â”€â”€ Filter bar â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let visible = render_filter_bar(ui, state);

    // â”€â”€ Grid â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let approx_cols = state.cols.max(1);
    handle_keyboard(ui, state, &mut actions, approx_cols, &visible);

    // Sort
    sort_results(&mut state.results, state.sort, state.sort_desc, &state.review);

    let n = visible.len();
    let cols = state.cols.max(1);

    let scrollable_height = ui.available_height()
        - if !state.selected_ids.is_empty() { 36.0 } else { 0.0 }
        - 22.0; // status bar

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

            let cell_size = ui.available_width() / cols as f32 - 8.0 * (cols as f32 - 1.0) / cols as f32;
            let rows = (n + cols - 1) / cols;

            egui::Grid::new("media_grid")
                .num_columns(cols)
                .spacing([8.0, 8.0])
                .min_col_width(cell_size)
                .max_col_width(cell_size)
                .show(ui, |ui| {
                    for row in 0..rows {
                        for col in 0..cols {
                            let idx = row * cols + col;
                            if idx >= n {
                                ui.label("");
                                continue;
                            }
                            let file_id = visible[idx];
                            let file_pos = state.results.iter().position(|f| f.id == file_id);
                            if let Some(pos) = file_pos {
                                let file = state.results[pos].clone();
                                let is_focused = state.selected_idx == Some(idx);
                                let is_selected = state.selected_ids.contains(&file_id);
                                let rev = state.review.get(&file_id).cloned().unwrap_or_default();
                                let resp = render_card(ui, &file, &rev, is_focused, is_selected, cell_size);
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
                        ui.end_row();
                    }
                });
        });

    // â”€â”€ Selection action bar â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    if !state.selected_ids.is_empty() {
        render_selection_bar(ui, state, &mut actions);
    }

    // â”€â”€ Status bar â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    render_status_bar(ui, state, n);

    actions
}

// â”€â”€â”€ Top bar â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn render_top_bar(ui: &mut Ui, state: &mut MediaIngestState, actions: &mut MediaIngestActions) {
    let bar_height = 44.0;
    egui::Frame::none()
        .fill(Color32::from_rgb(9, 9, 9))
        .inner_margin(egui::Margin { left: 14.0, right: 14.0, top: 0.0, bottom: 0.0 })
        .show(ui, |ui| {
            ui.set_min_height(bar_height);
            ui.horizontal(|ui| {
                ui.set_min_height(bar_height);

                // Hex logo
                draw_hex_logo(ui, 18.0);
                ui.add_space(6.0);

                // Title
                ui.label(
                    RichText::new("MEDIA")
                        .color(Colors::GREEN)
                        .size(11.0)
                        .strong(),
                );
                ui.label(RichText::new("Â·").color(Colors::BORDER2).size(11.0));
                ui.label(
                    RichText::new("INGEST")
                        .color(Colors::GREEN)
                        .size(11.0)
                        .strong(),
                );

                ui.add(egui::Separator::default().vertical().spacing(10.0));

                // Search input
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

                // SEARCH button
                if ui.add(
                    egui::Button::new(
                        RichText::new("SEARCH")
                            .color(Color32::BLACK)
                            .size(9.0)
                            .strong(),
                    )
                    .fill(Colors::GREEN)
                    .min_size(Vec2::new(60.0, 26.0)),
                ).clicked() {
                    state.query = build_effective_query(state);
                    actions.trigger_search = true;
                    state.loading = true;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // File count
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

                    ui.add(egui::Separator::default().vertical().spacing(10.0));

                    // Column selector (3/4/5/6)
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

                    ui.add(egui::Separator::default().vertical().spacing(10.0));

                    // Sort buttons
                    for s in [MediaSortField::Rating, MediaSortField::Type, MediaSortField::Size, MediaSortField::Name, MediaSortField::Date] {
                        let active = state.sort == s;
                        let label = if active {
                            format!("{}{}", s.label(), if state.sort_desc { "â†“" } else { "â†‘" })
                        } else {
                            s.label().to_string()
                        };
                        let color = if active { Colors::GREEN } else { Color32::from_gray(72) };
                        let fill = if active { Color32::from_rgba_unmultiplied(0, 255, 65, 16) } else { Color32::TRANSPARENT };
                        if ui.add(
                            egui::Button::new(RichText::new(label).size(7.5).color(color))
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

                    ui.label(
                        RichText::new("SORT")
                            .color(Colors::TEXT_MUTED)
                            .size(7.0)
                            .extra_letter_spacing(2.0),
                    );
                    ui.add_space(4.0);
                });
            });
        });

    ui.add(egui::Separator::default().horizontal().spacing(0.0));
}

// â”€â”€â”€ Filter bar â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Returns the ordered list of file IDs that pass the current filter.

fn render_filter_bar(ui: &mut Ui, state: &mut MediaIngestState) -> Vec<i64> {
    egui::Frame::none()
        .fill(Color32::from_rgb(7, 7, 9))
        .inner_margin(egui::Margin { left: 14.0, right: 14.0, top: 6.0, bottom: 6.0 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // TYPE label
                ui.label(RichText::new("TYPE").color(Colors::TEXT_MUTED).size(7.0).extra_letter_spacing(2.0));
                ui.add_space(2.0);

                // Type chips
                for t in [
                    MediaFileType::All,
                    MediaFileType::Raw,
                    MediaFileType::Video,
                    MediaFileType::Photo,
                    MediaFileType::Audio,
                    MediaFileType::Psd,
                    MediaFileType::Ai,
                    MediaFileType::Doc,
                    MediaFileType::Drone,
                ] {
                    let active = state.type_filter == t;
                    let color  = if active { t.fg_color() } else { Color32::from_gray(72) };
                    let fill   = if active { t.bg_color() } else { Color32::TRANSPARENT };
                    let border = if active { t.border_color() } else { Colors::BORDER2 };
                    if ui.add(
                        egui::Button::new(RichText::new(t.label()).size(7.5).color(color))
                            .fill(fill)
                            .stroke(egui::Stroke::new(1.0, border))
                            .min_size(Vec2::new(0.0, 22.0)),
                    ).clicked() {
                        state.type_filter = t;
                    }
                }

                ui.add(egui::Separator::default().vertical().spacing(6.0));

                // SOURCE label
                ui.label(RichText::new("SOURCE").color(Colors::TEXT_MUTED).size(7.0).extra_letter_spacing(2.0));
                ui.add_space(2.0);

                // Source chips
                for s in [
                    MediaSourceType::All,
                    MediaSourceType::Drone,
                    MediaSourceType::Action,
                    MediaSourceType::Mirrorless,
                    MediaSourceType::Phone,
                    MediaSourceType::AudioRec,
                    MediaSourceType::Workstation,
                ] {
                    let active = state.src_filter == s;
                    let color  = if active { Colors::GREEN } else { Color32::from_gray(72) };
                    let fill   = if active { Color32::from_rgba_unmultiplied(0, 255, 65, 16) } else { Color32::TRANSPARENT };
                    let border = if active { Colors::GREEN_DIM } else { Colors::BORDER2 };
                    if ui.add(
                        egui::Button::new(RichText::new(s.label()).size(7.5).color(color))
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
                            egui::Button::new(RichText::new("CLEAR").size(7.5).color(Color32::from_gray(68)))
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
                        egui::Button::new(RichText::new("SEL ALL").size(7.5).color(Color32::from_gray(68)))
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0, Colors::BORDER2)),
                    ).clicked() {
                        state.selected_ids = visible_ids.iter().cloned().collect();
                    }
                });
            });
        });

    ui.add(egui::Separator::default().horizontal().spacing(0.0));

    // Return the filtered, sorted list of file IDs
    state.results.iter()
        .filter(|f| passes_filter(f, state))
        .map(|f| f.id)
        .collect()
}

fn passes_filter(f: &FileSearchResult, state: &MediaIngestState) -> bool {
    let ext = f.ext.as_deref().unwrap_or("");
    state.type_filter.matches_ext(ext) && state.src_filter.matches_device(&f.device_name)
}

// â”€â”€â”€ File card â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn render_card(
    ui: &mut Ui,
    file: &FileSearchResult,
    rev: &FileReviewState,
    is_focused: bool,
    is_selected: bool,
    width: f32,
) -> egui::Response {
    let t = MediaFileType::from_ext(file.ext.as_deref().unwrap_or(""));
    let fg = t.fg_color();
    let bg = t.bg_color();
    let bd = t.border_color();

    let pick_color = match rev.pick_flag.as_str() {
        PICK_PICK   => Colors::GREEN,
        PICK_REJECT => Colors::RED,
        _           => if is_selected || is_focused { Colors::GREEN } else { Color32::from_gray(26) },
    };

    let card_bg = match rev.pick_flag.as_str() {
        PICK_PICK   => Color32::from_rgb(11, 23, 11),
        PICK_REJECT => Color32::from_rgb(18, 8,  8),
        _           => Color32::from_rgb(15, 15, 15),
    };

    let stroke_w = if is_selected || is_focused { 1.5 } else { 1.0 };

    let resp = egui::Frame::none()
        .fill(card_bg)
        .stroke(egui::Stroke::new(stroke_w, pick_color))
        .inner_margin(0.0)
        .show(ui, |ui| {
            ui.set_max_width(width);
            ui.set_min_width(width);

            // Pick/reject accent bar
            if rev.pick_flag != PICK_NONE {
                let accent = if rev.pick_flag == PICK_PICK { Colors::GREEN } else { Colors::RED };
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 2.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 0.0, accent);
            }

            // â”€â”€ Title bar â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            egui::Frame::none()
                .fill(bg)
                .inner_margin(egui::Margin { left: 8.0, right: 6.0, top: 4.0, bottom: 4.0 })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_max_width(width - 14.0);

                        // Type badge
                        let badge = type_badge(file);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&badge)
                                    .size(7.5)
                                    .strong()
                                    .color(fg)
                                    .background_color(Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 22))
                            )
                        );

                        // Filename
                        let name = file.path.file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| file.name.clone());
                        let display_name = truncate_name(&name, 14);
                        ui.add(
                            egui::Label::new(
                                RichText::new(display_name).size(8.0).color(Color32::from_gray(200))
                            )
                            .truncate(true)
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Selected check
                            if is_selected {
                                let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                                ui.painter().rect_filled(rect, 0.0, Colors::GREEN);
                                ui.painter().text(
                                    rect.center(), egui::Align2::CENTER_CENTER, "âœ“",
                                    egui::FontId::monospace(9.0), Color32::BLACK,
                                );
                            }
                            // Color label dot
                            if let Some(lbl) = &rev.color_label {
                                let c = label_color(lbl);
                                let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                                ui.painter().circle_filled(rect.center(), 3.5, c);
                            }
                        });
                    });
                });

            // â”€â”€ Thumbnail / Blueprint area (square) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            let sq = width;
            let (img_rect, _) = ui.allocate_exact_size(Vec2::new(sq, sq), egui::Sense::hover());
            let painter = ui.painter_at(img_rect);

            // Blueprint background
            painter.rect_filled(img_rect, 0.0, Color32::from_rgb(5, 7, 8));

            // File-type icon placeholder (simulates blueprint)
            let icon = type_icon(file.ext.as_deref().unwrap_or(""));
            painter.text(
                img_rect.center(),
                egui::Align2::CENTER_CENTER,
                icon,
                egui::FontId::proportional(sq * 0.32),
                Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 60),
            );

            // Extension text (large)
            let ext_up = file.ext.as_deref().unwrap_or("").to_uppercase();
            painter.text(
                img_rect.center() + egui::vec2(0.0, sq * 0.22),
                egui::Align2::CENTER_CENTER,
                &ext_up,
                egui::FontId::monospace(sq * 0.09),
                Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 90),
            );

            // Reject overlay
            if rev.pick_flag == PICK_REJECT {
                painter.rect_filled(
                    img_rect, 0.0,
                    Color32::from_rgba_unmultiplied(180, 20, 20, 55),
                );
                painter.text(
                    img_rect.center(), egui::Align2::CENTER_CENTER, "âœ—",
                    egui::FontId::proportional(sq * 0.45),
                    Color32::from_rgba_unmultiplied(220, 60, 60, 200),
                );
            }

            // Corner brackets (FUI aesthetic)
            let s = 8.0;
            let corners = [
                (img_rect.min,                        [1.0,0.0,0.0,1.0f32]),   // TL
                (img_rect.right_top()  - egui::vec2(s,0.0), [-1.0,0.0,0.0,1.0]), // TR
                (img_rect.left_bottom() - egui::vec2(0.0,s),  [1.0,0.0,0.0,-1.0]), // BL
                (img_rect.max          - egui::vec2(s,s),   [-1.0,0.0,0.0,-1.0]), // BR
            ];
            for (origin, dirs) in &corners {
                let [dx, _dy, _dx2, dy] = dirs;
                let c_color = Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 80);
                // horizontal arm
                painter.line_segment(
                    [*origin, *origin + egui::vec2(dx * s, 0.0)],
                    egui::Stroke::new(1.0, c_color),
                );
                // vertical arm
                painter.line_segment(
                    [*origin, *origin + egui::vec2(0.0, dy * s)],
                    egui::Stroke::new(1.0, c_color),
                );
            }

            // Status text bottom
            let status = if rev.pick_flag == PICK_PICK {
                "PICK"
            } else if rev.pick_flag == PICK_REJECT {
                "REJECTED"
            } else {
                "PREVIEWÂ·READY"
            };
            painter.text(
                img_rect.center_bottom() - egui::vec2(0.0, 6.0),
                egui::Align2::CENTER_CENTER,
                status,
                egui::FontId::monospace(5.0),
                Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 50),
            );

            // â”€â”€ Meta panel â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            egui::Frame::none()
                .fill(Color32::from_rgb(10, 10, 10))
                .inner_margin(egui::Margin { left: 8.0, right: 8.0, top: 7.0, bottom: 7.0 })
                .show(ui, |ui| {
                    ui.set_max_width(width - 16.0);

                    meta_row(ui, "SOURCE", &file.device_name, Color32::from_gray(255));
                    ui.add(egui::Separator::default().horizontal().spacing(3.0));

                    meta_row(ui, "SIZE", &fmt_size(file.size), fg);
                    meta_row(ui, "DATE", &fmt_date(file.modified), Color32::from_gray(220));

                    let ext = file.ext.as_deref().unwrap_or("").to_lowercase();
                    if !ext.is_empty() {
                        ui.add(egui::Separator::default().horizontal().spacing(3.0));
                        meta_row(ui, "EXT",  &ext.to_uppercase(), Color32::from_gray(180));
                    }

                    ui.add_space(4.0);
                    ui.add(egui::Separator::default().horizontal().spacing(1.0));
                    ui.add_space(4.0);

                    // Stars
                    let rating = rev.rating.unwrap_or(0);
                    ui.horizontal(|ui| {
                        for star in 1u8..=5 {
                            let c = if star <= rating {
                                Color32::from_rgb(255, 210, 40)
                            } else {
                                Color32::from_gray(45)
                            };
                            ui.label(RichText::new("â˜…").size(9.0).color(c));
                        }
                    });

                    // PICK / RJCT buttons
                    ui.horizontal(|ui| {
                        let pick_active = rev.pick_flag == PICK_PICK;
                        let rjct_active = rev.pick_flag == PICK_REJECT;
                        let _ = ui.add(
                            egui::Button::new(
                                RichText::new("PICK")
                                    .size(7.5)
                                    .color(if pick_active { Colors::GREEN } else { Color32::from_gray(56) })
                            )
                            .fill(if pick_active { Color32::from_rgba_unmultiplied(0, 255, 65, 20) } else { Color32::TRANSPARENT })
                            .stroke(egui::Stroke::new(1.0, if pick_active { Colors::GREEN } else { Colors::BORDER2 }))
                        );
                        let _ = ui.add(
                            egui::Button::new(
                                RichText::new("RJCT")
                                    .size(7.5)
                                    .color(if rjct_active { Colors::RED } else { Color32::from_gray(56) })
                            )
                            .fill(if rjct_active { Color32::from_rgba_unmultiplied(255, 34, 68, 20) } else { Color32::TRANSPARENT })
                            .stroke(egui::Stroke::new(1.0, if rjct_active { Colors::RED } else { Colors::BORDER2 }))
                        );
                    });
                });
        })
        .response;

    resp
}

fn meta_row(ui: &mut Ui, label: &str, value: &str, value_color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(6.0).color(Color32::from_gray(68)).extra_letter_spacing(1.5));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(value).size(7.5).color(value_color).strong()
                )
                .truncate(true)
            );
        });
    });
}

// â”€â”€â”€ Selection action bar â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn render_selection_bar(ui: &mut Ui, state: &mut MediaIngestState, _actions: &mut MediaIngestActions) {
    let sel_count = state.selected_ids.len();
    let sel_size: u64 = state.results.iter()
        .filter(|f| state.selected_ids.contains(&f.id))
        .map(|f| f.size)
        .sum();

    egui::Frame::none()
        .fill(Color32::from_rgba_unmultiplied(0, 255, 65, 10))
        .stroke(egui::Stroke::new(1.0, Colors::GREEN))
        .inner_margin(egui::Margin { left: 14.0, right: 14.0, top: 0.0, bottom: 0.0 })
        .show(ui, |ui| {
            ui.set_min_height(36.0);
            ui.horizontal(|ui| {
                ui.set_min_height(36.0);

                ui.label(
                    RichText::new(format!("{} FILE{} SELECTED", sel_count, if sel_count == 1 { "" } else { "S" }))
                        .color(Colors::GREEN)
                        .size(9.0)
                        .strong(),
                );
                ui.label(
                    RichText::new(fmt_size(sel_size))
                        .color(Colors::GREEN_DIM)
                        .size(7.5),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(
                        egui::Button::new(RichText::new("âœ• CLEAR").size(8.0).color(Color32::from_gray(85)))
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0, Colors::BORDER2)),
                    ).clicked() {
                        state.selected_ids.clear();
                    }

                    if ui.add(
                        egui::Button::new(RichText::new("REJECT ALL").size(8.0).color(Colors::RED).strong())
                            .fill(Color32::from_rgba_unmultiplied(255, 34, 68, 20))
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

// â”€â”€â”€ Status bar â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
                    ui.label(RichText::new(item).size(7.0).color(Colors::TEXT_MUTED).extra_letter_spacing(1.0));
                    ui.add(egui::Separator::default().vertical().spacing(0.0));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("THEGRID Â· MEDIA INGEST Â· MVP")
                            .size(7.0)
                            .color(Color32::from_gray(21)),
                    );
                });
            });
        });
}

// â”€â”€â”€ Keyboard handling â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
}

// â”€â”€â”€ Hex logo (TheGrid brand mark) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€â”€ Helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn type_icon(ext: &str) -> &'static str {
    match MediaFileType::from_ext(ext) {
        MediaFileType::Raw   => "â—ˆ",
        MediaFileType::Video => "â–¶",
        MediaFileType::Photo => "âŠŸ",
        MediaFileType::Audio => "â™ª",
        MediaFileType::Psd   => "â—§",
        MediaFileType::Ai    => "â—­",
        MediaFileType::Drone => "â¬¡",
        MediaFileType::Doc   => "â‰¡",
        MediaFileType::All   => "â–¡",
    }
}

fn truncate_name(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        name.to_string()
    } else {
        let cut = name.char_indices().nth(max).map(|(i, _)| i).unwrap_or(name.len());
        format!("{}â€¦", &name[..cut])
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
        None => "â€”".to_string(),
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

/// Build the effective query string including quick-filter tokens.
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
