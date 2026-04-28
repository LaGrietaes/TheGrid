use anyhow::Result;
use chrono::Local;
use semver::Version;
use serde::Deserialize;
use sysinfo::System;

use std::collections::VecDeque;
use std::io::{self, IsTerminal, Write};
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use thegrid_core::{AppEvent, Config, models::SyncHealthMetrics};
use thegrid_runtime::AppRuntime;

// ── ratatui / crossterm ───────────────────────────────────────────────────────
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line as TuiLine, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
};

const RELEASES_LATEST_URL: &str = "https://api.github.com/repos/LaGrietaes/TheGrid/releases/latest";
const SIGNATURE_LINE: &str = "> Powered and Designed by: sinergias.lagrieta.es";
const LAST_UPDATE_ENV: &str = "THEGRID_LAST_UPDATE";

const ANSI_RESET: &str = "\x1B[0m";
const ANSI_BOLD: &str = "\x1B[1m";
const ANSI_DIM: &str = "\x1B[2m";
const ANSI_GREEN: &str = "\x1B[32m";         // border / banner accent
const ANSI_GREEN_BRIGHT: &str = "\x1B[92m";  // live-node indicator (◉ LIVE)
const ANSI_WHITE: &str = "\x1B[37m";

// S2-C1: command registry metadata used by help output and TUI hints.
const COMMAND_REGISTRY: &[(&str, &str)] = &[
    ("help", "Show command list"),
    ("devices", "Refresh connected device list"),
    ("ping <ip|#|name>", "Ping device + agent"),
    ("pingdev <ip|#|name>", "Ping device endpoint"),
    ("pingagent <ip|#|name>", "Ping agent endpoint"),
    ("mesh [status]", "Sync health overview"),
    ("mesh sync <ip|#|name>", "Trigger sync to device"),
    ("scan [path]", "Index a path (or all watch_paths)"),
    ("storage", "Storage stats: files indexed, size, dupes"),
    ("dupes", "Scan and report duplicate files"),
    ("history | !! | !N", "Command history and replay"),
    ("update", "Check latest release"),
    ("gitupdate", "Fetch, pull, build, restart"),
    ("quit", "Stop node"),
];

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
    Scan,
    Storage,
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
        "scan" | "index" => ParsedCommand::Scan,
        "storage" | "store" => ParsedCommand::Storage,
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

fn try_rebuild_binaries() -> Result<String> {
    let build = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("thegrid-node")
        .arg("-p")
        .arg("thegrid-gui")
        .output()?;

    if !build.status.success() {
        let stderr = String::from_utf8_lossy(&build.stderr);
        anyhow::bail!("cargo build failed: {}", stderr.trim());
    }

    Ok("Build completed for thegrid-node + thegrid-gui".to_string())
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
    Local::now().format("%d/%m/%y %H:%M:%S").to_string()
}

fn print_banner(device_name: &str, port: u16) {
    // ASCII art derived from The Grid icon (hexagonal mesh / circuit motif)
    println!("{}{}{}", ANSI_GREEN, ANSI_BOLD, ANSI_RESET);
    println!("{}{}  ╔══╗  ┌─────────────────────────────────────────────────╗{}", ANSI_GREEN, ANSI_BOLD, ANSI_RESET);
    println!("{}{}  ║  ║  │  ████████╗██╗  ██╗███████╗                     │{}", ANSI_GREEN, ANSI_BOLD, ANSI_RESET);
    println!("{}{}  ╠══╣  │     ██╔══╝██║  ██║██╔════╝                     │{}", ANSI_GREEN, ANSI_BOLD, ANSI_RESET);
    println!("{}{}  ║  ╠══╣     ██║   ███████║█████╗   GRID                 │{}", ANSI_GREEN, ANSI_BOLD, ANSI_RESET);
    println!("{}{}  ╠══╣  │     ██║   ██╔══██║██╔══╝                       │{}", ANSI_GREEN, ANSI_BOLD, ANSI_RESET);
    println!("{}{}  ║  ║  │     ██║   ██║  ██║███████╗                     │{}", ANSI_GREEN, ANSI_BOLD, ANSI_RESET);
    println!("{}{}  ╚══╝  │     ╚═╝   ╚═╝  ╚═╝╚══════╝  HEADLESS NODE      │{}", ANSI_GREEN, ANSI_BOLD, ANSI_RESET);
    println!("{}{}        └─────────────────────────────────────────────────┘{}", ANSI_GREEN, ANSI_BOLD, ANSI_RESET);
    println!();
    let vb = "\u{2551}"; // ║
    println!("{}\u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2557}{}", ANSI_GREEN, ANSI_RESET);
    println!("{}{} {} THE GRID HEADLESS NODE v{:<35} {}{}{}", ANSI_GREEN, vb, ANSI_BOLD, env!("CARGO_PKG_VERSION"), ANSI_RESET, vb, ANSI_RESET);
    println!("{}{} {} {:<61} {}{}{}", ANSI_GREEN, vb, ANSI_DIM, SIGNATURE_LINE, ANSI_RESET, vb, ANSI_RESET);
    println!("{}\u{2560}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2563}{}", ANSI_GREEN, ANSI_RESET);
    println!("{}{} {} Device:     {}{:<51}{}{}{}", ANSI_GREEN, vb, ANSI_RESET, ANSI_GREEN_BRIGHT, device_name, ANSI_RESET, vb, ANSI_RESET);
    println!("{}{} {} Agent Port: {}{:<51}{}{}{}", ANSI_GREEN, vb, ANSI_RESET, ANSI_WHITE, port, ANSI_RESET, vb, ANSI_RESET);
    println!("{}\u{255A}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255D}{}", ANSI_GREEN, ANSI_RESET);
}

fn event_line(icon: &str, label: &str, message: impl AsRef<str>) {
    println!("{} {} {:<12} {}", ts(), icon, label, message.as_ref());
}

// ── ratatui terminal lifecycle ───────────────────────────────────────────────
fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
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
    sync_health: std::collections::HashMap<String, SyncHealthMetrics>,
    /// Last known ping result per IP: true = ok, false = failed, absent = untested.
    node_status: std::collections::HashMap<String, bool>,
    dirty: bool,
    // ── system metrics (sysinfo) ──────────────────────────────────────────────
    cpu_pct: f32,
    ram_used_mb: u64,
    ram_total_mb: u64,
    // ── ratatui input & scroll state ─────────────────────────────────────────
    /// Current text being typed in the input bar.
    input_buf: String,
    /// Lines scrolled back from tail (0 = auto-scroll to newest).
    log_scroll: usize,
    /// Index into command_history for Up/Down navigation (None = not navigating).
    history_cursor: Option<usize>,
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
            node_status: std::collections::HashMap::new(),
            dirty: true,
            cpu_pct: 0.0,
            ram_used_mb: 0,
            ram_total_mb: 0,
            input_buf: String::new(),
            log_scroll: 0,
            history_cursor: None,
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

// ── ratatui draw helpers ─────────────────────────────────────────────────────

fn log_style(line: &str) -> Style {
    if line.contains(" \u{2717} ") {
        Style::default().fg(Color::Red)
    } else if line.contains(" \u{26A0} ") || line.contains(" ! ") {
        Style::default().fg(Color::Yellow)
    } else if line.contains(" \u{21C4} ") || line.contains(" \u{0394} ") {
        Style::default().fg(Color::Green)
    } else if line.contains(" \u{00B7} ") || line.contains(" \u{2139} ") {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    }
}

fn status_style_for(status: &str) -> Style {
    let s = status.to_ascii_lowercase();
    if s.contains("fail") || s.contains("error") || s.contains("refused")
        || s.contains("timeout") || s.contains("unreachable")
    {
        Style::default().fg(Color::Red)
    } else if s.contains("warn") || s.contains("mismatch") || s.contains("retry") {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Green)
    }
}

fn pulse_char(elapsed: Duration) -> &'static str {
    match (elapsed.as_millis() / 300) % 4 {
        0 => "\u{25F4}",
        1 => "\u{25F7}",
        2 => "\u{25F6}",
        _ => "\u{25F5}",
    }
}

fn draw_left_panel(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &TuiState,
    device_name: &str,
    port: u16,
) {
    let elapsed = state.started_at.elapsed();
    let uptime = elapsed.as_secs();
    let uptime_str = if uptime >= 3600 {
        format!("{}h {:02}m {:02}s", uptime / 3600, (uptime % 3600) / 60, uptime % 60)
    } else if uptime >= 60 {
        format!("{}m {:02}s", uptime / 60, uptime % 60)
    } else {
        format!("{}s", uptime)
    };
    let pulse = pulse_char(elapsed);

    let dim = Style::default().fg(Color::DarkGray);
    let cyan = Style::default().fg(Color::Green);
    let white = Style::default().fg(Color::White);
    let green_bold = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
    let cyan_bold = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);

    // Grid logo using unicode escapes to avoid encoding issues
    let top_row    = "\u{2554}\u{2550}\u{2550}\u{2550}\u{256A}\u{2550}\u{2550}\u{2550}\u{256A}\u{2550}\u{2550}\u{2550}\u{2557}";
    let mid_row    = "\u{2551} \u{25C8} \u{2551} \u{25C8} \u{2551} \u{25C8} \u{2551}";
    let div_row    = "\u{2560}\u{2550}\u{2550}\u{2550}\u{256C}\u{2550}\u{2550}\u{2550}\u{256C}\u{2550}\u{2550}\u{2550}\u{2563}";
    let bot_row    = "\u{255A}\u{2550}\u{2550}\u{2550}\u{2569}\u{2550}\u{2550}\u{2550}\u{2569}\u{2550}\u{2550}\u{2550}\u{255D}";

    let lines: Vec<TuiLine> = vec![
        TuiLine::from(vec![
            Span::styled(format!("{} ", pulse), green_bold),
            Span::styled(top_row, cyan),
        ]),
        TuiLine::from(Span::styled(format!("   {}", mid_row), cyan)),
        TuiLine::from(Span::styled(format!("   {}", div_row), cyan)),
        TuiLine::from(Span::styled(format!("   {}", mid_row), cyan)),
        TuiLine::from(Span::styled(format!("   {}", bot_row), cyan)),
        TuiLine::from(Span::styled("   T H E   G R I D", cyan_bold)),
        TuiLine::from(""),
        TuiLine::from(vec![
            Span::styled("  v", dim),
            Span::styled(env!("CARGO_PKG_VERSION"), white.add_modifier(Modifier::BOLD)),
            Span::styled("  NODE", dim),
        ]),
        TuiLine::from(vec![
            Span::styled("  Device  ", dim),
            Span::styled(device_name.to_string(), green_bold),
        ]),
        TuiLine::from(vec![
            Span::styled("  Port    ", dim),
            Span::styled(format!("{}", port), white),
        ]),
        TuiLine::from(vec![
            Span::styled("  Uptime  ", dim),
            Span::styled(uptime_str, white),
        ]),
        TuiLine::from(vec![
            Span::styled("  Ping    ", dim),
            Span::styled(format!("{}", state.ping_ok), Style::default().fg(Color::Green)),
            Span::styled("/", dim),
            Span::styled(format!("{}", state.ping_fail), Style::default().fg(Color::Red)),
            Span::styled(" ok/fail", dim),
        ]),
        TuiLine::from(vec![
            Span::styled("  Status  ", dim),
            Span::styled(state.last_status.clone(), status_style_for(&state.last_status)),
        ]),
        TuiLine::from(vec![
            Span::styled("  ", dim),
            Span::styled("sinergias.lagrieta.es", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(cyan)
                .title(Span::styled(" NODE ", cyan_bold)),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(para, area);
}

fn draw_commands_panel(frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let cyan = Style::default().fg(Color::Green);
    let cyan_bold = cyan.add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let white = Style::default().fg(Color::White);

    let mut lines: Vec<TuiLine> = Vec::new();
    for (usage, desc) in COMMAND_REGISTRY {
        lines.push(TuiLine::from(Span::styled(format!("  {}", usage), white)));
        lines.push(TuiLine::from(Span::styled(format!("    {}", desc), dim)));
    }

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(cyan)
                .title(Span::styled(" COMMANDS ", cyan_bold)),
        );
    frame.render_widget(para, area);
}

fn draw_mesh_devices_panel(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &TuiState,
) {
    let cyan = Style::default().fg(Color::Green);
    let cyan_bold = cyan.add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let green_bright = Style::default().fg(Color::LightGreen);
    let red = Style::default().fg(Color::Red);

    // Collect mesh entries keyed by device id
    let mut entries: Vec<(&String, &SyncHealthMetrics)> = state.sync_health.iter().collect();
    entries.sort_by_key(|(id, _)| id.as_str());
    let mesh_ids: std::collections::HashSet<&str> =
        entries.iter().map(|(id, _)| id.as_str()).collect();

    let mut rows: Vec<Row> = Vec::new();

    // --- Devices WITH mesh health ---
    for (id, h) in entries.iter().take(8) {
        let age = h.sync_age_secs
            .map(|s| format!("{}s", s))
            .unwrap_or_else(|| "\u{2014}".to_string());
        let is_live = h.sync_age_secs.map(|s| s < 120).unwrap_or(false);
        let (dot, ping_style) = match state.node_status.get(*id) {
            Some(true) => ("\u{25C9}", green_bright),
            Some(false) => ("\u{25CC}", red),
            None => if is_live { ("\u{25C9}", green_bright) } else { ("\u{25C8}", dim) },
        };
        let state_str = if is_live { "LIVE " } else { "STALE" };
        let state_style = if is_live { green_bright } else { dim };
        rows.push(Row::new(vec![
            Cell::from(Span::styled(format!(" {} ", dot), ping_style)),
            Cell::from(Span::styled(id.as_str().to_string(), Style::default().fg(Color::Green))),
            Cell::from(Span::styled(format!("age {}", age), dim)),
            Cell::from(Span::styled(format!("tombs {:>3}", h.tombstone_count), dim)),
            Cell::from(Span::styled(format!("fail {:>2}", h.sync_failures), dim)),
            Cell::from(Span::styled(state_str.to_string(), state_style)),
        ]));
    }

    // --- Connected devices NOT yet in mesh ---
    for (name, ip) in state.devices.iter().take(8) {
        if mesh_ids.contains(name.as_str()) {
            continue;
        }
        let (dot, dot_style) = match state.node_status.get(ip.as_str()) {
            Some(true) => ("\u{25C9}", green_bright),
            Some(false) => ("\u{25CC}", red),
            None => ("\u{25C8}", dim),
        };
        rows.push(Row::new(vec![
            Cell::from(Span::styled(format!(" {} ", dot), dot_style)),
            Cell::from(Span::styled(name.clone(), dim)),
            Cell::from(Span::styled(format!("({})", ip), dim)),
            Cell::from(Span::styled("pending sync", dim)),
            Cell::from(""),
            Cell::from(""),
        ]));
    }

    if rows.is_empty() {
        rows.push(Row::new(vec![
            Cell::from(Span::styled("  \u{25C8}", dim)),
            Cell::from(Span::styled("none — run: devices", dim)),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
        ]));
    }

    let widths = [
        Constraint::Length(4),
        Constraint::Fill(1),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(9),
        Constraint::Length(6),
    ];

    let table = Table::new(rows, widths)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(cyan)
                .title(Span::styled(" MESH & DEVICES ", cyan_bold)),
        )
        .header(
            Row::new(vec![
                Cell::from(""),
                Cell::from(Span::styled("Device", dim)),
                Cell::from(Span::styled("Age", dim)),
                Cell::from(Span::styled("Tombstones", dim)),
                Cell::from(Span::styled("Failures", dim)),
                Cell::from(Span::styled("State", dim)),
            ])
            .style(dim),
        );
    frame.render_widget(table, area);
}

fn draw_log_panel(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &TuiState,
) {
    let cyan = Style::default().fg(Color::Green);
    let cyan_bold = cyan.add_modifier(Modifier::BOLD);

    let lines: Vec<TuiLine> = state
        .recent_logs
        .iter()
        .map(|line| TuiLine::from(Span::styled(line.clone(), log_style(line))))
        .collect();

    let total = lines.len() as u16;
    let visible = area.height.saturating_sub(2);
    let scroll_from_top = total
        .saturating_sub(visible)
        .saturating_sub(state.log_scroll as u16);

    let title = if state.log_scroll > 0 {
        format!(" LOG  \u{2191} {}L scrolled  PgDn=tail ", state.log_scroll)
    } else {
        " LOG ".to_string()
    };

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(cyan)
                .title(Span::styled(title, cyan_bold)),
        )
        .scroll((scroll_from_top, 0));
    frame.render_widget(para, area);
}

fn draw_input_bar(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &TuiState,
) {
    let cyan = Style::default().fg(Color::Green);
    let cyan_bold = cyan.add_modifier(Modifier::BOLD);
    let green_bold = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);

    let prompt_line = TuiLine::from(vec![
        Span::styled("> ", green_bold),
        Span::styled(state.input_buf.clone(), Style::default().fg(Color::White)),
    ]);

    let para = Paragraph::new(vec![prompt_line]).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(cyan)
            .title(Span::styled(" command ", cyan_bold)),
    );
    frame.render_widget(para, area);

    // Place cursor after typed text (inside block: +1 border, +2 for "> ")
    let cursor_x = area.x + 1 + 2 + state.input_buf.len() as u16;
    let cursor_y = area.y + 1;
    if cursor_x < area.x + area.width.saturating_sub(1) {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn draw_sysinfo_bar(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &TuiState) {
    let cyan = Style::default().fg(Color::Green);
    let cyan_bold = cyan.add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let white = Style::default().fg(Color::White);
    let bar_width = (area.width.saturating_sub(4)) as usize;

    fn filled_bar(pct: f32, width: usize) -> String {
        let filled = ((pct / 100.0) * width as f32).round() as usize;
        let filled = filled.min(width);
        format!("{}{}", "\u{2588}".repeat(filled), "\u{2591}".repeat(width - filled))
    }

    let ram_pct = if state.ram_total_mb > 0 {
        state.ram_used_mb as f32 / state.ram_total_mb as f32 * 100.0
    } else {
        0.0
    };

    let cpu_bar = filled_bar(state.cpu_pct, (bar_width / 2).saturating_sub(12));
    let ram_bar = filled_bar(ram_pct, (bar_width / 2).saturating_sub(12));

    let line = TuiLine::from(vec![
        Span::styled("  CPU ", dim),
        Span::styled(format!("{:5.1}% ", state.cpu_pct), white),
        Span::styled(cpu_bar, Style::default().fg(Color::Cyan)),
        Span::styled("   RAM ", dim),
        Span::styled(format!("{:5.1}% ", ram_pct), white),
        Span::styled(ram_bar, Style::default().fg(Color::Blue)),
        Span::styled(format!("  {}/{}MB", state.ram_used_mb, state.ram_total_mb), dim),
    ]);

    let para = Paragraph::new(vec![line]).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(cyan)
            .title(Span::styled(" SYSTEM ", cyan_bold)),
    );
    frame.render_widget(para, area);
}

fn draw_tui(frame: &mut ratatui::Frame, state: &TuiState, device_name: &str, port: u16) {
    let area = frame.area();
    let has_mesh = !state.sync_health.is_empty() || !state.devices.is_empty();
    let mesh_h = if has_mesh {
        (state.sync_health.len().max(state.devices.len()).min(6) + 3) as u16
    } else {
        0
    };

    // Outer vertical chunks: sysinfo | top panel | mesh+devices | log | input
    let outer = Layout::vertical([
        Constraint::Length(3),   // CPU/RAM bar
        Constraint::Fill(1),     // top: left info + commands
        Constraint::Length(mesh_h), // merged mesh & devices panel
        Constraint::Length(13),  // log
        Constraint::Length(3),   // input bar
    ])
    .split(area);

    // Top: left info | commands
    let top = Layout::horizontal([
        Constraint::Percentage(32),
        Constraint::Fill(1),
    ])
    .split(outer[1]);

    draw_sysinfo_bar(frame, outer[0], state);
    draw_left_panel(frame, top[0], state, device_name, port);
    draw_commands_panel(frame, top[1]);
    if has_mesh {
        draw_mesh_devices_panel(frame, outer[2], state);
    }
    draw_log_panel(frame, outer[3], state);
    draw_input_bar(frame, outer[4], state);
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

/// Resolves a command target (<ip>, <#index>, or <device name>) to (label, ip).
/// Name matching: exact first, then case-insensitive substring; falls back to treating as IP.
fn resolve_ping_target(target: &str, ui_state: &Arc<Mutex<TuiState>>) -> Option<(String, String)> {
    // Numeric index
    if let Ok(idx) = target.parse::<usize>() {
        let guard = ui_state.lock().ok()?;
        if idx == 0 || idx > guard.devices.len() {
            return None;
        }
        let (name, ip) = guard.devices[idx - 1].clone();
        return Some((format!("#{} {}", idx, name), ip));
    }

    // Name match (exact then substring, case-insensitive)
    if let Ok(guard) = ui_state.lock() {
        let lower = target.to_ascii_lowercase();
        // Exact name match
        for (i, (name, ip)) in guard.devices.iter().enumerate() {
            if name.to_ascii_lowercase() == lower {
                return Some((format!("#{} {}", i + 1, name), ip.clone()));
            }
        }
        // Substring match
        for (i, (name, ip)) in guard.devices.iter().enumerate() {
            if name.to_ascii_lowercase().contains(&lower) {
                return Some((format!("#{} {}", i + 1, name), ip.clone()));
            }
        }
    }

    // Fallback: treat as raw IP
    Some((target.to_string(), target.to_string()))
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
        ParsedCommand::Scan => {
            let explicit_path = parts.get(1).map(|p| std::path::PathBuf::from(p));
            let cfg = runtime.config.lock().unwrap().clone();
            let device_id   = cfg.device_name.clone();
            let device_name = cfg.device_name.clone();
            if let Some(path) = explicit_path {
                emit(ui_state, tui_mode, "▶", "INDEX", format!("Scanning {}", path.display()));
                runtime.spawn_index_directory(path, device_id, device_name);
            } else if cfg.watch_paths.is_empty() {
                emit(ui_state, tui_mode, "⚠", "INDEX", "No watch_paths configured. Use: scan <path>");
            } else {
                emit(ui_state, tui_mode, "▶", "INDEX",
                    format!("Scanning {} watch path(s)…", cfg.watch_paths.len()));
                runtime.spawn_index_directories(cfg.watch_paths, device_id, device_name);
            }
        }
        ParsedCommand::Storage => {
            let cfg = runtime.config.lock().unwrap().clone();
            emit(ui_state, tui_mode, "ℹ", "STORE",
                format!("Watch paths: {}", if cfg.watch_paths.is_empty() { "none".to_string() }
                    else { cfg.watch_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ") }));
            if let Ok(db) = runtime.db.lock() {
                match db.get_storage_stats() {
                    Ok((files, size_bytes, devices)) => {
                        let size_mb = size_bytes / 1_048_576;
                        emit(ui_state, tui_mode, "ℹ", "STORE",
                            format!("{} files indexed  |  {} MB total  |  {} device(s)", files, size_mb, devices));
                    }
                    Err(e) => emit(ui_state, tui_mode, "⚠", "STORE", format!("DB query failed: {}", e)),
                }
                match db.count_files_needing_hash() {
                    Ok(n) if n > 0 => emit(ui_state, tui_mode, "ℹ", "STORE",
                        format!("{} file(s) still need hashing — run 'dupes' after", n)),
                    _ => {}
                }
            }
            emit(ui_state, tui_mode, "ℹ", "STORE", "Run 'scan' to index, 'dupes' to find duplicates");
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
                    emit(ui_state, tui_mode, "↻", "BUILD", "Rebuilding thegrid-node + thegrid-gui...");
                    match try_rebuild_binaries() {
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
            "--width" => {
                if i + 1 < args.len() {
                    if let Ok(w) = args[i + 1].parse::<usize>() {
                        if w >= 40 {
                            std::env::set_var("THEGRID_WIDTH", w.to_string());
                        }
                    }
                    i += 1;
                }
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

    // Startup config validation — catch common misconfigurations early.
    if config.api_key.is_empty() {
        emit(&ui_state, tui_mode, "⚠", "CONFIG",
            "api_key is empty — device discovery and Tailscale trust will fail. Set it in config.json");
    } else if !config.api_key.starts_with("tskey-") {
        emit(&ui_state, tui_mode, "⚠", "CONFIG",
            format!("api_key prefix unexpected (got: {}) — verify config.json",
                &config.api_key[..config.api_key.len().min(12)]));
    }
    if config.agent_port == 0 {
        emit(&ui_state, tui_mode, "⚠", "CONFIG",
            "agent_port is 0 — agent will pick a random port. Set it explicitly in config.json");
    }

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
                            emit(&ui_state, tui_mode, "↻", "BUILD", "Rebuilding thegrid-node + thegrid-gui...");
                            match try_rebuild_binaries() {
                                Ok(build_msg) => {
                                    emit(&ui_state, tui_mode, "✓", "BUILD", build_msg);
                                    emit(&ui_state, tui_mode, "↻", "RESTART", "Launching updated node process...");
                                    match restart_current_node_process(Some((&from, &to))) {
                                        Ok(msg) => {
                                            emit(&ui_state, tui_mode, "✓", "RESTART", msg);
                                            return Ok(());
                                        }
                                        Err(e) => {
                                            emit(&ui_state, tui_mode, "⚠", "RESTART", format!("> Update Failed Check logs: {}", e));
                                            emit(&ui_state, tui_mode, "ℹ", "UPDATE", "Continuing with current process.");
                                        }
                                    }
                                }
                                Err(e) => {
                                    emit(&ui_state, tui_mode, "⚠", "BUILD", format!("> Update Failed Check logs: {}", e));
                                    emit(&ui_state, tui_mode, "ℹ", "UPDATE", "Continuing with current process.");
                                }
                            }
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

    // Auto-discover peers on startup without requiring the user to type 'devices'.
    runtime.spawn_load_devices();

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

    // ── TUI mode: set up ratatui terminal; plain: start stdin reader ──────────
    let mut tui_terminal: Option<Terminal<CrosstermBackend<io::Stdout>>> = None;
    let plain_cmd_rx: Option<mpsc::Receiver<String>>;
    if tui_mode {
        // Restore terminal on panic so the shell is not left broken.
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
            original_hook(info);
        }));
        tui_terminal = Some(setup_terminal()?);
        plain_cmd_rx = None;
    } else {
        let (cmd_tx, rx) = mpsc::channel::<String>();
        spawn_command_reader(cmd_tx, Arc::clone(&running));
        plain_cmd_rx = Some(rx);
    }

    let mut last_render = Instant::now();

    // ── sysinfo: background thread updates CPU/RAM every ~2 seconds ──────────
    {
        let ui_state_sys = Arc::clone(&ui_state);
        let running_sys = Arc::clone(&running);
        std::thread::spawn(move || {
            let mut sys = System::new_all();
            // First refresh to seed CPU baseline (sysinfo needs two reads for CPU %)
            sys.refresh_cpu_all();
            std::thread::sleep(Duration::from_millis(500));
            while running_sys.load(Ordering::Relaxed) {
                sys.refresh_cpu_all();
                sys.refresh_memory();
                let cpu: f32 = {
                    let cpus = sys.cpus();
                    if cpus.is_empty() { 0.0 } else {
                        cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / cpus.len() as f32
                    }
                };
                let ram_used = sys.used_memory() / 1_048_576;
                let ram_total = sys.total_memory() / 1_048_576;
                if let Ok(mut s) = ui_state_sys.lock() {
                    s.cpu_pct = cpu;
                    s.ram_used_mb = ram_used;
                    s.ram_total_mb = ram_total;
                    s.dirty = true;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    }

    // Main event loop
    while running.load(Ordering::Relaxed) {
        // ── TUI: handle crossterm keyboard events ─────────────────────────────
        if tui_mode {
            while event::poll(Duration::ZERO).unwrap_or(false) {
                match event::read() {
                    Ok(Event::Key(key)) => match key.code {
                        KeyCode::Enter => {
                            let cmd = {
                                let mut s = ui_state.lock().unwrap();
                                let c = s.input_buf.trim().to_string();
                                s.input_buf.clear();
                                s.history_cursor = None;
                                s.dirty = true;
                                c
                            };
                            if !cmd.is_empty() {
                                if let Some(eff) = resolve_history_alias(&cmd, &ui_state, tui_mode) {
                                    execute_command(&eff, &runtime, &ui_state, tui_mode, &running);
                                }
                            }
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            running.store(false, Ordering::Relaxed);
                        }
                        KeyCode::Char(ch) => {
                            if let Ok(mut s) = ui_state.lock() {
                                s.input_buf.push(ch);
                                s.dirty = true;
                            }
                        }
                        KeyCode::Backspace => {
                            if let Ok(mut s) = ui_state.lock() {
                                s.input_buf.pop();
                                s.dirty = true;
                            }
                        }
                        KeyCode::Delete => {
                            if let Ok(mut s) = ui_state.lock() {
                                s.input_buf.clear();
                                s.dirty = true;
                            }
                        }
                        KeyCode::Up => {
                            if let Ok(mut s) = ui_state.lock() {
                                let hlen = s.command_history.len();
                                if hlen > 0 {
                                    let idx = match s.history_cursor {
                                        None => hlen - 1,
                                        Some(0) => 0,
                                        Some(i) => i - 1,
                                    };
                                    s.history_cursor = Some(idx);
                                    s.input_buf = s.command_history[idx].clone();
                                    s.dirty = true;
                                }
                            }
                        }
                        KeyCode::Down => {
                            if let Ok(mut s) = ui_state.lock() {
                                let hlen = s.command_history.len();
                                match s.history_cursor {
                                    None => {}
                                    Some(i) if i + 1 >= hlen => {
                                        s.history_cursor = None;
                                        s.input_buf.clear();
                                        s.dirty = true;
                                    }
                                    Some(i) => {
                                        s.history_cursor = Some(i + 1);
                                        s.input_buf = s.command_history[i + 1].clone();
                                        s.dirty = true;
                                    }
                                }
                            }
                        }
                        KeyCode::PageUp => {
                            if let Ok(mut s) = ui_state.lock() {
                                s.log_scroll = s.log_scroll.saturating_add(8);
                                s.dirty = true;
                            }
                        }
                        KeyCode::PageDown => {
                            if let Ok(mut s) = ui_state.lock() {
                                s.log_scroll = s.log_scroll.saturating_sub(8);
                                s.dirty = true;
                            }
                        }
                        _ => {}
                    },
                    Ok(Event::Resize(_, _)) => {
                        if let Ok(mut s) = ui_state.lock() {
                            s.dirty = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        // ── Plain mode: drain stdin reader thread ─────────────────────────────
        if let Some(ref rx) = plain_cmd_rx {
            while let Ok(cmd_line) = rx.try_recv() {
                if let Some(effective_cmd) = resolve_history_alias(&cmd_line, &ui_state, tui_mode) {
                    execute_command(&effective_cmd, &runtime, &ui_state, tui_mode, &running);
                }
            }
        }

        // Drain events
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::DevicesLoaded(devices) => {
                    let peer_ips: Vec<String>;
                    let mut no_ip: Vec<String> = Vec::new();
                    {
                        let mut s = ui_state.lock().unwrap();
                        s.devices = devices
                            .iter()
                            .filter_map(|d| {
                                if let Some(ip) = d.primary_ip() {
                                    // Use only the short hostname before the first '.'
                                    let full = d.display_name();
                                    let short = full.split('.').next().unwrap_or(full);
                                    Some((short.to_string(), ip.to_string()))
                                } else {
                                    let full = d.display_name();
                                    let short = full.split('.').next().unwrap_or(full);
                                    no_ip.push(short.to_string());
                                    None
                                }
                            })
                            .collect();
                        peer_ips = s.devices.iter().map(|(_, ip)| ip.clone()).collect();
                    }
                    emit(&ui_state, tui_mode, "✓", "DEVICES",
                        format!("Loaded {} device(s) from tailnet", devices.len()));
                    // Warn about any device that has no routable IP (e.g., never connected to Tailscale).
                    for name in &no_ip {
                        emit(&ui_state, tui_mode, "⚠", "DEVICES",
                            format!("{}: no Tailscale IP — device may not be connected to tailnet", name));
                    }
                    // Auto-ping all discovered peers (silent — only updates node_status dot).
                    for ip in peer_ips {
                        runtime.spawn_ping(ip, false);
                    }
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
                    if let Ok(mut s) = ui_state.lock() {
                        s.node_status.insert(ip.clone(), true);
                        if manual { s.ping_ok += 1; }
                    }
                    if manual {
                        emit(&ui_state, tui_mode, "✓", "PING", format!("{} OK (auth={})", ip, response.authorized));
                    }
                }
                AppEvent::AgentPingFailed { ip, error, manual } => {
                    if let Ok(mut s) = ui_state.lock() {
                        s.node_status.insert(ip.clone(), false);
                        if manual { s.ping_fail += 1; }
                    }
                    if manual {
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
                    if let Ok(mut s) = ui_state.lock() {
                        s.sync_health.insert(device_id, metrics);
                        s.dirty = true;
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
                    if msg.starts_with("restart_requested:") {
                        emit(&ui_state, tui_mode, "↻", "RESTART", "Remote config saved — restarting node…");
                        match restart_current_node_process(None) {
                            Ok(_) => running.store(false, Ordering::Relaxed),
                            Err(e) => emit(&ui_state, tui_mode, "⚠", "RESTART", format!("Restart failed: {}", e)),
                        }
                    } else if msg.starts_with("db_error:") {
                        emit(&ui_state, tui_mode, "⚠", "DB", &msg["db_error:".len()..]);
                    } else if msg.starts_with("agent_start_failed:") {
                        let parts: Vec<&str> = msg.splitn(3, ':').collect();
                        if parts.len() == 3 {
                            emit(&ui_state, tui_mode, "✗", "AGENT", format!("Port {} failed: {}", parts[1], parts[2]));
                        } else {
                            emit(&ui_state, tui_mode, "✗", "AGENT", "Startup failed");
                        }
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

        // ── ratatui draw (TUI mode only) ──────────────────────────────────────
        if let Some(ref mut terminal) = tui_terminal {
            let should_render = {
                let s = ui_state.lock().unwrap();
                last_render.elapsed() >= Duration::from_millis(500) || s.dirty
            };
            if should_render {
                let s = ui_state.lock().unwrap();
                let cfg = runtime.config.lock().unwrap();
                let device_name = cfg.device_name.clone();
                let port = cfg.agent_port;
                drop(cfg);
                terminal.draw(|frame| draw_tui(frame, &s, &device_name, port))?;
                drop(s);
                if let Ok(mut s) = ui_state.lock() {
                    s.dirty = false;
                }
                last_render = Instant::now();
            }
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    // ── Restore terminal before exit ──────────────────────────────────────────
    if let Some(ref mut terminal) = tui_terminal {
        restore_terminal(terminal)?;
    }

    Ok(())
}
