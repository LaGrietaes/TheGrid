use egui::{Ui, Color32, ScrollArea, RichText};
use thegrid_core::models::PreviewKind;
use crate::app::ViewportState;
use crate::views::video_preview::{
    backend_install_hint,
    extract_video_frame_png,
    format_duration_short,
    is_video_ext,
    render_media_preview,
    probe_video_meta,
    VideoPreviewError,
};

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
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();

                if is_video_ext(&ext) {
                    ui.label(RichText::new("VIDEO FILE").strong());
                    ui.add_space(4.0);
                    ui.label(RichText::new("Inline playback is available when a local media backend is present.").color(Color32::GRAY));
                    if let Some(meta) = probe_video_meta(&file.path) {
                        let duration = meta.duration_secs.map(format_duration_short).unwrap_or_else(|| "--:--".to_string());
                        let resolution = match (meta.width, meta.height) {
                            (Some(w), Some(h)) => format!("{}x{}", w, h),
                            _ => "?x?".to_string(),
                        };
                        let fps = meta.fps.map(|v| format!("{:.2}fps", v)).unwrap_or_else(|| "--fps".to_string());
                        ui.label(RichText::new(format!("{}  |  {}  |  {}", duration, resolution, fps)).color(Color32::GRAY));
                    }
                    ui.add_space(8.0);
                    if !render_media_preview(ui, &mut state.media_preview, &file.path, 280.0) {
                        if let Some(texture) = &state.texture {
                            let avail = ui.available_width() - 24.0;
                            ui.add(egui::Image::from_texture(texture)
                                .max_width(avail)
                                .maintain_aspect_ratio(true));
                        } else if let Some(err) = state.content.strip_prefix("video_preview_error:") {
                            ui.label(RichText::new(format!("Could not extract preview frame: {}", err)).color(Color32::RED));
                        } else {
                            match extract_video_frame_png(&file.path) {
                                Ok(bytes) => match image::load_from_memory(&bytes) {
                                    Ok(img) => {
                                        let size = [img.width() as usize, img.height() as usize];
                                        let rgba = img.to_rgba8();
                                        let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                            size, rgba.as_flat_samples().as_slice()
                                        );
                                        state.texture = Some(ui.ctx().load_texture(
                                            "viewport_video_frame", color_image, Default::default()
                                        ));
                                        ui.ctx().request_repaint();
                                        ui.label(RichText::new("Loading preview frame...").color(Color32::GRAY));
                                    }
                                    Err(e) => {
                                        state.content = format!("video_preview_error:decode failed ({})", e);
                                        ui.label(RichText::new("Could not decode extracted video frame.").color(Color32::RED));
                                    }
                                },
                                Err(VideoPreviewError::BackendMissing) => {
                                    state.content = "video_preview_error:ffmpeg not available".to_string();
                                    ui.label(RichText::new("Could not extract preview frame: ffmpeg not available").color(Color32::RED));
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(backend_install_hint()).color(Color32::GRAY));
                                }
                                Err(VideoPreviewError::ExtractFailed) => {
                                    state.content = "video_preview_error:frame extraction failed".to_string();
                                    ui.label(RichText::new("Could not extract preview frame: frame extraction failed").color(Color32::RED));
                                }
                            }
                        }
                    }

                    ui.add_space(8.0);
                    if ui.button("Open Externally").clicked() {
                        let _ = open::that(&file.path);
                    }
                    return;
                } else if crate::views::video_preview::is_audio_ext(&ext) {
                    ui.label(RichText::new("AUDIO FILE").strong());
                    ui.add_space(8.0);
                    if render_media_preview(ui, &mut state.media_preview, &file.path, 180.0) {
                        return;
                    }
                }

                let ext_upper = ext.to_uppercase();
                let (label, note) = if matches!(ext_upper.as_str(), "AI" | "EPS") {
                    ("ADOBE ILLUSTRATOR / EPS", "Vector files cannot be rendered inline.")
                } else if matches!(ext_upper.as_str(), "MP4" | "MKV" | "AVI" | "MOV" | "WEBM") {
                    ("VIDEO FILE", "Video playback is not supported inline.")
                } else if matches!(ext_upper.as_str(), "MP3" | "FLAC" | "WAV" | "OGG" | "M4A" | "AAC") {
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
