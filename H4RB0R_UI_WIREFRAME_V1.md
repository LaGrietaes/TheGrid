# TheGrid H4RB0R UI Wireframe v1

Status: Draft v1
Date: 2026-04-30
Depends on: H4RB0R_BLUEPRINT_V1.md

## 1. Goal
Define H4RB0R panel layout, interactions, and UI-to-runtime contracts for predictable implementation.

## 2. Screen Topology
Primary H4RB0R workspace uses 4 zones:
- Zone A: Port Inventory Grid
- Zone B: Port Detail and Policy Inspector
- Zone C: Action Composer
- Zone D: Audit and Event Timeline

Overlays:
- O1: Confirm Action Dialog
- O2: Conflict Resolver Dialog
- O3: Add/Edit Mapping Dialog

## 3. Desktop Wireframe (Reference)

```text
+--------------------------------------------------------------------------------------------------+
| H4RB0R | Scope: Local/Remote | Search | Filters | Last Refresh | Refresh | Health              |
+--------------------------------------------+--------------------------------+--------------------+
| A: PORT INVENTORY                           | B: DETAIL + POLICY INSPECTOR   | C: ACTION COMPOSER |
| - Host/Protocol/Port                        | - Owner identity and role       | - Restart          |
| - State badge                               | - Capability check result       | - Stop             |
| - Source (config/runtime/remote)            | - Conflict list and severity    | - Redirect         |
| - Owner (service/process/rule)              | - Impact preview                | - Add Mapping      |
| - Fast status filter chips                  | - Recommended next action       | - Edit Mapping     |
+--------------------------------------------+--------------------------------+--------------------+
| D: TIMELINE (Started/Completed/Failed/Audit Notes)                                          |
+--------------------------------------------------------------------------------------------------+
```

## 4. Compact Layout Rules
When width is constrained:
- Zone A remains primary.
- Zone B and Zone C collapse into right-side tab sheet.
- Zone D stays as bottom collapsible strip with latest N events.
- Confirm dialogs stay modal and concise.

## 5. Zone-Level Specification
## 5.1 Zone A: Port Inventory Grid
Columns:
- state
- host
- protocol
- port
- owner
- source
- updated_at

Interactions:
- single click selects row
- double click opens detail focus in Zone B
- multi-filter chips (state, protocol, scope, owner kind)
- inline quick refresh for selected row

Outputs:
- selected_port_ref
- active_filters

## 5.2 Zone B: Detail and Policy Inspector
Displays:
- canonical identity of selected entry
- current policy gates for requested actions
- conflict diagnostics
- impact summary for pending action

Outputs:
- policy_decision
- conflict_context

## 5.3 Zone C: Action Composer
Actions:
- refresh selected
- restart owner
- stop owner
- redirect port
- add mapping
- edit mapping

Rules:
- disabled buttons show reason
- destructive operations open O1 confirmation
- redirect/add/edit use structured forms

Outputs:
- action_request

## 5.4 Zone D: Audit Timeline
Displays:
- action start/progress/result
- policy denials
- conflict detections/resolutions

Rules:
- newest first
- each entry includes timestamp and correlation id
- failed entries include one actionable remediation line

## 6. Overlay Contracts
## 6.1 O1 Confirm Action Dialog
Required fields:
- target summary
- impact summary
- reason input
- confirmation text for critical services

## 6.2 O2 Conflict Resolver Dialog
Shows:
- conflicting entries
- severity and reason
- options (redirect, stop non-critical owner, cancel)

## 6.3 O3 Add/Edit Mapping Dialog
Fields:
- mapping name
- from host/protocol/port
- to host/protocol/port
- enabled toggle
- optional note

Validation:
- required fields
- protocol and port range validation
- collision check before save

## 7. Interaction Sequences
## 7.1 Quick Conflict Fix (Redirect)
1. Conflict detected in Zone A.
2. Operator opens O2.
3. Operator selects redirect strategy.
4. O1 confirmation appears.
5. Action emitted and tracked in Zone D.
6. Zone A refreshes with new mapping state.

## 7.2 Safe Restart
1. Operator selects entry in Zone A.
2. Zone B confirms capability and impact.
3. Operator clicks restart in Zone C.
4. O1 confirmation required for protected services.
5. Timeline shows progress and completion.

## 8. Telemetry Hooks
Emit:
- harbor.inventory.refresh
- harbor.action.started
- harbor.action.completed
- harbor.action.failed
- harbor.conflict.detected
- harbor.policy.denied

## 9. UI to Runtime Mapping
- click refresh -> HarborRefreshPorts
- click restart -> HarborRestartOwner
- click stop -> HarborStopOwner
- submit redirect -> HarborRedirectPort
- submit add mapping -> HarborAddMapping
- submit edit mapping -> HarborEditMapping

## 10. Definition of Done
- all core actions reachable from panel
- policy and confirmation behaviors are consistent
- audit timeline provides enough context to diagnose failures
- compact mode remains usable for tablet-class width
