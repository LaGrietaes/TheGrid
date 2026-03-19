// ═══════════════════════════════════════════════════════════════════════════════
// THE GRID — main.rs
//
// Entry point. Sets up:
//   - env_logger for RUST_LOG-controlled logging
//   - eframe NativeOptions (window size, frameless, icon)
//   - Runs the TheGridApp event loop
//
// To run with debug logging:
//   RUST_LOG=debug cargo run -p thegrid-gui
// ═══════════════════════════════════════════════════════════════════════════════

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// ↑ On release builds, suppress the console window on Windows.
//   Remove this attribute during development so you can see RUST_LOG output.

mod app;
mod theme;
mod icons;
mod telemetry;
mod views;

pub use std::path::PathBuf;
use image;

pub use app::TheGridApp;

fn main() -> eframe::Result<()> {
    // Initialize logging. In debug builds defaults to 'info'; set RUST_LOG=debug
    // to see per-crate verbose output.
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).init();

    log::info!("THE GRID v{} starting", env!("CARGO_PKG_VERSION"));

    let native_opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Start at a comfortable size; user can resize
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            // Frameless window — TheGridApp renders its own title bar
            .with_decorations(false)
            // Transparent background so our bg color is the only bg
            .with_transparent(false)
            .with_icon(load_icon())
            .with_title("THE GRID"),

        // Persist window position/size between sessions
        persist_window: true,

        ..Default::default()
    };

    eframe::run_native(
        "THE GRID",
        native_opts,
        Box::new(|cc| {
            // Apply brutalist theme before the first frame renders
            theme::apply(&cc.egui_ctx);
            Box::new(TheGridApp::new(cc))
        }),
    )
}

fn load_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon.png");
    let image = image::load_from_memory(icon_bytes).expect("Failed to load icon");
    let image = image.to_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}
