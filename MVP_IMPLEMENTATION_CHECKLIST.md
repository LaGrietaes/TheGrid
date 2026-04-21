# THE GRID MVP Implementation Checklist

**Status**: Transitioning from GUI/Telemetry Polish → File Indexing MVP

**Current Date**: 2026-04-21  
**Session**: Pivoting to MVP file organization and device storage coordination

---

## MVP Scope: "Properly Index Files in Systems Installed"

### Primary Goal
Users can start the GUI or headless node and immediately begin discovering, indexing, and organizing files across their Tailscale mesh devices.

### Success Criteria
- [ ] User configures `watch_paths` in `config.json`
- [ ] GUI starts and shows indexing progress in telemetry
- [ ] Files are discoverable via search
- [ ] Multiple devices sync file indexes automatically
- [ ] User can organize files via Projects/Categories/Smart Rules
- [ ] README provides clear setup instructions for both GUI and headless deployments

---

## Current Infrastructure Status

### ✅ In Place
- **Database Schema** (`thegrid-core/src/db.rs`):
  - `files` table with FTS5 index
  - `file_tombstones` for deletion tracking
  - `transfers` table
  - `index_checkpoints` for resume capability
  - `user_rules`, `smart_rules`, `categories`, `projects`

- **Event System** (`thegrid-core/src/events.rs`):
  - `FileSystemChanged` (watcher events)
  - `IndexProgress`, `IndexComplete`, `IndexUpdated`
  - `SyncRequest`, `SyncComplete`, `SyncFailed`
  - `SyncHealthUpdated` (observability)
  - `SearchResults`

- **Config System** (`thegrid-core/src/config.rs`):
  - `watch_paths: Vec<PathBuf>`
  - `smart_rules`, `projects`, `categories`
  - Projects and Categories with tags

- **File Watcher** (`thegrid-core/src/watcher.rs`):
  - Monitors `watch_paths` for real-time changes
  - Emits `FileSystemChanged` events

- **Runtime Services** (`thegrid-runtime/src/runtime.rs`):
  - `AppRuntime` with database and watcher
  - `file_watcher`, `semantic_search` stubs
  - Event dispatch infrastructure
  - Sync health tracking

- **Node CLI** (`thegrid-node/src/main.rs`):
  - Command registry (`help`, `devices`, `ping`, `history`, `update`, `quit`)
  - Terminal UX with progress lines
  - Agent server startup

- **GUI Foundation** (`thegrid-gui/src/`):
  - Dashboard with telemetry band
  - Device list and node selection
  - Status bar with counters

---

## Missing/Incomplete Components for MVP

### 🔴 Critical Path
1. **Index Scanning Logic** (`thegrid-core` or `thegrid-runtime`)
   - Traverse `watch_paths` directories recursively
   - Fingerprint each file (size, mtime, ext)
   - Compute full hash for duplicates/integrity
   - Batch insert into database with `detected_by='full_scan'`
   - Emit `IndexProgress` and `IndexComplete` events
   - **Rationale**: Currently no code walks the filesystem and populates the `files` table on startup

2. **Sync Delta Computation** (`thegrid-core`)
   - Function to compute `SyncDelta` given a `since` timestamp
   - Include new/modified files + tombstones
   - Compress for network transfer
   - **Rationale**: `SyncRequest` event exists but response payload handler is unclear

3. **Watcher Integration** (connect watcher → database)
   - Route `FileSystemChanged` events to database updates
   - Insert new files with `detected_by='watcher'`
   - Mark deletions as tombstones
   - Update existing files if modified
   - **Rationale**: Watcher emits events but nothing listens to them

4. **Search Handler** (GUI ↔ Database)
   - Route `SearchResults` events from database FTS queries
   - Expose search UI in GUI
   - Rank by relevance, date, device
   - **Rationale**: Search infrastructure exists but no query API

5. **Smart Rules Engine**
   - Apply `user_rules` and `smart_rules` to files as they're indexed
   - Auto-tag files matching patterns
   - Associate files with projects
   - **Rationale**: Tables exist, no evaluation logic

6. **Sync Health Dashboard** (GUI)
   - Display `SyncHealthUpdated` events
   - Show files added, tombstone count, last sync time
   - Indicate detection source distribution
   - **Rationale**: Events emitted, but GUI telemetry doesn't display them yet

7. **Index Status in Telemetry** (GUI)
   - Show `Indexed: N files | M GB`
   - Progress bar during initial scan
   - Sync status per device
   - **Rationale**: Telemetry band designed but no indexing stats wired in

### 🟡 High Priority (Post-MVP)
- Semantic search (embeddings generation)
- Collection management UI
- Backup scheduling
- RDP proxy

---

## Implementation Plan

### Phase 0: Foundation (This Session)
- [x] Review current architecture
- [x] Update README with MVP configuration/usage guide
- [ ] Create implementation tasks list (↑ this file)

### Phase 1: Core Indexing (Next 1-2 Days)
1. **Implement directory scanner**
   - Add `scan_directory(root: &Path)` function in `thegrid-core`
   - Recursively walk, fingerprint files
   - Emit `IndexProgress` for UI feedback
   - Insert into database with `detected_by='full_scan'`
   - Emit `IndexComplete` when done

2. **Wire watcher to database**
   - Listen for `FileSystemChanged` events in `AppRuntime`
   - Insert new files, update modified, mark deleted as tombstones
   - Emit `SyncHealthUpdated` on watcher activity

3. **Implement sync delta**
   - Add `compute_sync_delta(since: i64, device_id: &str)` in Database
   - Query `files` modified after `since` and relevant `tombstones`
   - Return `SyncDelta { files, tombstones, ...}`

4. **Add search query API**
   - Add `Database::search_files(query: &str, limit: usize)` using FTS5
   - Return ranked `FileSearchResult`s

5. **Wire to GUI**
   - Update telemetry to show `Indexed: N files`
   - Add search box and result panel
   - Display sync health per device
   - Show indexing progress during startup

### Phase 2: Validation & Polish (Next 2-3 Days)
- [ ] `cargo check --workspace` passes
- [ ] `cargo run -p thegrid-gui` starts and indexes sample directory
- [ ] `cargo run -p thegrid-node` on secondary device syncs with primary
- [ ] Search returns expected results
- [ ] Smart rules auto-tag new files

### Phase 3: Documentation & Release (Next 1 Day)
- [ ] Update SETUP.md with quick-start examples
- [ ] Document config.json schema
- [ ] Create sample project/category setup
- [ ] Test on Windows + Linux + Android

---

## File Assignments

| Component | File | Status | Owner Notes |
|-----------|------|--------|-------------|
| Directory Scanner | `thegrid-core/src/lib.rs` (new fn) + `thegrid-core/src/utils.rs` | TODO | Leverage existing `fingerprint_file` |
| Watcher Handler | `thegrid-runtime/src/runtime.rs` | TODO | Listen in main event loop |
| Sync Delta | `thegrid-core/src/db.rs` | TODO | New method `compute_sync_delta` |
| Search API | `thegrid-core/src/db.rs` | TODO | Wrap FTS5 queries |
| GUI Integration | `thegrid-gui/src/views/dashboard.rs` + telemetry | TODO | Hook into IndexProgress events |
| Smart Rules | `thegrid-core/src/lib.rs` (new module) | TODO | Match patterns, tag files |

---

## Validation Checklist

Before marking MVP complete:

- [ ] User can add watch paths to `config.json`
- [ ] GUI shows indexing progress during startup
- [ ] Files appear in the database and are searchable
- [ ] Two devices in mesh automatically sync file indexes
- [ ] Search results rank by relevance
- [ ] Smart rules auto-categorize files
- [ ] Watcher detects new/modified/deleted files in real-time
- [ ] Headless node works on Android (or at least Linux)
- [ ] README has clear setup instructions
- [ ] No `cargo check` or `cargo run` errors

---

## Next Immediate Action

**Now**: Understand which component to tackle first based on user priority.

**Likely First Task**: Implement the directory scanner so that `cargo run -p thegrid-gui` actually populates the database instead of remaining empty.

---

## Rollback Strategy

If needed, revert uncommitted index-related changes:
```bash
git status  # See what's new
git diff    # Review changes
git checkout -- <files>  # Or git reset if staged
```

Existing commits to preserve:
- `9b70c02` (GUI Telemetry needs rework)
