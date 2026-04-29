# H4RB0R Frontend Agent README

Status: Active Brief
Date: 2026-04-30
Audience: Coding agents and implementers working on the H4RB0R frontend inside TheGrid GUI

## 1. Mission
Build the H4RB0R frontend like an operator-grade control surface, not a demo panel.

The screen must help a real user answer three things fast:
- what is happening on this node right now
- what is safe to change
- what action should be taken next

## 2. Behavior Rules for the Agent
- Work in small, reviewable slices.
- Keep the UI responsive at all times.
- Prefer read-first implementation before control actions.
- Reuse existing TheGrid patterns before introducing new abstractions.
- Never hide risk: if an action is destructive, surface impact clearly.
- Use explicit states instead of vague UI language.

## 3. Frontend Quality Bar
The H4RB0R panel should feel:
- operational
- legible under stress
- compact but not crowded
- professional on desktop and usable on tablet widths

Avoid:
- generic dashboard filler
- decorative complexity without information value
- blocking calls in render paths
- unclear button labels like "Run" or "Apply" without context

Prefer labels like:
- Refresh Inventory
- Restart Owner
- Stop Listener
- Redirect Port
- Resolve Conflict

## 4. Design Rules to Match Current TheGrid Style
- Follow the existing brutalist/futuristic operational tone already present in TheGrid.
- Prioritize dense but readable information layout.
- Use status colors with text labels, never color alone.
- Keep panels sharp and intentional; avoid soft consumer-app styling.
- Keep interaction vocabulary consistent with dashboard and media views.

## 5. Layout Rules
The H4RB0R screen is a 4-zone workspace:
- Inventory grid is primary.
- Detail and policy inspector explains current selection.
- Action composer exposes only valid actions.
- Audit timeline proves what happened.

Implementation rule:
- if space is tight, preserve the inventory first
- collapse secondary panels into tabs or drawers before shrinking the table into noise

## 6. Interaction Rules
- Single click selects a port row.
- Double click opens deeper detail focus.
- Disabled actions must show the reason.
- Dangerous actions require confirmation with impact text.
- Conflict states must offer a next action, not only an error.

## 7. Engineering Rules
- No networking, probing, or DB work directly in egui render functions.
- All refreshes and actions go through runtime workers and AppEvent messages.
- UI state should be data-backed and serializable where practical.
- Prefer additive state structs over scattered booleans.
- Every async action must map to visible started/progress/completed/failed states.

## 8. Definition of Professional Frontend Completion
Do not consider the H4RB0R frontend done unless all are true:
- inventory is readable and filterable
- selected-port detail is clear
- action availability matches policy
- failure states are actionable
- audit history is visible
- compact mode remains usable
- no frame-time hitching during refresh

## 9. Recommended Delivery Order
1. Read-only inventory table and filters
2. Selected-port detail and policy panel
3. Audit timeline scaffold
4. Add/edit/redirect mapping dialogs
5. Restart/stop guarded actions
6. Compact-mode refinement and validation

## 10. Validation Checklist
Before calling the frontend slice complete:
- run cargo check for the touched crate(s)
- verify no blocking behavior during repeated refresh
- verify disabled actions explain why
- verify conflict and failure copy is understandable without source code context
- verify layout remains usable at reduced width

## 11. Agent Operating Stance
Act like a senior frontend engineer embedded in an ops product:
- reduce ambiguity
- prefer clarity over cleverness
- expose system truth, not optimistic guesses
- keep every control accountable to feedback and audit