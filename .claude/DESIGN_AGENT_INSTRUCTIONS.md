# Claude Design Agent Instructions - TheGrid Media Care

Status: Active
Audience: Design agent and designer team

## Mission
Design a production-ready Media Care Station for TheGrid that is directly compatible with current frontend and backend architecture.

## Non-Negotiable Scope
- Focus only on Media Care Station work for this repository.
- Do not redesign unrelated product sections.
- Produce outputs that can be implemented with current crates and event-driven threading model.

## Read Order (Mandatory)
1. MEDIA_CARE_MASTER_INDEX_V1.md
2. MEDIA_CARE_BLUEPRINT_V1.md
3. MEDIA_CARE_UI_WIREFRAME_V1.md
4. MEDIA_CARE_EXISTING_PROJECT_CONNECTION_V1.md
5. MEDIA_CARE_DEPENDENCY_MATRIX_V1.md
6. MEDIA_CARE_BACKLOG_V1.md

## Existing Architecture Constraints
- GUI: egui immediate-mode in thegrid-gui
- Runtime: worker-style spawn_* methods in thegrid-runtime
- Core coordination: AppEvent message bus in thegrid-core
- Persistence: SQLite via thegrid-core db helpers
- Rule: keep update loop non-blocking

## Required Design Output Contract
Every proposal must include:
- Frontend behavior description
- Backend event and data contract impact
- DB schema impact (if any)
- Failure and retry behavior
- Capability fallback behavior when tools are missing
- Incremental rollout plan aligned to Milestones A, B, C

## Guardrails
- Preserve current Media Ingest interactions (selection, rating, pick/reject, inline preview).
- Extend existing architecture; avoid introducing parallel frameworks.
- Keep Gyroflow integration external in v1.
- Prioritize additive changes over disruptive refactors.

## Delivery Format for Each Proposal
1. Decision summary
2. UI impact
3. Runtime impact
4. Core/AppEvent impact
5. DB impact
6. Risks and mitigations
7. Acceptance criteria

## Out of Scope for This Design Pass
- Full timeline editor replacement
- Non-media platform redesign
- Cloud-only rendering requirements

## Quality Bar
- Plug-and-play handoff to engineering
- Minimal ambiguity on file touchpoints
- Explicit compatibility with current TheGrid architecture
