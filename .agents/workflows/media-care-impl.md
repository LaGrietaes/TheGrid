---
description: Media Care Milestone A implementation — DB schema + MediaJob queue + first GUI surface
turbo-all: true
ai_policy: local_only
---

## Purpose
Implement Milestone A of the Media Care Station.  
This is the foundation slice: DB tables, background worker scaffolding, and the minimal queue UI.  
All source docs are in `.claude/MEDIA_CARE_CONTEXT.json` and the Blueprint/Backlog v1 files.

## Pre-Work (Read First)
1. `.claude/MEDIA_CARE_DESIGN_START_HERE.md`
2. `MEDIA_CARE_BLUEPRINT_V1.md` §6 Architecture
3. `MEDIA_CARE_EXISTING_PROJECT_CONNECTION_V1.md`
4. `MEDIA_CARE_BACKLOG_V1.md` Milestone A tasks

## Implementation Sequence

### Step 1 — DB Schema (`thegrid-core/src/db.rs`)
Add tables:
```sql
CREATE TABLE IF NOT EXISTS media_jobs (
  id          TEXT PRIMARY KEY,
  kind        TEXT NOT NULL,       -- "image_resize" | "audio_cleanup" | "video_stabilize" | "ai_assist"
  status      TEXT NOT NULL,       -- "queued" | "running" | "done" | "failed" | "cancelled"
  priority    INTEGER DEFAULT 5,
  created_at  INTEGER NOT NULL,
  started_at  INTEGER,
  finished_at INTEGER,
  error       TEXT
);

CREATE TABLE IF NOT EXISTS media_job_items (
  id          TEXT PRIMARY KEY,
  job_id      TEXT NOT NULL REFERENCES media_jobs(id),
  file_id     INTEGER REFERENCES files(id),
  input_path  TEXT NOT NULL,
  output_path TEXT,
  status      TEXT NOT NULL,
  step        TEXT,
  metrics_json TEXT
);

CREATE TABLE IF NOT EXISTS media_job_ops (
  id         TEXT PRIMARY KEY,
  job_id     TEXT NOT NULL REFERENCES media_jobs(id),
  op_index   INTEGER NOT NULL,
  op_type    TEXT NOT NULL,
  params_json TEXT
);
```

### Step 2 — AppEvent additions (`thegrid-core/src/events.rs`)
```rust
MediaJobQueued   { job_id: String, kind: String },
MediaJobStarted  { job_id: String },
MediaJobProgress { job_id: String, done: usize, total: usize, current_file: String },
MediaJobComplete { job_id: String, outputs: Vec<String> },
MediaJobFailed   { job_id: String, error: String },
```

### Step 3 — MediaJob service (`thegrid-runtime/src/media_jobs.rs`)
- `MediaJobService::new(db, event_tx)` — holds a queue
- `spawn_media_worker()` — background thread pops jobs, runs ops, emits progress events
- For Milestone A, implement only: image resize via `image` crate (already in thegrid-ai deps)

### Step 4 — MediaCareState (`thegrid-gui/src/views/media_care.rs`)
- Selection context (mirrors existing ingest selection)
- Active job list (binds to `MediaJobQueued/Progress/Complete`)
- Simple job card in right panel (kind, status, progress bar)

## AI Provider Policy
- All Milestone A operations are CPU-only (resize, format convert).
- No AI inference required in Milestone A.
- AI assist ops (transcription, auto-profile) are Milestone C.

## Validation Gate
- `cargo check --workspace` must pass after each step.
- Step 1 must be validated with a migration test in `thegrid-core/src/db.rs` tests.
- Step 4: egui panel must render without blocking the update loop (`cargo run` smoke).

## Acceptance Criteria (Milestone A)
- [ ] `media_jobs`, `media_job_items`, `media_job_ops` tables created on DB init
- [ ] `MediaJobQueued` event emitted when user queues an image resize
- [ ] Background worker picks up job and emits `MediaJobProgress` per file
- [ ] GUI renders a queue card with status and progress bar
- [ ] `cargo check --workspace` passes clean
