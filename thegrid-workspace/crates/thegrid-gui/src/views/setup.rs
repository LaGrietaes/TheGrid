// ═══════════════════════════════════════════════════════════════════════════════
// views/setup.rs — First-Run Configuration Screen
//
// Shown only when no config exists (api_key is empty).
// Collects: API key, device label, default RDP username.
// On submit: calls Tailscale API to validate the key, saves config,
//            fires SetupComplete or SetupFailed event.
// ═══════════════════════════════════════════════════════════════════════════════

use egui::{Color32, RichText, Ui};
use crate::theme::{self, Colors};

/// All the mutable state the setup form needs.
/// Stored in TheGridApp and passed by reference here.
pub struct SetupState {
    pub api_key:     String,
    pub device_name: String,
    pub rdp_user:    String,
    pub error:       Option<String>,
    pub loading:     bool,
}

impl Default for SetupState {
    fn default() -> Self {
        Self {
            api_key:     String::new(),
            device_name: String::new(),
            rdp_user:    String::new(),
            error:       None,
            loading:     false,
        }
    }
}

/// Returns true when the user clicks "INITIALIZE CONNECTION" and we should
/// fire the background validation task.
pub fn render(ui: &mut Ui, state: &mut SetupState, local_hostname: &str) -> bool {
    let available = ui.available_rect_before_wrap();
    let mut submitted = false;

    ui.allocate_ui_at_rect(available, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(available.height() * 0.18);

            // ── Header ─────────────────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::icons::Glyphs::BRAND_HEX).color(Colors::GREEN).size(18.0));
                ui.add_space(8.0);
                ui.label(RichText::new("THE GRID").color(Colors::GREEN).size(18.0).strong());
                ui.add_space(8.0);
                ui.label(RichText::new("v0.1.0").color(Colors::TEXT_MUTED).size(10.0));
            });

            ui.add_space(32.0);

            // ── Config box ─────────────────────────────────────────────────────
            // Fixed-width panel, centered
            let box_width = 460.0;
            ui.allocate_ui_with_layout(
                egui::vec2(box_width, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    // Top accent bar
                    let top = ui.cursor();
                    let bar_rect = egui::Rect::from_min_size(
                        top.min,
                        egui::vec2(box_width, 2.0),
                    );
                    ui.painter().rect_filled(bar_rect, egui::Rounding::ZERO, Colors::GREEN);
                    ui.add_space(2.0);

                    egui::Frame::none()
                        .fill(Colors::BG_PANEL)
                        .stroke(egui::Stroke::new(1.0, Colors::BORDER2))
                        .inner_margin(egui::Margin::same(24.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("// INITIAL CONFIGURATION")
                                    .color(Colors::GREEN)
                                    .size(9.0)
                                    .strong()
                            );
                            ui.add_space(20.0);

                            // ── API Key ────────────────────────────────────────
                            field_label(ui, "TAILSCALE API KEY");
                            ui.label(
                                RichText::new("Generate at: tailscale.com/admin/settings/keys")
                                    .color(Colors::TEXT_MUTED)
                                    .size(9.0)
                            );
                            ui.add_space(4.0);
                            ui.add(theme::password_input(
                                &mut state.api_key,
                                "tskey-api-••••••••••••••••••",
                            ));

                            ui.add_space(16.0);

                            // ── Device label ───────────────────────────────────
                            field_label(ui, "THIS DEVICE LABEL (optional)");
                            ui.add(theme::text_input(
                                &mut state.device_name,
                                local_hostname,
                            ));

                            ui.add_space(16.0);

                            // ── RDP username ───────────────────────────────────
                            field_label(ui, "DEFAULT RDP USERNAME (optional)");
                            ui.add(theme::text_input(
                                &mut state.rdp_user,
                                "e.g. Administrator",
                            ));

                            ui.add_space(24.0);

                            // ── Submit button ──────────────────────────────────
                            ui.set_enabled(!state.loading && !state.api_key.trim().is_empty());

                            let btn_label = if state.loading {
                                "CONNECTING..."
                            } else {
                                "INITIALIZE CONNECTION"
                            };

                            if theme::primary_button(ui, btn_label).clicked() {
                                submitted = true;
                            }

                            // ── Error message ──────────────────────────────────
                            if let Some(ref err) = state.error {
                                ui.add_space(12.0);
                                egui::Frame::none()
                                    .fill(Color32::from_rgba_premultiplied(255, 34, 68, 20))
                                    .stroke(egui::Stroke::new(1.0, Colors::RED))
                                    .inner_margin(egui::Margin::same(8.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(err)
                                                .color(Colors::RED)
                                                .size(10.0)
                                        );
                                    });
                            }
                        });
                },
            );
        });
    });

    submitted
}

fn field_label(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .color(Colors::TEXT_DIM)
            .size(9.0)
            .strong()
    );
    ui.add_space(4.0);
}
