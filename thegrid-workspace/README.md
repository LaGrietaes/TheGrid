# The Grid Workspace

The Grid is a distributed, cross-platform mesh application designed for rich data integration and telemetry.

## Branches & Environments

This repository hosts two primary working environments isolated by git branches:

### 1. `main` Branch (Desktop / Rich Environment)
The `main` branch is the comprehensive workspace containing all components, specifically:
- `thegrid-gui`: The `egui` based desktop application for controlling nodes, viewing semantic graphs, and searching the mesh network.
- Requires full graphical rendering capabilities natively supported by Windows/macOS/Linux Desktop.

### 2. `node` Branch (Headless / Android / IoT)
**The `node` branch is explicitly dedicated as the Headless Version.** 
- All GUI crates (`thegrid-gui`, `eframe`) are removed to ensure low-footprint, error-free compilation on devices lacking graphical capabilities.
- Perfect for deployment on Android tablets (via Termux), Raspberry Pi, or any remote Linux servers.
- It only contains the proper environment code for data processing, the Tailscale agent, and indexing capabilities.

### Headless Update Command
If you are updating only the TUI/headless line, run this from `thegrid-workspace`:

```powershell
gitupdate
```

What it does:
- fetches and fast-forwards `origin/node`
- switches to `node` if needed
- runs `cargo check -p thegrid-node`

Flags:
- `gitupdate -NoCheck` to skip cargo validation
- `gitupdate -ReturnToPrevious` to switch back after update

---

## USB Connectivity & Debugging (Android)
While Tailscale mesh is the primary communication vector, you can connect directly to Android / Termux nodes seamlessly over a physical USB connection using `scrcpy` (Screen Copy).

`scrcpy` provides extremely low-latency screen mirroring and remote control via `adb`.
To use it:
1. Enable **USB Debugging** on the Android device.
2. Connect it to your Notebook via USB.
3. Install `scrcpy` on Windows (e.g., `scoop install scrcpy` or `choco install scrcpy`).
4. Run `scrcpy` in your terminal.

You can also route network connections seamlessly between the device and your PC via ADB using `adb forward tcp:47731 tcp:47731`, which completely bypasses the need for Wi-Fi or Tailscale!
