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
