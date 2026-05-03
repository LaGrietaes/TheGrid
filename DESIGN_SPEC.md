# THE GRID — GUI Design Specification

> **Purpose:** Complete screen / panel / sub-page inventory with backend data contracts,
> ready for handoff to a visual designer. Every panel maps to the `// GUI_HOOK:` annotations
> in the backend source and to specific `AppEvent` variants in `thegrid-core/src/events.rs`.

---

## Design Language

| Token | Value | Notes |
|-------|-------|-------|
| Primary bg | `#0A0E14` | Deep space black |
| Surface bg | `#0F1520` | Card / panel surface |
| Border | `#1E2D40` | Subtle dividers |
| Accent (active) | `#00E5FF` / `#00BCD4` | Cyan — mesh is live |
| Accent (warning) | `#FFB300` | Amber — AFK lock |
| Accent (danger)  | `#F44336` | Red — HVT / error |
| Accent (success) | `#4CAF50` | Green — online / done |
| Text primary | `#E8EDF2` | |
| Text secondary | `#8FA3B8` | Muted labels |
| Font mono | `JetBrains Mono` / `Fira Code` | Code / IPs / hashes |
| Font UI | `Inter` | All other text |
| Border radius | `6px` card, `4px` button | |
| Spacing unit | `8px` | All margins / paddings are multiples |

### Security Tint System
Applied as a full-screen overlay tint **behind** the chrome:

| Stance | Tint | Overlay |
|--------|------|---------|
| `Active` | None | None |
| `AfkTacticalLock` | AMBER `rgba(255,179,0,0.06)` | Blur + lock screen over content panes |
| `HighValueTarget` | RED `rgba(244,67,54,0.08)` | Confirmation modal (blocking) |

---

## Global Navigation

**Layout:** Vertical sidebar (collapsed: 48px icon rail, expanded: 220px) + top bar + main content.

### Sidebar Items (top to bottom)
1. **Logo / App Name** — THE GRID mark, click to go to Dashboard
2. **Dashboard** — grid icon
3. **File Manager** — folder icon
4. **Global Search** — search icon
5. **Media Care** — photo icon
6. **Timeline** — clock icon
7. **Clean Up** — sparkle icon
8. **Transfers** — arrows icon
9. **Compute** — chip icon
10. **Terminal** — terminal icon
11. *(divider)*
12. **Settings** — gear icon (bottom-pinned)

### Top Bar
- Left: breadcrumb of current screen + panel
- Center: quick-search input (opens Global Search on focus)
- Right: security stance dot (color per stance) + mesh status (N nodes online) + AI queue badge

---

## Screen 1 — Dashboard

**Route:** `/` (default)
**Purpose:** Mesh health at a glance. No interactions required — everything auto-refreshes.

### Panel 1.1 — StorageOverview (top strip, 3 tiles)
| Tile | Data source | Format |
|------|-------------|--------|
| Total Files | `db.get_storage_stats().0` | `1.2M files` |
| Total Indexed Size | `db.get_storage_stats().1` | `847 GB` |
| Devices | `db.get_storage_stats().2` | `7 nodes` |

**AppEvents:** `IndexProgress`, `IndexComplete` → re-query on each.

### Panel 1.2 — NodeGrid (main card grid)
One card per `TailscaleDevice` from `AppEvent::DevicesLoaded`.

**NodeCard layout:**
```
┌─────────────────────────────┐
│  ● HOSTNAME      [OS badge] │  ← DeviceDisplayState dot color + label
│  100.x.x.x                  │  ← primary_ip() in mono
│  ─────────────────────────  │
│  CPU ████░░ 42%   RAM ██░ 68%│  ← NodeTelemetry mini gauges (poll /telemetry)
│  Disk ███░░ 55%   GPU ░░░  0%│
│  ─────────────────────────  │
│  [INDEXING] [2 dupes: 14GB] │  ← state badges from DeviceDisplayState
│  Last seen: 2 min ago        │
└─────────────────────────────┘
```

**Click → opens NodeDetail (Screen 1a)**

**AppEvents:** `DevicesLoaded`, `NodeTelemetryReceived`, `PingResult`, `NodeIndexProgress`

### Panel 1.3 — IndexProgress widget (right sidebar or inline)
Visible while `IndexStats.scanning == true`.
- Progress bar: `scan_progress / scan_total`
- Rate: `smoothed_files_per_sec` → `3.2k files/sec`
- ETA: `scan_eta_secs` → `~4 min`
- Type breakdown: `type_counts` as mini horizontal bar (images / video / docs / other)

**AppEvents:** `IndexProgress { scanned, total, path }`, `IndexComplete`

### Panel 1.4 — RecentActivity (bottom strip)
Last 10 `TemporalEntry` items from `db.get_recent_files(10, None)`.
Compact row: `glyph() name  device_name  size  modified_relative`.

**AppEvents:** `TemporalEntryAdded`

### Panel 1.5 — AIQueue badges (top-bar area)
Three small pill badges, hidden when count = 0:
- `⚙ N hashing` → `db.count_files_needing_hash()`
- `⬡ N embedding` → `db.count_unindexed_files()`
- `✦ N media AI` → `db.count_files_needing_media_ai()`

---

## Screen 1a — NodeDetail (modal/drawer over Dashboard)

**Opened by:** clicking a NodeCard  
**Data:** the selected `TailscaleDevice` + its last `NodeTelemetry`

### Tab A — Hardware
- CPU: name, cores, freq, per-core utilization bars, temperature gauge (color: green→yellow→red)
- RAM: total, used bar, ram_modules table (slot, capacity, speed, type, latency)
- Disk: used/total bar per `DeviceCapabilities.drives`
- GPU: list of `GpuDevice` — name, vendor, VRAM bar, utilization, is_rtx/ai_capable badges
- Network: rx_bps / tx_bps live counters
- Processes: `running_processes` count + `top_processes` list

### Tab B — Capabilities
Table from `DeviceCapabilities`:
- AI models list
- Camera / Mic / Speakers / RDP toggle badges (read-only)
- File access badge
- Compute: `ComputeCapabilities` — supported task types as chips, max_parallel_tasks

### Tab C — Duplicates
Call `db.crosscheck_duplicates_for_device(device_id)` → returns `(groups, files, bytes, known_devices)`.
Show summary stat + link to CleanUp screen filtered to this device.

### Tab D — Actions
- Restart node: `PUT /config` + `POST /restart` (HVT lock)
- Trigger update: `POST /update` (HVT lock)
- Open terminal: creates terminal session → navigate to Terminal screen
- Enable RDP: `POST /rdp/enable` (HVT lock)
- Browse files: navigate to FileManager scoped to this node

---

## Screen 2 — File Manager

**Route:** `/files`
**Layout:** Left tree (directories) + Right content (file list + preview pane)

### Panel 2.1 — DirectoryTree (left, 280px)
- Local roots from `Config.index_roots`
- Remote nodes: expand per `TailscaleDevice` → calls `AgentClient.browse_remote_directory()`
- `AppEvent::RemoteBrowseLoaded { device_id, path, files }` → populates subtree

### Panel 2.2 — FileList (center)
Columns: icon, name, size, modified, ext, device badge, hash status indicator.
- Virtual scrolling — can handle 1M+ rows
- Sort: any column header click
- Filter bar (above list): text search (`db.search_fts()`), device chip selector, ext filter

### Panel 2.3 — PreviewPane (right, 320px, collapsible)
- Text / code: syntax-highlighted content via `GET /files/{path}` or `GET /preview`
- Image: thumbnail + EXIF overlay (from `ai_metadata` JSON)
- Media: basic info (size, fps, duration from ai_metadata)
- Binary: hex dump header

### Panel 2.4 — ContextMenu (right-click)
- Open / Preview
- Copy to node → device picker → calls `AgentClient.upload_file()` + Courier for large files
- Move / Rename → `PUT /files/{path}` (rename), `POST /files/move`
- Delete → `DELETE /files/{path}` (HVT lock prompt)
- Show in CleanUp (if duplicated)
- Tag / Assign to project → opens tag modal

### AppEvents
`RemoteBrowseLoaded`, `RemoteFileDownloaded`, `RemoteFileUploaded`, `FileSent`, `FileReceived`

---

## Screen 3 — Global Search

**Route:** `/search`
**Layout:** Full-width search bar + results list + optional preview pane

### Panel 3.1 — SearchBar
- Text input: triggers `db.search_fts()` on Enter or 300ms debounce
- "Expand query" toggle: calls `Librarian.expand_query()` — shows spinner, replaces input text
- Scope selector: All Nodes / Local Only / specific device chip
- Mode toggle: FTS | Semantic (calls `/ai/search` on selected node)

### Panel 3.2 — ResultsList
Grouped by device. Each row:
```
[ext icon] filename.ext      ← name
           device › parent_dir  ← display_path()
           42.1 MB  ·  3 days ago  ·  rank: 0.87
```
Click → opens preview in side pane.

### Panel 3.3 — PeerResultsSections
`AppEvent::LibrarianSearchResult { query, results, peer_results }` — each peer gets a collapsible section with its device badge and result count.

---

## Screen 4 — Media Care

**Route:** `/media`
**Layout:** Left sidebar (filters) + main gallery grid + right review panel

### Panel 4.1 — FilterSidebar (left, 240px)
Drives `db.search_fts_with_media_filters()`. Controls:
- Text search input
- Device filter chip selector
- **Focus toggle** (`in_focus` bool)
- **Quality** slider 0–100 (`min_quality`)
- **Focus score** slider 0–100 (`min_focus_score`)
- **Min megapixels** input
- **Camera model** text filter
- **Lens model** text filter
- ISO range (min/max)
- Aperture range f/1.4–f/22
- Focal length range (mm)
- Captured date range picker
- GPS required toggle
- Star rating minimum (1–5 stars)
- Pick flag: All / Picks / Rejects / Unflagged
- "Apply Filters" button (or live)

### Panel 4.2 — Gallery (main area)
Masonry / fixed grid toggle. Each card:
```
┌──────────────────────────┐
│                          │
│   [thumbnail / preview]  │
│                          │
│  ★★★☆☆  [PICK]  [●GPS]  │  ← rating, pick_flag, gps indicator
│  filename.jpg             │
│  camera_model · f/2.8     │  ← from ai_metadata
└──────────────────────────┘
```
- Click card → opens ReviewPanel (4.3)
- Keyboard: J/K navigate, P=pick, X=reject, 1–5=rate, Space=preview fullscreen

### Panel 4.3 — ReviewPanel (right drawer, 320px)
Opens on card click.
- Full-size preview / video player
- Star rating widget → calls `db.set_media_review()`
- Pick / Reject / Unflag buttons
- Color label selector (red/yellow/green/blue/purple)
- AI metadata section:
  - Quality score bar
  - Focus score bar
  - `in_focus` badge
  - Camera, lens, ISO, aperture, focal length, shutter speed
  - GPS coordinates → mini map embed (if available)
  - Captured at timestamp
- Tags list + "Add tag" input
- "Queue for Edit" button → `AppRuntime.enqueue_media_edit_job()`
- "Queue Audio Cleanup" (if video) → `enqueue_audio_cleanup_job()`

### Panel 4.4 — BulkActionsBar (bottom, appears when ≥2 selected)
- Selected count badge
- Export picks / rejects to folder
- Bulk delete rejects (HVT lock)
- Bulk tag assignment

### AppEvents
`MediaJobQueued`, `MediaJobStarted`, `MediaJobProgress`, `MediaJobComplete`, `MediaJobFailed`

---

## Screen 5 — Timeline

**Route:** `/timeline`
**Layout:** Full-width chronological feed

### Panel 5.1 — ActivityFeed
Sorted list of `TemporalEntry` from `db.get_recent_files()` + tombstones.
Each row:
```
[glyph]  filename.ext   [device badge]   size   relative time
⊕  created      camera_roll/IMG_3421.jpg   Laptop-W   4.2 MB   5 min ago
⊙  modified     docs/report.docx           NAS-01     128 KB   1 hr ago
⊘  deleted      temp/cache_old.bin         Laptop-W   22 MB    3 hrs ago
```
Color coding: ⊕ green, ⊙ yellow, ⊘ red.

### Panel 5.2 — Filters
- Device multi-select
- Kind toggle (Created / Modified / Deleted)
- Date range picker
- Extension filter

### Panel 5.3 — Sync Health strip (top)
Per-node `SyncHealthMetrics`:
- Last sync time → "synced 2m ago" chip per node
- `sync_failures` → warning badge if > 0
- `tombstone_count` → tombstone badge

---

## Screen 6 — Clean Up

**Route:** `/cleanup`
**Layout:** Sub-nav tabs

### Tab 6.1 — Duplicate Groups
Source: `db.get_duplicate_groups()` → shown as `DuplicateGroup` cards.

Each card:
```
┌───────────────────────────────────────────────────┐
│  HASH: a3f4c9…  ·  SIZE: 847 MB  ·  4 copies       │
│  Sources: [Laptop-W] [NAS-01] [Drive-Ext]          │
│  ──────────────────────────────────────────────── │
│  ◉ KEEP  /photos/2024/IMG_3421.jpg  [NAS-01]       │  ← suggested_anchor highlighted
│  ○ DEL   /camera_roll/IMG_3421.jpg  [Laptop-W]     │
│  ○ DEL   /backup/IMG_3421.jpg       [Drive-Ext]    │
│                              [APPLY — free 1.7 GB]  │
└───────────────────────────────────────────────────┘
```
- "Apply" → HVT confirmation → `DELETE /files/{path}` on each marked-delete node
- Bulk: "Delete All Duplicates (keep suggested anchors)" → HVT lock
- Header stats: N groups, N files, X GB wasted

### Tab 6.2 — Deletion Audit
Immutable log from `deletion_audit` table. Columns: executed_at, file_path, device_id, action, reason, file_size.
Read-only. Export as CSV button.

### Tab 6.3 — Drive Buffer
`DriveBufferManifest` display: quota, session, staged files breakdown by category.
`DriveBufferEntry` list: source → staged path, hash, size, category.

### Tab 6.4 — Tombstones
`FileTombstone` log: recently deleted files with detection source, hash, device. 
Can "un-tombstone" a file (restore metadata record) if the file was re-created.

---

## Screen 7 — Transfers

**Route:** `/transfers`
**Layout:** Active list + history log

### Panel 7.1 — ActiveTransfers
Each active `CourierTransfer`:
```
┌──────────────────────────────────────────────────────────┐
│  ⬆ bigfile.zip → NAS-01                                  │
│  ████████████████░░░░░░░  72%  ·  214 MB / 298 MB        │
│  4.2 MB/s  ·  ETA ~20s                                   │
│                                              [Cancel]     │
└──────────────────────────────────────────────────────────┘
```
- `AppEvent::CourierProgress` → update progress bar live
- `AppEvent::CourierComplete` → move to history with ✓ badge
- `AppEvent::CourierFailed` → show error inline with retry option

### Panel 7.2 — TransferHistory
`transfers` table entries. Columns: direction (↑/↓), filename, peer_ip, size, status, created_at.

### Panel 7.3 — ClipboardSync
Last `ClipboardEntry` received from mesh. Shows: content preview, sender device, received time.
"Copy to clipboard" button. Incoming clips emit a toast notification.

---

## Screen 8 — Compute

**Route:** `/compute`
**Layout:** Mesh load panel + task queue + active sessions

### Panel 8.1 — MeshLoad
One row per peer showing `ComputeCapabilities`:
```
[device badge]  GPU: RTX 3080 · 10GB  ·  CPU: 16c  ·  RAM: 24GB avail
                Tasks: [TEXT EMBED] [IMG EMBED] [LLM]  · max 4 parallel
                Status: ● available (0/4 busy)
```

### Panel 8.2 — ActiveSessions
Each `ComputeSession`:
- borrower → provider arrow
- task_type badge, task_id (truncated)
- elapsed time (started_at)
- Cancel button → `DELETE /compute/{task_id}` with HVT for running tasks

### Panel 8.3 — TaskQueue (local node)
`ComputeStatus` from local `/compute/status`:
- available toggle, active_tasks / queued_tasks gauges
- busy_until_estimate if busy
- "Accept Compute Requests" toggle (writes to Config)

---

## Screen 9 — Terminal

**Route:** `/terminal`
**Layout:** Tab bar of sessions + terminal view

### Panel 9.1 — SessionTabs
One tab per active terminal session (`POST /terminal/session` response).
New tab → device picker → creates session.

### Panel 9.2 — TerminalView
Full xterm-style terminal:
- Input → `POST /terminal/{session_id}/input`
- Output → poll `GET /terminal/{session_id}/output` (or WebSocket if added)
- Close tab → `DELETE /terminal/{session_id}` 

---

## Screen 10 — Settings

**Route:** `/settings`
**Layout:** Left category list + right form area

### Section 10.1 — Mesh / Network
| Field | Config key | Notes |
|-------|-----------|-------|
| Tailscale API key | `tailscale_api_key` | Masked input; "Test connection" button |
| Agent port | `agent_port` | Default 5000 |
| Agent API key | `api_key` | Auto-generated UUID; copy button |
| Trust Tailscale | `trust_tailscale` | Toggle — skip X-Grid-Key on tailnet peers |

### Section 10.2 — Indexing
| Field | Config key | Notes |
|-------|-----------|-------|
| Index roots | `index_roots` | Multi-value path list; browse button |
| AI policy | `ai_policy` | Dropdown: local_only / cloud_allowed |
| Max parallel jobs | — | Number input |
| Indexing overrides | `IndexingOverride[]` | Table: pattern, action (force include/exclude/metadata) |

### Section 10.3 — Google Drive
- `is_authorized()` → show "Connect" or "Connected as email@..." badge
- "Connect" button → calls `DriveClient.authorize()` → spinner "Waiting for browser..." (5m timeout)
- "Sync Now" button → `index_all_files()` → progress bar (DriveIndexProgress events)
- Storage gauge: DriveAbout.storage_used / storage_limit
- Disconnect button (deletes token file; HVT lock)

### Section 10.4 — Rules
Table of `UserRule`:
- Columns: name, pattern, project chip, tag chip, active toggle
- "Add Rule" row at bottom: inline form → calls `db.add_rule()`
- Trash icon per row → `db.delete_rule()` (HVT)
- Patterns support glob (`*.mp4`) and basic regex

### Section 10.5 — AI / Inference
| Field | Notes |
|-------|-------|
| Embedding provider | Dropdown: FastEmbed (local) / Ollama / OpenAI-compat |
| Ollama URL | Text input (shown if Ollama selected) |
| Model name | Text input |
| Inference provider | Dropdown: Ollama / Gemini / Claude |
| API key | Masked input |
| "Test inference" | Sends a ping prompt; shows latency |

### Section 10.6 — Node (local agent)
- Device name (read-only, from hostname)
- Version (read-only)
- "Restart agent" → `POST /restart` (HVT)
- "Check for updates" → `POST /update` (HVT)

---

## Security Overlays (global, rendered above all screens)

### AFK Lock Overlay
Triggered by: `AppEvent::SecurityStanceChanged(AfkTacticalLock)`.
- Full-screen semi-opaque layer (AMBER tint)
- Centered lock icon + "Locked due to inactivity"
- Unlock input: password / PIN field
- On success → `Sentinel.user_active()` → stance returns to Active

### HVT Confirmation Modal
Triggered by: `AppEvent::SecurityStanceChanged(HighValueTarget { action_description })`.
- Blocking modal (cannot dismiss by clicking outside)
- Shows `action_description` in large text
- "Confirm" + "Cancel" buttons
- On Confirm: the pending action proceeds
- On Cancel: stance returns to Active, action aborted

### Agent Alert Toasts
`AppEvent::AgentAlert { agent, level, message }` → non-blocking toast (top-right, 5s timeout).
- `level == "info"` → cyan border
- `level == "warn"` → amber border  
- `level == "error"` → red border

---

## Data Contracts Summary

| Backend type | Screen → Panel |
|-------------|----------------|
| `TailscaleDevice` | Dashboard → NodeGrid card |
| `NodeTelemetry` | Dashboard → NodeGrid mini-bars; NodeDetail → HardwareTab |
| `DeviceDisplayState` | Dashboard → NodeGrid status dot |
| `IndexStats` | Dashboard → IndexProgress widget |
| `FileSearchResult` | FileManager → FileList rows; GlobalSearch → ResultsList |
| `TemporalEntry` | Timeline → ActivityFeed rows |
| `DuplicateGroup` | CleanUp → DuplicateGroups cards |
| `DeletionRecord` | CleanUp → DeletionAudit table |
| `CourierTransfer` | Transfers → ActiveTransfers |
| `ComputeSession` | Compute → ActiveSessions |
| `ComputeCapabilities` | Compute → MeshLoad; NodeDetail → ComputeTab |
| `ComputeStatus` | Compute → TaskQueue |
| `UserRule` | Settings → Rules |
| `DriveAbout` | Settings → GoogleDrive |
| `SecurityStance` | App-wide tint + overlays |
| `SyncHealthMetrics` | Timeline → SyncHealth strip |
| `AgentPingResponse` | Dashboard → NodeGrid last-seen |
| `ClipboardEntry` | Transfers → ClipboardSync |

---

## AppEvent → Screen Mapping (key events)

| AppEvent | Primary target | Update |
|---------|---------------|--------|
| `DevicesLoaded` | Dashboard → NodeGrid | Re-render all cards |
| `NodeTelemetryReceived` | Dashboard → NodeGrid; NodeDetail | Update gauges |
| `IndexProgress` | Dashboard → IndexProgress | Tick progress bar |
| `IndexComplete` | Dashboard → IndexProgress | Hide widget; update storage stats |
| `SecurityStanceChanged` | App-wide | Tint + overlay |
| `RemoteBrowseLoaded` | FileManager → DirectoryTree | Populate subtree |
| `LibrarianSearchResult` | GlobalSearch → ResultsList | Render peer sections |
| `CourierProgress` | Transfers → ActiveTransfers | Update progress bar |
| `CourierComplete` | Transfers → ActiveTransfers | Move to history |
| `CourierFailed` | Transfers → ActiveTransfers | Show error + retry |
| `MediaJobComplete` | MediaCare → Gallery | Re-fetch card ai_metadata |
| `DriveIndexProgress` | Settings → GoogleDrive | Progress bar |
| `DriveIndexComplete` | Settings → GoogleDrive | Show total count |
| `ComputeTaskUpdate` | Compute → ActiveSessions | Update task state |
| `AgentAlert` | App-wide toast | Show notification |
| `TemporalEntryAdded` | Timeline + Dashboard | Prepend row |
| `ClipboardReceived` | Transfers → ClipboardSync; toast | Update panel + notify |

---

*Generated from backend source audit — `thegrid-core`, `thegrid-net`, `thegrid-ai`, `thegrid-runtime`.*
*All `// GUI_HOOK:` annotations in source are cross-referenced here.*
