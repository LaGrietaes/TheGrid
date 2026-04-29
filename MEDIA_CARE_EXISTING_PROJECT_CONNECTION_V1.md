# Media Care Connection Guide for Existing TheGrid Project v1

Status: Implementation Guide
Date: 2026-04-29
Depends on: MEDIA_CARE_BLUEPRINT_V1.md, MEDIA_CARE_UI_WIREFRAME_V1.md, MEDIA_CARE_BACKLOG_V1.md

## 1. Objective
Explain exactly how the Media Care design plugs into the current TheGrid codebase without rewriting existing architecture.

## 2. Current Integration Baseline (Already Present)
The project already has a strong Media Ingest foundation:
- Screen routing and rendering path for Media Ingest
- Selection, review, filter, and thumbnail workflows
- Inline media preview and ffmpeg-based media handling
- Runtime worker pattern based on spawn_* methods
- Core AppEvent message bus for GUI-runtime coordination
- DB support for persisted media review metadata

This means we extend, not replace.

## 3. File-Level Connection Map
## 3.1 GUI Entry Points
- crates/thegrid-gui/src/app.rs
  - Existing MediaIngest screen rendering path
  - Existing action routing to runtime and local resize
  - Connection plan:
    - Replace local-only resize execution with job enqueue calls
    - Add queue panel rendering and job action dispatch

- crates/thegrid-gui/src/views/media_ingest.rs
  - Existing ingest grid, filters, selection bar, preview panel
  - Connection plan:
    - Keep current ingest interactions as Zone B
    - Add Smart Care stack controls (Zone C)
    - Emit media job submit actions along with current review events

- crates/thegrid-gui/src/views/video_preview.rs
  - Existing ffmpeg probe and preview extraction
  - Connection plan:
    - Reuse tool probing utilities for Tool Health modal

## 3.2 Runtime Entry Points
- crates/thegrid-runtime/src/runtime.rs
  - Existing worker orchestration style and media analyzer worker
  - Existing media review spawn methods
  - Connection plan:
    - Add spawn_media_job_submit
    - Add spawn_media_job_cancel
    - Add spawn_media_job_retry
    - Add spawn_media_job_worker_loop
    - Keep current worker style and event_tx semantics

## 3.3 Core Event Bus and Data Contracts
- crates/thegrid-core/src/events.rs
  - Existing media review events in AppEvent
  - Connection plan:
    - Add media job lifecycle events:
      - MediaJobQueued
      - MediaJobStarted
      - MediaJobProgress
      - MediaJobItemProgress
      - MediaJobCompleted
      - MediaJobFailed
      - MediaJobCanceled

## 3.4 Core Database
- crates/thegrid-core/src/db.rs
  - Existing queue patterns (index_queue, embedding_queue)
  - Existing media review table and methods
  - Connection plan:
    - Add media_jobs, media_job_items, media_job_ops, media_job_artifacts
    - Mirror existing queue query patterns for consistency
    - Add CRUD helpers matching runtime needs

## 4. Architecture Alignment Rules
- Do not block egui update loop
- Keep background work in runtime threads/workers
- Keep GUI purely event-driven and state-driven
- Persist queue state in DB to recover after restart
- Follow existing naming pattern: spawn_* runtime entry methods

## 5. Concrete Connection Sequence
## Step 1: Core Types and Events
1. Add media job structs and enums to core models.
2. Extend AppEvent with media job lifecycle variants.
3. Add serialization and validation tests.

## Step 2: DB Schema and Accessors
1. Add new media job tables and indexes in DB init path.
2. Add DB methods:
- insert_media_job
- list_media_jobs_by_status
- update_media_job_status
- upsert_media_job_item_progress
- insert_media_job_artifact
3. Add db tests following existing embedding_queue test style.

## Step 3: Runtime Worker Integration
1. Add spawn_media_job_submit in runtime.
2. Add worker loop that consumes queued jobs with bounded concurrency.
3. Emit AppEvent progress updates to GUI.
4. Wire cancel and retry handlers.

## Step 4: GUI Queue Integration
1. Add queue state to app-level state in app.rs.
2. Render Queue Timeline dock in MediaIngest screen.
3. Route cancel/retry actions to runtime spawn methods.
4. Show per-job status and per-item progress.

## Step 5: Convert Existing Resize Flow
1. Replace spawn_media_resize direct execution path with enqueue job path.
2. Keep current presets and selection mapping.
3. Output status through queue events, not ad-hoc status strings.

## Step 6: Add Tool Health Layer
1. Surface ffmpeg/ffprobe checks in UI modal.
2. Add gyroflow and AI runtime checks.
3. Gate unsupported actions based on capability tier.

## Step 7: Add Operation Adapters
1. Image ops adapter
2. Audio cleanup adapter
3. Video chain adapter
4. Gyroflow external adapter

## 6. Mapping from New Docs to Existing Files
- Blueprint architecture maps to:
  - crates/thegrid-core/src/events.rs
  - crates/thegrid-core/src/db.rs
  - crates/thegrid-runtime/src/runtime.rs
  - crates/thegrid-gui/src/app.rs
  - crates/thegrid-gui/src/views/media_ingest.rs

- Wireframe layout maps to:
  - crates/thegrid-gui/src/views/media_ingest.rs
  - crates/thegrid-gui/src/app.rs

- Dependency matrix maps to:
  - crates/thegrid-gui/Cargo.toml
  - crates/thegrid-runtime/Cargo.toml
  - workspace Cargo.toml where shared dependencies fit

- Backlog milestones map to phased implementation across core, runtime, gui in that order.

## 7. Minimal First Commit Plan
Commit 1:
- Core events and models for media jobs
- DB tables and access methods
- Tests for schema and queue operations

Commit 2:
- Runtime submit/cancel/retry and worker skeleton
- Event emission for lifecycle

Commit 3:
- GUI queue panel scaffold
- Replace resize direct path with queued path

Commit 4:
- Tool health modal
- Capability gating

## 8. Acceptance Criteria for Connection Completion
- MediaIngest can enqueue jobs instead of local-only resize execution
- Runtime executes queued jobs asynchronously
- GUI receives and renders live progress
- Job state survives restart
- Failures are actionable and retryable

## 9. Implementation Notes
- Preserve existing Media Ingest UX behaviors (selection shortcuts, review actions, inline preview)
- Avoid regressions in thumbnail pipeline responsiveness
- Prefer additive changes first, then refactor legacy direct-execution paths

## 10. Ready-To-Start Checklist
- Design suite approved
- Event names approved
- DB schema approved
- Runtime queue policy approved (concurrency and retry)
- First milestone tickets created
