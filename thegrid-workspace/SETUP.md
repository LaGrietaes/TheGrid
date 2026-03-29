# ⬡ THE GRID — Setup Guide

## 1. Install Rust

Open PowerShell as Administrator and run:
```powershell
winget install Rustlang.Rustup
```

Or download the installer from: https://rustup.rs/

After installation, restart your terminal and verify:
```bash
rustc --version   # should show rustc 1.75 or newer
cargo --version
```

---

## 2. Install Build Tools (Windows)

Rust on Windows needs the MSVC linker. Install Visual Studio Build Tools:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools
```

When the installer opens, select **"Desktop development with C++"** workload.

> Alternative: Install the full Visual Studio 2022 Community (free) — it includes
> all required tools.

---

## 3. Clone / Extract the Project

```bash
cd C:\Users\YOU\Projects
# If you have the zip:
# Extract THE GRID-Rust.zip here
cd thegrid-workspace
```

---

## 4. Build & Run (Development)

```bash
# First build takes ~3-5 minutes (compiles egui + all deps)
# Subsequent builds: ~5-15 seconds (incremental)
RUST_LOG=info cargo run -p thegrid-gui
```

On Windows PowerShell:
```powershell
$env:RUST_LOG="info"
cargo run -p thegrid-gui
```

Headless node (without GUI crate):

```powershell
$env:RUST_LOG="info"
cargo run -p thegrid-node
```

---

## 5. Build Release Binary

```bash
cargo build --release -p thegrid-gui
# Output: target/release/thegrid.exe (~15-25 MB)

# Headless binary
cargo build --release -p thegrid-node

# Headless-only full workspace build (exclude GUI crate)
cargo build --release --workspace --exclude thegrid-gui
```

---

## 6. First Run — Getting Your Tailscale API Key

1. Open: https://login.tailscale.com/admin/settings/keys
2. Click **"Generate access token"**
3. Give it a name (e.g., "THE GRID")
4. Permission needed: **Read** → "Devices"
5. Copy the key (starts with `tskey-api-...`)
6. Paste it into THE GRID's setup screen

---

## 7. Enable RDP on Target Machines

On any Windows machine you want to remote into:

```
Settings → System → Remote Desktop → Enable Remote Desktop: ON
```

Or via PowerShell (run as admin):
```powershell
Set-ItemProperty -Path 'HKLM:\System\CurrentControlSet\Control\Terminal Server' -Name "fDenyTSConnections" -Value 0
Enable-NetFirewallRule -DisplayGroup "Remote Desktop"
```

---

## 8. Project Structure

```
thegrid-workspace/
├── Cargo.toml                      ← Workspace root
└── crates/
    ├── thegrid-core/              ← Shared models, config, SQLite DB
    │   └── src/lib.rs
    ├── thegrid-net/               ← Tailscale API, RDP launcher, HTTP agent
    │   └── src/lib.rs
    ├── thegrid-ai/                ← Semantic layer stubs (Phase 4)
    │   └── src/lib.rs
    ├── thegrid-runtime/           ← Shared runtime/service orchestration
    │   └── src/runtime.rs
    ├── thegrid-node/              ← Headless binary
    │   └── src/main.rs
    └── thegrid-gui/               ← The app binary (egui)
        └── src/
            ├── main.rs             ← Entry point, eframe setup
            ├── app.rs              ← State machine, event bus, render loop
            ├── theme.rs            ← Brutalist visual system
            └── views/
                ├── boot.rs         ← Animated boot screen
                ├── setup.rs        ← First-run configuration
                └── dashboard.rs    ← Main split-panel UI
```

---

## 9. Development Notes

### Logging
```bash
RUST_LOG=debug cargo run -p thegrid-gui  # verbose
RUST_LOG=thegrid_net=debug               # only network logs
```

### Making iterative UI changes
The egui immediate-mode model means: change the render code → rerun → see it.
No hot-reload needed. `cargo run` incremental builds are fast.

### Adding a new feature
1. Add data model to `thegrid-core/src/lib.rs`
2. Add network operation to `thegrid-net/src/lib.rs`
3. Add `AppEvent` variants for the result
4. Add `spawn_*()` method in `app.rs`
5. Handle event in `process_events()` in `app.rs`
6. Wire UI in `views/dashboard.rs`
7. Re-index if needed

### Development roadmap
- **Phase 1 (current):** Core engine foundations + MVP GUI ✓
- **Phase 2:** `notify-rs` filesystem watcher → populate SQLite index
- **Phase 3:** Network hardening, WoL sentry, index replication across mesh
- **Phase 4:** Semantic AI layer via `fastembed` + `usearch`

---

## 10. Troubleshooting

**"linker not found" on Windows**
→ Run the Visual Studio Build Tools installer and select C++ workload

**"failed to resolve: use of undeclared type `Clipboard`"**
→ Make sure `arboard` is in Cargo.toml (it is — this means cargo update is needed)
→ Run: `cargo update`

**"Tailscale API returned 401"**
→ Your API key is wrong or expired. Generate a new one.

**RDP says "can't connect"**
→ Ensure Remote Desktop is enabled on the target machine
→ Ensure Tailscale is running and connected on BOTH machines
→ Check `tailscale status` — both machines should show as "online"

**Files not transferring**
→ Both machines need THE GRID running (to serve the local agent on port 47731)
→ Check if Windows Firewall is blocking port 47731
→ To allow it: `netsh advfirewall firewall add rule name="THE GRID Agent" dir=in action=allow protocol=TCP localport=47731`
