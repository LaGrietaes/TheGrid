# TheGrid H4RB0R Implementation Backlog v1

Status: Draft v1
Date: 2026-04-30
Depends on: H4RB0R_BLUEPRINT_V1.md, H4RB0R_UI_WIREFRAME_V1.md, H4RB0R_EXISTING_PROJECT_CONNECTION_V1.md

## 1. Goal
Convert H4RB0R blueprint into implementable milestones with clear acceptance criteria and low regression risk.

## 2. Estimation Model
- S: 0.5 to 1.5 days
- M: 2 to 4 days
- L: 5 to 8 days
- XL: 9+ days

## 3. Milestone A (Read-First Foundation)
## A1. Port Domain Types and Validation
- Size: M
- Area: thegrid-core models/events
- Deliverables:
  - port models and enums
  - AppEvent contract for inventory/action lifecycle
  - model validation tests
- Acceptance criteria:
  - contracts compile and serialize cleanly
  - invalid protocol/port inputs are rejected

## A2. DB Schema and Access Layer
- Size: M
- Area: thegrid-core db
- Deliverables:
  - new port tables and indexes
  - list/upsert/query methods
- Acceptance criteria:
  - schema applies on fresh and existing db
  - read/write operations validated by tests

## A3. Runtime Inventory Refresh Worker
- Size: M
- Area: thegrid-runtime
- Deliverables:
  - refresh worker and conflict scan worker
  - event emission for loaded/failed states
- Acceptance criteria:
  - local inventory loads in background
  - conflicts are detected and surfaced
  - UI thread remains responsive

## A4. GUI Read-Only Harbor Panel
- Size: M
- Area: thegrid-gui
- Deliverables:
  - HarborState and harbor view scaffold
  - table filters and selected-port detail panel
- Acceptance criteria:
  - inventory visible and filterable
  - selected row details update in real time

## 4. Milestone B (Safe Control Actions)
## B1. Add/Edit/Redirect Mapping Flow
- Size: L
- Area: core + runtime + gui
- Deliverables:
  - mapping dialog and validation
  - redirect flow with collision checks
  - audit write path
- Acceptance criteria:
  - mappings persist and apply after refresh
  - redirect blocks on unavailable target unless override policy allows

## B2. Restart/Stop Owner Actions
- Size: L
- Area: runtime + gui
- Deliverables:
  - guarded action handlers
  - confirmation and reason capture
- Acceptance criteria:
  - action requires capability and confirmation
  - action result and errors shown in timeline

## B3. Audit Timeline and Correlation IDs
- Size: M
- Area: gui + core db
- Deliverables:
  - timeline list with status and timestamps
  - correlation id per action
- Acceptance criteria:
  - actions are traceable end to end
  - failed actions include remediation hint

## 5. Milestone C (Remote Snapshot and Guarded Remote Control)
## C1. Remote Port Snapshot
- Size: M
- Area: net + runtime + gui
- Deliverables:
  - fetch and display remote inventory snapshots
  - stale/online indicators
- Acceptance criteria:
  - remote data clearly marked as snapshot and age-stamped

## C2. Guarded Remote Actions
- Size: L
- Area: net + runtime + gui
- Deliverables:
  - remote action command routing via capability checks
  - strict error handling and audit trail
- Acceptance criteria:
  - unauthorized action attempts denied with clear reason
  - audit trail includes remote target identity

## 6. Cross-Cutting Tasks
## X1. Policy Engine Baseline
- Size: M
- Deliverables:
  - protected-port/service policy rules
  - capability-based action gate checks
- Acceptance criteria:
  - policy denials deterministic and test-covered

## X2. Observability and Diagnostics
- Size: M
- Deliverables:
  - structured logs for refresh and action lifecycle
  - telemetry counters for success/failure/denial
- Acceptance criteria:
  - operators can diagnose failed actions from GUI timeline + logs

## X3. Stress and Safety Tests
- Size: M
- Deliverables:
  - concurrent refresh test
  - mapping collision tests
  - protected target action-denial tests
- Acceptance criteria:
  - no deadlocks or UI stalls under repeated refresh and actions

## 7. Suggested Delivery Order
1. A1 -> A2 -> A3 -> A4
2. B1 -> B3
3. B2
4. C1 -> C2
5. X1/X2/X3 continuously

## 8. Release Gates
Gate 1 (Foundation):
- read-only inventory and conflict visibility stable

Gate 2 (Local Control):
- add/edit/redirect and restart/stop with full audit and policy checks

Gate 3 (Remote Control):
- remote snapshots and guarded remote actions stable

## 9. Definition of Done for Backlog v1
- each milestone has owner-ready tasks
- acceptance criteria are testable and implementation-ready
- sequence supports parallel work with installer stream without blocking it
