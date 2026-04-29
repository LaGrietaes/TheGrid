# TheGrid H4RB0R Port Control Blueprint v1

Status: Draft v1 (Foundation)
Date: 2026-04-30
Owners: TheGrid GUI, Core, Runtime, Node Ops
Purpose: Define a safe, observable, and controllable in-app port management system for future release.

## 1. Product Vision
H4RB0R is TheGrid's unified control surface for network ports.
It must answer, in one panel:
- what ports are active, reserved, conflicting, or unreachable
- why each port exists and which service owns it
- how to safely refresh, restart, stop, add, edit, or redirect ports

H4RB0R is operations-first: avoid silent network behavior and provide deterministic, auditable actions.

## 2. Primary Outcomes
- Operator can see local and remote port inventory with ownership metadata.
- Operator can perform controlled lifecycle actions without leaving TheGrid.
- Port conflicts are detected early with actionable remediation.
- Changes are persisted, logged, and reversible.

## 3. Scope (v1 Foundation)
### In Scope
- Port registry model for local node and optional remote node snapshots.
- Live status polling plus manual refresh.
- Actions: restart listener/service, stop listener/service, add mapping, edit mapping, redirect mapping.
- Policy gates by role/capability and risk level.
- Event and telemetry integration for status/progress/failure.

### Out of Scope (v1)
- Full firewall rules editor replacement.
- Arbitrary shell command execution from GUI.
- Auto-remediation without operator confirmation.

## 4. Functional Requirements
### FR1. Port Inventory
System must expose, per entry:
- bind address (host/ip)
- protocol (tcp/udp)
- port
- owner kind (TheGrid service, external process, reserved rule)
- owner identity (service key or process id/name when available)
- state (up, down, conflicted, unknown)
- source (config, runtime scan, remote telemetry)

### FR2. Action Set
Supported actions:
- refresh inventory
- restart owner service
- stop owner service (where permitted)
- add mapping rule
- edit mapping rule
- redirect mapping (old -> new)

Action behavior rules:
- destructive actions require confirmation with impact summary
- redirect checks availability before commit
- all actions emit start/progress/result events

### FR3. Configuration and Persistence
- Port mappings and reservations persist in local config/DB.
- Runtime-discovered entries are non-authoritative overlays unless promoted.
- Changes include who/when/why metadata for audit.

### FR4. Conflict Management
System detects:
- duplicate bindings (same host/protocol/port)
- privileged/blocked ports based on policy
- unavailable target on redirect

Conflict flow:
1. detect
2. classify severity
3. present remediation options
4. require explicit operator choice

### FR5. Remote Node Support (Phased)
- v1: read-only remote snapshots with local action routing only for trusted endpoints
- v1.1+: guarded remote actions through agent capability checks

## 5. Non-Functional Requirements
- GUI update loop must remain non-blocking.
- Polling and probes run in background workers.
- Port list update target: less than 2s for typical node inventory.
- Action latency feedback: progress visible within 250ms after trigger.
- All failures are operator-readable and include next-step hints.

## 6. Security and Policy Model
### Capability Gates
Required capability tags:
- ports.read
- ports.control.restart
- ports.control.stop
- ports.control.redirect
- ports.control.write

### Safety Rules
- No action without capability verification.
- Stop/restart/redirect operations require explicit confirmation.
- Protected service keys (core sync, agent API, control plane) require elevated confirmation.
- Audit log is append-only for action history.

## 7. Architecture Blueprint
## 7.1 Core (thegrid-core)
Add domain models:
- PortEntry
- PortOwner
- PortState
- PortActionRequest
- PortActionResult
- PortConflict
- PortPolicyDecision

Add AppEvent variants for lifecycle and telemetry wiring.

## 7.2 Database (thegrid-core db)
Add tables:
- port_registry
  - id, scope, host, protocol, port, owner_kind, owner_key, state, source, updated_at
- port_mappings
  - id, name, from_host, from_protocol, from_port, to_host, to_protocol, to_port, enabled, updated_at
- port_action_audit
  - id, action_type, target_ref, actor, reason, result, details_json, created_at
- port_conflicts
  - id, host, protocol, port, severity, reason, detected_at, resolved_at

Indexes:
- port_registry(scope, protocol, port)
- port_mappings(enabled, from_protocol, from_port)
- port_action_audit(created_at desc)

## 7.3 Runtime (thegrid-runtime)
Add worker entry points:
- spawn_port_inventory_refresh
- spawn_port_action_apply
- spawn_port_conflict_scan
- spawn_port_remote_snapshot

Execution policy:
- all IO/probes/actions off UI thread
- bounded concurrency for probes
- single-flight per identical action target

## 7.4 GUI (thegrid-gui)
Add H4RB0R panel with 4 zones:
- inventory table
- details/policy panel
- action bar
- audit timeline

State container:
- HarborState
  - filters
  - selected entry
  - pending action
  - latest policy checks
  - audit preview

## 8. Event Contract (Initial)
Add AppEvent variants:
- PortInventoryLoaded { scope, entries }
- PortInventoryFailed { scope, error }
- PortConflictDetected { conflicts }
- PortActionStarted { action_id, target }
- PortActionProgress { action_id, step, message }
- PortActionCompleted { action_id, result }
- PortActionFailed { action_id, error }

## 9. API/Command Contract (Runtime-facing)
Commands:
- HarborListPorts { scope, filters }
- HarborRefreshPorts { scope }
- HarborAddMapping { mapping }
- HarborEditMapping { mapping_id, patch }
- HarborRedirectPort { mapping_id or inline_target }
- HarborRestartOwner { target }
- HarborStopOwner { target }

Each command returns:
- correlation_id
- accepted/rejected with policy reason
- async completion events

## 10. Style and Implementation Rules (Must Follow Current Project Style)
- Keep additive changes first; avoid replacing stable paths in one pass.
- Reuse existing spawn_* runtime pattern and AppEvent transport.
- Keep egui update non-blocking; no direct network/probe in render paths.
- Use clear operator language (status + next action), matching existing operational UX tone.
- Prefer data-backed state over ad-hoc flags.
- Preserve current crate boundaries (core contracts, runtime execution, gui rendering).

## 11. Risks and Controls
- Risk: accidental interruption of critical services.
  - Control: protected-service policy and elevated confirmation.
- Risk: stale inventory causing wrong decisions.
  - Control: timestamped snapshot + explicit refresh confidence state.
- Risk: remote action abuse.
  - Control: capability gating and action audit.

## 12. Increment Plan
- Milestone A: core contracts + db schema + inventory refresh (read-first)
- Milestone B: safe local actions (restart/stop/redirect/add/edit) + audit timeline
- Milestone C: remote snapshots and guarded remote control

## 13. Definition of Done for Blueprint v1
- Functional scope and boundaries approved.
- Event and command contract approved.
- Data schema and policy model approved.
- Implementation sequence approved by GUI, runtime, and node owners.
