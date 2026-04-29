# Start Here - TheGrid Media Care Design Pack

## Goal
Provide one-click context loading for Claude design work focused only on Media Care.

## Primary Sources
- MEDIA_CARE_MASTER_INDEX_V1.md
- MEDIA_CARE_BLUEPRINT_V1.md
- MEDIA_CARE_UI_WIREFRAME_V1.md
- MEDIA_CARE_EXISTING_PROJECT_CONNECTION_V1.md
- MEDIA_CARE_DEPENDENCY_MATRIX_V1.md
- MEDIA_CARE_BACKLOG_V1.md

## Critical Integration Anchors in Existing Code
- Media Ingest screen orchestration:
  - thegrid-workspace/crates/thegrid-gui/src/app.rs
  - thegrid-workspace/crates/thegrid-gui/src/views/media_ingest.rs
- Runtime worker integration pattern:
  - thegrid-workspace/crates/thegrid-runtime/src/runtime.rs
- Core event bus:
  - thegrid-workspace/crates/thegrid-core/src/events.rs
- DB queue and review patterns:
  - thegrid-workspace/crates/thegrid-core/src/db.rs

## What Designer Team Should Produce
- Interaction refinements that map to existing event and queue model
- High-fidelity operation stack behaviors for image/video/audio/ai
- Queue and tool-health UX details ready for implementation tickets

## Do Not Do
- Do not propose architecture that blocks egui update loop
- Do not remove existing review/culling UX
- Do not embed Gyroflow internals in v1

## Handoff Expectation
Design outputs must map back to:
- AppEvent changes
- Runtime spawn_* handlers
- DB table/schema changes
- GUI panel/state changes
