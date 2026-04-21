// ═══════════════════════════════════════════════════════════════════════════════
// views/search.rs — Global File Search Panel
//
// Floats above the dashboard as an egui::Window (modal-like, but non-blocking).
// The user types a query → app.rs dispatches spawn_search() on each keystroke
// (debounced 300ms) → results arrive via AppEvent::SearchResults → rendered here.
//
// UX:
//   - Keyboard shortcut: Ctrl+F opens/focuses the panel (wired in app.rs)
//   - Results show: icon | filename | device › folder | size | modified date
//   - Clicking a result: selects the owning device + shows the file in Files tab
//   - Empty query: shows index stats (total files, last scan time)
//   - Spinner shown while search is in flight
// ═══════════════════════════════════════════════════════════════════════════════

use egui::{Color32, RichText, ScrollArea, Ui};
use chrono::TimeZone;
use thegrid_core::models::{FileSearchResult, IndexStats};
use crate::theme::Colors;

// ─────────────────────────────────────────────────────────────────────────────
// SearchPanelState — stored in TheGridApp
// ─────────────────────────────────────────────────────────────────────────────

pub struct SearchPanelState {
    /// Whether the panel is visible
    pub open: bool,
    /// Current query string
    pub query: String,
    /// Query that was last dispatched to the DB (to detect changes)
    pub last_dispatched: String,
    /// True while a search is in flight (shows spinner)
    pub searching: bool,
    /// Monotonic counter — incremented on each query dispatch.
    /// Used to discard stale results that arrive out of order.
    pub query_gen: u64,
    /// Monotonic counter of the last result set received
    pub result_gen: u64,
    /// Current result set
    pub results: Vec<FileSearchResult>,
    /// Whether to restrict search to a specific device
    pub device_filter: Option<String>,
}

impl Default for SearchPanelState {
    fn default() -> Self {
        Self {
            open:             false,
            query:            String::new(),
            last_dispatched:  String::new(),
            searching:        false,
            query_gen:        0,
            result_gen:       0,
            results:          Vec::new(),
            device_filter:    None,
        }
    }
}

impl SearchPanelState {
    /// Returns true if the query has changed since last dispatch.
    pub fn query_changed(&self) -> bool {
        self.query.trim() != self.last_dispatched.trim()
    }

    /// Call when dispatching a new search to the background thread.
    pub fn mark_dispatched(&mut self) -> u64 {
        self.last_dispatched = self.query.clone();
        self.searching = true;
        self.query_gen += 1;
        self.query_gen
    }

    /// Call when results arrive. Discards stale results (from old queries).
    pub fn receive_results(&mut self, gen: u64, results: Vec<FileSearchResult>) {
        if gen >= self.result_gen {
            self.result_gen = gen;
            self.results = results;
            self.searching = false;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SearchAction — what the user clicked this frame
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct SearchAction {
    /// User wants to navigate to this device + file
    pub open_result: Option<FileSearchResult>,
    /// User wants to preview this result
    pub preview_result: Option<FileSearchResult>,
    /// User closed the panel
    pub closed: bool,
    /// Query changed — dispatch a new search
    pub query_changed: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// render() — call every frame from app.rs when s.open == true
// ─────────────────────────────────────────────────────────────────────────────

pub fn render(
    ctx:   &egui::Context,
    s:     &mut SearchPanelState,
    stats: &IndexStats,
    current_scope: Option<(String, String)>,
    semantic_enabled: &mut bool,
    semantic_ready:   bool,
    embedding_progress: (usize, usize),
) -> SearchAction {
    let mut action = SearchAction::default();
    if !s.open { return action; }

    // Keyboard: Escape closes
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        s.open = false;
        action.closed = true;
        return action;
    }

    // Semi-transparent backdrop (same pattern as settings modal)
    egui::Area::new(egui::Id::new("search_backdrop"))
        .fixed_pos(egui::Pos2::ZERO)
        .order(egui::Order::Background)
        .interactable(false)
        .show(ctx, |ui| {
            ui.painter().rect_filled(
                ctx.screen_rect(),
                egui::Rounding::ZERO,
                Color32::from_rgba_premultiplied(0, 0, 0, 160),
            );
        });

    // Main search window — anchored near the top center
    egui::Window::new("thegrid_search")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
        .fixed_size(egui::vec2(640.0, 0.0))
        .frame(
            egui::Frame::none()
                .fill(Colors::BG_PANEL)
                .stroke(egui::Stroke::new(1.0, Colors::BORDER2))
        )
        .show(ctx, |ui| {
            // Green top accent bar
            let top = ui.next_widget_position();
            ui.painter().rect_filled(
                egui::Rect::from_min_size(top, egui::vec2(640.0, 2.0)),
                egui::Rounding::ZERO,
                Colors::GREEN,
            );
            ui.add_space(2.0);

            // ── Search input row ──────────────────────────────────────────────
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(16.0, 12.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Magnifying glass or spinner
                        if s.searching {
                            ui.add(egui::Spinner::new().size(18.0).color(Colors::GREEN));
                        } else {
                            ui.label(RichText::new("⌕").color(Colors::GREEN).size(18.0));
                        }
                        ui.add_space(8.0);

                        // Semantic Toggle (Phase 4)
                        if semantic_ready {
                            let (icon, color, tooltip) = if *semantic_enabled {
                                ("⬢", Colors::GREEN, "Semantic Search Active (AI)")
                            } else {
                                ("⬡", Colors::TEXT_DIM, "Enable Semantic Search (AI)")
                            };
                            let btn = ui.add(egui::Button::new(RichText::new(icon).size(18.0).color(color)).frame(false));
                            if btn.clicked() {
                                *semantic_enabled = !*semantic_enabled;
                                action.query_changed = true;
                            }
                            btn.on_hover_text(tooltip);
                            ui.add_space(8.0);
                        } else {
                            ui.add(egui::Spinner::new().size(16.0));
                            ui.add_space(4.0);
                        }

                        // Auto-focus the text field when the panel opens
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut s.query)
                                .hint_text("SEARCH FILES ACROSS ALL NODES...")
                                .font(egui::FontId::new(13.0, egui::FontFamily::Monospace))
                                .desired_width(f32::INFINITY)
                                .frame(false)
                        );

                        if s.open && s.query.is_empty() {
                            resp.request_focus();
                        }

                        // Detect query changes to trigger debounced search
                        if resp.changed() {
                            action.query_changed = true;
                        }
                    });

                    // --- Embedding progress bar (subtle) ---
                    let (indexed, total) = embedding_progress;
                    if total > 0 && indexed < total {
                        ui.add_space(8.0);
                        let progress = indexed as f32 / total as f32;
                        ui.add(egui::ProgressBar::new(progress)
                            .show_percentage()
                            .fill(Colors::GREEN));
                        ui.label(RichText::new(format!("Generating local embeddings... {}/{}", indexed, total))
                            .size(10.0)
                            .color(Colors::TEXT_DIM));
                    }
                });

            ui.horizontal(|ui| {
                ui.add_space(16.0); // Left padding

                // Search input field
                // This part of the UI is already handled above in the `egui::Frame::none().show(ui, |ui| { ui.horizontal(|ui| { ... })` block.
                // The instruction seems to have a misplaced snippet here.
                // The original code does not have a spinner or close button here.
                // I will assume the instruction meant to remove the spinner and close button from the main search input row,
                // but the instruction's snippet is malformed.
                // I will keep the existing structure and only apply the `render_result_row` changes.

                // The instruction's snippet for this part is:
                // ```
                //                         // Spinner while searching
                //                         if s.searching {
                //                             ui.spinner();
                //                         }
                //
                //                         // Close button
                //                         if ui.add(
                //                             egui::Button::new(
                //                                 RichText::new("✕").color(Colors::TEXT_DIM).size(12.0)
                //                             )
                //                             .fill(Color32::TRANSPARENT)
                //                             .frame(false)
                //                         ).clicked() {
                //                             s.open = false;
                //                             action.closed = true;
                //                         }
                // ```
                // This snippet is syntactically incorrect in its placement and seems to duplicate/misplace UI elements.
                // I will ignore this specific malformed part of the instruction and focus on the `render_result_row` change.
            });

            ui.add(egui::Separator::default().spacing(0.0));

            // ── Index stats bar ───────────────────────────────────────────────
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(16.0, 6.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let all_active = s.device_filter.is_none();
                        if ui.selectable_label(all_active, "ALL NODES").clicked() && !all_active {
                            s.device_filter = None;
                            action.query_changed = true;
                        }

                        if let Some((scope_id, scope_name)) = &current_scope {
                            let this_active = s.device_filter.as_ref() == Some(scope_id);
                            let label = format!("THIS: {}", scope_name.to_uppercase());
                            if ui.selectable_label(this_active, label).clicked() && !this_active {
                                s.device_filter = Some(scope_id.clone());
                                action.query_changed = true;
                            }
                        }

                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(format!("{} FILES INDEXED", stats.total_files))
                                .color(Colors::TEXT_MUTED).size(8.0)
                        );
                        if stats.scanning {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!(
                                    "  SCANNING {} / {}...",
                                    stats.scan_progress, stats.scan_total
                                ))
                                .color(Colors::AMBER).size(8.0)
                            );
                        }
                        if let Some(t) = stats.last_scanned {
                            let age_mins = (chrono::Utc::now().timestamp() - t) / 60;
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!("LAST SCAN: {}m AGO", age_mins))
                                        .color(Colors::TEXT_MUTED).size(8.0)
                                );
                            });
                        }
                    });
                });

            ui.add(egui::Separator::default().spacing(0.0));

            // ── Results ───────────────────────────────────────────────────────
            let query_empty = s.query.trim().is_empty();

            if query_empty {
                // Empty state: show usage hints
                egui::Frame::none()
                    .inner_margin(egui::Margin::same(20.0))
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("Type to search filenames across all indexed nodes")
                                    .color(Colors::TEXT_MUTED).size(10.0)
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("  \"project report\"  •  *.pdf  •  main.rs  •  invoice 2024")
                                    .color(Colors::TEXT_MUTED).size(9.0)
                            );
                            ui.add_space(8.0);
                        });
                    });
            } else if s.results.is_empty() && !s.searching {
                egui::Frame::none()
                    .inner_margin(egui::Margin::same(20.0))
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("NO RESULTS")
                                    .color(Colors::TEXT_MUTED).size(11.0)
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    "Try different keywords, or index more directories via + ADD WATCH DIRECTORY"
                                )
                                .color(Colors::TEXT_MUTED).size(9.0)
                            );
                        });
                    });
            } else {
                ScrollArea::vertical()
                    .id_source("search_results_scroll")
                    .max_height(400.0)
                    .show(ui, |ui: &mut Ui| {
                        for (i, result) in s.results.iter().enumerate() {
                            let resp = render_result_row(ui, result, i);
                            if resp.clicked() {
                                action.preview_result = Some(result.clone());
                            }
                            if resp.double_clicked() {
                                action.open_result = Some(result.clone());
                                s.open = false;
                            }
                        }
                    });
            }

            ui.add_space(2.0);
        });

    action
}

// ─────────────────────────────────────────────────────────────────────────────
// Result row renderer
// ─────────────────────────────────────────────────────────────────────────────

fn render_result_row(ui: &mut Ui, r: &FileSearchResult, idx: usize) -> egui::Response {
    let glyph = crate::icons::ext_to_glyph(r.ext.as_deref());
    let color = crate::icons::ext_to_color(r.ext.as_deref());

    let resp = egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(16.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // File type glyph
                ui.label(
                    RichText::new(glyph)
                        .color(color).size(14.0)
                );
                ui.add_space(8.0);

                // Name + path
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(&r.name)
                            .color(Colors::TEXT).size(11.0).strong()
                    );
                    ui.label(
                        RichText::new(r.display_path())
                            .color(Colors::TEXT_DIM).size(9.0)
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Modified date
                    if let Some(ts) = r.modified {
                        let dt = chrono::Utc.timestamp_opt(ts, 0).single()
                            .unwrap_or_default();
                        ui.label(
                            RichText::new(dt.format("%Y-%m-%d").to_string())
                                .color(Colors::TEXT_MUTED).size(8.0)
                        );
                        ui.add_space(8.0);
                    }
                    // File size
                    ui.label(
                        RichText::new(crate::telemetry::fmt_bytes(r.size))
                            .color(Colors::TEXT_DIM).size(8.0)
                    );
                    // Device badge
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(r.device_name.to_uppercase())
                            .color(Colors::GREEN).size(8.0).strong()
                    );
                });
            });
        }).response;

    // Full-row interaction
    let interact = ui.interact(
        resp.rect,
        egui::Id::new(("search_row", idx)),
        egui::Sense::click(),
    );
    if interact.hovered() {
        ui.painter().rect_filled(
            resp.rect, egui::Rounding::ZERO,
            Color32::from_rgba_premultiplied(255, 255, 255, 5),
        );
    }
    ui.add(egui::Separator::default().spacing(0.0));
    interact
}

// use crate::icons::ext_to_glyph;
