# TheGrid Media Care Station UI Wireframe v1

Status: Draft v1
Date: 2026-04-29
Depends on: MEDIA_CARE_BLUEPRINT_V1.md

## 1. Goal
Define a concrete screen layout and interaction contract for Media Care Station so frontend and backend remain synchronized during implementation.

## 2. Screen Topology
Primary screen uses a 4-zone layout:
- Zone A (left rail): source and collection navigation
- Zone B (center): ingest grid + inline preview
- Zone C (right rail): Smart Care operation stack
- Zone D (bottom dock): processing queue timeline

Secondary overlays:
- O1: Job detail drawer
- O2: Tool health modal
- O3: Batch preset save/load dialog

## 3. Desktop Wireframe (Reference)

```text
+----------------------------------------------------------------------------------------------------+
| TITLEBAR | Project | Source | Profile | Tool Health | Active Jobs | Settings                      |
+----------------------+------------------------------------+--------------------------------------+
| A: SOURCES           | B: INGEST GRID / PREVIEW           | C: SMART CARE STACK                  |
| - Local drives       | - Search, filters, sort            | - Quick Fix presets                  |
| - Watched folders    | - Card grid with pick/rate/label   | - Image ops                          |
| - Device groups      | - Inline preview pane              | - Video ops                          |
| - Project bins       | - Selection strip                  | - Audio ops                          |
|                      | - Metadata panel (collapsible)     | - AI assist ops                      |
+----------------------+------------------------------------+--------------------------------------+
| D: QUEUE TIMELINE: [Queued] [Running] [Done] [Failed] [Retry] [Cancel] [Logs]                    |
+----------------------------------------------------------------------------------------------------+
```

## 4. Mobile/Compact Layout Rules
When width is constrained:
- A collapses into top drawer
- C becomes tabbed sheet (Quick, Image, Video, Audio, AI)
- D remains sticky bottom with compact progress chips
- B stays primary visible area

## 5. Zone-Level Specification
## 5.1 Zone A: Sources
UI elements:
- Source tree
- Source type chips (drone, mirrorless, phone, audio recorder)
- Session filters (today, week, unreviewed, picks)

Interactions:
- Single click selects source context
- Ctrl+click supports multi-source selection on desktop
- Right click opens actions: scan, watch, open explorer

State outputs:
- active_sources: [source_id]
- source_filter_profile: string

## 5.2 Zone B: Ingest Grid + Preview
UI elements:
- Top row: query, media type filter, sort, density, quality filters
- Grid cards: thumbnail, flags, rating, quick actions
- Inline preview panel linked to focused item

Interactions:
- Card click selects/focuses
- Ctrl+click toggles multi-select
- Enter triggers open detail
- Number keys set rating
- P/X/U set pick states

State outputs:
- selected_ids: [file_id]
- focused_file_id: file_id
- review_overlay_map

## 5.3 Zone C: Smart Care Stack
UI elements:
- Preset shelf
- Operation cards in stack order
- Parameter forms per operation
- Dry-run estimate panel (time, output count, expected size)

Operation card anatomy:
- Header: op name + enabled toggle
- Body: key params
- Footer: scope selector (selection, filtered, all)

Interactions:
- Drag reorder operation cards
- Enable/disable each op
- Save stack as preset
- Run now or enqueue

State outputs:
- active_pipeline: [operation]
- operation_params_map
- run_scope

## 5.4 Zone D: Queue Timeline
UI elements:
- Queue tabs: queued/running/done/failed
- Job rows with progress bar and step label
- Row actions: pause, cancel, retry, open logs
- Batch actions: retry failed, clear done

Interactions:
- Selecting a row opens O1 job detail drawer
- Double click opens output folder/artifact panel

State outputs:
- selected_job_id
- queue_filters

## 6. Overlays
## 6.1 O1 Job Detail Drawer
Shows:
- job metadata
- per-item progress
- operation step timeline
- errors with remediation text
- artifact links

## 6.2 O2 Tool Health Modal
Tools:
- ffmpeg
- ffprobe
- gyroflow
- ai runtime (onnx/whisper)

States:
- ready, missing, incompatible version

Actions:
- open install docs
- recheck tools

## 6.3 O3 Preset Dialog
Actions:
- save pipeline preset
- update existing preset
- import/export preset json

## 7. Interaction Sequences
## 7.1 Quick Fix Run
1. User selects assets in B
2. User picks Quick Fix preset in C
3. User clicks Enqueue
4. Jobs appear in D with immediate progress
5. Completion toast links to artifacts

## 7.2 Gyroflow Stabilization
1. User selects video clips
2. User enables Stabilize operation in C
3. Preflight checks tool health (O2 if missing)
4. Job executes external workflow
5. Reimported output appears in queue artifacts

## 7.3 Audio Cleanup
1. User selects audio clips
2. User enables Silence Cut + Denoise + Normalize in C
3. Dry-run estimate displayed
4. User confirms run
5. Per-item output available in D

## 8. Accessibility and Usability
- All actions reachable by keyboard shortcuts
- Focus indicators on selected/focused card and active controls
- Color is not sole status indicator; include text and icon status
- Error messages include one actionable next step

## 9. Telemetry Hooks (UI)
Emit non-sensitive events:
- mediacare.pipeline.applied
- mediacare.job.started
- mediacare.job.failed
- mediacare.job.completed
- mediacare.toolhealth.missing

## 10. Frontend-Backend Contract Mapping
UI action to backend command mapping:
- enqueue_pipeline -> MediaJobSubmit
- cancel_job -> MediaJobCancel
- retry_job -> MediaJobRetry
- inspect_artifacts -> MediaJobArtifactsGet
- tool_health_check -> MediaToolHealthCheck

## 11. Definition of Done
- Layout matches this topology on desktop and compact modes
- All core interactions map to backend commands
- Queue and tool health are visible and actionable
- Quick Fix, video stabilize, and audio cleanup flows are end-to-end wired
