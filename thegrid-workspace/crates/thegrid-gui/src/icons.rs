// ═══════════════════════════════════════════════════════════════════════════════
// icons.rs — Centralized Glyph & Icon System
//
// "Brutalist" design uses high-contrast Unicode glyphs for a terminal aesthetic.
// This module provides standardized mappings for file types, events, and status.
// ═══════════════════════════════════════════════════════════════════════════════

use egui::Color32;
use crate::theme::Colors;

// Re-export phosphor for convenience in other modules
pub use egui_phosphor::regular as ph;

/// Standard Glyph Set — backed by Phosphor icon font.
/// All constants are Unicode chars from the Phosphor Private-Use-Area font
/// embedded via `egui_phosphor`. They render correctly once the font is
/// registered in `theme::configure_fonts`.
#[allow(dead_code)]
pub struct Glyphs;

#[allow(dead_code)]
impl Glyphs {
    // Brand
    pub const BRAND_HEX:    &'static str = ph::HEXAGON;            // hexagon outline
    pub const BRAND_HEX_F:  &'static str = ph::HEXAGON;            // (phosphor has one weight per variant)

    // UI Elements
    pub const DELETE:       &'static str = ph::X;
    pub const CLOSE:        &'static str = ph::X;
    pub const MAXIMIZE:     &'static str = ph::ARROWS_OUT;
    pub const MINIMIZE:     &'static str = ph::MINUS;
    pub const SEARCH:       &'static str = ph::MAGNIFYING_GLASS;
    pub const REFRESH:      &'static str = ph::ARROW_CLOCKWISE;
    pub const SCAN:         &'static str = ph::SCAN;
    pub const DOWNLOAD:     &'static str = ph::ARROW_DOWN;
    pub const SEND:         &'static str = ph::ARROW_UP;
    pub const LOCK:         &'static str = ph::LOCK_SIMPLE;
    pub const UNLOCK:       &'static str = ph::LOCK_SIMPLE_OPEN;

    // Flow Markers
    pub const CREATED:      &'static str = ph::PLUS_CIRCLE;
    pub const MODIFIED:     &'static str = ph::PENCIL_SIMPLE;
    pub const DELETED:      &'static str = ph::X_CIRCLE;

    // Status (for inline badge text)
    pub const STATUS_ONLINE:  &'static str = ph::CHECK_CIRCLE;
    pub const STATUS_UP:      &'static str = ph::WIFI_HIGH;
    pub const STATUS_OFFLINE: &'static str = ph::CIRCLE;
    pub const STATUS_AI:      &'static str = ph::BRAIN;

    // File Types
    pub const FILE_CODE:    &'static str = ph::FILE_CODE;
    pub const FILE_DOC:     &'static str = ph::FILE_DOC;
    pub const FILE_IMG:     &'static str = ph::IMAGE;
    pub const FILE_VIDEO:   &'static str = ph::VIDEO;
    pub const FILE_AUDIO:   &'static str = ph::FILE_AUDIO;
    pub const FILE_ZIP:     &'static str = ph::FILE_ZIP;
    pub const FILE_TEXT:    &'static str = ph::FILE;
    pub const FILE_EXE:     &'static str = ph::TERMINAL_WINDOW;
    pub const FILE_DEF:     &'static str = ph::FILE;

    // Arrows / Nav
    pub const ARROW_L:      &'static str = ph::ARROW_ELBOW_DOWN_RIGHT;
}

/// Map file extension to a brutalist glyph.
pub fn ext_to_glyph(ext: Option<&str>) -> &'static str {
    match ext {
        Some("rs") | Some("go") | Some("py") | Some("js") | Some("ts") |
        Some("cpp") | Some("c") | Some("h") | Some("java") | Some("swift") => Glyphs::FILE_CODE,
        
        Some("pdf") | Some("doc") | Some("docx") | Some("xls") | Some("xlsx") |
        Some("ppt") | Some("pptx") => Glyphs::FILE_DOC,
        
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") |
        Some("psd") | Some("ai") | Some("sketch") | Some("fig") => Glyphs::FILE_IMG,
        
        Some("mp4") | Some("mov") | Some("mkv") | Some("avi") => Glyphs::FILE_VIDEO,
        Some("mp3") | Some("wav") | Some("flac") | Some("aac") => Glyphs::FILE_AUDIO,
        
        Some("zip") | Some("tar") | Some("gz") | Some("rar") | Some("7z") => Glyphs::FILE_ZIP,
        
        Some("md") | Some("txt") | Some("log") | Some("toml") | Some("json") |
        Some("yaml") | Some("yml") => Glyphs::FILE_TEXT,
        
        Some("exe") | Some("msi") | Some("app") | Some("bat") | Some("sh") => Glyphs::FILE_EXE,
        
        _ => Glyphs::FILE_DEF,
    }
}

/// Map file extension to a color theme.
pub fn ext_to_color(ext: Option<&str>) -> Color32 {
    match ext {
        Some("rs") | Some("go") | Some("py") => Colors::GREEN,
        Some("pdf") | Some("doc") | Some("docx") => Colors::GREEN,
        Some("png") | Some("jpg") | Some("psd") | Some("fig") => Colors::AMBER,
        Some("mp4") | Some("mov") => Colors::RED,
        Some("zip") | Some("rar") => Colors::AMBER,
        _ => Colors::TEXT_DIM,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// THE GRID  —  Vector Logo  (egui Painter)
//
// Geometry:  flat-top hexagon  (top/bottom edges horizontal)
//   Outer hex   — bold border
//   Inner hex   — at 0.62·R
//   Radial spokes  — 6 lines: center → outer-hex vertex
//   Mid-ring connectors — 12 short lines creating the triangular lattice
//   Seed pattern  — 6 small circles at 0.45·R  +  center dot
//   Glow halos  — two transparent rings for the neon effect
// ═══════════════════════════════════════════════════════════════════════════════

/// Draw the THE GRID logo centered at `center` with outer-hexagon radius `r`.
/// `color` should be `Colors::GREEN` (#00ff41).  Pass a dimmer tone for
/// inactive / watermark uses.
pub fn draw_thegrid_logo(
    painter: &egui::Painter,
    center:  egui::Pos2,
    r:       f32,
    color:   Color32,
) {
    // ── helpers ────────────────────────────────────────────────────────────
    let hex_pt = |angle_deg: f32, radius: f32| -> egui::Pos2 {
        let a = angle_deg.to_radians();
        egui::pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
    };

    // flat-top: vertices at 0°, 60°, 120°, 180°, 240°, 300°
    let outer: Vec<egui::Pos2> = (0..6).map(|i| hex_pt(i as f32 * 60.0, r)).collect();
    let inner: Vec<egui::Pos2> = (0..6).map(|i| hex_pt(i as f32 * 60.0, r * 0.62)).collect();

    // ── glow halos (outermost first, most transparent) ─────────────────────
    let glow_a1 = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 18);
    let glow_a2 = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 38);
    painter.circle_stroke(center, r * 1.04, egui::Stroke::new(r * 0.08, glow_a1));
    painter.circle_stroke(center, r * 0.98, egui::Stroke::new(r * 0.05, glow_a2));

    // ── outer hexagon ──────────────────────────────────────────────────────
    let thick = (r * 0.055).max(1.5);
    for i in 0..6usize {
        painter.line_segment([outer[i], outer[(i + 1) % 6]], egui::Stroke::new(thick, color));
    }

    // ── inner hexagon ──────────────────────────────────────────────────────
    let thin = (r * 0.025).max(0.8);
    for i in 0..6usize {
        painter.line_segment([inner[i], inner[(i + 1) % 6]], egui::Stroke::new(thin, color));
    }

    // ── radial spokes: center → outer vertices (6 lines) ──────────────────
    for v in &outer {
        painter.line_segment([center, *v], egui::Stroke::new(thin, color));
    }

    // ── mid-ring triangular connectors ─────────────────────────────────────
    // Each outer-hex edge has a midpoint; connect those midpoints to form
    // the subdivided triangular lattice typical in geodesic / hex grids.
    let mid_outer: Vec<egui::Pos2> = (0..6).map(|i| {
        let a = ((i as f32) * 60.0 + 30.0).to_radians();
        egui::pos2(center.x + r * a.cos(), center.y + r * a.sin())
    }).collect();

    for i in 0..6usize {
        // midpoint of outer edge → corresponding inner vertex
        painter.line_segment([mid_outer[i], inner[i]],        egui::Stroke::new(thin, color));
        painter.line_segment([mid_outer[i], inner[(i+1)%6]], egui::Stroke::new(thin, color));
    }

    // ── inner sub-triangles (inner hex diagonals) ──────────────────────────
    for i in 0..6usize {
        painter.line_segment([inner[i], inner[(i + 2) % 6]], egui::Stroke::new(thin * 0.7, color));
    }

    // ── seed-of-life circles (6 petals + center) ───────────────────────────
    let seed_r   = r * 0.17;
    let seed_off = r * 0.32;
    let seed_stroke = egui::Stroke::new((thin * 0.8).max(0.6), color);
    painter.circle_stroke(center, seed_r, seed_stroke);
    for i in 0..6usize {
        let a = (i as f32 * 60.0).to_radians();
        let sc = egui::pos2(center.x + seed_off * a.cos(), center.y + seed_off * a.sin());
        painter.circle_stroke(sc, seed_r, seed_stroke);
    }

    // ── center bright dot ──────────────────────────────────────────────────
    painter.circle_filled(center, (r * 0.04).max(1.5), color);
}

/// Draw a compact (single-line-height) THE GRID wordmark below a given rect.
pub fn draw_thegrid_wordmark(
    painter:  &egui::Painter,
    rect:     egui::Rect,
    color:    Color32,
    font_id:  egui::FontId,
) {
    let text_pos = egui::pos2(rect.center().x, rect.max.y + font_id.size * 0.15);
    painter.text(
        text_pos,
        egui::Align2::CENTER_TOP,
        "THE GRID",
        font_id,
        color,
    );
}

