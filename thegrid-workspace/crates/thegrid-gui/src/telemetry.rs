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
use thegrid_core::models::{GpuDevice, NodeTelemetry, RamModule};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Collect a single telemetry snapshot for the local machine.
///
/// This function blocks for a brief moment (sysinfo requires a short sleep
/// between two CPU measurements for a meaningful utilisation number).
/// Always call this from a background thread.
pub fn collect_local() -> NodeTelemetry {
    // ── Pass 1: initialize CPU baseline ─────────────────────────────────────
    let mut sys = System::new();
    sys.refresh_cpu_usage();

    // sysinfo requires a short interval between two CPU reads for a meaningful
    // utilisation delta. MINIMUM_CPU_UPDATE_INTERVAL is ~200ms on most platforms.
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);

    // ── Pass 2: read accurate CPU usage after the delta window ───────────────
    sys.refresh_all();

    // CPU: average across all logical cores (sysinfo returns 0..100 per core)
    let cpu_pct = {
        let cpus = sys.cpus();
        if cpus.is_empty() {
            0.0
        } else {
            let sum: f32 = cpus.iter().map(|c| c.cpu_usage()).sum();
            (sum / cpus.len() as f32).clamp(0.0, 100.0)
        }
    };
    let cpu_cores_pct = Some(
        sys.cpus()
            .iter()
            .map(|c| c.cpu_usage().clamp(0.0, 100.0))
            .collect::<Vec<f32>>()
    );
    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty());
    let cpu_physical_cores = sys.physical_core_count().map(|n| n as u32);
    let cpu_logical_processors = {
        let n = sys.cpus().len() as u32;
        if n > 0 { Some(n) } else { None }
    };
    let cpu_freq_ghz = sys
        .cpus()
        .first()
        .map(|c| c.frequency() as f32 / 1000.0)
        .filter(|f| *f > 0.0);

    // RAM
    let ram_used  = sys.used_memory();   // bytes
    let ram_total = sys.total_memory();  // bytes

    // Disk: collect all drives first, then aggregate totals from that same list
    // so UI rows and header totals always match.
    let disks = Disks::new_with_refreshed_list();

    // CPU temperature (only available on some platforms/hardware)
    // sysinfo returns temperatures via Components. We look for a component
    // whose label contains "CPU" or "Package".
    // Note: on Windows this often requires admin rights; we tolerate None.
    let cpu_temp = None; // TODO: sysinfo Components API changed in 0.30 — re-enable later

    // AI capability: heuristic based on RAM
    let is_ai_capable = ram_total >= 8 * 1024 * 1024 * 1024; // 8 GB

    // GPU info (Windows WMIC best-effort)
    let gpu_devices = local_gpu_devices();
    let gpu_name = gpu_devices.first().map(|g| g.name.clone());

    let (ram_slots_used, ram_slots_total, ram_speed_mhz, ram_form_factor, ram_modules) = local_ram_slot_info();

    // Drive details
    let storage_hint = local_storage_kind_hint();
    let mut drive_infos = Vec::new();
    for disk in &disks {
        drive_infos.push(thegrid_core::models::DriveInfo {
            name: disk.mount_point().to_string_lossy().to_string(),
            used: disk.total_space() - disk.available_space(),
            total: disk.total_space(),
            kind: storage_hint.clone(),
        });
    }

    let (disk_used, disk_total) = if drive_infos.is_empty() {
        find_primary_disk(&disks)
    } else {
        let used = drive_infos.iter().map(|d| d.used).sum();
        let total = drive_infos.iter().map(|d| d.total).sum();
        (used, total)
    };
    
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

    let (running_processes, top_processes) = local_process_summary();

    // Network throughput: measure delta over a short window using sysinfo Networks
    let (net_rx_bps, net_tx_bps) = {
        use sysinfo::Networks;
        let mut nets = Networks::new_with_refreshed_list();
        std::thread::sleep(Duration::from_millis(500));
        nets.refresh();
        let (rx, tx) = nets.iter().fold((0u64, 0u64), |(r, t), (_, n)| {
            (r + n.received(), t + n.transmitted())
        });
        // received/transmitted are bytes since last refresh (~500ms) → scale to /s
        (Some(rx * 2), Some(tx * 2))
    };

    NodeTelemetry {
        device_type: "Desktop".to_string(), 
        cpu_pct,
        cpu_model,
        cpu_physical_cores,
        cpu_logical_processors,
        cpu_cores_pct,
        cpu_freq_ghz,
        ram_used,
        ram_total,
        ram_slots_used,
        ram_slots_total,
        ram_speed_mhz,
        ram_form_factor,
        ram_modules,
        disk_used,
        disk_total,
        cpu_temp,
        is_ai_capable,
        gpu_devices,
        gpu_name,
        gpu_pct: None,
        gpu_mem_used: None,
        gpu_mem_total: None,
        local_ips,
        running_processes,
        top_processes,
        ai_status: Some("Idle".into()),
        ai_tokens_per_sec: None,
        ai_thoughts: None,
        net_rx_bps,
        net_tx_bps,
        capabilities: thegrid_core::models::DeviceCapabilities {
            drives: drive_infos,
            has_rdp: thegrid_net::win_sys::is_rdp_enabled(),
            ..Default::default()
        },
    }
}

fn local_gpu_devices() -> Vec<GpuDevice> {
    if !cfg!(target_os = "windows") {
        return Vec::new();
    }

    if let Some(json) = run_powershell_json(
        "$g=Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,PNPDeviceID,VideoProcessor; $g | ConvertTo-Json -Compress"
    ) {
        let mut devices = Vec::<GpuDevice>::new();
        for item in json_items(&json) {
            let name = item.get("Name").and_then(|v| v.as_str()).unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            let is_discrete = is_discrete_gpu_name(name);
            let is_rtx = name.to_ascii_lowercase().contains("rtx");
            let mem_total = item
                .get("AdapterRAM")
                .and_then(|v| v.as_u64())
                .filter(|v| *v > 0);
            let pnp = item.get("PNPDeviceID").and_then(|v| v.as_str()).unwrap_or("");
            let bus_type = if is_discrete {
                Some("PCIe".to_string())
            } else if pnp.to_ascii_lowercase().contains("pci") {
                Some("Integrated".to_string())
            } else {
                Some("Integrated".to_string())
            };

            devices.push(GpuDevice {
                name: name.to_string(),
                is_discrete,
                is_integrated: !is_discrete,
                is_shared: !is_discrete,
                is_rtx,
                ai_capable: is_rtx,
                vendor: Some(gpu_vendor(name)),
                bus_type,
                vram_type: gpu_memory_tech(name),
                gpu_pct: None,
                mem_used: None,
                mem_total,
            });
        }
        if !devices.is_empty() {
            devices.sort_by_key(|g| if g.is_discrete { 0_u8 } else { 1_u8 });
            return devices;
        }
    }

    // WMIC fallback for older systems
    let output = match run_command_output_timeout(
        "wmic",
        &["path", "win32_VideoController", "get", "name"],
        Duration::from_secs(2),
    ) {
        Some(out) => out,
        None => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::<GpuDevice>::new();
    for line in text.lines() {
        let name = line.trim();
        if name.is_empty() || name.eq_ignore_ascii_case("name") {
            continue;
        }
        let is_discrete = is_discrete_gpu_name(name);
        let is_rtx = name.to_ascii_lowercase().contains("rtx");
        devices.push(GpuDevice {
            name: name.to_string(),
            is_discrete,
            is_integrated: !is_discrete,
            is_shared: !is_discrete,
            is_rtx,
            ai_capable: is_rtx,
            vendor: Some(gpu_vendor(name)),
            bus_type: Some(if is_discrete { "PCIe".to_string() } else { "Integrated".to_string() }),
            vram_type: gpu_memory_tech(name),
            gpu_pct: None,
            mem_used: None,
            mem_total: None,
        });
    }
    devices.sort_by_key(|g| if g.is_discrete { 0_u8 } else { 1_u8 });
    devices
}

fn is_discrete_gpu_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let integrated_markers = [
        "intel(r) uhd",
        "intel(r) iris",
        "intel hd",
        "radeon graphics",
        "adreno",
        "mali",
    ];
    if integrated_markers.iter().any(|m| n.contains(m)) {
        return false;
    }

    let discrete_markers = ["rtx", "gtx", "quadro", "tesla", "radeon rx", "arc "];
    discrete_markers.iter().any(|m| n.contains(m))
}

fn local_ram_slot_info() -> (Option<u32>, Option<u32>, Option<u32>, Option<String>, Vec<RamModule>) {
    if !cfg!(target_os = "windows") {
        return (None, None, None, None, Vec::new());
    }

    if let Some(json) = run_powershell_json(
        "$mods=Get-CimInstance Win32_PhysicalMemory | Select-Object DeviceLocator,BankLabel,Capacity,Speed,ConfiguredClockSpeed,SMBIOSMemoryType,FormFactor,Manufacturer,PartNumber; $slots=(Get-CimInstance Win32_PhysicalMemoryArray | Select-Object -First 1 -ExpandProperty MemoryDevices); [pscustomobject]@{slots=$slots;modules=$mods} | ConvertTo-Json -Compress"
    ) {
        let mut modules = Vec::<RamModule>::new();
        let mut speeds = Vec::<u32>::new();
        let mut form = None;

        let total_slots = json
            .get("slots")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .filter(|v| *v > 0);

        if let Some(mods_val) = json.get("modules") {
            for m in json_items(mods_val) {
                let capacity = m.get("Capacity").and_then(|v| v.as_u64()).unwrap_or(0);
                if capacity == 0 {
                    continue;
                }
                let speed = m.get("Speed").and_then(|v| v.as_u64()).map(|v| v as u32).filter(|v| *v > 0);
                let configured_speed = m.get("ConfiguredClockSpeed").and_then(|v| v.as_u64()).map(|v| v as u32).filter(|v| *v > 0);
                if let Some(s) = configured_speed.or(speed) {
                    speeds.push(s);
                }
                let ff = m.get("FormFactor").and_then(|v| v.as_u64()).map(|v| v as u32);
                let ff_txt = ff.and_then(form_factor_name);
                if form.is_none() {
                    form = ff_txt.clone();
                }
                let smbios = m.get("SMBIOSMemoryType").and_then(|v| v.as_u64()).map(|v| v as u32);
                let mem_type = smbios.and_then(memory_type_name);
                let slot = m.get("DeviceLocator").and_then(|v| v.as_str()).unwrap_or("SLOT").to_string();
                let part_number = m.get("PartNumber").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                modules.push(RamModule {
                    slot,
                    capacity,
                    speed_mhz: speed,
                    configured_speed_mhz: configured_speed,
                    memory_type: mem_type,
                    form_factor: ff_txt,
                    latency_cl: part_number.as_deref().and_then(parse_latency_cl),
                    manufacturer: m.get("Manufacturer").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
                    part_number,
                });
            }
        }

        if !modules.is_empty() {
            let used_slots = Some(modules.len() as u32);
            let avg_speed = if speeds.is_empty() { None } else { Some(speeds.iter().sum::<u32>() / speeds.len() as u32) };
            return (used_slots, total_slots, avg_speed, form, modules);
        }
    }

    let mut used_slots = None;
    let mut speed = None;
    let mut form_factor = None;
    let mut modules = Vec::<RamModule>::new();

    if let Some(output) = run_command_output_timeout(
        "wmic",
        &["memorychip", "get", "FormFactor,Speed,Capacity"],
        Duration::from_secs(2),
    ) {
        let text = String::from_utf8_lossy(&output.stdout);
        let mut used = 0_u32;
        let mut speeds = Vec::<u32>::new();
        let mut factors = Vec::<u32>::new();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.to_ascii_lowercase().contains("capacity") {
                continue;
            }
            let cols: Vec<&str> = l.split_whitespace().collect();
            if cols.len() >= 3 {
                used += 1;
                if let Ok(ff) = cols[0].parse::<u32>() {
                    if ff > 0 {
                        factors.push(ff);
                    }
                }
                if let Ok(mhz) = cols[1].parse::<u32>() {
                    if mhz > 0 {
                        speeds.push(mhz);
                    }
                }
                if let Ok(capacity) = cols[2].parse::<u64>() {
                    modules.push(RamModule {
                        slot: format!("SLOT {}", used),
                        capacity,
                        speed_mhz: cols[1].parse::<u32>().ok(),
                        configured_speed_mhz: cols[1].parse::<u32>().ok(),
                        memory_type: None,
                        form_factor: None,
                        latency_cl: None,
                        manufacturer: None,
                        part_number: None,
                    });
                }
            }
        }
        if used > 0 {
            used_slots = Some(used);
        }
        if !speeds.is_empty() {
            let avg = speeds.iter().sum::<u32>() / speeds.len() as u32;
            speed = Some(avg);
        }
        if let Some(ff) = factors.first().copied() {
            form_factor = Some(match ff {
                8 => "DIMM".to_string(),
                12 => "SODIMM".to_string(),
                _ => format!("FF{}", ff),
            });
        }
    }

    let mut total_slots = None;
    if let Some(output) = run_command_output_timeout(
        "wmic",
        &["memphysical", "get", "MemoryDevices"],
        Duration::from_secs(2),
    ) {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.to_ascii_lowercase().contains("memorydevices") {
                continue;
            }
            if let Ok(v) = l.parse::<u32>() {
                if v > 0 {
                    total_slots = Some(v);
                    break;
                }
            }
        }
    }

    (used_slots, total_slots, speed, form_factor, modules)
}

fn run_powershell_json(script: &str) -> Option<serde_json::Value> {
    let out = run_command_output_timeout(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
        Duration::from_secs(3),
    )?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(&text).ok()
}

fn json_items(v: &serde_json::Value) -> Vec<&serde_json::Value> {
    match v {
        serde_json::Value::Array(arr) => arr.iter().collect(),
        serde_json::Value::Object(_) => vec![v],
        _ => Vec::new(),
    }
}

fn run_command_output_timeout(cmd: &str, args: &[&str], timeout: Duration) -> Option<std::process::Output> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let started = Instant::now();
    loop {
        match child.try_wait().ok()? {
            Some(_) => return child.wait_with_output().ok(),
            None => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

fn gpu_vendor(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    if n.contains("nvidia") || n.contains("geforce") || n.contains("quadro") {
        "NVIDIA".to_string()
    } else if n.contains("amd") || n.contains("radeon") {
        "AMD".to_string()
    } else if n.contains("intel") {
        "INTEL".to_string()
    } else {
        "UNKNOWN".to_string()
    }
}

fn gpu_memory_tech(name: &str) -> Option<String> {
    let n = name.to_ascii_lowercase();
    if n.contains("rtx 50") {
        Some("GDDR7".to_string())
    } else if n.contains("rtx") || n.contains("arc") {
        Some("GDDR6".to_string())
    } else if n.contains("gtx") {
        Some("GDDR5/6".to_string())
    } else if n.contains("intel") || n.contains("uhd") || n.contains("iris") {
        Some("SHARED DDR".to_string())
    } else {
        None
    }
}

fn form_factor_name(v: u32) -> Option<String> {
    Some(match v {
        8 => "DIMM".to_string(),
        12 => "SODIMM".to_string(),
        26 => "SODIMM".to_string(),
        _ => return None,
    })
}

fn memory_type_name(v: u32) -> Option<String> {
    Some(match v {
        20 => "DDR".to_string(),
        21 => "DDR2".to_string(),
        24 => "DDR3".to_string(),
        26 => "DDR4".to_string(),
        34 => "DDR5".to_string(),
        _ => return None,
    })
}

fn local_process_summary() -> (Option<u32>, Vec<String>) {
    if cfg!(target_os = "windows") {
        if let Some(json) = run_powershell_json(
            "$p=Get-Process | Sort-Object CPU -Descending | Select-Object -First 5 ProcessName,MainWindowTitle; [pscustomobject]@{count=(Get-Process).Count;top=$p} | ConvertTo-Json -Compress"
        ) {
            let count = json.get("count").and_then(|v| v.as_u64()).map(|v| v as u32);
            let mut top = Vec::<String>::new();
            if let Some(v) = json.get("top") {
                for item in json_items(v) {
                    let name = item.get("ProcessName").and_then(|v| v.as_str()).unwrap_or("").trim();
                    if !name.is_empty() {
                        top.push(name.to_string());
                    }
                }
            }
            return (count, top);
        }
    }

    let mut sys = System::new_all();
    sys.refresh_all();
    let count = Some(sys.processes().len() as u32);
    let mut top = sys
        .processes()
        .values()
        .take(5)
        .map(|p| p.name().to_string())
        .collect::<Vec<_>>();
    top.sort();
    (count, top)
}

fn parse_latency_cl(part_number: &str) -> Option<u32> {
    let upper = part_number.to_ascii_uppercase();
    let pos = upper.find("CL")?;
    let digits: String = upper[pos + 2..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok()
    }
}

fn local_storage_kind_hint() -> Option<String> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let output = run_command_output_timeout(
        "wmic",
        &["diskdrive", "get", "Model,MediaType"],
        Duration::from_secs(2),
    )?;
    let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    if text.contains("nvme") {
        Some("NVME".to_string())
    } else if text.contains("ssd") || text.contains("solid state") {
        Some("SSD".to_string())
    } else if text.contains("hdd") || text.contains("hard disk") {
        Some("HDD".to_string())
    } else {
        None
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
