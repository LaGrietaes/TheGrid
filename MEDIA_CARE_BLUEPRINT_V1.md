# TheGrid Media Care Station Blueprint v1

Status: Draft v1 (Foundation)
Date: 2026-04-29
Owners: TheGrid GUI, Core, Runtime
Purpose: Shared frontend + backend blueprint for synchronized implementation and external design collaboration.

## 1. Product Vision
Build a single Media Care Station inside TheGrid that helps creators ingest, clean, enhance, and export image, video, and audio assets with progressive complexity:
- Beginner: one-click Smart Care presets
- Intermediate: guided controls with safe defaults
- Advanced: full pipeline and batch queue control

## 2. Primary Outcomes
- Fast culling and ingest without UI stalls
- Reliable background processing queue with persistence and resume
- Unified workflows for image resize/enhance, video cleanup/stabilization, audio cleanup, and AI assist
- Clear compatibility and fallback behavior across systems

## 3. Scope (v1 Foundation)
### In Scope
- Extend current Media Ingest view into a full processing station
- Add a unified job queue model and UI
- Add image operations beyond resize presets (pipeline-ready)
- Add audio cleanup baseline (silence cut + denoise + loudness normalize)
- Add external Gyroflow stabilization workflow integration
- Add AI assist hooks for transcription and auto-profile suggestions

### Out of Scope (v1)
- Full NLE timeline editing replacement
- Complex compositing or VFX graph editor
- Cloud-first rendering dependency

## 4. User Personas
- Social Creator: fast turnaround, one-click exports
- Video Creator: stable footage, cleaner voice, subtitle-ready output
- Drone Creator: gyro stabilization + cinematic output presets
- Podcast Creator: silence cut + noise cleanup + loudness consistency

## 5. UX Blueprint (System-Level)
### Station Layout
- Left rail: sources, collections, and filters
- Center: ingest grid and inline preview
- Right panel: Smart Care stack (operation chain)
- Bottom dock: processing queue timeline (progress, retries, logs)

### Interaction Model
- Selection drives operation context
- Operations are composed as stackable steps
- Preview-before-apply when feasible
- Batch apply across selected assets
- Long-running tasks always backgrounded

## 6. Architecture Blueprint
## 6.1 Frontend (thegrid-gui)
- Keep egui immediate-mode rendering with no blocking operations
- Add MediaCareState as parent controller for:
  - Selection context
  - Active operation stack
  - Queue monitor state
  - Tool health status (ffmpeg, gyroflow, ai runtime)
- Reuse existing preview and thumbnail pathways
- Add operation cards with parameter surfaces and presets

## 6.2 Backend Runtime (thegrid-runtime + thegrid-core)
- Introduce MediaJob service with durable queue semantics
- Job execution workers process each operation type in isolated steps
- Job state persisted to DB for crash recovery and resume
- Event bus emits granular progress updates to GUI

## 6.3 Storage and State (thegrid-core DB)
Add tables:
- media_jobs
  - id, kind, status, priority, created_at, started_at, finished_at, error
- media_job_items
  - id, job_id, file_id, input_path, output_path, status, step, metrics_json
- media_job_ops
  - id, job_id, op_index, op_type, params_json
- media_job_artifacts
  - id, job_item_id, artifact_type, path, meta_json

Add indexes:
- media_jobs(status, priority, created_at)
- media_job_items(job_id, status)

## 7. Processing Pipeline Blueprint
Standardized pipeline phases for all operations:
1. Resolve inputs
2. Validate tools and codec support
3. Analyze media metadata
4. Execute operation chain
5. Validate outputs
6. Register artifacts + emit completion

### Failure Policy
- Retry policy by error category:
  - transient tool spawn failure: retry
  - unsupported codec/filter: fail fast with actionable message
  - output write conflict: allow overwrite/rename strategy
- No silent failures
- Every failure maps to user-facing remediation hints

## 8. Operation Families
## 8.1 Image Care
- Resize variants (social, print, ads, free)
- Denoise/light sharpen
- Format conversion and quality target
- Optional watermark-safe export templates

## 8.2 Video Care
- Stabilization (Gyroflow external flow)
- Denoise and transcode profile
- Frame-safe export presets
- Audio extraction and cleanup handoff

## 8.3 Audio Care
- Silence cut (VAD-driven)
- Noise reduction
- Loudness normalization profile (voice-first)
- Optional speech segmentation for transcript alignment

## 8.4 AI Assist
- Transcription hooks
- Auto profile suggestion
- Basic quality scoring and recommendations

## 9. Integration Blueprint (Open Source)
### Execution Backbone
- FFmpeg orchestration via ffmpeg-sidecar
- Keep external ffmpeg binary strategy for compatibility with existing TheGrid behavior

### Audio and AI Stack
- symphonia: decode and metadata assist
- silero or webrtc-vad: voice activity/silence detection
- nnnoiseless or deep_filter: denoise path options
- whisper-rs: transcription
- ort: ONNX runtime for local inferencing where needed

### Stabilization
- Gyroflow integration as external workflow in v1
- Detect install, invoke job, collect outputs/project data, and track status in queue

## 10. Contract with External Design Partner (Claude Sync)
Use this document as source-of-truth contract:
- Visual design can iterate panel and interaction presentation
- Functional contracts are fixed unless revised here:
  - operation taxonomy
  - queue state model
  - failure semantics
  - artifact outputs

Change process:
1. Propose UI/flow change
2. Check contract impact
3. Update this blueprint
4. Apply matching implementation changes

## 11. Non-Functional Requirements
- UI responsiveness maintained under active queue load
- Background processing cancellable per job and per item
- Deterministic outputs for same operation parameters
- Explicit provenance of generated artifacts
- Minimum telemetry for reliability diagnostics

## 12. Risks and Controls
- License risk (third-party media tools): maintain dependency review per release
- Tool availability risk: preflight health checks + guided install messages
- Performance risk: bounded worker concurrency + resource-aware scheduling
- Data safety risk: default to non-destructive outputs in v1

## 13. Increment Plan
- Milestone A: queue foundation + image/audio baseline + tool health checks
- Milestone B: video pro workflows + Gyroflow integration + resilient retries
- Milestone C: AI assist expansion + smarter recommendations + profile packs

## 14. Definition of Done for v1 Blueprint
- Shared front/back architecture agreed
- Queue schema and event model approved
- Operation families and boundaries defined
- External tool integration strategy accepted
- Milestone plan approved for implementation sequencing

---

This is Blueprint First (Foundation).
Next documents to deliver in sequence:
- 1: Concrete UI wireframe map and interactions
- 2: Dependency decision matrix (size, license, risk, platform)
- 3: Implementation backlog with estimates and acceptance criteria
