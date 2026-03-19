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
    pub const BORDER_FOC: Color32 = Color32::from_rgb(0,   255, 65);   // green focus ring

    // Accent colors
    pub const GREEN:      Color32 = Color32::from_rgb(0,   255, 65);   // #00ff41
    pub const GREEN_DIM:  Color32 = Color32::from_rgb(0,   128, 32);   // #008020
    pub const CYAN:       Color32 = Color32::from_rgb(0,   229, 255);  // #00e5ff
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

    // For now: override the default text style sizes to monospace at our scale
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
    v.hyperlink_color = Colors::CYAN;

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

/// Status badge (ONLINE/OFFLINE/etc.)
pub fn status_badge(ui: &mut Ui, label: &str, online: bool) {
    let color = if online { Colors::GREEN } else { Colors::TEXT_MUTED };
    let btn = egui::Button::new(
        RichText::new(label).color(color).size(9.0).strong()
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::new(1.0, color));
    ui.add(btn);
}

/// Colored indicator dot
pub fn status_dot(ui: &mut Ui, online: bool) {
    let color = if online { Colors::GREEN } else { Colors::TEXT_MUTED };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// Horizontal rule styled as a terminal separator
pub fn separator(ui: &mut Ui) {
    ui.add(egui::Separator::default().spacing(0.0));
}
