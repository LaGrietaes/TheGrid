# ⬡ THE GRID — DEVELOPER CONTINUATION MANIFEST
**Version:** Phase 3 Complete / Phase 4 Ready  
**Codebase:** `thegrid-workspace/` (rename from `wormhole-workspace/`)  
**Stack:** 100% Rust · egui 0.27 · Tailscale · SQLite FTS5  
**Date written:** 2026-03-18  
**Author:** Claude Sonnet 4.6 (Dev Chief handoff to VSCode continuation)

---

## 0. RENAME CHECKLIST (Do This First)

The project was renamed from **WORMHOLE** to **THE GRID** mid-development.
The code files still use `wormhole_*` identifiers. Before writing new code:

```bash
# Rename workspace folder
mv wormhole-workspace thegrid-workspace

# In every Cargo.toml, rename crate names:
#   wormhole-core  → thegrid-core
#   wormhole-net   → thegrid-net
#   wormhole-ai    → thegrid-ai
#   wormhole-gui   → thegrid-gui

# In every .rs file, replace:
#   wormhole_core::  → thegrid_core::
#   wormhole_net::   → thegrid_net::
#   wormhole-*       → thegrid-*

# The binary name in thegrid-gui/Cargo.toml:
#   name = "thegrid"   (was "wormhole")

# The config directory (core/src/lib.rs, Config::config_path()):
#   dirs::config_dir().join("thegrid")   (was "wormhole")

# The DB path (gui/src/app.rs, WormholeApp::new):
#   .join("thegrid").join("index.db")

# The agent server port stays 47731 — no change needed.
```

---

## 1. WHERE THE CODEBASE CURRENTLY STANDS

### ✅ Phase 1 — Core Architecture (DONE)
- 4-crate workspace: `core`, `net`, `gui`, `ai`
- Config persistence (`%APPDATA%/thegrid/config.json`)
- `AppEvent` mpsc bus — all background threads communicate via this
- Tailscale API client (`GET /api/v2/tailnet/-/devices`)
- egui boot screen → setup screen → dashboard state machine
- Brutalist dark theme (green-on-black, zero rounding, JetBrains Mono)
- Frameless window with custom titlebar

### ✅ Phase 2 — Connectivity MVP (DONE)
- RDP launcher (`mstsc.exe` with resolution options)
- Local HTTP agent server (port 47731): `/ping`, `/filelist`, `/files/{name}`, `/upload`, `/clipboard`
- `AgentClient`: ping, list_files, download_file, upload_file, send_clipboard
- File transfer (drag-drop + file picker, send queue with status)
- Clipboard sync (push/receive with inbox)
- `notify-rs` + `debouncer-mini` filesystem watcher (500ms debounce)
- Settings modal (in-app config editor)

### ✅ Phase 3 — Intelligence Layer (DONE)
- **SQLite indexer** (`db.rs`): full directory walk, FTS5 virtual table with auto-sync triggers, incremental updates from watcher events
- **FTS5 search** (`views/search.rs`): debounced 300ms, result rows with ext glyphs, Ctrl+F shortcut
- **Timeline / "The Flow"** (`views/timeline.rs`): recent files sorted by modified time, day separators, relative timestamps
- **Telemetry** (`telemetry.rs`): sysinfo CPU/RAM/disk, gauge renderer in device header
- **WoL stubs** (`wol.rs`): magic packet sender written, but MAC input UI not done
- **`Arc<Mutex<Database>>`** in `WormholeApp` — DB shared safely across spawned threads
- **`IndexStats`**: total file count shown in titlebar and status bar

### ⬜ Phase 4 — AI + Security + FUI Polish (YOU ARE HERE)

---

## 2. PHASE 4 IMPLEMENTATION PLAN

### 4A. Visual Identity — TheGrid FUI (PRIORITY: HIGH)
*Align with TheGrid branding manifest before any new features.*

#### 4A-1. Color System Update (`theme.rs`)
Replace current color constants with TheGrid palette:

```rust
// In Colors impl:
pub const BG:        Color32 = Color32::from_rgb(0,   0,   0);    // Vantablack #000000
pub const BG_PANEL:  Color32 = Color32::from_rgb(26,  26,  26);   // Gunmetal #1A1A1A
pub const GREEN:     Color32 = Color32::from_rgb(0,   255, 65);   // Phosphor #00FF41
pub const AMBER:     Color32 = Color32::from_rgb(255, 176, 0);    // Tactical #FFB000
pub const AI_BLUE:   Color32 = Color32::from_rgb(0,   229, 255);  // AI Glow #00E5FF (CYAN alias)
pub const RED:       Color32 = Color32::from_rgb(255, 0,   60);   // Alert #FF003C

// NEW: Security Stance colors (used for UI-wide tint, Phase 4B)
pub const STANCE_ACTIVE:  Color32 = GREEN;
pub const STANCE_AFK:     Color32 = AMBER;
pub const STANCE_HVT:     Color32 = RED;
```

#### 4A-2. Chamfered Edges
egui doesn't support clip-path polygons natively. Simulate chamfered corners:

```rust
// In theme.rs — add helper:
pub fn chamfered_frame(fill: Color32, border: Color32) -> egui::Frame {
    // Use a standard Frame but paint chamfer triangles manually via Painter
    // in the show() callback. Size: 6px cut at 45°.
    egui::Frame::none().fill(fill).stroke(egui::Stroke::new(1.0, border))
}

// In show() callback, after the frame renders:
pub fn paint_chamfer(painter: &egui::Painter, rect: egui::Rect, size: f32, color: Color32) {
    // Top-left chamfer
    painter.add(egui::Shape::convex_polygon(vec![
        rect.min,
        egui::pos2(rect.min.x + size, rect.min.y),
        egui::pos2(rect.min.x, rect.min.y + size),
    ], color, egui::Stroke::NONE));
    // Repeat for other 3 corners...
}
```

#### 4A-3. Font Loading (Rajdhani + Fira Code)
In `theme.rs::configure_fonts()`:

```rust
// Embed at compile time — add font files to thegrid-gui/assets/fonts/
let rajdhani = egui::FontData::from_static(
    include_bytes!("../assets/fonts/Rajdhani-Bold.ttf")
);
let fira_code = egui::FontData::from_static(
    include_bytes!("../assets/fonts/FiraCode-Regular.ttf")
);

fonts.font_data.insert("Rajdhani".to_owned(), rajdhani);
fonts.font_data.insert("FiraCode".to_owned(), fira_code);

// Proportional → Rajdhani (headers, node names)
fonts.families.get_mut(&FontFamily::Proportional)
    .unwrap().insert(0, "Rajdhani".to_owned());

// Monospace → Fira Code (logs, data streams)
fonts.families.get_mut(&FontFamily::Monospace)
    .unwrap().insert(0, "FiraCode".to_owned());
```

#### 4A-4. AI Glow Effect
When `thegrid-ai` is actively processing, panels should pulse to AI_BLUE.
State: add `ai_active: bool` to `TheGridApp`. When true, use `AI_BLUE` for
the border color of search, timeline, and node card panels instead of `GREEN`.

```rust
// In app.rs: new field
ai_processing: bool,

// In process_events, when AI search starts:
AppEvent::AiSearchStarted => { self.ai_processing = true; }
AppEvent::SearchResults(_) => { self.ai_processing = false; }

// In views, pass ai_processing as a bool and switch border color:
let accent = if ai_processing { Colors::AI_BLUE } else { Colors::GREEN };
```

#### 4A-5. LagScreen Effect (Background wallpaper from remote node)
New agent endpoint: `GET /wallpaper`
Returns a heavily compressed JPEG (max 200x150, 8-bit dithered) of the
desktop wallpaper of the remote machine.

```rust
// In agent.rs::handle_request():
if method == "GET" && url == "/wallpaper" {
    let wallpaper = get_desktop_wallpaper_thumbnail(); // platform-specific
    req.respond(Response::from_data(wallpaper)
        .with_header(Header::from_bytes(b"Content-Type", b"image/jpeg").unwrap())
    )?;
    return Ok(());
}

// Platform implementations:
// Windows: SHGetDesktopFolder + IDesktopWallpaper COM interface
// Linux:   gsettings get org.gnome.desktop.background picture-uri
// macOS:   NSWorkspace.shared.desktopImageURL

// In dashboard.rs: render as egui::Image with low opacity (0.08)
// and a scanlines shader overlay (custom egui::painter scanline pattern)
```

#### 4A-6. Command Palette (`Ctrl+K`)
New file: `thegrid-gui/src/views/command_palette.rs`

```rust
pub struct CommandPaletteState {
    pub open:    bool,
    pub input:   String,
    pub results: Vec<CommandResult>,
    pub selected: usize,
}

pub enum CommandResult {
    FileResult(FileSearchResult),
    ActionResult { label: String, action: CommandAction },
}

pub enum CommandAction {
    SendToNode { file_path: PathBuf, target_device: String },
    OpenRdp    { device_id: String },
    SyncIndex,
    KillProcess { pid: u32 },
}
```

The palette dispatches to either FTS5 search (instant) or semantic search
(if AI node available). Natural language like "find contract send to tablet"
should be parsed into structured `CommandAction` — stub the NLP parser in
Phase 4, wire to the real AI in Phase 5.

---

### 4B. Security Stances
New module: `thegrid-core/src/security.rs`

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum SecurityStance {
    /// Authenticated, keys in RAM, full access
    Active,
    /// Inactivity timeout — keys purged, UI blurred
    AfkTacticalLock,
    /// Destructive action pending — requires secondary auth
    HighValueTarget { action_description: String },
}
```

State in `TheGridApp`:
- `stance: SecurityStance` — drives UI tint
- `last_activity: Instant` — updated on any mouse/keyboard input
- `afk_timeout: Duration` — configurable, default 5 minutes

In `update()`, check `last_activity.elapsed() > afk_timeout` → transition to AfkTacticalLock.

UI effect: when `stance == AfkTacticalLock`, render the entire central panel
with a blur overlay (egui painter fill with semi-transparent amber tint,
and a "TACTICAL LOCK — AUTHENTICATE TO RESUME" centered label).

**SQLCipher integration** (replaces plain SQLite):
```toml
# In thegrid-core/Cargo.toml, replace rusqlite:
rusqlite = { version = "0.31", features = ["sqlcipher", "bundled-sqlcipher"] }
```
The encryption key comes from a `[u8; 32]` held in a `SecretBox<[u8; 32]>` 
in RAM (use `secrecy` crate). On AfkTacticalLock, zeroize it via `Zeroize` trait.

```toml
# New deps in thegrid-core:
secrecy  = "0.8"
zeroize  = { version = "1", features = ["derive"] }
```

---

### 4C. Semantic AI — `thegrid-ai` Activation

This crate is currently all stubs. Phase 4 wires it up.

#### Cargo.toml additions for `thegrid-ai`:
```toml
fastembed = "3"       # Local ONNX embedding models (MiniLM-L6-v2 ~80MB)
usearch   = "2"       # Approximate nearest neighbor vector search
ort       = "2"       # ONNX Runtime (required by fastembed)
```

#### New types to implement:

**`EmbeddingEngine`** (in `thegrid-ai/src/embedding.rs`):
```rust
pub struct EmbeddingEngine {
    model: fastembed::TextEmbedding,
}

impl EmbeddingEngine {
    /// Downloads model on first run (~80MB), caches in %APPDATA%/thegrid/models/
    pub fn new() -> Result<Self> {
        let model = fastembed::TextEmbedding::try_new(
            fastembed::InitOptions::new(fastembed::EmbeddingModel::AllMiniLML6V2)
        )?;
        Ok(Self { model })
    }

    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(self.model.embed(texts.to_vec(), None)?)
    }
}
```

**`VectorStore`** (in `thegrid-ai/src/vector_store.rs`):
```rust
pub struct VectorStore {
    index: usearch::Index,
    /// Maps USearch internal key → SQLite file_id
    key_to_file_id: HashMap<u64, i64>,
}

impl VectorStore {
    pub fn new(dimensions: usize) -> Result<Self> {
        let index = usearch::Index::new(&usearch::IndexOptions {
            dimensions,
            metric: usearch::MetricKind::Cos,
            ..Default::default()
        })?;
        Ok(Self { index, key_to_file_id: HashMap::new() })
    }

    pub fn add(&mut self, file_id: i64, vector: &[f32]) -> Result<()> {
        let key = file_id as u64;
        self.index.add(key, vector)?;
        self.key_to_file_id.insert(key, file_id);
        Ok(())
    }

    pub fn search(&self, query: &[f32], k: usize) -> Vec<(i64, f32)> {
        let results = self.index.search(query, k).unwrap_or_default();
        results.keys.iter().zip(results.distances.iter())
            .filter_map(|(&key, &dist)| {
                self.key_to_file_id.get(&key).map(|&id| (id, dist))
            })
            .collect()
    }
}
```

#### AppEvent additions for AI:
```rust
// In thegrid-core/src/lib.rs events module:
AiSearchStarted,
AiSearchResults(Vec<FileSearchResult>),
AiModelLoading { progress_pct: u8 },
AiModelReady,
AiModelUnavailable(String),
```

#### Integration in `app.rs`:
```rust
// New field:
ai_engine: Option<Arc<Mutex<thegrid_ai::EmbeddingEngine>>>,
vector_store: Arc<Mutex<thegrid_ai::VectorStore>>,

// In spawn_search(), check if AI engine available:
// If yes AND query looks like natural language → semantic search
// If no → FTS5 keyword search (current behavior)
fn spawn_search(&mut self) {
    let is_natural_language = looks_like_nl(&self.search.query);
    if is_natural_language && self.ai_engine.is_some() {
        self.spawn_semantic_search();
    } else {
        self.spawn_fts_search(); // current implementation
    }
}

fn looks_like_nl(q: &str) -> bool {
    // Simple heuristic: contains spaces and no * or " operators
    q.split_whitespace().count() > 2
        && !q.contains('*')
        && !q.contains('"')
}
```

---

### 4D. Federated Distributed Search
*From TheGrid Security Manifest §1: "Distributed Queries"*

When the user searches, results should come from ALL online peers, not just
the local index.

#### New AppEvent:
```rust
PeerSearchRequest {
    query:      String,
    request_id: uuid::Uuid,  // add uuid = "1" to thegrid-core
},
PeerSearchResponse {
    request_id: uuid::Uuid,
    peer_ip:    String,
    results:    Vec<FileSearchResult>,
},
```

#### New agent endpoint: `POST /search`
```rust
// In agent.rs:
if method == "POST" && url == "/search" {
    let mut body = String::new();
    req.as_reader().read_to_string(&mut body)?;
    
    #[derive(Deserialize)] struct Req { query: String, limit: usize }
    if let Ok(payload) = serde_json::from_str::<Req>(&body) {
        // Run FTS5 against local DB
        let results = LOCAL_DB.lock().unwrap()
            .search_fts(&payload.query, payload.limit, None)
            .unwrap_or_default();
        let json = serde_json::to_string(&results)?;
        req.respond(Response::from_string(json)...)?;
    }
    return Ok(());
}
```

#### `AgentClient::search_peer()`:
```rust
pub fn search_peer(&self, query: &str, limit: usize) -> Result<Vec<FileSearchResult>> {
    let url = format!("{}/search", self.base_url);
    let body = serde_json::json!({ "query": query, "limit": limit });
    let resp: Vec<FileSearchResult> = self.http.post(&url).json(&body).send()?.json()?;
    Ok(resp)
}
```

In `spawn_search()`, after local FTS5, fan out to all online peers in parallel:
```rust
for device in &self.devices {
    if device.is_likely_online() {
        if let Some(ip) = device.primary_ip() {
            self.spawn_peer_search(ip.to_string(), query.clone(), gen);
        }
    }
}
```

Results stream in via `PeerSearchResponse` events and get merged into
`self.search.results` (sorted by rank, deduplicated by path+device_id).

---

### 4E. Wake-on-LAN — Complete the UI

The `WolSentry::send()` function works. What's missing is a way to input
the MAC address. Two approaches:

**Option A (simple):** Add a `mac_address: String` field to the `known_devices`
SQLite table. When a device successfully pings (agent responds to `/ping`),
also query `/mac` endpoint which returns the MAC via:
```rust
// Windows: GetAdaptersInfo / iphlpapi
// Linux:   /sys/class/net/{iface}/address
// macOS:   getifaddrs
```
Store it in `known_devices`. On WoL, look it up from DB.

**Option B (UI prompt):** Add a `MACInputDialog` widget in `dashboard.rs`.
When user clicks WAKE on an offline node, show a small inline text input
pre-filled with the last known MAC (or empty). On submit → `spawn_wol()`.

Recommend Option A — zero user friction once set up.

---

### 4F. Ghost Index — Encrypted Metadata Cache
*From TheGrid Security Manifest §1: "Ghost Index"*

When a node comes online, it should push its file metadata snapshot to
all peers so they can browse it even when it goes offline.

```rust
// New AppEvent:
GhostIndexReceived { device_id: String, snapshot: GhostSnapshot },
GhostIndexSent { device_id: String },

// New model in thegrid-core:
#[derive(Serialize, Deserialize)]
pub struct GhostSnapshot {
    pub device_id:   String,
    pub device_name: String,
    pub file_count:  u64,
    pub entries:     Vec<GhostEntry>,  // lightweight, no content
    pub generated_at: i64,
}

#[derive(Serialize, Deserialize)]
pub struct GhostEntry {
    pub path:     String,
    pub name:     String,
    pub ext:      Option<String>,
    pub size:     u64,
    pub modified: i64,
    pub hash:     Option<String>, // SHA-256, for dedup
}
```

New agent endpoint `GET /ghost` — returns the local DB as a compressed
GhostSnapshot JSON (gzipped, max ~2MB for 50k files).

On `DevicesLoaded`, for each online device, spawn a goroutine to fetch `/ghost`
and insert into the local SQLite with `device_id` set to the remote peer's ID.
This is how search works for offline nodes.

---

### 4G. VerteX Module (Chunked P2P Transfer)
*From TheGrid Expansion Pipeline §3*

For files > 1GB, the current HTTP upload breaks. VerteX replaces it.

New crate: NOT needed — implement in `thegrid-net` as a sub-module.

```rust
// thegrid-net/src/vertex.rs
pub struct VertexSender {
    pub chunk_size: usize,  // default 4MB
    pub resume_offset: u64, // 0 for new transfers
}

impl VertexSender {
    /// Chunked upload with resume. Creates a `.vxpart` manifest on the receiver.
    pub fn send_file(&self, path: &Path, ip: &str, port: u16) -> Result<()> {
        let file = File::open(path)?;
        let total = file.metadata()?.len();
        let name  = path.file_name().unwrap_or_default().to_string_lossy();
        
        // POST /vertex/init → receiver creates manifest
        // POST /vertex/chunk/{offset} → send each chunk
        // POST /vertex/complete → receiver assembles file
        todo!()
    }
}
```

New agent endpoints: `/vertex/init`, `/vertex/chunk/{offset}`, `/vertex/complete`
Implement progress reporting via `AppEvent::VertexProgress { name, sent, total }`.

---

### 4H. PRISM Module (Visual Neural Indexing)
*From TheGrid Expansion Pipeline §4*

New crate: `thegrid-prism` (separate from `thegrid-ai` to keep AI crate lean).

```toml
# thegrid-prism/Cargo.toml
[dependencies]
thegrid-core = { path = "../thegrid-core" }
ort          = "2"        # ONNX Runtime
image        = "0.24"     # Image decoding
```

```rust
// thegrid-prism/src/lib.rs
pub struct PrismIndexer {
    clip_model: OrtSession,  // CLIP ViT-B/32 via ort
}

impl PrismIndexer {
    /// Generate a 512-dim embedding from an image path.
    pub fn embed_image(&self, path: &Path) -> Result<Vec<f32>> {
        let img = image::open(path)?.resize_exact(224, 224, FilterType::Lanczos3);
        // Preprocess → run CLIP visual encoder → return embedding
        todo!()
    }
}
```

PRISM results use the same `VectorStore` as text embeddings, but with a
`source: EmbeddingSource` field (`Text` vs `Image`) to distinguish result types
in the search panel.

---

### 4I. GitHub Sync Module
*From TheGrid Expansion Pipeline §2*

New module in `thegrid-gui/src/views/github_sync.rs`.

```toml
# thegrid-gui/Cargo.toml additions:
octocrab = "0.38"
tokio    = { workspace = true }  # octocrab is async
```

UI: New dashboard tab `REPOS` (next to TIMELINE). Shows local project folders
cross-referenced with GitHub repos. Stale repos (6+ months, synced with remote)
shown with amber `SYNC ✓ — FREE 2.4 GB?` suggestion badge.

---

## 3. COMPLETE FILE CHANGELIST FOR PHASE 4

### Files to CREATE:
```
thegrid-gui/src/views/command_palette.rs   ← Ctrl+K palette
thegrid-gui/src/views/github_sync.rs       ← GitHub repo manager
thegrid-gui/assets/fonts/Rajdhani-Bold.ttf ← Download from Google Fonts
thegrid-gui/assets/fonts/FiraCode-Regular.ttf
thegrid-core/src/security.rs               ← SecurityStance enum + AFK timer
thegrid-ai/src/embedding.rs                ← fastembed EmbeddingEngine
thegrid-ai/src/vector_store.rs             ← usearch VectorStore
thegrid-prism/                             ← New crate (CLIP visual indexing)
thegrid-net/src/vertex.rs                  ← Chunked P2P transport
```

### Files to MODIFY:
```
thegrid-core/src/lib.rs
  - models: add GhostSnapshot, GhostEntry, SecurityStance
  - events: add AiSearch*, GhostIndex*, Vertex*, PeerSearch* variants
  - db.rs:  switch rusqlite → rusqlite with sqlcipher feature
            add insert_ghost_snapshot(), search_peer_index()

thegrid-net/src/lib.rs
  - agent: add /wallpaper, /search, /mac, /ghost, /vertex/* endpoints
  - AgentClient: add search_peer(), get_wallpaper(), get_mac(), get_ghost()

thegrid-gui/src/app.rs
  - Fields: ai_engine, vector_store, ai_processing, stance, last_activity
  - Spawners: spawn_semantic_search(), spawn_peer_search(), spawn_ghost_fetch()
  - Events: handle all new Phase 4 AppEvents

thegrid-gui/src/theme.rs
  - Colors: update to TheGrid palette (Vantablack, Tactical Amber, AI Blue)
  - Add: chamfered_frame(), paint_chamfer(), SecurityStance visual tints

thegrid-gui/src/views/dashboard.rs
  - render_device_panel: add LagScreen wallpaper background
  - DetailState: add wallpaper_texture field
  - Add REPOS tab for GitHub sync

thegrid-gui/src/views/search.rs
  - Add AI mode indicator (AI_BLUE border pulse when ai_processing)
  - Add peer result rows with different styling (device badge in cyan)
  - Add command palette trigger (redirect Ctrl+K)

Cargo.toml (workspace)
  - Add: thegrid-prism to members
  - Add to workspace.dependencies: uuid, secrecy, zeroize, tokio
```

---

## 4. DEPENDENCY MATRIX — WHAT TO ADD WHERE

| Crate | New Dependency | Purpose |
|---|---|---|
| `thegrid-core` | `rusqlite` with `sqlcipher` feature | Encrypted DB |
| `thegrid-core` | `secrecy = "0.8"` | RAM-only key storage |
| `thegrid-core` | `zeroize = { version="1", features=["derive"] }` | Key zeroing on lock |
| `thegrid-core` | `uuid = { version="1", features=["v4"] }` | Peer search correlation |
| `thegrid-ai` | `fastembed = "3"` | Local embedding model |
| `thegrid-ai` | `usearch = "2"` | ANN vector search |
| `thegrid-ai` | `ort = "2"` | ONNX Runtime for fastembed |
| `thegrid-prism` | `ort = "2"`, `image = "0.24"` | CLIP visual indexing |
| `thegrid-net` | `flate2 = "1"` | gzip compress ghost snapshots |
| `thegrid-gui` | `octocrab = "0.38"` | GitHub API |
| `thegrid-gui` | `egui_extras` (already in eframe) | Image texture loading for LagScreen |

---

## 5. THEMING — SECURITY STANCE UI SPEC

The HUD should globally tint based on the current security stance.
Every panel's accent color, border glow, and status dot should switch:

| Stance | Accent | Border | Status Glyph |
|---|---|---|---|
| ACTIVE | `#00FF41` Phosphor Green | Thin green | `◉ ACTIVE` |
| AFK LOCK | `#FFB000` Tactical Amber | Amber pulse | `⚠ TACTICAL LOCK` |
| HVT | `#FF003C` Crimson | Red thick | `☢ AUTHORIZATION REQUIRED` |
| AI PROCESSING | `#00E5FF` Electric Blue | Blue pulse | `⬡ AI ACTIVE` |

Implementation: pass `accent_color: Color32` down from `TheGridApp` into
every view function instead of hardcoding `Colors::GREEN`. The app computes
the accent based on stance + ai_processing state each frame.

---

## 6. KNOWN ISSUES & TECH DEBT

| Issue | Severity | Location | Fix |
|---|---|---|---|
| `sysinfo` dep in `thegrid-gui/Cargo.toml` but only used in `telemetry.rs` | Warning | Cargo.toml | Already used — suppress with `#[allow(unused_crate_dependencies)]` if still warned |
| Remote telemetry agent returns `NodeTelemetry::default()` (all zeros) | Medium | `wormhole-net/agent.rs::collect_telemetry()` | Add `sysinfo` dep to `thegrid-net`, implement real collection |
| WoL MAC address has no persistence | High | `app.rs::handle_detail_actions()` | Implement Option A: `/mac` agent endpoint + `known_devices` DB column |
| Search generation counter uses Status piggyback (fragile) | Low | `app.rs::spawn_search()` | Add `SearchResultsGen(u64, Vec<FileSearchResult>)` AppEvent variant |
| `render_detail_panel` (non-timeline version) still exists but is unused | Low | `dashboard.rs` | Remove it, only `render_detail_panel_with_timeline` is called |
| No error handling if DB lock is poisoned | Medium | All `db.lock()` calls | Add `.unwrap_or_else(|e| e.into_inner())` for poisoned mutex recovery |

---

## 7. RENAME MAP — IDENTIFIER SEARCH/REPLACE

Full list of strings to find-replace after workspace rename:

```
WormholeApp         → TheGridApp
wormhole_core       → thegrid_core
wormhole_net        → thegrid_net
wormhole_gui        → thegrid_gui
wormhole_ai         → thegrid_ai
"wormhole"          → "thegrid"   (config dir, DB path, log prefixes)
WORMHOLE            → THE GRID    (UI display strings only)
⬡ WORMHOLE          → ⬡ THE GRID  (titlebar)
```

The `⬡` glyph (hexagon) stays as the logo mark — it fits TheGrid aesthetic.
Consider replacing with `◈` or `⊟` for a more grid-like feel, but the
hexagon is already established in the codebase so keep it for now.

---

## 8. BUILD & RUN QUICK REFERENCE

```bash
# Development build (fast)
RUST_LOG=info cargo run -p thegrid-gui

# Release build (optimized, ~15-25MB exe)
cargo build --release -p thegrid-gui

# Run with verbose AI logs
RUST_LOG=thegrid_ai=debug,thegrid_net=info cargo run -p thegrid-gui

# Run specific crate tests (when tests are added)
cargo test -p thegrid-core

# Check entire workspace compiles (no run)
cargo check --workspace
```

---

## 9. LORE / PROJECT CONTEXT

- **Project name:** THE GRID
- **Ecosystem:** LaGrieta / TheRiftProgram
- **Design paradigm:** Tactical Brutalism / FUI (Fictional User Interface)
- **Core identity:** Every device is a sovereign node. No cloud. No central server.
  The user owns their data, their keys, and their mesh.
- **The ⬡ glyph:** Represents a network node — a single hexagonal cell in The Grid.

---

*End of handoff document. Good luck in VSCode. The pipes are clean.*
*— Dev Chief*
