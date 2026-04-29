# TheGrid H4RB0R Port Control Suite v1

Status: Active (Future Update Track)
Date: 2026-04-30

## Purpose
Single entry point for planning and implementation of H4RB0R, the in-app port registry and control system for TheGrid.

H4RB0R goals:
- make all active and reserved ports visible
- show ownership (service, process, profile, device role)
- allow controlled actions (refresh, restart, stop, add, edit, redirect)
- keep networking behavior auditable and recoverable

## Document Set
1. H4RB0R_BLUEPRINT_V1.md
- product and architecture blueprint
- functional/non-functional requirements
- data contracts and security model

2. H4RB0R_UI_WIREFRAME_V1.md
- screen topology and interaction schematic
- desktop and compact layout behavior
- command routing map (UI to runtime)

3. H4RB0R_EXISTING_PROJECT_CONNECTION_V1.md
- exact integration into current crates and scripts
- file-level touchpoints and implementation order
- compatibility with current style and runtime patterns

4. H4RB0R_BACKLOG_V1.md
- phased backlog with estimates
- acceptance criteria and release gates
- implementation sequencing for low-risk delivery

5. H4RB0R_FRONTEND_AGENT_README.md
- frontend execution brief for coding agents
- behavior, quality bar, and implementation rules
- interaction, layout, and validation expectations

## Working Agreement
For any H4RB0R scope change:
1. update impacted H4RB0R doc
2. add dated change note
3. map crate/file impact in connection guide
4. keep backlog acceptance criteria synchronized

## Versioning Rules
- Major structural changes: create v2 files
- Minor updates: append dated change notes in affected file
- Implementation-only updates: update backlog and connection guide first

## Next Execution Gate
- Execute Milestone A from H4RB0R_BACKLOG_V1.md
- Implement in order: core contracts -> db -> runtime handlers -> gui panel
