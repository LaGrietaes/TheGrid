use eframe::egui;
use std::collections::VecDeque;

/// A simple terminal emulator view for THE GRID.
/// It renders a grid of characters and handles ANSI-like scrolling.
pub struct TerminalView {
    pub lines: VecDeque<String>,
    pub max_lines: usize,
    pub input_buffer: String,
    pub session_id:   Option<String>,
}

impl TerminalView {
    pub fn new() -> Self {
        let mut lines = VecDeque::new();
        lines.push_back("Connected to remote terminal...".to_string());
        Self {
            lines,
            max_lines: 500,
            input_buffer: String::new(),
            session_id: None,
        }
    }

    pub fn push_output(&mut self, data: &[u8]) {
        let text = String::from_utf8_lossy(data);
        for c in text.chars() {
            if c == '\n' {
                self.lines.push_back(String::new());
            } else if c == '\r' {
                // Ignore for now or handle carriage return
            } else {
                if self.lines.is_empty() {
                    self.lines.push_back(String::new());
                }
                self.lines.back_mut().unwrap().push(c);
            }
        }

        while self.lines.len() > self.max_lines {
            self.lines.pop_front();
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut pending_input = None;

        ui.vertical(|ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        for line in &self.lines {
                            ui.add(egui::Label::new(
                                egui::RichText::new(line)
                                    .font(egui::FontId::monospace(14.0))
                                    .color(egui::Color32::from_rgb(200, 200, 200))
                            ));
                        }
                    });
                });

            ui.separator();

            let response = ui.add(
                egui::TextEdit::singleline(&mut self.input_buffer)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("> type here or press keys...")
                    .lock_focus(true)
                    .desired_width(f32::INFINITY)
            );

            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let mut cmd = self.input_buffer.clone();
                cmd.push('\n');
                pending_input = Some(cmd);
                self.input_buffer.clear();
                response.request_focus();
            }

            // Also capture raw keys if the terminal is focused
            if response.has_focus() {
                ui.input(|i| {
                    for event in &i.events {
                        if let egui::Event::Key { key, pressed: true, .. } = event {
                             // Handle special keys like arrows, etc.
                             match key {
                                 egui::Key::ArrowUp => pending_input = Some("\x1b[A".to_string()),
                                 egui::Key::ArrowDown => pending_input = Some("\x1b[B".to_string()),
                                 egui::Key::ArrowRight => pending_input = Some("\x1b[C".to_string()),
                                 egui::Key::ArrowLeft => pending_input = Some("\x1b[D".to_string()),
                                 _ => {}
                             }
                        }
                    }
                });
            }
        });

        pending_input
    }
}
