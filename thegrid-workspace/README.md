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

---

## Organized Dual Targets (Single Branch)

To work on GUI and NODE in sync while keeping headless builds free of GUI extras,
use package-scoped commands from the root workspace.

### Run Commands

```bash
# GUI target
cargo run -p thegrid-gui

# Headless target (only node dependency graph)
cargo run -p thegrid-node

# Full headless validation build (all non-GUI crates)
cargo check --workspace --exclude thegrid-gui
```

### Optional NPM Shortcuts

From `thegrid-workspace/`:

```bash
npm run run:gui
npm run run:node
npm run build:gui
npm run build:node
```

---

## MVP: File Indexing & Device Organization

THE GRID's core MVP provides cross-device file discovery, indexing, and semantic organization across your mesh network.

### Quick Start

**1. Configure Watch Paths**

Edit or create `~/.config/thegrid/config.json` and add the directories you want indexed:

```json
{
  "device_name": "MY-LAPTOP",
  "device_type": "Laptop",
  "watch_paths": [
    "/home/user/Documents",
    "/home/user/Pictures",
    "/mnt/storage/Archive"
  ],
  "transfers_dir": "/home/user/TheGrid_Transfers"
}
```

**2. Start the GUI**

```bash
cd thegrid-workspace
cargo run -p thegrid-gui
```

The GUI will:
- Discover all Tailscale mesh devices automatically.
- Index local watch directories on startup.
- Display indexing progress in the telemetry band.
- Show synchronized files across all connected devices.

**3. Start Headless Nodes (Optional)**

For Android, Raspberry Pi, or server deployments without GUI:

```bash
cargo run -p thegrid-node
```

Then connect from another device in the mesh:
- The node will index its configured watch paths.
- Files and telemetry become accessible via the GUI on any connected device.

---

### File Indexing Infrastructure

#### What Gets Indexed

- **File Metadata**: name, path, size, modified time, extension
- **Content Hash**: full SHA256 hash for deduplication and integrity verification
- **Detection Source**: tracks if files were discovered via full scan, watcher, or remote sync
- **Deletion History**: tombstone records for tracking deleted files

#### Index Storage

- Local SQLite database at `~/.config/thegrid/index.db`
- Full-text search (FTS5) on file names and paths
- Embeddings table for semantic search (Phase 2+)
- Per-device sync checkpoint tracking

#### Sync Between Devices

When two nodes connect:
1. **Initial Sync**: Smaller device sends file deltas to larger/primary device.
2. **Incremental Sync**: Only new/modified/deleted files are transferred.
3. **Conflict Resolution**: Modified times and hashes determine winners.

---

### Device Organization Guide

#### Device Roles

- **Primary/Hub**: Larger storage, higher CPU. Collects and aggregates indexes from all nodes. Example: Desktop workstation or NAS.
- **Lightweight**: Android tablet, Raspberry Pi. Indexes local storage but syncs up to hub.
- **Archive**: Rarely active. Optional participants in mesh. Scanned on demand.

#### Organizing via Projects & Categories

Edit `config.json` to define Projects and Categories for semantic grouping:

```json
{
  "projects": [
    { "id": "p1", "name": "Work", "tags": ["#client-a", "#confidential"] },
    { "id": "p2", "name": "Media Production", "tags": ["#video", "#4k"] },
    { "id": "p3", "name": "Archive", "tags": ["#backup", "#cold-storage"] }
  ],
  "categories": [
    { "id": "c1", "name": "Documents", "icon": "📄" },
    { "id": "c2", "name": "Photos", "icon": "📷" },
    { "id": "c3", "name": "Videos", "icon": "🎥" },
    { "id": "c4", "name": "Code", "icon": "💾" }
  ],
  "smart_rules": [
    {
      "name": "Work PDFs",
      "pattern": "*.pdf",
      "project": "p1",
      "tag": "#documents"
    },
    {
      "name": "4K Video",
      "pattern": "*.{mp4,mov}",
      "project": "p2",
      "tag": "#video"
    }
  ]
}
```

Use the GUI's **Smart Rules** engine to automatically categorize and tag files as they're indexed.

---

### Usage Patterns

#### Pattern 1: Desktop-Centric Hub

1. **Primary**: Desktop with THE GRID GUI running continuously.
2. **Sync**: All devices index locally, then send deltas to the desktop.
3. **Access**: Search and browse all files from the desktop GUI.

```json
{
  "device_name": "WORKSTATION-HUB",
  "device_type": "Desktop",
  "watch_paths": [
    "C:\\Users\\User\\Documents",
    "C:\\Users\\User\\Pictures",
    "D:\\Archive"
  ]
}
```

#### Pattern 2: Distributed Lightweight Mesh

1. **Multiple Nodes**: Each device runs headless `thegrid-node` with local indexing.
2. **Sync Bidirectional**: All nodes share file indexes with each other.
3. **Access**: Any device with THE GRID GUI can see the full mesh index.

#### Pattern 3: Archive + Production Split

1. **Production**: Fast NVMe with `watch_paths` for active work.
2. **Archive**: Slower HDD with `watch_paths` for cold storage.
3. **Integration**: Both devices in mesh; GUI distinguishes by device name and storage kind.

---

### Monitoring Indexing Progress

In the GUI:

- **Telemetry Band** displays:
  - Indexing progress (% complete)
  - Files indexed count
  - Storage size totals
  - Sync status and last update time
- **Status Bar** shows:
  - `INDEXED: 15,243 files | 127.4 GB`
  - `SYNC: 2 devices in mesh | last sync: 2m ago`

In the CLI (headless node):

```bash
> help
# Shows all available commands

> devices
# Lists connected devices and their last sync time

> ping <device>
# Pings a device and shows indexing stats

> update
# Checks for upstream updates
```

---

## Feature Roadmap

### Phase 1: MVP File Indexing ✅ (Current)
- [x] Local file discovery and hashing
- [x] SQLite index database with FTS
- [x] Device sync with delta compression
- [x] GUI dashboard with telemetry
- [x] Headless node CLI with commands
- [ ] **Complete**: Semantic search (Phase 2 pending)

### Phase 2: Smart Categorization & Semantic Search
- [ ] Embedding generation (OpenAI / local Ollama)
- [ ] Semantic similarity search ("find similar images")
- [ ] AI-powered project auto-tagging
- [ ] Natural language file discovery

### Phase 3: Advanced Organization
- [ ] Collections (user-defined file groups)
- [ ] Smart rules with regex and glob patterns
- [ ] Automated backup scheduling
- [ ] Compression and archival workflows

### Phase 4: Remote Access & Collaboration
- [ ] RDP proxy via mesh
- [ ] Secure remote terminal
- [ ] File transfer with bandwidth limits
- [ ] Real-time clipboard sync

---

## Headless Terminal UX

`thegrid-node` prints structured Unicode status lines for key runtime events
(boot, sync, watcher, transfer, AI, indexing) to make terminal progress easier to scan.

Routine ping logs were reduced from `info` to `debug` to avoid terminal clutter.

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
