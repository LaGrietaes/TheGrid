// ═══════════════════════════════════════════════════════════════════════════════
// views/boot.rs — Animated Boot Sequence
//
// Rendered during the first ~2.5 seconds. Displays:
//   - Large THE GRID glyph and title
//   - Rolling log of "initialization" steps
//   - Progress bar that advances with real time
//
// No user input here. When progress reaches 1.0, the app transitions to
// Setup (if unconfigured) or Dashboard (if config already exists).
// ═══════════════════════════════════════════════════════════════════════════════

use egui::{Align, Color32, Layout, RichText, Ui};
use crate::theme::Colors;

/// Log messages shown rolling during boot
static BOOT_LOG: &[&str] = &[
    "[INIT] THE GRID kernel v0.1.0 loading...",
    "[SYS]  Spawning local agent on port 47731...",
    "[DB]   Opening SQLite index...",
    "[CFG]  Loading user configuration...",
    "[NET]  Checking Tailscale socket...",
    "[SEC]  Wireguard encryption layer active",
    "[OK]   File transfer service ready",
    "[UI]   Rendering command center...",
];

/// Minimum boot duration in seconds
const BOOT_DURATION: f32 = 2.5;

pub fn render(ui: &mut Ui, boot_elapsed: f32, startup_status: &str, startup_ready: bool) -> bool {
    // Return true when minimum boot time elapsed and real startup has completed.
    let progress = (boot_elapsed / BOOT_DURATION).min(1.0);
    let done = progress >= 1.0 && startup_ready;

    // Full-screen centered layout
    let available = ui.available_rect_before_wrap();
    ui.allocate_ui_at_rect(available, |ui| {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            // Vertical centering hack — push content down by ~30% of height
            ui.add_space(available.height() * 0.28);

            // ── Glyph ──────────────────────────────────────────────────────────
            // Pulsing color based on sin wave of elapsed time
            let glow_phase = (boot_elapsed * 2.0).sin() * 0.5 + 0.5;
            let glow_r = (0.0 * (1.0 - glow_phase) + 0.0 * glow_phase) as u8;
            let glow_g = (200.0 + 55.0 * glow_phase) as u8;
            let glow_b = (30.0 * glow_phase) as u8;
            let glyph_color = Color32::from_rgb(glow_r, glow_g, glow_b);

            ui.label(
                RichText::new(crate::icons::Glyphs::BRAND_HEX)
                    .color(glyph_color)
                    .size(52.0)
                    .strong()
            );

            ui.add_space(12.0);

            // ── Title ──────────────────────────────────────────────────────────
            ui.label(
                RichText::new("THE GRID")
                    .color(Colors::GREEN)
                    .size(32.0)
                    .strong()
            );

            ui.label(
                RichText::new("SECURE REMOTE ACCESS SYSTEM  v0.1.0")
                    .color(Colors::TEXT_MUTED)
                    .size(9.0)
            );

            ui.add_space(32.0);

            // ── Rolling boot log ───────────────────────────────────────────────
            // Show progressively more lines as time passes
            let lines_to_show = ((progress * BOOT_LOG.len() as f32) as usize)
                .min(BOOT_LOG.len());

            // Show the last 4 lines of whatever has been "logged" so far
            let start = lines_to_show.saturating_sub(4);
            for line in &BOOT_LOG[start..lines_to_show] {
                ui.label(
                    RichText::new(*line)
                        .color(Colors::GREEN)
                        .size(10.0)
                );
            }

            // Fixed height spacer so the progress bar doesn't jump
            ui.add_space((4_usize.saturating_sub(lines_to_show.min(4))) as f32 * 18.0);
            ui.add_space(10.0);
            ui.label(
                RichText::new(startup_status)
                    .color(Colors::TEXT_MUTED)
                    .size(9.0)
            );
            ui.add_space(20.0);

            // ── Progress bar ───────────────────────────────────────────────────
            let bar_width = 420.0;
            let bar_rect = ui.allocate_exact_size(
                egui::vec2(bar_width, 2.0),
                egui::Sense::hover()
            ).0;

            // Background track
            ui.painter().rect_filled(
                bar_rect,
                egui::Rounding::ZERO,
                Colors::BORDER,
            );

            // Filled portion
            let filled = egui::Rect::from_min_size(
                bar_rect.min,
                egui::vec2(bar_width * progress, 2.0),
            );
            ui.painter().rect_filled(
                filled,
                egui::Rounding::ZERO,
                Colors::GREEN,
            );
        });
    });

    // Keep repainting during the animation
    if !done {
        ui.ctx().request_repaint();
    }

    done
}
