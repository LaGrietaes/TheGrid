use anyhow::Result;
use chrono::Local;
use semver::Version;
use serde::Deserialize;
use terminal_size::{Width, terminal_size};

use std::collections::VecDeque;
use std::io::{self, IsTerminal, Write};
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use thegrid_core::{AppEvent, Config, models::SyncHealthMetrics};
use thegrid_runtime::AppRuntime;

const RELEASES_LATEST_URL: &str = "https://api.github.com/repos/LaGrietaes/TheGrid/releases/latest";
const SIGNATURE_LINE: &str = "> Powered and Designed by: sinergias.lagrieta.es";
const LAST_UPDATE_ENV: &str = "THEGRID_LAST_UPDATE";

const ANSI_RESET: &str = "\x1B[0m";
const ANSI_BOLD: &str = "\x1B[1m";
const ANSI_DIM: &str = "\x1B[2m";
const ANSI_GREEN: &str = "\x1B[32m";
const ANSI_WHITE: &str = "\x1B[37m";

// S2-C1: command registry metadata used by help output and TUI hints.
const COMMAND_REGISTRY: &[(&str, &str)] = &[
    ("help", "Show command list"),
    ("devices", "Refresh connected device list"),
    ("ping <ip|#>", "Ping device + agent"),
    ("pingdev <ip|#>", "Ping device endpoint"),
    ("pingagent <ip|#>", "Ping agent endpoint"),
    ("mesh [status]", "Sync health overview"),
    ("mesh sync <ip|#>", "Trigger sync to device"),
    ("dupes", "Scan and report duplicate files"),
    ("history | !! | !N", "Command history and replay"),
    ("update", "Check latest release"),
    ("gitupdate", "Fetch, pull, build, restart"),
    ("quit", "Stop node"),
];

fn command_hint_lines(max_lines: usize) -> Vec<String> {
    COMMAND_REGISTRY
        .iter()
        .take(max_lines)
        .map(|(usage, _)| format!("  {}", usage))
        .collect()
}

fn command_help_lines() -> Vec<String> {
    COMMAND_REGISTRY
        .iter()
        .map(|(usage, desc)| format!("{} - {}", usage, desc))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedCommand<'a> {
    Help,
    Devices,
    Ping,
    PingDevice,
    PingAgent,
    Mesh,
    Dupes,
    History,
    Update,
    GitUpdate,
    Quit,
    Unknown(&'a str),
}

fn parse_command(token: &str) -> ParsedCommand<'_> {
    match token.to_ascii_lowercase().as_str() {
        "help" => ParsedCommand::Help,
        "devices" => ParsedCommand::Devices,
        "ping" => ParsedCommand::Ping,
        "pingdev" => ParsedCommand::PingDevice,
        "pingagent" => ParsedCommand::PingAgent,
        "mesh" => ParsedCommand::Mesh,
        "dupes" | "duplicates" => ParsedCommand::Dupes,
        "history" => ParsedCommand::History,
        "update" => ParsedCommand::Update,
        "gitupdate" => ParsedCommand::GitUpdate,
        "quit" | "exit" => ParsedCommand::Quit,
        _ => ParsedCommand::Unknown(token),
    }
}

#[derive(Debug, Deserialize)]
struct ReleaseInfo {
    tag_name: String,
    html_url: String,
}

enum GitUpdateOutcome {
    UpToDate { head: String },
    Updated { from: String, to: String },
}

fn parse_version_tag(tag: &str) -> Option<Version> {
    let clean = tag.trim().trim_start_matches('v').trim_start_matches('V');
    Version::parse(clean).ok()
}

fn check_latest_release() -> Result<Option<ReleaseInfo>> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let response = client
        .get(RELEASES_LATEST_URL)
        .header("User-Agent", format!("thegrid-node/{}", env!("CARGO_PKG_VERSION")))
        .send()?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // Repository has no published releases yet.
        return Ok(None);
    }

    let release = response.error_for_status()?.json::<ReleaseInfo>()?;

    let latest = match parse_version_tag(&release.tag_name) {
        Some(v) => v,
        None => return Ok(None),
    };

    if latest > current {
        Ok(Some(release))
    } else {
        Ok(None)
    }
}

fn prompt_yes_no(prompt: &str) -> bool {
    print!("{}", prompt);
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn try_git_update() -> Result<GitUpdateOutcome> {
    let probe = Command::new("git")
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output()?;

    if !probe.status.success() {
        anyhow::bail!("Current directory is not a git repository");
    }

    let before = Command::new("git")
        .arg("rev-parse")
        .arg("--short")
        .arg("HEAD")
        .output()?;
    if !before.status.success() {
        anyhow::bail!("Failed to read current git HEAD");
    }
    let before_head = String::from_utf8_lossy(&before.stdout).trim().to_string();

    let fetch = Command::new("git")
        .arg("fetch")
        .arg("--prune")
        .output()?;
    if !fetch.status.success() {
        let stderr = String::from_utf8_lossy(&fetch.stderr);
        anyhow::bail!("git fetch failed: {}", stderr.trim());
    }

    let pull = Command::new("git")
        .arg("pull")
        .arg("--ff-only")
        .output()?;

    if !pull.status.success() {
        let stderr = String::from_utf8_lossy(&pull.stderr);
        anyhow::bail!("git pull failed: {}", stderr.trim());
    }

    let after = Command::new("git")
        .arg("rev-parse")
        .arg("--short")
        .arg("HEAD")
        .output()?;
    if !after.status.success() {
        anyhow::bail!("Failed to read updated git HEAD");
    }
    let after_head = String::from_utf8_lossy(&after.stdout).trim().to_string();

    if before_head == after_head {
        Ok(GitUpdateOutcome::UpToDate { head: after_head })
    } else {
        Ok(GitUpdateOutcome::Updated {
            from: before_head,
            to: after_head,
        })
    }
}

fn git_branch_head() -> Option<String> {
    let branch_out = Command::new("git")
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .output()
        .ok()?;
    if !branch_out.status.success() {
        return None;
    }

    let head_out = Command::new("git")
        .arg("rev-parse")
        .arg("--short")
        .arg("HEAD")
        .output()
        .ok()?;
    if !head_out.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&branch_out.stdout).trim().to_string();
    let head = String::from_utf8_lossy(&head_out.stdout).trim().to_string();
    Some(format!("{} @ {}", branch, head))
}

fn try_rebuild_node() -> Result<String> {
    let build = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("thegrid-node")
        .output()?;

    if !build.status.success() {
        let stderr = String::from_utf8_lossy(&build.stderr);
        anyhow::bail!("cargo build failed: {}", stderr.trim());
    }

    Ok("Build completed for thegrid-node".to_string())
}

fn restart_current_node_process(updated_from_to: Option<(&str, &str)>) -> Result<String> {
    let exe = std::env::current_exe()?;
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // Avoid immediate release-check prompt loop after self-restart.
    if !args.iter().any(|a| a == "--skip-update-check") {
        args.push("--skip-update-check".to_string());
    }

    let mut direct_cmd = Command::new(&exe);
    direct_cmd
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some((from, to)) = updated_from_to {
        direct_cmd.env(LAST_UPDATE_ENV, format!("{}:{}", from, to));
    }
    let direct = direct_cmd.spawn();

    match direct {
        Ok(_) => {
            return Ok(format!("Launched updated binary: {}", exe.display()));
        }
        Err(direct_err) => {
            // Fallback: if direct binary path is unavailable (common in some cargo-run layouts),
            // relaunch through cargo from current workspace.
            let mut fallback_cmd = Command::new("cargo");
            fallback_cmd
                .arg("run")
                .arg("-p")
                .arg("thegrid-node")
                .arg("--")
                .arg("--skip-update-check")
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            if let Some((from, to)) = updated_from_to {
                fallback_cmd.env(LAST_UPDATE_ENV, format!("{}:{}", from, to));
            }
            let fallback = fallback_cmd.spawn();

            match fallback {
                Ok(_) => {
                    return Ok("Direct restart path missing; launched via cargo run fallback".to_string());
                }
                Err(fallback_err) => {
                    anyhow::bail!(
                        "Direct restart failed ({}) and cargo fallback failed ({})",
                        direct_err,
                        fallback_err
                    );
                }
            }
        }
    }
}

fn ts() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

fn print_banner(device_name: &str, port: u16) {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║ THE GRID HEADLESS NODE v{:<35} ║", env!("CARGO_PKG_VERSION"));
    println!("║ {:<61} ║", SIGNATURE_LINE);
    println!("╠═══════════════════════════════════════════════════════════════╣");
    println!("║ Device: {:<55}║", device_name);
    println!("║ Agent Port: {:<51}║", port);
    println!("╚═══════════════════════════════════════════════════════════════╝");
}

fn event_line(icon: &str, label: &str, message: impl AsRef<str>) {
    println!("{} {} {:<12} {}", ts(), icon, label, message.as_ref());
}

#[derive(Debug)]
struct TuiState {
    started_at: Instant,
    last_status: String,
    recent_logs: VecDeque<String>,
    devices: Vec<(String, String)>, // (display_name, ip)
    ping_ok: u64,
    ping_fail: u64,
    command_history: VecDeque<String>,
    // S2-C3: sync health snapshot keyed by device_id
    sync_health: std::collections::HashMap<String, SyncHealthMetrics>,
    dirty: bool,
}

impl TuiState {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            last_status: "READY".to_string(),
            recent_logs: VecDeque::with_capacity(256),
            devices: Vec::new(),
            ping_ok: 0,
            ping_fail: 0,
            command_history: VecDeque::with_capacity(128),
            sync_health: std::collections::HashMap::new(),
            dirty: true,
        }
    }

    fn push_log(&mut self, line: String) {
        self.recent_logs.push_back(line);
        while self.recent_logs.len() > 200 {
            let _ = self.recent_logs.pop_front();
        }
    }

    fn push_cmd_history(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        if self.command_history.back().map(|c| c == cmd).unwrap_or(false) {
            return;
        }
        self.command_history.push_back(cmd.to_string());
        while self.command_history.len() > 100 {
            let _ = self.command_history.pop_front();
        }
    }
}

fn term_width() -> usize {
    terminal_size()
        .map(|(Width(w), _)| w as usize)
        .or_else(|| {
            std::env::var("COLUMNS")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
        })
        .map(|v| v.max(80))
        .unwrap_or(110)
}

fn status_color(_status: &str) -> &'static str {
    ANSI_GREEN
}

fn log_color(_line: &str) -> &'static str {
    ANSI_WHITE
}

fn pulse_frame(elapsed: Duration) -> &'static str {
    match (elapsed.as_millis() / 260) % 4 {
        0 => "◴",
        1 => "◷",
        2 => "◶",
        _ => "◵",
    }
}

fn render_labeled_line(
    left: &str,
    right: &str,
    left_w: usize,
    right_w: usize,
    left_color: &str,
    right_color: &str,
) {
    println!(
        "{ANSI_GREEN}║{ANSI_RESET} {}{}{ANSI_RESET} {ANSI_GREEN}│{ANSI_RESET} {}{}{ANSI_RESET} {ANSI_GREEN}║{ANSI_RESET}",
        left_color,
        trim_fit(left, left_w),
        right_color,
        trim_fit(right, right_w),
    );
}

fn trim_fit(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return format!("{s:<width$}");
    }
    if width <= 1 {
        return "".to_string();
    }
    let mut out = String::new();
    for c in s.chars().take(width - 1) {
        out.push(c);
    }
    out.push('…');
    out
}

fn render_tui(state: &TuiState, device_name: &str, port: u16) {
    let width = term_width();
    let left_w = (width.saturating_sub(7)) / 2;
    let right_w = width.saturating_sub(7).saturating_sub(left_w);
    let lower_w = width.saturating_sub(4);

    print!("\x1B[2J\x1B[H");

    let uptime = state.started_at.elapsed().as_secs();
    let pulse = pulse_frame(state.started_at.elapsed());
    // S2-C3: build sync health summary line(s)
    let sync_summary: Vec<String> = {
        let mut lines = Vec::new();
        for (id, h) in &state.sync_health {
            let age = h.sync_age_secs.map(|s| format!("{}s ago", s)).unwrap_or_else(|| "never".to_string());
            lines.push(format!("Sync {}: age={} tombs={} fail={}", id, age, h.tombstone_count, h.sync_failures));
        }
        lines
    };

    let mut left = vec![
        format!("{} ████████╗██╗  ██╗███████╗ ██████╗ ██████╗ ██╗██████╗", pulse),
        "   ╚══██╔══╝██║  ██║██╔════╝██╔════╝ ██╔══██╗██║██╔══██╗".to_string(),
        "      ██║   ███████║█████╗  ██║  ███╗██████╔╝██║██║  ██║".to_string(),
        "      ██║   ██╔══██║██╔══╝  ██║   ██║██╔══██╗██║██║  ██║".to_string(),
        "      ██║   ██║  ██║███████╗╚██████╔╝██║  ██║██║██████╔╝".to_string(),
        "      ╚═╝   ╚═╝  ╚═╝╚══════╝ ╚═════╝ ╚═╝  ╚═╝╚═╝╚═════╝".to_string(),
        format!("NODE v{}", env!("CARGO_PKG_VERSION")),
        SIGNATURE_LINE.to_string(),
        format!("Device: {device_name}"),
        format!("Agent Port: {port}"),
        format!("Uptime: {}s", uptime),
        format!("Ping OK/Fail: {}/{}", state.ping_ok, state.ping_fail),
    ];
    if sync_summary.is_empty() {
        left.push("Sync: no data yet".to_string());
    } else {
        left.extend(sync_summary.into_iter().take(3));
    }
    left.push(format!("Last: {}", state.last_status));

    let mut logo_rows = 7usize;
    // Keep key runtime fields near the bottom when width is tight.
    if left_w < 52 {
        logo_rows = 1;
        left = vec![
            format!("{} TG NODE v{}", pulse, env!("CARGO_PKG_VERSION")),
            "Powered and Designed by: sinergias.lagrieta.es".to_string(),
            format!("Device: {device_name}"),
            format!("Agent Port: {port}"),
            format!("Uptime: {}s", uptime),
            format!("Ping OK/Fail: {}/{}", state.ping_ok, state.ping_fail),
            format!("Last: {}", state.last_status),
        ];
    }

    let mut commands = vec!["COMMANDS".to_string()];
    commands.extend(command_hint_lines(8));
    commands.push("CONNECTED".to_string());

    let mut dev_commands = vec![
        "DEV COMMANDS".to_string(),
        "  gitupdate".to_string(),
        "  pull+build+restart".to_string(),
    ];
    if let Some(refs) = git_branch_head() {
        dev_commands.push(format!("  {}", refs));
    } else {
        dev_commands.push("  git: unavailable".to_string());
    }
    dev_commands.push("  git status (shell)".to_string());

    for (idx, (name, ip)) in state.devices.iter().take(5).enumerate() {
        commands.push(format!("  {}. {} ({})", idx + 1, name, ip));
    }
    if state.devices.is_empty() {
        commands.push("  none yet (run: devices)".to_string());
    }

    let cmd_w = (right_w.saturating_sub(3)) / 2;
    let dev_w = right_w.saturating_sub(3).saturating_sub(cmd_w);
    let right_lines = commands.len().max(dev_commands.len());

    let upper_lines = left.len().max(right_lines);
    println!(
        "{ANSI_GREEN}╔{}╦{}╗{ANSI_RESET}",
        "═".repeat(left_w + 2),
        "═".repeat(right_w + 2)
    );
    for i in 0..upper_lines {
        let l = left.get(i).map_or("", |s| s.as_str());
        let c = commands.get(i).map_or("", |s| s.as_str());
        let d = dev_commands.get(i).map_or("", |s| s.as_str());
        let r = format!("{} │ {}", trim_fit(c, cmd_w), trim_fit(d, dev_w));
        let right_color = if i == 0 { ANSI_BOLD } else { ANSI_WHITE };
        let left_color = if i < logo_rows { ANSI_GREEN } else { ANSI_WHITE };
        let status_idx = left.len().saturating_sub(1);
        if i == status_idx {
            println!(
                "{ANSI_GREEN}║{ANSI_RESET} {status_c}{}{ANSI_RESET} {ANSI_GREEN}│{ANSI_RESET} {}{}{ANSI_RESET} {ANSI_GREEN}║{ANSI_RESET}",
                trim_fit(l, left_w),
                right_color,
                trim_fit(&r, right_w),
                status_c = status_color(&state.last_status),
            );
        } else {
            render_labeled_line(l, &r, left_w, right_w, left_color, right_color);
        }
    }
    println!("{ANSI_GREEN}╠{}╣{ANSI_RESET}", "═".repeat(width.saturating_sub(2)));

    let max_logs = 12usize;
    let start = state.recent_logs.len().saturating_sub(max_logs);
    for line in state.recent_logs.iter().skip(start) {
        println!(
            "{ANSI_GREEN}║{ANSI_RESET} {}{}{ANSI_RESET} {ANSI_GREEN}║{ANSI_RESET}",
            log_color(line),
            trim_fit(line, lower_w)
        );
    }
    for _ in state.recent_logs.iter().skip(start).count()..max_logs {
        println!("{ANSI_GREEN}║{ANSI_RESET} {} {ANSI_GREEN}║{ANSI_RESET}", " ".repeat(lower_w));
    }

    println!("{ANSI_GREEN}╠{}╣{ANSI_RESET}", "═".repeat(width.saturating_sub(2)));
    println!(
        "{ANSI_GREEN}║{ANSI_RESET} {ANSI_DIM}{}{ANSI_RESET} {ANSI_GREEN}║{ANSI_RESET}",
        trim_fit("Type command then Enter (help for list, history for recall)", lower_w)
    );
    println!("{ANSI_GREEN}╚{}╝{ANSI_RESET}", "═".repeat(width.saturating_sub(2)));
    print!("{ANSI_BOLD}{ANSI_GREEN}> {ANSI_RESET}");
    let _ = io::stdout().flush();
}

fn emit(state: &Arc<Mutex<TuiState>>, tui_mode: bool, icon: &str, label: &str, message: impl AsRef<str>) {
    let message = message.as_ref().to_string();
    if !tui_mode {
        event_line(icon, label, &message);
    }

    if let Ok(mut s) = state.lock() {
        s.last_status = format!("{} {}", label, message);
        s.push_log(format!("{} {} {:<10} {}", ts(), icon, label, message));
        s.dirty = true;
    }
}

fn spawn_command_reader(cmd_tx: mpsc::Sender<String>, running: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let stdin = io::stdin();
        while running.load(Ordering::Relaxed) {
            let mut line = String::new();
            let read = match stdin.read_line(&mut line) {
                Ok(n) => n,
                Err(_) => break,
            };
            if read == 0 {
                break;
            }
            let cmd = line.trim().to_string();
            if cmd.is_empty() {
                continue;
            }
            if cmd_tx.send(cmd).is_err() {
                break;
            }
        }
    });
}

fn resolve_history_alias(
    cmd_line: &str,
    ui_state: &Arc<Mutex<TuiState>>,
    tui_mode: bool,
) -> Option<String> {
    let mut effective_cmd = cmd_line.trim().to_string();
    if effective_cmd == "!!" {
        if let Ok(s) = ui_state.lock() {
            if let Some(last) = s.command_history.back() {
                effective_cmd = last.clone();
            } else {
                emit(ui_state, tui_mode, "⚠", "CMD", "No command history yet");
                return None;
            }
        }
    } else if let Some(n_str) = effective_cmd.strip_prefix('!') {
        if let Ok(n) = n_str.parse::<usize>() {
            if let Ok(s) = ui_state.lock() {
                if n == 0 || n > s.command_history.len() {
                    emit(
                        ui_state,
                        tui_mode,
                        "⚠",
                        "CMD",
                        format!("History index out of range: {}", n),
                    );
                    return None;
                }
                effective_cmd = s.command_history[n - 1].clone();
            }
        }
    }

    Some(effective_cmd)
}

fn resolve_ping_target(target: &str, ui_state: &Arc<Mutex<TuiState>>) -> Option<(String, String)> {
    if let Ok(idx) = target.parse::<usize>() {
        if let Ok(s) = ui_state.lock() {
            if idx == 0 || idx > s.devices.len() {
                None
            } else {
                let (name, ip) = s.devices[idx - 1].clone();
                Some((format!("#{} {}", idx, name), ip))
            }
        } else {
            None
        }
    } else {
        Some((target.to_string(), target.to_string()))
    }
}

// S2-C2: isolate command execution from the main loop for safer incremental CLI growth.
fn execute_command(
    effective_cmd: &str,
    runtime: &AppRuntime,
    ui_state: &Arc<Mutex<TuiState>>,
    tui_mode: bool,
    running: &Arc<AtomicBool>,
) {
    if let Ok(mut s) = ui_state.lock() {
        s.push_cmd_history(effective_cmd);
    }

    let parts: Vec<&str> = effective_cmd.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }

    match parse_command(parts[0]) {
        ParsedCommand::Help => {
            for line in command_help_lines() {
                emit(ui_state, tui_mode, "ℹ", "CMD", line);
            }
        }
        ParsedCommand::Devices => {
            runtime.spawn_load_devices();
            emit(ui_state, tui_mode, "↻", "CMD", "Refreshing connected device list...");
        }
        ParsedCommand::Ping => {
            if let Some(target) = parts.get(1) {
                if let Some((label, ip)) = resolve_ping_target(target, ui_state) {
                    runtime.spawn_ping_device(ip.clone(), true);
                    runtime.spawn_ping(ip.clone(), true);
                    emit(ui_state, tui_mode, "◎", "CMD", format!("Ping {} -> device + agent", label));
                } else {
                    emit(ui_state, tui_mode, "⚠", "CMD", format!("Device index {} not found", target));
                }
            } else {
                emit(ui_state, tui_mode, "⚠", "CMD", "Usage: ping <ip|device_index>");
            }
        }
        ParsedCommand::PingDevice => {
            if let Some(target) = parts.get(1) {
                if let Some((label, ip)) = resolve_ping_target(target, ui_state) {
                    runtime.spawn_ping_device(ip, true);
                    emit(ui_state, tui_mode, "◎", "CMD", format!("Device ping {}", label));
                } else {
                    emit(ui_state, tui_mode, "⚠", "CMD", format!("Device index {} not found", target));
                }
            } else {
                emit(ui_state, tui_mode, "⚠", "CMD", "Usage: pingdev <ip|device_index>");
            }
        }
        ParsedCommand::PingAgent => {
            if let Some(target) = parts.get(1) {
                if let Some((label, ip)) = resolve_ping_target(target, ui_state) {
                    runtime.spawn_ping(ip, true);
                    emit(ui_state, tui_mode, "◎", "CMD", format!("Agent ping {}", label));
                } else {
                    emit(ui_state, tui_mode, "⚠", "CMD", format!("Device index {} not found", target));
                }
            } else {
                emit(ui_state, tui_mode, "⚠", "CMD", "Usage: pingagent <ip|device_index>");
            }
        }
        // S2-C4: mesh command group — status + per-device sync trigger
        ParsedCommand::Mesh => {
            let sub = parts.get(1).copied().unwrap_or("status");
            match sub {
                "sync" => {
                    if let Some(target) = parts.get(2) {
                        if let Some((label, ip)) = resolve_ping_target(target, ui_state) {
                            // Use the label as both device_id and hostname for now;
                            // the runtime will do the actual push over the agent protocol.
                            runtime.spawn_sync_node(label.clone(), ip.clone(), label.clone());
                            emit(ui_state, tui_mode, "⇄", "MESH", format!("Sync triggered -> {}", label));
                        } else {
                            emit(ui_state, tui_mode, "⚠", "MESH", format!("Device {} not found", target));
                        }
                    } else {
                        emit(ui_state, tui_mode, "⚠", "MESH", "Usage: mesh sync <ip|#>");
                    }
                }
                "status" | _ => {
                    if let Ok(s) = ui_state.lock() {
                        if s.sync_health.is_empty() {
                            emit(ui_state, tui_mode, "ℹ", "MESH", "No sync health data yet");
                        } else {
                            for (id, h) in &s.sync_health {
                                let age = h.sync_age_secs
                                    .map(|a| format!("{}s", a))
                                    .unwrap_or_else(|| "never".to_string());
                                emit(
                                    ui_state, tui_mode, "⇄", "MESH",
                                    format!("{}: sync_age={} tombs={} fails={} sources={{fs={},watch={},sync={}}}",
                                        id, age,
                                        h.tombstone_count, h.sync_failures,
                                        h.detection_sources.full_scan,
                                        h.detection_sources.watcher,
                                        h.detection_sources.sync,
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
        // Duplicate file detection — triggers async scan; results arrive via DuplicatesFound event
        ParsedCommand::Dupes => {
            emit(ui_state, tui_mode, "◈", "DUPES", "Scanning index for duplicate files...");
            runtime.spawn_duplicates_scan();
        }
        ParsedCommand::History => {
            let history_lines = if let Ok(s) = ui_state.lock() {
                if s.command_history.is_empty() {
                    Vec::new()
                } else {
                    s.command_history
                        .iter()
                        .enumerate()
                        .map(|(idx, cmd)| format!("{}: {}", idx + 1, cmd))
                        .collect::<Vec<_>>()
                }
            } else {
                Vec::new()
            };

            if history_lines.is_empty() {
                emit(ui_state, tui_mode, "ℹ", "HISTORY", "No commands in history yet");
            } else {
                for line in history_lines {
                    emit(ui_state, tui_mode, "·", "HISTORY", line);
                }
            }
        }
        ParsedCommand::Update => {
            emit(ui_state, tui_mode, "…", "UPDATE", "Checking latest release...");

            let (release_tx, release_rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = release_tx.send(check_latest_release());
            });

            match release_rx.recv_timeout(Duration::from_secs(8)) {
                Ok(Ok(Some(release))) => {
                    emit(
                        ui_state,
                        tui_mode,
                        "⬆",
                        "UPDATE",
                        format!("New release {} available: {}", release.tag_name, release.html_url),
                    );
                }
                Ok(Ok(None)) => emit(ui_state, tui_mode, "✓", "UPDATE", "Already up to date"),
                Ok(Err(e)) => emit(ui_state, tui_mode, "⚠", "UPDATE", format!("Check failed: {}", e)),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    emit(ui_state, tui_mode, "⚠", "UPDATE", "Check timed out after 8s");
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    emit(ui_state, tui_mode, "⚠", "UPDATE", "Check failed: worker disconnected");
                }
            }
        }
        ParsedCommand::GitUpdate => {
            emit(ui_state, tui_mode, "↻", "GIT", "Fetching + fast-forward pull...");
            match try_git_update() {
                Ok(GitUpdateOutcome::UpToDate { head }) => {
                    emit(ui_state, tui_mode, "✓", "GIT", format!("Version at latest version : {}", head));
                }
                Ok(GitUpdateOutcome::Updated { from, to }) => {
                    emit(ui_state, tui_mode, "✓", "GIT", format!("Updated {} -> {}", from, to));
                    emit(ui_state, tui_mode, "↻", "BUILD", "Rebuilding thegrid-node...");
                    match try_rebuild_node() {
                        Ok(build_msg) => {
                            emit(ui_state, tui_mode, "✓", "BUILD", build_msg);
                            emit(ui_state, tui_mode, "↻", "RESTART", "Launching updated node process...");
                            match restart_current_node_process(Some((&from, &to))) {
                                Ok(msg) => {
                                    emit(ui_state, tui_mode, "✓", "RESTART", msg);
                                    emit(ui_state, tui_mode, "✓", "RESTART", "Closing old process...");
                                    running.store(false, Ordering::Relaxed);
                                }
                                Err(e) => {
                                    emit(ui_state, tui_mode, "⚠", "RESTART", format!("> Update Failed Check logs: {}", e));
                                }
                            }
                        }
                        Err(e) => {
                            emit(ui_state, tui_mode, "⚠", "BUILD", format!("> Update Failed Check logs: {}", e));
                        }
                    }
                }
                Err(e) => emit(ui_state, tui_mode, "⚠", "GIT", format!("> Update Failed Check logs: {}", e)),
            }
        }
        ParsedCommand::Quit => {
            emit(ui_state, tui_mode, "■", "CMD", "Stopping node...");
            running.store(false, Ordering::Relaxed);
        }
        ParsedCommand::Unknown(other) => {
            emit(ui_state, tui_mode, "⚠", "CMD", format!("Unknown command: {}", other));
        }
    }
}

fn main() -> Result<()> {
    // Quick pre-scan args + env to know if TUI will be active BEFORE logger init.
    // In TUI mode we silence INFO noise — the rolling log panel captures events via emit().
    let raw_args: Vec<String> = std::env::args().collect();
    let pre_plain = raw_args.iter().any(|a| a == "--plain")
        || std::env::var("THEGRID_PLAIN").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let pre_force = raw_args.iter().any(|a| a == "--force-tui")
        || std::env::var("THEGRID_FORCE_TUI").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let expect_tui = !pre_plain && (pre_force || (io::stdin().is_terminal() && io::stdout().is_terminal()));

    // In TUI mode silence ALL log output — env_logger writes to stderr which cannot be
    // cleared by the TUI's ANSI escape and would corrupt the layout. All meaningful events
    // are captured through emit() into the rolling log panel instead.
    let log_default = if expect_tui { "off" } else { "info" };
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(log_default)
    ).init();

    log::info!("THE GRID Headless Node v{} starting", env!("CARGO_PKG_VERSION"));

    // 1. Load Config
    let mut config = Config::load().unwrap_or_else(|e| {
        log::warn!("Failed to load config: {}. Using default.", e);
        Config::default()
    });

    // Support environment variables
    if let Ok(key) = std::env::var("THEGRID_API_KEY") {
        config.api_key = key;
    }
    if let Ok(name) = std::env::var("THEGRID_DEVICE_NAME") {
        config.device_name = name;
    }

    // Robust CLI Argument Parsing
    let args: Vec<String> = std::env::args().collect();
    let mut skip_update_check = std::env::var("THEGRID_SKIP_UPDATE_CHECK")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let mut auto_update = std::env::var("THEGRID_AUTO_UPDATE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let mut plain_mode = std::env::var("THEGRID_PLAIN")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let mut force_tui = std::env::var("THEGRID_FORCE_TUI")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--api-key" => {
                if i + 1 < args.len() {
                    config.api_key = args[i + 1].trim().to_string();
                    i += 1;
                }
            }
            "--device-name" => {
                if i + 1 < args.len() {
                    config.device_name = args[i + 1].clone();
                    i += 1;
                }
            }
            "--port" | "--agent-port" => {
                if i + 1 < args.len() {
                    if let Ok(p) = args[i + 1].parse::<u16>() {
                        config.agent_port = p;
                        i += 1;
                    }
                }
            }
            "--skip-update-check" => {
                skip_update_check = true;
            }
            "--yes-update" => {
                auto_update = true;
            }
            "--plain" => {
                plain_mode = true;
            }
            "--force-tui" | "--froce-tui" => {
                force_tui = true;
            }
            _ => {
                log::warn!("Unknown argument: {}", args[i]);
            }
        }
        i += 1;
    }

    if config.device_name.is_empty() {
        config.device_name = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "UNKNOWN-NODE".to_string());
    }

    log::info!("Config: device={}, port={}, key_len={}", 
        config.device_name, config.agent_port, config.api_key.len());

    let stdin_tty = io::stdin().is_terminal();
    let stdout_tty = io::stdout().is_terminal();
    let tui_mode = !plain_mode && (force_tui || (stdin_tty && stdout_tty));

    if !tui_mode {
        print_banner(&config.device_name, config.agent_port);
        println!(
            "  TUI mode : OFF  (stdin_tty={}, stdout_tty={}, plain={}, force={})",
            stdin_tty, stdout_tty, plain_mode, force_tui
        );
        println!("  Tip      : run with --force-tui to enable the interactive interface");
        println!();
    }

    let ui_state = Arc::new(Mutex::new(TuiState::new()));
    if tui_mode {
        emit(&ui_state, tui_mode, "ℹ", "BOOT", "TUI mode active");
    }
    emit(&ui_state, tui_mode, "✓", "BOOT", "Configuration loaded");
    if let Ok(v) = std::env::var(LAST_UPDATE_ENV) {
        if let Some((from, to)) = v.split_once(':') {
            emit(
                &ui_state,
                tui_mode,
                "✓",
                "UPDATE",
                format!("> Version updated {} to {} successfully", from, to),
            );
        }
    }

    if !skip_update_check {
        match check_latest_release() {
            Ok(Some(release)) => {
                emit(
                    &ui_state,
                    tui_mode,
                    "⬆",
                    "UPDATE",
                    format!(
                        "New release {} available (current v{})",
                        release.tag_name,
                        env!("CARGO_PKG_VERSION")
                    ),
                );
                emit(&ui_state, tui_mode, "ℹ", "UPDATE", format!("Release: {}", release.html_url));

                let should_update = if auto_update {
                    true
                } else if io::stdin().is_terminal() {
                    prompt_yes_no("Update now? [y/N]: ")
                } else {
                    false
                };

                if should_update {
                    match try_git_update() {
                        Ok(GitUpdateOutcome::UpToDate { head }) => {
                            emit(&ui_state, tui_mode, "✓", "UPDATE", format!("Version at latest version : {}", head));
                        }
                        Ok(GitUpdateOutcome::Updated { from, to }) => {
                            emit(&ui_state, tui_mode, "✓", "UPDATE", format!("Updated {} -> {}", from, to));
                            emit(&ui_state, tui_mode, "ℹ", "UPDATE", "Restart node to run latest release.");
                            return Ok(());
                        }
                        Err(e) => {
                            emit(&ui_state, tui_mode, "⚠", "UPDATE", format!("Auto-update failed: {}", e));
                            emit(&ui_state, tui_mode, "ℹ", "UPDATE", "You can continue now and update later.");
                        }
                    }
                } else {
                    emit(&ui_state, tui_mode, "⏭", "UPDATE", "Skipped for now");
                }
            }
            Ok(None) => {
                log::debug!("No newer release found.");
            }
            Err(e) => {
                log::debug!("Release check failed (continuing): {}", e);
            }
        }
    }

    // 2. Initialize Runtime
    let (tx, rx) = mpsc::channel::<AppEvent>();
    let runtime = AppRuntime::new(config, tx)?;
    runtime.start_services();

    emit(&ui_state, tui_mode, "▶", "RUNTIME", "Services started. Press Ctrl+C to stop.");

    {
        let cfg = runtime.config.lock().unwrap().clone();
        if cfg.watch_paths.is_empty() {
            emit(
                &ui_state,
                tui_mode,
                "ℹ",
                "INDEX",
                "No watch paths configured. Edit config.json to start indexing.",
            );
        } else {
            let resuming = runtime.db.lock()
                .ok()
                .and_then(|db| db.has_pending_index_tasks().ok())
                .unwrap_or(false);
            emit(
                &ui_state,
                tui_mode,
                "▶",
                "INDEX",
                if resuming {
                    "Resuming unfinished index queue from previous run".to_string()
                } else {
                    format!("Bootstrapping initial index for {} watch path(s)", cfg.watch_paths.len())
                },
            );
            runtime.spawn_index_directories(
                cfg.watch_paths,
                cfg.device_name.clone(),
                cfg.device_name.clone(),
            );
        }
    }

    let running = Arc::new(AtomicBool::new(true));
    let (cmd_tx, cmd_rx) = mpsc::channel::<String>();
    spawn_command_reader(cmd_tx, Arc::clone(&running));
    let mut last_render = Instant::now();

    if tui_mode {
        if let Ok(s) = ui_state.lock() {
            let cfg = runtime.config.lock().unwrap();
            render_tui(&s, &cfg.device_name, cfg.agent_port);
        }
    }

    // Simple loop to handle events
    while running.load(Ordering::Relaxed) {
        while let Ok(cmd_line) = cmd_rx.try_recv() {
            if let Some(effective_cmd) = resolve_history_alias(&cmd_line, &ui_state, tui_mode) {
                execute_command(&effective_cmd, &runtime, &ui_state, tui_mode, &running);
            }
        }

        // Drain events
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::DevicesLoaded(devices) => {
                    if let Ok(mut s) = ui_state.lock() {
                        s.devices = devices
                            .iter()
                            .filter_map(|d| d.primary_ip().map(|ip| (d.display_name().to_string(), ip.to_string())))
                            .collect();
                    }
                    emit(&ui_state, tui_mode, "✓", "DEVICES", format!("Loaded {} devices", devices.len()));
                }
                AppEvent::DevicesFailed(err) => {
                    emit(&ui_state, tui_mode, "⚠", "DEVICES", format!("Load failed: {}", err));
                }
                AppEvent::SyncRequest { after, requester_device, response_tx } => {
                    emit(&ui_state, tui_mode, "⇄", "SYNC", format!("Incoming sync request (after={})", after));
                    if let Ok(guard) = runtime.db.lock() {
                        match guard.get_sync_delta_after_filtered(after, requester_device.as_deref()) {
                            Ok(delta) => {
                                let _ = response_tx.send(delta);
                            }
                            Err(e) => {
                                log::error!("Failed to query files for sync: {}", e);
                                let _ = response_tx.send(thegrid_core::SyncDelta::default());
                            }
                        }
                    }
                }
                AppEvent::FileSystemChanged { changes, summary } => {
                    emit(&ui_state, tui_mode, "Δ", "WATCHER", format!("{} ({} changes)", summary, changes.len()));
                    runtime.spawn_incremental_index(changes);
                }
                AppEvent::ClipboardReceived(entry) => {
                    let preview: String = entry.content.chars().take(80).collect();
                    emit(&ui_state, tui_mode, "⎘", "CLIPBOARD", format!("from {}: {}", entry.sender, preview));
                    
                    // Attempt to set Termux clipboard if running on Android
                    #[cfg(target_os = "linux")]
                    {
                        if let Err(e) = std::process::Command::new("termux-clipboard-set")
                            .arg(&entry.content)
                            .output() 
                        {
                            log::debug!("Failed to set termux clipboard (might not be Termux): {}", e);
                        }
                    }
                }
                AppEvent::FileReceived { name, size } => {
                    emit(&ui_state, tui_mode, "↓", "TRANSFER", format!("{} ({} bytes)", name, size));
                }
                AppEvent::RemoteAiEmbedRequest { text, response_tx } => {
                    emit(&ui_state, tui_mode, "◇", "AI", format!("Embedding request ({} chars)", text.len()));
                    runtime.handle_remote_ai_embed(text, response_tx);
                }
                AppEvent::RemoteAiSearchRequest { query, k, response_tx } => {
                    emit(&ui_state, tui_mode, "◈", "AI", format!("Search request k={} query='{}'", k, query));
                    runtime.handle_remote_ai_search(query, k, response_tx);
                }
                AppEvent::RefreshAiServices => {
                    emit(&ui_state, tui_mode, "↻", "AI", "Refreshing AI services");
                    runtime.refresh_ai_services();
                }
                AppEvent::EnableAdb { .. } => {
                    emit(&ui_state, tui_mode, "⇢", "ADB", "Enabling ADB over TCP/IP (port 5555)");
                    match std::process::Command::new("adb")
                        .arg("tcpip")
                        .arg("5555")
                        .output() 
                    {
                        Ok(output) => {
                            if output.status.success() {
                                log::info!("ADB daemon restarted on port 5555.");
                            } else {
                                let err = String::from_utf8_lossy(&output.stderr);
                                log::error!("ADB enable failed: {}. Is ADB installed via 'pkg install android-tools'?", err);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to execute 'adb': {}. Ensure android-tools are installed.", e);
                        }
                    }
                }
                AppEvent::EnableRdp { .. } => {
                    log::warn!("EnableRdp received: RDP enablement is not supported on this headless node.");
                }
                AppEvent::AgentPingOk { ip, response, manual } => {
                    if manual {
                        if let Ok(mut s) = ui_state.lock() {
                            s.ping_ok += 1;
                        }
                        emit(&ui_state, tui_mode, "✓", "PING", format!("{} OK (auth={})", ip, response.authorized));
                    }
                }
                AppEvent::AgentPingFailed { ip, error, manual } => {
                    if manual {
                        if let Ok(mut s) = ui_state.lock() {
                            s.ping_fail += 1;
                        }
                        emit(&ui_state, tui_mode, "⚠", "PING", format!("{} failed: {}", ip, error));
                    }
                }
                AppEvent::DuplicatesFound(groups) => {
                    if groups.is_empty() {
                        emit(&ui_state, tui_mode, "✓", "DUPES", "No duplicate files found in index");
                    } else {
                        let total_files: usize = groups.iter().map(|(_, _, f)| f.len()).sum();
                        let wasted: u64 = groups.iter().map(|(_, size, f)| size * (f.len() as u64 - 1)).sum();
                        emit(&ui_state, tui_mode, "!", "DUPES",
                            format!("{} group(s), {} redundant files, ~{} MB wasted",
                                groups.len(), total_files - groups.len(),
                                wasted / 1_048_576));
                        for (hash, size, files) in groups.iter().take(10) {
                            let paths: Vec<String> = files.iter()
                                .map(|f| f.path.file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| f.path.to_string_lossy().to_string()))
                                .collect();
                            emit(&ui_state, tui_mode, "·", "DUPES",
                                format!("[{}] {}B — {}", &hash[..8], size, paths.join(" | ")));
                        }
                        if groups.len() > 10 {
                            emit(&ui_state, tui_mode, "·", "DUPES",
                                format!("{} more group(s) not shown", groups.len() - 10));
                        }
                    }
                }
                AppEvent::SyncHealthUpdated { device_id, metrics } => {
                    let age_str = metrics.sync_age_secs
                        .map(|a| format!("{}s ago", a))
                        .unwrap_or_else(|| "never".to_string());
                    emit(&ui_state, tui_mode, "⇄", "SYNC",
                        format!("{}: age={} tombs={} fails={}",
                            device_id, age_str, metrics.tombstone_count, metrics.sync_failures));
                    if let Ok(mut s) = ui_state.lock() {
                        s.sync_health.insert(device_id, metrics);
                    }
                }
                AppEvent::IndexProgress { scanned, total, current, .. } => {
                    if total > 0 {
                        emit(&ui_state, tui_mode, "◷", "INDEX", format!("{}/{} [{}]", scanned, total, current));
                    }
                }
                AppEvent::IndexComplete { files_added, duration_ms, .. } => {
                    emit(&ui_state, tui_mode, "✓", "INDEX", format!("Complete: {} files in {} ms", files_added, duration_ms));
                }
                AppEvent::Status(msg) => {
                    if msg.starts_with("db_error:") {
                        emit(&ui_state, tui_mode, "⚠", "DB", &msg["db_error:".len()..]);
                    } else if msg.starts_with("device_ping_ok:") {
                        let parts: Vec<&str> = msg.splitn(3, ':').collect();
                        if parts.len() == 3 {
                            if parts[2] == "true" {
                                emit(&ui_state, tui_mode, "✓", "DEVICE", format!("{} reachable", parts[1]));
                            }
                        }
                    } else if msg.starts_with("device_ping_fail:") {
                        let parts: Vec<&str> = msg.splitn(4, ':').collect();
                        if parts.len() == 4 {
                            if parts[2] == "true" {
                                emit(&ui_state, tui_mode, "⚠", "DEVICE", format!("{} unreachable: {}", parts[1], parts[3]));
                            }
                        }
                    } else if msg.starts_with("config_update:") {
                        let parts = &msg["config_update:".len()..];
                        let mut model = None;
                        let mut url = None;
                        for part in parts.split(',') {
                            if part.starts_with("model=") {
                                let val = part["model=".len()..].trim_matches('"').to_string();
                                if val != "None" && !val.is_empty() { model = Some(val); }
                            } else if part.starts_with("url=") {
                                let val = part["url=".len()..].trim_matches('"').to_string();
                                if val != "None" && !val.is_empty() { url = Some(val); }
                            }
                        }

                        let mut cfg = { runtime.config.lock().unwrap().clone() };
                        let mut changed = false;
                        if model.is_some() { cfg.ai_model = model; changed = true; }
                        if url.is_some() { cfg.ai_provider_url = url; changed = true; }
                        
                        if changed {
                            emit(&ui_state, tui_mode, "⚙", "CONFIG", "Received remote config update");
                            let _ = cfg.save();
                            {
                                let mut runtime_cfg = runtime.config.lock().unwrap();
                                *runtime_cfg = cfg;
                            }
                            runtime.refresh_ai_services();
                        }
                    } else if msg.starts_with("index_count:") {
                        log::debug!("Remote index count update: {}", msg);
                    } else {
                        log::debug!("Status: {}", msg);
                    }
                }
                _ => {
                    // Ignore GUI-only events
                    log::trace!("Ignored event: {:?}", event);
                }
            }
        }

        if tui_mode {
            let force_heartbeat = last_render.elapsed() >= Duration::from_secs(20);
            let mut should_render = force_heartbeat;
            if let Ok(s) = ui_state.lock() {
                should_render = should_render || s.dirty;
            }

            if should_render {
                if let Ok(mut s) = ui_state.lock() {
                    let cfg = runtime.config.lock().unwrap();
                    render_tui(&s, &cfg.device_name, cfg.agent_port);
                    s.dirty = false;
                }
                last_render = Instant::now();
            }
        }

        std::thread::sleep(Duration::from_millis(140));
    }

    Ok(())
}
