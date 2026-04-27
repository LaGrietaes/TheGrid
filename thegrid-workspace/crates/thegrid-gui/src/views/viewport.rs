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
            Some(f) => f.clone(),
            None => {
                ui.label(RichText::new("No file selected for preview.").color(Color32::GRAY));
                return;
            }
        };

        ui.label(RichText::new(&file.name).strong().size(18.0));
        ui.label(format!("Path: {}", file.path.display()));
        ui.separator();

        match &state.preview_kind.clone() {
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
                let ext = file.path.extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();

                if let Some(texture) = &state.texture {
                    let avail = ui.available_width() - 24.0;
                    ui.add(egui::Image::from_texture(texture)
                        .max_width(avail)
                        .maintain_aspect_ratio(true));
                } else if ext == "svg" {
                    // SVG: load via egui_extras bytes loader
                    match std::fs::read(&file.path) {
                        Ok(bytes) => {
                            let uri: std::borrow::Cow<'static, str> =
                                std::borrow::Cow::Owned(format!("bytes://viewport_{}", file.name));
                            let src = egui::ImageSource::Bytes {
                                uri,
                                bytes: egui::load::Bytes::Shared(std::sync::Arc::from(bytes.as_slice())),
                            };
                            let avail = ui.available_width() - 24.0;
                            ui.add(egui::Image::new(src).max_width(avail).maintain_aspect_ratio(true));
                        }
                        Err(e) => {
                            ui.label(RichText::new(format!("Could not read SVG: {}", e)).color(Color32::RED));
                        }
                    }
                } else {
                    // Raster image: load bytes → RGBA → texture (cached in state.texture)
                    match std::fs::read(&file.path) {
                        Ok(bytes) => match image::load_from_memory(&bytes) {
                            Ok(img) => {
                                let size = [img.width() as usize, img.height() as usize];
                                let rgba = img.to_rgba8();
                                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                    size, rgba.as_flat_samples().as_slice()
                                );
                                state.texture = Some(ui.ctx().load_texture(
                                    "viewport_image", color_image, Default::default()
                                ));
                                // texture will be rendered on next frame
                                ui.ctx().request_repaint();
                            }
                            Err(e) => {
                                ui.label(RichText::new(format!("Could not decode image: {}", e))
                                    .color(Color32::RED));
                            }
                        },
                        Err(e) => {
                            ui.label(RichText::new(format!("Could not read file: {}", e))
                                .color(Color32::RED));
                        }
                    }
                }
            }
            PreviewKind::Psd => {
                if let Some(texture) = &state.texture {
                    let avail = ui.available_width() - 24.0;
                    ui.add(egui::Image::from_texture(texture)
                        .max_width(avail)
                        .maintain_aspect_ratio(true));
                } else {
                    match std::fs::read(&file.path) {
                        Ok(bytes) => match psd::Psd::from_bytes(&bytes) {
                            Ok(psd_doc) => {
                                let w = psd_doc.width() as usize;
                                let h = psd_doc.height() as usize;
                                let rgba = psd_doc.rgba();
                                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                    [w, h], &rgba
                                );
                                state.texture = Some(ui.ctx().load_texture(
                                    "viewport_psd", color_image, Default::default()
                                ));
                                ui.ctx().request_repaint();
                            }
                            Err(e) => {
                                ui.label(RichText::new(format!("PSD decode error: {}", e))
                                    .color(Color32::RED));
                                ui.add_space(6.0);
                                if ui.button("Open Externally").clicked() {
                                    let _ = open::that(&file.path);
                                }
                            }
                        },
                        Err(e) => {
                            ui.label(RichText::new(format!("Could not read PSD: {}", e))
                                .color(Color32::RED));
                        }
                    }
                }
            }
            PreviewKind::Pdf => {
                ui.label(RichText::new("PDF DOCUMENT").strong());
                ui.add_space(4.0);
                ui.label(RichText::new("Inline PDF rendering is not supported.").color(Color32::GRAY));
                ui.add_space(8.0);
                if ui.button("Open Externally").clicked() {
                    let _ = open::that(&file.path);
                }
            }
            PreviewKind::UnSupported => {
                let ext = file.path.extension()
                    .map(|e| e.to_string_lossy().to_uppercase())
                    .unwrap_or_default();
                let (label, note) = if matches!(ext.as_str(), "AI" | "EPS") {
                    ("ADOBE ILLUSTRATOR / EPS", "Vector files cannot be rendered inline.")
                } else if matches!(ext.as_str(), "MP4" | "MKV" | "AVI" | "MOV" | "WEBM") {
                    ("VIDEO FILE", "Video playback is not supported inline.")
                } else if matches!(ext.as_str(), "MP3" | "FLAC" | "WAV" | "OGG" | "M4A" | "AAC") {
                    ("AUDIO FILE", "Audio playback is not supported inline.")
                } else {
                    ("UNSUPPORTED FORMAT", "No preview available for this file type.")
                };
                ui.label(RichText::new(label).strong());
                ui.add_space(4.0);
                ui.label(RichText::new(note).color(Color32::GRAY));
                ui.add_space(8.0);
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
