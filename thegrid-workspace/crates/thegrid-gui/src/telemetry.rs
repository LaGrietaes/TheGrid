// ═══════════════════════════════════════════════════════════════════════════════
// telemetry.rs — Local Hardware Telemetry Collector
//
// Uses the `sysinfo` crate to read CPU, RAM, and disk stats for the LOCAL
// machine. Results are:
//   1. Displayed in the local device's Node Card in the device panel
//   2. Sent to remote devices via the agent /telemetry endpoint (future)
//   3. Used by AiNodeDetector to decide if this machine can serve embeddings
//
// Why local-only?
//   sysinfo is a GUI crate dep. Remote devices run their own agent which
//   will self-report via GET /telemetry once fully implemented in Phase 3+.
//   For now the GUI shows accurate telemetry for the local node and graceful
//   fallbacks ("—") for remote nodes.
//
// Refresh rate: 5 seconds, driven by a background thread that sends
// AppEvent::TelemetryUpdate(local_device_id, telemetry).
// ═══════════════════════════════════════════════════════════════════════════════

use sysinfo::{System, Disks};
use thegrid_core::models::NodeTelemetry;

/// Collect a single telemetry snapshot for the local machine.
///
/// This function blocks for a brief moment (sysinfo requires a short sleep
/// between two CPU measurements for a meaningful utilisation number).
/// Always call this from a background thread.
pub fn collect_local() -> NodeTelemetry {
    let mut sys = System::new_all();

    // sysinfo needs a short window between two reads for accurate CPU %
    // The sleep is inside sysinfo when you call refresh_cpu_usage() twice.
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_all();

    // CPU: average across all logical cores
    let cpu_pct = {
        let cpus = sys.cpus();
        if cpus.is_empty() {
            0.0
        } else {
            cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / cpus.len() as f32
        }
    };

    // RAM
    let ram_used  = sys.used_memory();   // bytes
    let ram_total = sys.total_memory();  // bytes

    // Disk: find the disk that contains the user's home directory.
    // Fallback: use the first disk found.
    let disks = Disks::new_with_refreshed_list();
    let (disk_used, disk_total) = find_primary_disk(&disks);

    // CPU temperature (only available on some platforms/hardware)
    // sysinfo returns temperatures via Components. We look for a component
    // whose label contains "CPU" or "Package".
    // Note: on Windows this often requires admin rights; we tolerate None.
    let cpu_temp = None; // TODO: sysinfo Components API changed in 0.30 — re-enable later

    // AI capability: heuristic based on RAM
    let is_ai_capable = ram_total >= 8 * 1024 * 1024 * 1024; // 8 GB

    // GPU info (experimental for Windows)
    let mut gpu_name = None;
    if cfg!(target_os = "windows") {
        if let Ok(output) = std::process::Command::new("wmic")
            .args(&["path", "win32_VideoController", "get", "name"])
            .output() {
                let s = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = s.lines().collect();
                if lines.len() > 1 {
                    gpu_name = Some(lines[1].trim().to_string());
                }
            }
    }

    // Drive details
    let mut drive_infos = Vec::new();
    for disk in &disks {
        drive_infos.push(thegrid_core::models::DriveInfo {
            name: disk.mount_point().to_string_lossy().to_string(),
            used: disk.total_space() - disk.available_space(),
            total: disk.total_space(),
        });
    }
    
    // BPW: Local IPs
    let mut local_ips = Vec::new();
    if let Ok(ifs) = get_if_addrs::get_if_addrs() {
        for interface in ifs {
            if !interface.is_loopback() {
                if let std::net::IpAddr::V4(addr) = interface.ip() {
                    local_ips.push(addr.to_string());
                }
            }
        }
    }

    NodeTelemetry {
        device_type: "Desktop".to_string(), 
        cpu_pct,
        ram_used,
        ram_total,
        disk_used,
        disk_total,
        cpu_temp,
        is_ai_capable,
        gpu_name,
        gpu_pct: None,
        gpu_mem_used: None,
        gpu_mem_total: None,
        local_ips,
        ai_status: Some("Idle".into()),
        ai_tokens_per_sec: None,
        ai_thoughts: None,
        capabilities: thegrid_core::models::DeviceCapabilities {
            drives: drive_infos,
            has_rdp: thegrid_net::win_sys::is_rdp_enabled(),
            ..Default::default()
        },
    }
}

fn find_primary_disk(disks: &Disks) -> (u64, u64) {
    if disks.list().is_empty() {
        return (0, 0);
    }

    // Try to find the system/home disk by mount point priority
    let priority = if cfg!(target_os = "windows") {
        vec!["C:\\", "C:/"]
    } else {
        vec!["/", "/home"]
    };

    for mount in &priority {
        if let Some(disk) = disks.list().iter().find(|d| {
            d.mount_point().to_string_lossy().eq_ignore_ascii_case(mount)
        }) {
            let total = disk.total_space();
            let avail = disk.available_space();
            return (total.saturating_sub(avail), total);
        }
    }

    // Fallback: largest disk
    if let Some(disk) = disks.list().iter().max_by_key(|d| d.total_space()) {
        let total = disk.total_space();
        let avail = disk.available_space();
        return (total.saturating_sub(avail), total);
    }

    (0, 0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Formatting helpers used by the Node Card renderer in dashboard.rs
// ─────────────────────────────────────────────────────────────────────────────

/// Format bytes as a human-readable storage size
pub fn fmt_bytes(b: u64) -> String {
    const K: u64 = 1024;
    if b < K           { format!("{} B",   b) }
    else if b < K * K  { format!("{:.1} KB", b as f64 / K as f64) }
    else if b < K*K*K  { format!("{:.1} MB", b as f64 / (K * K) as f64) }
    else               { format!("{:.1} GB", b as f64 / (K * K * K) as f64) }
}

/// Color for a percentage gauge: green < 60%, amber < 85%, red ≥ 85%
pub fn gauge_color(pct: f32) -> egui::Color32 {
    if pct < 60.0 {
        crate::theme::Colors::GREEN
    } else if pct < 85.0 {
        crate::theme::Colors::AMBER
    } else {
        crate::theme::Colors::RED
    }
}

/// Draw a compact horizontal gauge bar.
/// `label`: e.g. "CPU", `pct`: 0–100, `suffix`: e.g. "67%" or "8.2 GB / 16 GB"
pub fn render_gauge(ui: &mut egui::Ui, label: &str, icon: Option<crate::theme::IconType>, pct: f32, suffix: &str) {
    use egui::RichText;
    use crate::theme::Colors;

    let color = gauge_color(pct);
    let clamped = pct.clamp(0.0, 100.0);

    ui.horizontal(|ui| {
        // Icon
        if let Some(icon) = icon {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            crate::theme::draw_vector_icon(ui, rect, icon, Colors::TEXT_DIM);
            ui.add_space(4.0);
        }

        // Label
        ui.label(
            RichText::new(label)
                .color(Colors::TEXT_DIM)
                .size(8.0)
                .strong()
        );
        ui.add_space(4.0);

        // Gauge track + fill
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(80.0, 6.0),
            egui::Sense::hover(),
        );
        // Track
        ui.painter().rect_filled(rect, egui::Rounding::ZERO, Colors::BORDER);
        // Fill
        let fill_w = rect.width() * clamped / 100.0;
        if fill_w > 0.0 {
            let fill = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
            ui.painter().rect_filled(fill, egui::Rounding::ZERO, color);
        }

        ui.add_space(6.0);
        // Value label
        ui.label(
            RichText::new(suffix)
                .color(color)
                .size(8.0)
        );
    });
}
