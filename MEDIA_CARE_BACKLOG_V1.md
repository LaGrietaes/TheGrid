# TheGrid Media Care Implementation Backlog v1

Status: Draft v1
Date: 2026-04-29
Depends on: MEDIA_CARE_BLUEPRINT_V1.md, MEDIA_CARE_UI_WIREFRAME_V1.md, MEDIA_CARE_DEPENDENCY_MATRIX_V1.md

## 1. Goal
Translate blueprint decisions into implementable frontend/backend work packages with estimates, dependencies, and acceptance criteria.

## 2. Estimation Model
- S: 0.5 to 1.5 days
- M: 2 to 4 days
- L: 5 to 8 days
- XL: 9+ days

Assumes one engineer ownership per task with code review and test coverage.

## 3. Milestone A (Foundation)
## A1. Media Job Domain Model and Events
- Size: M
- Area: thegrid-core
- Deliverables:
  - Media job enums and payload structs
  - AppEvent variants for job lifecycle
  - serialization and validation
- Acceptance criteria:
  - Can create in-memory job definitions for image/video/audio/ai ops
  - Event payloads are stable and documented
  - Unit tests for validation and state transitions

## A2. DB Schema for Job Queue
- Size: M
- Area: thegrid-core db
- Deliverables:
  - migrations for media_jobs, media_job_items, media_job_ops, media_job_artifacts
  - queue indexes
- Acceptance criteria:
  - migration applies cleanly on fresh and existing db
  - queue insert/read/update paths covered by tests
  - rollback or compatibility strategy documented

## A3. Runtime Queue Worker Skeleton
- Size: L
- Area: thegrid-runtime
- Deliverables:
  - worker scheduler
  - bounded concurrency
  - retries and cancellation
- Acceptance criteria:
  - queued jobs execute and update progress
  - cancellation stops active execution safely
  - retries follow policy and log reason

## A4. GUI Queue Timeline (Zone D)
- Size: M
- Area: thegrid-gui
- Deliverables:
  - queue tabs and job rows
  - row actions (cancel/retry/log)
  - job detail drawer scaffold
- Acceptance criteria:
  - GUI reflects queued/running/done/failed states in real time
  - user can cancel and retry from UI
  - no frame-time blocking under active jobs

## A5. Tool Health System
- Size: M
- Area: thegrid-gui + runtime
- Deliverables:
  - ffmpeg/ffprobe/gyroflow/ai runtime checks
  - tool health modal and status badges
- Acceptance criteria:
  - missing tool states are clearly visible
  - each missing dependency has actionable remediation text
  - checks do not block update loop

## A6. Audio Cleanup Baseline
- Size: L
- Area: runtime + core + gui
- Deliverables:
  - silence cut operation (VAD)
  - denoise baseline operation
  - loudness normalize operation
- Acceptance criteria:
  - operation chain runs end-to-end on fixture audio
  - outputs meet expected duration and level constraints
  - failures include per-item reason and recovery action

## 4. Milestone B (Pro Video + Stabilization)
## B1. Video Operation Chain Adapter
- Size: L
- Area: runtime
- Deliverables:
  - transcode/cleanup op adapters
  - standardized ffmpeg command construction
- Acceptance criteria:
  - supports preset-driven video output profiles
  - command failures mapped to clear error categories

## B2. Gyroflow External Integration Adapter
- Size: L
- Area: runtime + gui
- Deliverables:
  - executable detection and version check
  - invocation wrapper
  - artifact reimport and tracking
- Acceptance criteria:
  - selected clips can be processed via external stabilization flow
  - queue records operation lifecycle and output artifacts
  - missing app path handled with guidance

## B3. Smart Care Stack UI (Zone C)
- Size: L
- Area: thegrid-gui
- Deliverables:
  - operation cards
  - parameter forms
  - pipeline reorder and preset save/load
- Acceptance criteria:
  - user can compose, reorder, and run operation stacks
  - stack config serializes into backend job payload
  - keyboard and mouse interactions both supported

## B4. Job Artifact Browser
- Size: M
- Area: thegrid-gui
- Deliverables:
  - artifact panel in job detail drawer
  - open output location actions
- Acceptance criteria:
  - each completed item exposes output artifacts
  - failed items expose error logs

## 5. Milestone C (AI Assist)
## C1. Transcription Operation
- Size: L
- Area: runtime
- Deliverables:
  - whisper-based transcription adapter
  - segment outputs linked to media items
- Acceptance criteria:
  - transcript generated for supported clips
  - output timing segments stored and queryable

## C2. AI Recommendation Engine (Profiles)
- Size: M
- Area: core + runtime
- Deliverables:
  - profile suggestion rules
  - optional onnx-backed scoring path
- Acceptance criteria:
  - recommendations appear in Smart Care panel with rationale
  - unsupported runtime tier degrades gracefully

## C3. UX Polishing and Preset Packs
- Size: M
- Area: gui
- Deliverables:
  - beginner/intermediate/advanced preset packs
  - onboarding tips for first-run flows
- Acceptance criteria:
  - first-time user can complete a Quick Fix run in under 2 minutes
  - power users can save and rerun custom stacks

## 6. Cross-Cutting Tasks
## X1. Observability and Logs
- Size: M
- Deliverables:
  - per-job structured logs
  - error code taxonomy
- Acceptance criteria:
  - failures can be diagnosed without reproducing manually

## X2. Performance Budgeting
- Size: M
- Deliverables:
  - worker limits by media type
  - queue throughput benchmarks
- Acceptance criteria:
  - UI remains responsive while processing mixed workloads

## X3. Test Fixtures and Golden Outputs
- Size: M
- Deliverables:
  - fixture pack for image/video/audio operations
  - baseline output checks
- Acceptance criteria:
  - CI can validate non-regression of core operations

## 7. Suggested Delivery Order
1. A1 -> A2 -> A3
2. A4 + A5
3. A6
4. B1 -> B2 -> B3 -> B4
5. C1 -> C2 -> C3
6. X1/X2/X3 continuously

## 8. Coordination Contract for Claude Sync
For each completed task:
- update design note with final behavior
- record API/event changes
- include screenshots for any UI-facing change
- attach known constraints and unresolved risks

## 9. Release Readiness Gates
Gate 1 (Foundation):
- queue persistence, cancellation, and tool health complete
- image + audio baseline stable

Gate 2 (Pro Video):
- video chain stable
- gyroflow external integration stable

Gate 3 (AI Assist):
- transcription and recommendations stable
- fallback behavior verified on lower capability tiers

## 10. Done Definition for Backlog v1
- All tasks have owner, estimate, and acceptance criteria
- Delivery order matches architecture dependencies
- Ready to convert into sprint tickets without ambiguity
