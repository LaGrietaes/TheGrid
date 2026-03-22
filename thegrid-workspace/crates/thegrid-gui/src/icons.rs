// ═══════════════════════════════════════════════════════════════════════════════
// icons.rs — Centralized Glyph & Icon System
//
// "Brutalist" design uses high-contrast Unicode glyphs for a terminal aesthetic.
// This module provides standardized mappings for file types, events, and status.
// ═══════════════════════════════════════════════════════════════════════════════

use egui::Color32;
use crate::theme::Colors;

/// Standard Glyph Set
#[allow(dead_code)]
pub struct Glyphs;

#[allow(dead_code)]
impl Glyphs {
    // Brand
    pub const BRAND_HEX:    &'static str = "⬡";    // Hexagon (logo)
    pub const BRAND_HEX_F:  &'static str = "⬢";    // Filled Hexagon (AI)


    // UI Elements
    pub const DELETE:       &'static str = "✕";
    pub const CLOSE:        &'static str = "✕";
    pub const MAXIMIZE:     &'static str = "□";
    pub const MINIMIZE:     &'static str = "─";
    pub const SEARCH:       &'static str = "⌕";
    pub const REFRESH:      &'static str = "↻";
    pub const SCAN:         &'static str = "⚭";
    pub const DOWNLOAD:     &'static str = "↓";
    pub const SEND:         &'static str = "↑";
    pub const LOCK:         &'static str = "⬖";
    pub const UNLOCK:       &'static str = "⬗";

    // Flow Markers
    pub const CREATED:      &'static str = "⊕";    // Circled Plus
    pub const MODIFIED:     &'static str = "⊙";    // Circled Dot
    pub const DELETED:      &'static str = "⊘";    // Circled Slash

    // File Types
    pub const FILE_CODE:    &'static str = "◈";    // Code (Black Rhombus)
    pub const FILE_DOC:     &'static str = "⊟";    // Document (Minus in Box)
    pub const FILE_IMG:     &'static str = "⊡";    // Image (Dot in Box)
    pub const FILE_VIDEO:   &'static str = "▷";    // Video (Triangle)
    pub const FILE_AUDIO:   &'static str = "♫";    // Audio (Notes)
    pub const FILE_ZIP:     &'static str = "⊛";    // Archive (Circled Asterisk)
    pub const FILE_TEXT:    &'static str = "≡";    // Text (Identical To)
    pub const FILE_EXE:     &'static str = "⊕";    // Executable
    pub const FILE_DEF:     &'static str = "◻";    // Default (Square)

    // Arrows
    pub const ARROW_L:      &'static str = "↳";
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

