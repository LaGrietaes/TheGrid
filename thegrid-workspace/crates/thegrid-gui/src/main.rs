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

// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// ↑ On release builds, suppress the console window on Windows.
//   Remove this attribute during development so you can see RUST_LOG output.

mod app;
mod cli;
mod theme;
mod icons;
mod telemetry;
mod views;

pub use std::path::PathBuf;
use image;

pub use app::TheGridApp;

fn main() -> eframe::Result<()> {
    // Parse CLI args early — before logging — so shell-integration paths are
    // available when the app struct is constructed inside the eframe closure.
    let launch_args = cli::LaunchArgs::parse();

    // Initialize logging. In debug builds defaults to 'info'; set RUST_LOG=debug
    // to see per-crate verbose output.
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).init();

    // Set a panic hook to log crashes to a file
    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            *s
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            &s[..]
        } else {
            "Unknown panic"
        };
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        let log_msg = format!("PANIC: {} at {}\n", msg, location);
        eprintln!("{}", log_msg);
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("crash.log")
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{}", log_msg)
            });
    }));

    log::info!("THE GRID v{} starting", env!("CARGO_PKG_VERSION"));

    let native_opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Start maximized so telemetry uses the real panel width from frame 1
            .with_maximized(true)
            .with_min_inner_size([900.0, 600.0])
            // Frameless window — TheGridApp renders its own title bar
            .with_decorations(false)
            // Transparent background so our bg color is the only bg
            .with_transparent(false)
            .with_icon(load_icon())
            .with_title("THE GRID"),

        // Disable persisted window state to avoid restoring off-screen/minimized sessions.
        persist_window: false,

        ..Default::default()
    };

    eframe::run_native(
        "THE GRID",
        native_opts,
        Box::new(move |cc| {
            // Install PNG/JPEG/SVG image loaders so egui::Image works
            egui_extras::install_image_loaders(&cc.egui_ctx);
            // Apply brutalist theme before the first frame renders
            theme::apply(&cc.egui_ctx);
            // Global readability bump for high-resolution displays
            cc.egui_ctx.set_pixels_per_point(1.12);
            Box::new(TheGridApp::new(cc, launch_args))
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
