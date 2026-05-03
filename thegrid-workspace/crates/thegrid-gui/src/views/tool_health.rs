// ═══════════════════════════════════════════════════════════════════════════
// views/tool_health.rs — Tool Health Badge & Modal
//
// Shows capability tier badge in the status bar and a pop-up modal with
// per-tool status rows and actionable install hints for missing tools.
// ═══════════════════════════════════════════════════════════════════════════

use egui::{Color32, RichText, Ui};
use thegrid_core::tool_health::{CapabilityTier, ToolHealthReport, ToolStatus};

// ── Badge ─────────────────────────────────────────────────────────────────

/// Compact badge rendered inside the status bar or toolbar.
/// Returns `true` if the user clicked it (toggle modal open).
pub fn render_tool_health_badge(ui: &mut Ui, report: &ToolHealthReport) -> bool {
    let [r, g, b] = report.tier.color_rgb();
    let tier_color = Color32::from_rgb(r, g, b);
    let tier_dim   = Color32::from_rgba_unmultiplied(r, g, b, 40);

    let has_missing = !report.missing_hints().is_empty();
    let icon = if has_missing { "⚠" } else { "✔" };
    let label = format!("{} {}", icon, report.tier.label());

    ui.add(
        egui::Button::new(
            RichText::new(label).size(7.5).color(tier_color),
        )
        .fill(tier_dim)
        .stroke(egui::Stroke::new(1.0, tier_color)),
    )
    .on_hover_text("Media tool capability tier — click for details")
    .clicked()
}

// ── Modal ────────────────────────────────────────────────────────────────

/// Full-screen modal (rendered as a centred panel overlay).
/// Call each frame while `open == true`; sets `*open = false` on close.
pub fn render_tool_health_modal(ctx: &egui::Context, open: &mut bool, report: &ToolHealthReport) {
    egui::Window::new("TOOL HEALTH")
        .collapsible(false)
        .resizable(false)
        .min_width(460.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(Color32::from_rgb(6, 8, 12))
                .stroke(egui::Stroke::new(1.5, Color32::from_rgb(60, 130, 230)))
                .inner_margin(egui::Margin::same(16.0)),
        )
        .show(ctx, |ui| {
            // Header
            let [r, g, b] = report.tier.color_rgb();
            let tier_color = Color32::from_rgb(r, g, b);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("⬡ MEDIA TOOL HEALTH")
                        .size(11.0)
                        .color(Color32::from_rgb(60, 130, 230))
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(RichText::new("✕ CLOSE").size(8.5).color(Color32::from_gray(140))).clicked() {
                        *open = false;
                    }
                });
            });

            ui.separator();
            ui.add_space(4.0);

            // Active tier banner
            egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(r, g, b, 20))
                .stroke(egui::Stroke::new(1.0, tier_color))
                .inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 5.0, bottom: 5.0 })
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Active: {}", report.tier.label()))
                            .size(9.5)
                            .color(tier_color)
                            .strong(),
                    );
                    let hints = report.missing_hints();
                    if hints.is_empty() {
                        ui.label(RichText::new("All tools available — full capability unlocked.").size(8.5).color(Color32::from_gray(180)));
                    } else {
                        ui.label(
                            RichText::new(format!("{} tool(s) missing — install to unlock higher tiers.", hints.len()))
                                .size(8.5)
                                .color(Color32::from_rgb(230, 160, 40)),
                        );
                    }
                });

            ui.add_space(8.0);

            // Tool rows
            let tools: [(&str, &ToolStatus, CapabilityTier); 5] = [
                ("ffmpeg",   &report.ffmpeg,   CapabilityTier::T1Ffmpeg),
                ("ffprobe",  &report.ffprobe,  CapabilityTier::T1Ffmpeg),
                ("ollama",   &report.ollama,   CapabilityTier::T3Transcription),
                ("gyroflow", &report.gyroflow, CapabilityTier::T4Gyroflow),
                ("fabric",   &report.fabric,   CapabilityTier::T3Transcription),
            ];

            egui::Grid::new("tool_health_grid")
                .num_columns(3)
                .min_col_width(90.0)
                .striped(false)
                .show(ui, |ui| {
                    // Header row
                    for hdr in ["TOOL", "STATUS", "REQUIRED FOR"] {
                        ui.label(RichText::new(hdr).size(7.5).color(Color32::from_gray(100)).strong());
                    }
                    ui.end_row();
                    ui.separator(); ui.separator(); ui.separator();
                    ui.end_row();

                    for (name, status, needs_tier) in &tools {
                        let (status_color, status_text) = match status {
                            ToolStatus::Ok { version, .. } => (
                                Color32::from_rgb(60, 200, 80),
                                format!("OK  {}", truncate(version, 40)),
                            ),
                            ToolStatus::Missing { .. } => (
                                Color32::from_rgb(220, 80, 60),
                                "MISSING".to_string(),
                            ),
                            ToolStatus::Error { message } => (
                                Color32::from_rgb(230, 140, 30),
                                format!("ERROR  {}", truncate(message, 35)),
                            ),
                        };

                        // Tool name
                        ui.label(RichText::new(*name).size(9.0).color(Color32::from_gray(210)).monospace());

                        // Status
                        ui.label(RichText::new(status_text).size(8.5).color(status_color));

                        // Tier gate
                        let [tr, tg, tb] = needs_tier.color_rgb();
                        ui.label(RichText::new(needs_tier.label()).size(7.5).color(Color32::from_rgb(tr, tg, tb)));

                        ui.end_row();

                        // Path row (indented) when Ok
                        if let ToolStatus::Ok { path, .. } = status {
                            ui.label("");
                            ui.label(
                                RichText::new(path.to_string_lossy().as_ref())
                                    .size(7.5)
                                    .color(Color32::from_gray(100))
                                    .italics(),
                            );
                            ui.label("");
                            ui.end_row();
                        }

                        // Install hint row when Missing
                        if let ToolStatus::Missing { hint } | ToolStatus::Error { message: hint } = status {
                            ui.label("");
                            ui.add(
                                egui::Label::new(
                                    RichText::new(hint.as_str())
                                        .size(7.5)
                                        .color(Color32::from_rgb(180, 140, 50))
                                        .italics(),
                                )
                                .wrap(true),
                            );
                            ui.label("");
                            ui.end_row();
                        }

                        ui.add_space(2.0); ui.label(""); ui.label("");
                        ui.end_row();
                    }
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // Tier ladder summary
            ui.label(RichText::new("CAPABILITY TIER LADDER").size(8.0).color(Color32::from_gray(100)).strong());
            ui.add_space(2.0);
            let tiers = [
                (CapabilityTier::T0Image,        "Always available — pure-Rust image ops (resize, crop, adjust)"),
                (CapabilityTier::T1Ffmpeg,        "ffmpeg + ffprobe — video/audio transforms, thumbnails, preview"),
                (CapabilityTier::T2VadDenoise,    "ffmpeg + VAD/denoise tools — advanced audio cleanup"),
                (CapabilityTier::T3Transcription, "Ollama — local AI transcription and media recommendations"),
                (CapabilityTier::T4Gyroflow,      "Gyroflow — gyroscope-based video stabilization"),
            ];
            for (tier, desc) in &tiers {
                let active = report.tier >= *tier;
                let [cr, cg, cb] = tier.color_rgb();
                let col = if active { Color32::from_rgb(cr, cg, cb) } else { Color32::from_gray(55) };
                let mark = if active { "▶" } else { "○" };
                ui.label(RichText::new(format!("{} {}", mark, desc)).size(8.0).color(col));
            }
        });
}

fn truncate(s: &str, max: usize) -> &str {
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    &s[..end]
}
