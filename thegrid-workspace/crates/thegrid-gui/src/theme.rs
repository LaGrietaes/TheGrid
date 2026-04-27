// ═══════════════════════════════════════════════════════════════════════════════
// theme.rs — Brutalist THE GRID Visual System
//
// Design language: Terminal green on near-black. Zero border radius. Heavy
// monospace typography. High contrast. No gradients. No drop shadows.
// Everything looks like it belongs on a rack server console.
// ═══════════════════════════════════════════════════════════════════════════════

use egui::{Color32, FontFamily, FontId, Rounding, Stroke, Style, Visuals, Context};
use egui::style::{Selection, WidgetVisuals, Widgets};

// ── Color Palette ─────────────────────────────────────────────────────────────

pub struct Colors;

impl Colors {
    // Backgrounds
    pub const BG:         Color32 = Color32::from_rgb(8,   8,   8);    // #080808
    pub const BG_PANEL:   Color32 = Color32::from_rgb(15,  15,  15);   // #0f0f0f
    pub const BG_WIDGET:  Color32 = Color32::from_rgb(20,  20,  20);   // #141414
    pub const BG_HOVER:   Color32 = Color32::from_rgb(28,  28,  28);   // #1c1c1c
    pub const BG_ACTIVE:  Color32 = Color32::from_rgb(0,   30,  8);    // subtle green bg

    // Borders
    pub const BORDER:     Color32 = Color32::from_rgb(30,  30,  30);   // #1e1e1e
    pub const BORDER2:    Color32 = Color32::from_rgb(42,  42,  42);   // #2a2a2a
    #[allow(dead_code)]
    pub const BORDER_FOC: Color32 = Color32::from_rgb(0,   255, 65);   // green focus ring

    // Accent colors
    pub const GREEN:      Color32 = Color32::from_rgb(0,   255, 65);   // #00ff41
    pub const GREEN_DIM:  Color32 = Color32::from_rgb(0,   128, 32);   // #008020
    pub const AMBER:      Color32 = Color32::from_rgb(255, 214, 0);    // #ffd600
    pub const RED:        Color32 = Color32::from_rgb(255, 34,  68);   // #ff2244

    // Device display-state tokens (Phase 4)
    pub const STATE_ONLINE:            Color32 = Color32::from_rgb(0,   255, 65);   // green  — same as GREEN
    pub const STATE_SYNCING:           Color32 = Color32::from_rgb(0,   180, 200);  // cyan
    pub const STATE_INDEXING:          Color32 = Color32::from_rgb(255, 214, 0);    // amber
    pub const STATE_COMPUTE_BORROW:    Color32 = Color32::from_rgb(120, 80,  255);  // violet — we are delegating out
    pub const STATE_COMPUTE_PROVIDE:   Color32 = Color32::from_rgb(50,  140, 255);  // blue   — we are serving a peer
    pub const STATE_BUSY:              Color32 = Color32::from_rgb(255, 130, 0);    // orange
    pub const STATE_ERROR:             Color32 = Color32::from_rgb(255, 34,  68);   // red    — same as RED
    pub const STATE_OFFLINE:           Color32 = Color32::from_rgb(51,  51,  51);   // muted

    // Text
    pub const TEXT:       Color32 = Color32::from_rgb(232, 232, 232);  // #e8e8e8
    pub const TEXT_DIM:   Color32 = Color32::from_rgb(102, 102, 102);  // #666
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(51,  51,  51);   // #333
}

// ── Apply theme to egui context ────────────────────────────────────────────────

pub fn apply(ctx: &Context) {
    // Font setup: every text element uses monospace
    configure_fonts(ctx);

    // Visual overrides
    ctx.set_visuals(build_visuals());

    // Style overrides (spacing, text sizes)
    ctx.set_style(build_style());
}

fn configure_fonts(ctx: &Context) {
    // egui ships with a bundled monospace font (Hack).
    // We use it for all text to enforce the terminal aesthetic.
    // Phase 2 enhancement: embed JetBrains Mono as bytes and install it here.
    //
    // To embed a custom font:
    //   let font_data = egui::FontData::from_static(include_bytes!("../fonts/JetBrainsMono-Regular.ttf"));
    //   fonts.font_data.insert("JetBrainsMono".to_owned(), font_data);
    //   fonts.families.get_mut(&FontFamily::Monospace).unwrap().insert(0, "JetBrainsMono".to_owned());
    //   fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "JetBrainsMono".to_owned());
    //   ctx.set_fonts(fonts);

    let fonts = egui::FontDefinitions::default();
    
    // Use default egui fonts (Hack for Monospace, Proportional for others)
    // We don't need explicit fallbacks now that we use Vector graphics for all symbols.
    ctx.set_fonts(fonts);

    // Override the default text style sizes to monospace at our scale
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (egui::TextStyle::Small,       FontId::new(10.0, FontFamily::Monospace)),
        (egui::TextStyle::Body,        FontId::new(12.0, FontFamily::Monospace)),
        (egui::TextStyle::Monospace,   FontId::new(12.0, FontFamily::Monospace)),
        (egui::TextStyle::Button,      FontId::new(11.0, FontFamily::Monospace)),
        (egui::TextStyle::Heading,     FontId::new(14.0, FontFamily::Monospace)),
    ].into();
    ctx.set_style(style);
}

fn build_visuals() -> Visuals {
    let mut v = Visuals::dark();

    // Window / panel backgrounds
    v.panel_fill        = Colors::BG_PANEL;
    v.window_fill       = Colors::BG_PANEL;
    v.extreme_bg_color  = Colors::BG;

    // Remove ALL rounding — brutalist aesthetic means hard corners everywhere
    v.window_rounding   = Rounding::ZERO;
    v.menu_rounding     = Rounding::ZERO;
    v.popup_shadow      = egui::epaint::Shadow::NONE;
    v.window_shadow     = egui::epaint::Shadow::NONE;

    // Selection highlight (text selection, focused items)
    v.selection = Selection {
        bg_fill: Colors::BG_ACTIVE,
        stroke:  Stroke::new(1.0, Colors::GREEN),
    };

    // Hyperlink color
    v.hyperlink_color = Colors::GREEN;

    // Override widget visuals for all states
    v.widgets = build_widgets();

    v
}

fn build_widgets() -> Widgets {
    // Helper: widget style for a given background color
    let make = |bg: Color32, border: Color32, text: Color32| WidgetVisuals {
        bg_fill:         bg,
        weak_bg_fill:    bg,
        bg_stroke:       Stroke::new(1.0, border),
        rounding:        Rounding::ZERO,   // hard corners everywhere
        fg_stroke:       Stroke::new(1.0, text),
        expansion:       0.0,
    };

    Widgets {
        noninteractive: make(Colors::BG_WIDGET,  Colors::BORDER,  Colors::TEXT_DIM),
        inactive:       make(Colors::BG_WIDGET,  Colors::BORDER2, Colors::TEXT_DIM),
        hovered:        make(Colors::BG_HOVER,   Colors::GREEN_DIM, Colors::TEXT),
        active:         make(Colors::BG_ACTIVE,  Colors::GREEN,   Colors::GREEN),
        open:           make(Colors::BG_HOVER,   Colors::BORDER2, Colors::TEXT),
    }
}

fn build_style() -> Style {
    let mut s = Style::default();
    s.spacing.item_spacing    = egui::vec2(8.0, 6.0);
    s.spacing.window_margin   = egui::Margin::same(0.0);
    s.spacing.button_padding  = egui::vec2(12.0, 6.0);
    s.spacing.indent          = 16.0;
    s.spacing.slider_width    = 120.0;
    s.spacing.text_edit_width = 200.0;
    s
}

// ── Reusable styled widgets ────────────────────────────────────────────────────
//
// These helper functions are used in the views to ensure consistent styling
// without repeating `egui::RichText::new(...).color(...)` everywhere.

use egui::{RichText, Ui, Response};

/// Large label styled as a section heading (e.g., "// NODE INFO")
pub fn section_title(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .color(Colors::GREEN)
            .size(9.0)
            .strong()
    );
}

/// Primary action button — green background, black text
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let btn = egui::Button::new(
        RichText::new(label)
            .color(Color32::BLACK)
            .size(11.0)
            .strong()
    )
    .fill(Colors::GREEN)
    .stroke(Stroke::NONE)
    .min_size(egui::vec2(0.0, 32.0));
    ui.add(btn)
}

/// Secondary action button — transparent background, dim text
pub fn secondary_button(ui: &mut Ui, label: &str) -> Response {
    let btn = egui::Button::new(
        RichText::new(label)
            .color(Colors::TEXT_DIM)
            .size(10.0)
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::new(1.0, Colors::BORDER2))
    .min_size(egui::vec2(0.0, 28.0));
    ui.add(btn)
}

/// Danger action button — red border, red text (for destructive operations)
pub fn danger_button(ui: &mut Ui, label: &str) -> Response {
    let btn = egui::Button::new(
        RichText::new(label)
            .color(Colors::RED)
            .size(10.0)
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::new(1.0, Colors::RED))
    .min_size(egui::vec2(0.0, 26.0));
    ui.add(btn)
}

/// Micro button (e.g., "SCAN", "↓ DL")
pub fn micro_button(ui: &mut Ui, label: &str) -> Response {
    let btn = egui::Button::new(
        RichText::new(label)
            .color(Colors::TEXT_DIM)
            .size(9.0)
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::new(1.0, Colors::BORDER))
    .min_size(egui::vec2(0.0, 22.0));
    ui.add(btn)
}

/// Styled text input field (monospace, green focus border)
pub fn text_input<'a>(text: &'a mut String, hint: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text)
        .hint_text(hint)
        .font(FontId::new(11.0, FontFamily::Monospace))
        .desired_width(f32::INFINITY)
}

/// Password field (for API key input)
pub fn password_input<'a>(text: &'a mut String, hint: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text)
        .hint_text(hint)
        .password(true)
        .font(FontId::new(11.0, FontFamily::Monospace))
        .desired_width(f32::INFINITY)
}






#[allow(dead_code)]
pub enum IconType {
    RDP,
    Folder,
    #[allow(dead_code)]
    Network,
    Pulse,
    Desktop,
    Laptop,
    Server,
    Cpu,
    Ram,
    Disk,
    Gpu,
    Ai,
    Globe,
    Power,
    Database,
    Camera,
    Microphone,
    Speakers,
    Tablet,
    Smartphone,
    Chromebook,
    FileUnknown,
    FileExt(String),
}

pub fn get_file_icon(ext: &str) -> (IconType, Color32) {
    if ext.is_empty() { return (IconType::FileUnknown, Colors::TEXT_DIM); }
    let mut ext_lower = ext.to_lowercase();
    if ext_lower.len() > 4 {
        ext_lower.truncate(4);
    }
    match ext_lower.as_str() {
        "txt" | "md" | "json" | "xml" | "yml" | "yaml" | "csv" | "log" | "doc" | "docx" => (IconType::FileExt(ext_lower), Colors::GREEN),
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "svg" => (IconType::FileExt(ext_lower), Colors::AMBER),
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => (IconType::FileExt(ext_lower), Color32::from_rgb(30, 215, 96)),
        "mp4" | "mkv" | "avi" | "mov" | "webm" => (IconType::FileExt(ext_lower), Color32::from_rgb(255, 140, 0)),
        "zip" | "tar" | "gz" | "rar" | "7z" | "bz2" => (IconType::FileExt(ext_lower), Color32::from_rgb(160, 160, 160)),
        "rs" | "js" | "ts" | "py" | "c" | "cpp" | "h" | "go" | "html" | "css" | "sh" | "bash" | "bat" | "ps1" => (IconType::FileExt(ext_lower), Color32::from_rgb(46, 204, 113)),
        "exe" | "msi" | "apk" | "dll" | "so" | "dylib" | "bin" => (IconType::FileExt(ext_lower), Colors::RED),
        "pdf" => (IconType::FileExt(ext_lower), Color32::from_rgb(244, 15, 2)),   // Adobe Red
        "ai"  => (IconType::FileExt(ext_lower), Color32::from_rgb(255, 154, 0)),  // AI Orange
        "psd" => (IconType::FileExt(ext_lower), Color32::from_rgb(49, 197, 244)), // PS Blue
        "indd" => (IconType::FileExt(ext_lower), Color32::from_rgb(255, 51, 102)), // ID Pink
        "fig" => (IconType::FileExt(ext_lower), Color32::from_rgb(162, 89, 255)), // Figma Purple
        "xls" | "xlsx" => (IconType::FileExt(ext_lower), Color32::from_rgb(39, 174, 96)), // Excel Green
        _ => (IconType::FileExt(ext_lower), Colors::TEXT_DIM),
    }
}

pub fn draw_vector_icon(ui: &mut Ui, rect: egui::Rect, icon: IconType, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.45;
    let stroke = egui::Stroke::new(1.5, color);

    match icon {
        IconType::RDP => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c - egui::vec2(s*0.2, s*0.2), egui::vec2(s*1.2, s*1.0)), egui::Rounding::ZERO, stroke);
            ui.painter().rect_stroke(egui::Rect::from_center_size(c + egui::vec2(s*0.2, s*0.2), egui::vec2(s*1.2, s*1.0)), egui::Rounding::ZERO, stroke);
        }
        IconType::Folder => {
            let points = vec![
                c + egui::vec2(-s, s*0.7),
                c + egui::vec2(s, s*0.7),
                c + egui::vec2(s, -s*0.4),
                c + egui::vec2(s*0.3, -s*0.4),
                c + egui::vec2(s*0.1, -s*0.7),
                c + egui::vec2(-s, -s*0.7),
            ];
            ui.painter().add(egui::Shape::closed_line(points, stroke));
        }
        IconType::Network => {
            ui.painter().circle_stroke(c + egui::vec2(0.0, -s*0.6), s*0.3, stroke);
            ui.painter().circle_stroke(c + egui::vec2(-s*0.7, s*0.6), s*0.3, stroke);
            ui.painter().circle_stroke(c + egui::vec2(s*0.7, s*0.6), s*0.3, stroke);
            ui.painter().line_segment([c + egui::vec2(0.0, -s*0.3), c + egui::vec2(-s*0.4, s*0.3)], stroke);
            ui.painter().line_segment([c + egui::vec2(0.0, -s*0.3), c + egui::vec2(s*0.4, s*0.3)], stroke);
        }
        IconType::Pulse => {
            let points = vec![
                c + egui::vec2(-s, 0.0),
                c + egui::vec2(-s*0.5, 0.0),
                c + egui::vec2(-s*0.3, -s),
                c + egui::vec2(0.0, s),
                c + egui::vec2(s*0.3, 0.0),
                c + egui::vec2(s, 0.0),
            ];
            ui.painter().add(egui::Shape::line(points, stroke));
        }
        IconType::Desktop => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c - egui::vec2(0.0, s*0.2), egui::vec2(s*1.6, s*1.2)), egui::Rounding::ZERO, stroke);
            ui.painter().line_segment([c + egui::vec2(-s*0.4, s*0.9), c + egui::vec2(s*0.4, s*0.9)], stroke);
            ui.painter().line_segment([c + egui::vec2(0.0, s*0.4), c + egui::vec2(0.0, s*0.9)], stroke);
        }
        IconType::Laptop => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c - egui::vec2(0.0, s*0.3), egui::vec2(s*1.4, s*1.0)), egui::Rounding::ZERO, stroke);
            ui.painter().line_segment([c + egui::vec2(-s, s*0.7), c + egui::vec2(s, s*0.7)], stroke);
            ui.painter().line_segment([c + egui::vec2(-s*0.9, s*0.7), c + egui::vec2(-s, s*0.9)], stroke);
            ui.painter().line_segment([c + egui::vec2(s*0.9, s*0.7), c + egui::vec2(s, s*0.9)], stroke);
            ui.painter().line_segment([c + egui::vec2(-s, s*0.9), c + egui::vec2(s, s*0.9)], stroke);
        }
        IconType::Server => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(s*1.2, s*1.6)), egui::Rounding::ZERO, stroke);
            ui.painter().line_segment([c + egui::vec2(-s*0.6, -s*0.3), c + egui::vec2(s*0.6, -s*0.3)], stroke);
            ui.painter().line_segment([c + egui::vec2(-s*0.6, 0.3), c + egui::vec2(s*0.6, 0.3)], stroke);
            ui.painter().circle_filled(c + egui::vec2(s*0.3, -s*0.6), 1.5, color);
            ui.painter().circle_filled(c + egui::vec2(s*0.3, 0.6), 1.5, color);
        }
        IconType::Cpu => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(s, s)), egui::Rounding::ZERO, stroke);
            ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(s*0.4, s*0.4)), egui::Rounding::ZERO, stroke);
            for i in -1..=1 {
                let off = i as f32 * s * 0.35;
                ui.painter().line_segment([c + egui::vec2(off, -s*0.7), c + egui::vec2(off, -s*0.5)], stroke);
                ui.painter().line_segment([c + egui::vec2(off, s*0.5), c + egui::vec2(off, s*0.7)], stroke);
                ui.painter().line_segment([c + egui::vec2(-s*0.7, off), c + egui::vec2(-s*0.5, off)], stroke);
                ui.painter().line_segment([c + egui::vec2(s*0.5, off), c + egui::vec2(s*0.7, off)], stroke);
            }
        }
        IconType::Ram => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(s*1.6, s*0.6)), egui::Rounding::ZERO, stroke);
            for i in -2..=2 {
                let off = i as f32 * s * 0.3;
                ui.painter().line_segment([c + egui::vec2(off, -s*0.3), c + egui::vec2(off, 0.0)], stroke);
            }
        }
        IconType::Disk => {
            ui.painter().circle_stroke(c, s*0.7, stroke);
            ui.painter().circle_stroke(c, s*0.2, stroke);
            ui.painter().line_segment([c + egui::vec2(s*0.4, -s*0.4), c + egui::vec2(s*0.8, -s*0.8)], stroke);
        }
        IconType::Gpu => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(s*1.6, s*1.0)), egui::Rounding::ZERO, stroke);
            ui.painter().circle_stroke(c, s*0.3, stroke);
            ui.painter().line_segment([c + egui::vec2(-s*0.8, -s*0.5), c + egui::vec2(-s*0.6, -s*0.5)], stroke);
        }
        IconType::Ai => {
            let r = s;
            let mut points = vec![];
            for i in 0..6 {
                let angle = std::f32::consts::PI / 3.0 * i as f32 + std::f32::consts::PI / 2.0;
                points.push(c + egui::vec2(r * angle.cos(), r * angle.sin()));
            }
            ui.painter().add(egui::Shape::closed_line(points, stroke));
            ui.painter().circle_filled(c, s*0.3, color);
        }
        IconType::Camera => {
            ui.painter().circle_stroke(c, s*0.8, stroke);
            ui.painter().circle_stroke(c, s*0.3, stroke);
            ui.painter().circle_filled(c, 1.5, color);
        }
        IconType::Microphone => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c - egui::vec2(0.0, s*0.2), egui::vec2(s*0.8, s*1.2)), egui::Rounding::same(s*0.4), stroke);
            ui.painter().line_segment([c + egui::vec2(0.0, s*0.4), c + egui::vec2(0.0, s*0.8)], stroke);
            ui.painter().line_segment([c + egui::vec2(-s*0.5, s*0.8), c + egui::vec2(s*0.5, s*0.8)], stroke);
        }
        IconType::Speakers => {
            let points = vec![
                c + egui::vec2(-s*0.2, -s*0.4),
                c + egui::vec2(s*0.4, -s*0.8),
                c + egui::vec2(s*0.4, s*0.8),
                c + egui::vec2(-s*0.2, s*0.4),
            ];
            ui.painter().add(egui::Shape::closed_line(points, stroke));
            ui.painter().rect_stroke(egui::Rect::from_center_size(c - egui::vec2(s*0.5, 0.0), egui::vec2(s*0.6, s*0.6)), egui::Rounding::ZERO, stroke);
        }
        IconType::Tablet => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(s*1.2, s*1.6)), egui::Rounding::same(s*0.1), stroke);
            ui.painter().circle_stroke(c + egui::vec2(0.0, s*0.65), 1.5, stroke);
        }
        IconType::Smartphone => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(s*0.8, s*1.6)), egui::Rounding::same(s*0.1), stroke);
            ui.painter().circle_stroke(c + egui::vec2(0.0, s*0.65), 1.2, stroke);
        }
        IconType::Chromebook => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c - egui::vec2(0.0, s*0.2), egui::vec2(s*1.6, s*1.1)), egui::Rounding::ZERO, stroke);
            ui.painter().line_segment([c + egui::vec2(-s*0.9, s*0.5), c + egui::vec2(s*0.9, s*0.5)], stroke);
            ui.painter().line_segment([c + egui::vec2(-s, s*0.5), c + egui::vec2(-s*1.1, s*0.7)], stroke);
            ui.painter().line_segment([c + egui::vec2(s, s*0.5), c + egui::vec2(s*1.1, s*0.7)], stroke);
            ui.painter().line_segment([c + egui::vec2(-s*1.1, s*0.7), c + egui::vec2(s*1.1, s*0.7)], stroke);
        }
        IconType::Globe => {
            ui.painter().circle_stroke(c, s * 0.8, stroke);
            ui.painter().line_segment([c - egui::vec2(s*0.8, 0.0), c + egui::vec2(s*0.8, 0.0)], stroke);
            ui.painter().line_segment([c - egui::vec2(0.0, s*0.8), c + egui::vec2(0.0, s*0.8)], stroke);
        }
        IconType::Power => {
            ui.painter().circle_stroke(c, s * 0.6, stroke);
            ui.painter().line_segment([c - egui::vec2(0.0, s*0.3), c - egui::vec2(0.0, s*0.9)], stroke);
        }
        IconType::Database => {
             ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(s*1.2, s*1.6)), egui::Rounding::ZERO, stroke);
             ui.painter().line_segment([c - egui::vec2(s*0.6, s*0.2), c + egui::vec2(s*0.6, -s*0.2)], stroke);
             ui.painter().line_segment([c - egui::vec2(s*0.6, -s*0.2), c + egui::vec2(s*0.6, s*0.2)], stroke);
        }

        IconType::FileUnknown => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(s*1.2, s*1.6)), egui::Rounding::ZERO, stroke);
            // Question markish
            ui.painter().circle_stroke(c + egui::vec2(0.0, -s*0.2), s*0.2, stroke);
            ui.painter().circle_filled(c + egui::vec2(0.0, s*0.4), 1.5, color);
        }
        IconType::FileExt(ref ext) => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(s*1.8, s*1.8)), egui::Rounding::same(1.0), stroke);
            ui.painter().text(c, egui::Align2::CENTER_CENTER, ext.to_uppercase(), egui::FontId::proportional(s * 0.9), color);
        }
    }
}

/// Renders a terminal/device icon with a pulsing CRT phosphor glow effect.
pub fn render_crt_icon(ui: &mut Ui, icon_type: IconType, size: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    
    let time = ui.input(|i| i.time);
    let pulse = ((time * 4.0).sin() as f32 * 0.15 + 0.85).max(0.1);
    
    // Draw the glow
    draw_vector_icon(ui, rect, icon_type, color.linear_multiply(pulse * 0.4));
    
    // Force active repaint for the animation
    ui.ctx().request_repaint();

    response
}

/// Status badge (ONLINE/OFFLINE/etc.) with optional icon
pub fn status_badge(ui: &mut Ui, label: &str, icon: Option<IconType>, active: bool) {
    let color = if active { Colors::GREEN } else { Colors::TEXT_MUTED };
    
    egui::Frame::none()
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0, color))
        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(icon) = icon {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    draw_vector_icon(ui, rect, icon, color);
                    ui.add_space(4.0);
                }
                ui.label(RichText::new(label).color(color).size(9.0).strong());
            });
        });
}

/// Map a DeviceDisplayState to its canonical theme color.
pub fn device_state_color(state: &thegrid_core::models::DeviceDisplayState) -> Color32 {
    use thegrid_core::models::DeviceDisplayState as DS;
    match state {
        DS::Online             => Colors::STATE_ONLINE,
        DS::Syncing            => Colors::STATE_SYNCING,
        DS::Indexing           => Colors::STATE_INDEXING,
        DS::ComputeBorrowing   => Colors::STATE_COMPUTE_BORROW,
        DS::ComputeProviding   => Colors::STATE_COMPUTE_PROVIDE,
        DS::Busy               => Colors::STATE_BUSY,
        DS::Error(_)           => Colors::STATE_ERROR,
        DS::Offline            => Colors::STATE_OFFLINE,
    }
}

/// Colored indicator dot
pub fn status_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// Horizontal rule styled as a terminal separator
#[allow(dead_code)]
pub fn separator(ui: &mut Ui) {
    ui.add(egui::Separator::default().spacing(0.0));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Fx — Sci-fi visual effects painter utilities
//
// These helpers paint raw geometry for the HUD aesthetic:
//   • Glow halos (layered alpha circles / stroked rects)
//   • Gradient fill via vertex-colored mesh
//   • Clipped-corner (chamfered) border polygon
//   • Ghost-number watermark background
//   • Raised-panel depth illusion (top highlight, bottom shadow)
//   • Scan-line overlay
// ═══════════════════════════════════════════════════════════════════════════════

use egui::epaint::Mesh;
use egui::pos2;

pub struct Fx;

impl Fx {
    /// Draw a rectangular glow halo around `rect`.
    /// `color` is the inner glow color; alpha falls off over `layers` concentric
    /// strokes of increasing width.
    pub fn glow_rect(painter: &egui::Painter, rect: egui::Rect, color: Color32, layers: u8) {
        for i in 1..=layers {
            let t = i as f32 / layers as f32;
            let alpha = ((1.0 - t) * 120.0) as u8;
            let expand = i as f32 * 1.5;
            let r = rect.expand(expand);
            painter.rect_stroke(
                r,
                Rounding::ZERO,
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(
                    color.r(), color.g(), color.b(), alpha,
                )),
            );
        }
    }

    /// Draw a pulsing glow around `rect`. Call every frame from `update()` to animate.
    /// `time` is `ctx.input(|i| i.time)`.
    pub fn pulse_glow_rect(painter: &egui::Painter, rect: egui::Rect, color: Color32, time: f64) {
        let pulse = ((time * 2.5).sin() as f32 * 0.4 + 0.6).clamp(0.2, 1.0);
        let color_pulsed = Color32::from_rgba_unmultiplied(
            color.r(), color.g(), color.b(),
            (pulse * 180.0) as u8,
        );
        Self::glow_rect(painter, rect, color_pulsed, 5);
    }

    /// Gradient fill for a rect: `top_color` at the top edge, `bot_color` at the bottom.
    pub fn gradient_rect(painter: &egui::Painter, rect: egui::Rect, top_color: Color32, bot_color: Color32) {
        let mut mesh = Mesh::default();
        // 4 vertices: TL, TR, BR, BL
        let tl = rect.left_top();
        let tr = rect.right_top();
        let br = rect.right_bottom();
        let bl = rect.left_bottom();
        mesh.colored_vertex(tl, top_color);  // 0
        mesh.colored_vertex(tr, top_color);  // 1
        mesh.colored_vertex(br, bot_color);  // 2
        mesh.colored_vertex(bl, bot_color);  // 3
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
    }

    /// Raised panel illusion: thin bright line on top edge, dim line on bottom edge.
    /// Gives the impression of a physical panel lit from above.
    pub fn raised_panel_border(painter: &egui::Painter, rect: egui::Rect, accent: Color32) {
        let highlight = Color32::from_rgba_unmultiplied(
            (accent.r() / 2).saturating_add(80),
            (accent.g() / 2).saturating_add(80),
            (accent.b() / 2).saturating_add(80),
            90,
        );
        let shadow = Color32::from_rgba_unmultiplied(0, 0, 0, 160);

        // Top-left and top-right corners → top highlight
        painter.line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, highlight),
        );
        painter.line_segment(
            [rect.left_top(), rect.left_bottom()],
            Stroke::new(1.0, highlight),
        );
        // Bottom and right → shadow
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, shadow),
        );
        painter.line_segment(
            [rect.right_top(), rect.right_bottom()],
            Stroke::new(1.0, shadow),
        );
    }

    /// Chamfered (clipped-corner) border: cuts the 4 corners by `clip` pixels.
    /// Draws a single-pixel stroke polygon.
    pub fn chamfered_border(painter: &egui::Painter, rect: egui::Rect, clip: f32, color: Color32, width: f32) {
        let l = rect.left();
        let r = rect.right();
        let t = rect.top();
        let b = rect.bottom();
        let c = clip;
        let points: Vec<egui::Pos2> = vec![
            pos2(l + c, t),
            pos2(r - c, t),
            pos2(r,     t + c),
            pos2(r,     b - c),
            pos2(r - c, b),
            pos2(l + c, b),
            pos2(l,     b - c),
            pos2(l,     t + c),
        ];
        painter.add(egui::Shape::closed_line(points, Stroke::new(width, color)));
    }

    /// Ghost-number watermark: draws a very large, low-alpha number string
    /// centred in `rect`. Good for node count, status codes, etc.
    pub fn ghost_number(painter: &egui::Painter, rect: egui::Rect, text: &str, color: Color32, size: f32) {
        let ghost = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 18);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::new(size, egui::FontFamily::Monospace),
            ghost,
        );
    }

    /// Horizontal scan-line overlay over `rect`. `time` drives a slow drift.
    /// Draws only every Nth pixel row as a semi-transparent stripe.
    pub fn scanlines(painter: &egui::Painter, rect: egui::Rect, time: f64) {
        let drift = ((time * 8.0) as f32).rem_euclid(8.0);
        let stripe_color = Color32::from_rgba_unmultiplied(0, 0, 0, 28);
        let mut y = rect.top() + drift;
        while y < rect.bottom() {
            let p0 = pos2(rect.left(), y);
            let p1 = pos2(rect.right(), y);
            painter.line_segment([p0, p1], Stroke::new(1.0, stripe_color));
            y += 8.0;
        }
    }

    /// Inner glow fill: semi-transparent `color` gradient fading from center
    /// to edges, useful for making a panel appear lit from within.
    pub fn inner_glow(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
        let c = rect.center();
        let max_r = rect.width().min(rect.height()) * 0.6;
        for i in 0..6u8 {
            let t = i as f32 / 6.0;
            let alpha = ((1.0 - t) * 40.0) as u8;
            let r_px = max_r * t;
            painter.circle_filled(
                c,
                r_px,
                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha),
            );
        }
    }

    /// Sci-fi action pad — full replacement for the flat `action_card` frame.
    /// Draws the background, chamfered border, depth lines, inner glow (on hover),
    /// and returns the interaction rect. The caller still allocates space via egui.
    pub fn action_pad(
        painter: &egui::Painter,
        rect: egui::Rect,
        accent: Color32,
        hovered: bool,
        time: f64,
    ) {
        let bg = Color32::from_rgb(10, 16, 12);
        // Base fill with subtle top-lit gradient
        Self::gradient_rect(painter, rect, Color32::from_rgb(22, 28, 22), bg);

        // Chamfered outer border
        let border_color = if hovered {
            accent
        } else {
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 70)
        };
        Self::chamfered_border(painter, rect, 5.0, border_color, if hovered { 1.5 } else { 1.0 });

        // Inner dim chamfered border (double-line effect)
        let inner = rect.shrink(3.0);
        Self::chamfered_border(painter, inner, 3.0,
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 25), 1.0);

        // Raised panel depth
        Self::raised_panel_border(painter, rect, accent);

        // Glow on hover
        if hovered {
            let pulse = ((time * 3.0).sin() as f32 * 0.2 + 0.8).clamp(0.5, 1.0);
            let glow = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), (pulse * 60.0) as u8);
            Self::glow_rect(painter, rect, glow, 4);
        }
    }

    /// Quick-launch slot — smaller variant of action_pad with slot index ghost
    pub fn quick_slot(
        painter: &egui::Painter,
        rect: egui::Rect,
        accent: Color32,
        slot_label: &str,
        filled: bool,
        hovered: bool,
        time: f64,
    ) {
        let bg_top = if filled { Color32::from_rgb(12, 24, 14) } else { Color32::from_rgb(10, 12, 10) };
        let bg_bot = Color32::from_rgb(6, 8, 6);
        Self::gradient_rect(painter, rect, bg_top, bg_bot);

        let border_color = if filled {
            if hovered {
                accent
            } else {
                Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 90)
            }
        } else {
            Color32::from_rgba_unmultiplied(40, 40, 40, 180)
        };

        Self::chamfered_border(painter, rect, 4.0, border_color, 1.0);

        if !filled {
            Self::ghost_number(painter, rect, slot_label, accent, rect.height() * 1.2);
        }

        if hovered {
            Self::glow_rect(painter, rect, accent, 3);
        }

        if filled {
            Self::raised_panel_border(painter, rect, accent);
            if hovered {
                let pulse = ((time * 2.5).sin() as f32 * 0.3 + 0.7).clamp(0.4, 1.0);
                let _ = pulse;
                Self::inner_glow(painter, rect, accent);
            }
        }
    }
}
