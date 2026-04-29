# H4RB0R Connection Guide for Existing TheGrid Project v1

Status: Implementation Guide
Date: 2026-04-30
Depends on: H4RB0R_BLUEPRINT_V1.md, H4RB0R_UI_WIREFRAME_V1.md, H4RB0R_BACKLOG_V1.md

## 1. Objective
Define exactly how H4RB0R integrates into current TheGrid crates and operational scripts without destabilizing current installer and MVP work.

## 2. Existing Baseline to Reuse
Already present in project architecture:
- AppEvent-driven GUI/runtime coordination
- spawn_* runtime worker pattern
- SQLite schema evolution path with add_column_if_missing and table/index creation
- existing telemetry and status rendering patterns in dashboard
- security/capability mindset in project planning docs

H4RB0R should extend these patterns, not replace them.

## 3. File-Level Connection Map
## 3.1 Core Contracts
Target file:
- thegrid-workspace/crates/thegrid-core/src/models.rs

Add:
- PortEntry, PortOwner, PortState
- PortMapping, PortConflict, PortActionRequest, PortActionResult
- HarborScope and HarborFilter models

## 3.2 Core Event Bus
Target file:
- thegrid-workspace/crates/thegrid-core/src/events.rs

Add AppEvent variants:
- PortInventoryLoaded
- PortInventoryFailed
- PortConflictDetected
- PortActionStarted
- PortActionProgress
- PortActionCompleted
- PortActionFailed

## 3.3 Core Database
Target file:
- thegrid-workspace/crates/thegrid-core/src/db.rs

Add schema blocks:
- port_registry
- port_mappings
- port_action_audit
- port_conflicts

Add methods:
- upsert_port_registry_entries
- list_port_registry
- upsert_port_mapping
- list_port_mappings
- record_port_action_audit
- upsert_port_conflicts

## 3.4 Runtime Workers
Target file:
- thegrid-workspace/crates/thegrid-runtime/src/runtime.rs

Add workers:
- spawn_port_inventory_refresh
- spawn_port_action_apply
- spawn_port_conflict_scan

Integration notes:
- use same non-blocking thread-spawn + event_tx strategy as existing workers
- route failures via AppEvent::PortActionFailed / PortInventoryFailed

## 3.5 GUI State and Views
Target files:
- thegrid-workspace/crates/thegrid-gui/src/app.rs
- thegrid-workspace/crates/thegrid-gui/src/views/ (new harbor.rs)

Add:
- HarborState in app state container
- H4RB0R screen tab/route
- render_harbor_view(ui, state, actions) module

Design alignment rules:
- preserve existing immediate-mode structure
- avoid frame-blocking probes
- use operational status tone consistent with dashboard and media ingest

## 3.6 Networking and Installer Context
Existing scripts and installer behavior reference networking setup and port opening.
Potential touchpoints:
- thegrid-workspace/setup_networking.ps1
- thegrid-workspace/scripts/mesh_connection_smoke.ps1

H4RB0R v1 should read and display known default ports and policy notes before attempting automated changes.

## 4. Integration Sequence (Low-Risk Order)
Step 1: add core models and AppEvent variants
Step 2: add DB schema and query/update helpers
Step 3: add runtime inventory refresh and conflict scan
Step 4: scaffold GUI view in read-only mode
Step 5: add action flows one by one (restart, stop, redirect, add/edit)
Step 6: add audit timeline and policy reasoning labels

## 5. Implementation Rules to Match Current Style
- additive first, refactor later
- small, reviewable slices per risk domain
- never run network or probe logic inside egui render functions
- keep all long operations async through runtime workers
- represent every action outcome via explicit AppEvent

## 6. Minimal First Commit Plan
Commit 1:
- core models + events
- db schema + list/upsert methods

Commit 2:
- runtime refresh worker
- read-only harbor panel with filters

Commit 3:
- redirect/add/edit mapping flow with confirmations
- action audit logging

Commit 4:
- restart/stop guarded actions + policy gating
- conflict resolver dialog

## 7. Acceptance Criteria for Connection Completion
- H4RB0R panel displays live local port inventory
- mappings persist across restart
- every control action writes audit entry
- protected targets cannot be modified without elevated confirmation
- no GUI frame stutter from inventory refresh
