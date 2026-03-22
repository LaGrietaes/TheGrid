use egui::{Ui, Color32, ScrollArea, RichText};
use thegrid_core::models::PreviewKind;
use crate::app::ViewportState;

pub fn show_viewport(ui: &mut Ui, state: &mut ViewportState) {
    ui.vertical(|ui| {
        ui.heading("PREVIEW VIEWPORT");
        ui.separator();

        if state.is_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Fetching content...");
            });
            return;
        }

        let file = match &state.active_file {
            Some(f) => f,
            None => {
                ui.label(RichText::new("No file selected for preview.").color(Color32::GRAY));
                return;
            }
        };

        ui.label(RichText::new(&file.name).strong().size(18.0));
        ui.label(format!("Path: {}", file.path.display()));
        ui.separator();

        match state.preview_kind {
            PreviewKind::Text => {
                ScrollArea::both().auto_shrink([false; 2]).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut state.content)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .lock_focus(true)
                    );
                });
            }
            PreviewKind::Image => {
                ui.label("Image Preview (Placeholder)");
                // Future: use egui_extras to show actual image
            }
            PreviewKind::Pdf => {
                ui.label("PDF Preview (Not supported in-app)");
                ui.horizontal(|ui| {
                    if ui.button("Open Externally").clicked() {
                        let _ = open::that(&file.path);
                    }
                });
            }
            PreviewKind::UnSupported => {
                ui.label("Preview not available for this file type.");
                if ui.button("Open Externally").clicked() {
                    let _ = open::that(&file.path);
                }
            }
            PreviewKind::None => {
                ui.label("Select a file to preview.");
            }
        }
    });
}
