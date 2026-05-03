---
description: Sentinel agent — security stance management and anomaly detection
turbo-all: true
ai_policy: local_only
---

## Purpose
Sentinel is the always-on security gatekeeper for TheGrid.  
It manages `SecurityStance` transitions, AFK locking, and pre-flight checks before destructive operations.  
**Never calls cloud AI.** Heuristics and pattern matching only.

## Responsibilities
1. **AFK lock** — transition to `AfkTacticalLock` after 10 min idle, purge in-RAM keys.
2. **HVT gate** — transition to `HighValueTarget` when operator initiates destructive action (delete, push, config wipe). Require explicit re-confirmation.
3. **Anomaly detection** — flag unusual file-delete spikes, repeated auth failures, unexpected mesh join events.
4. **Stance restoration** — return to `Active` on confirmed user interaction.

## Trigger Events (incoming)
| Event | Action |
|---|---|
| `UserIdle(true)` | Transition to `AfkTacticalLock` |
| `UserIdle(false)` | Transition to `Active` |
| `DeleteFiles` | Emit `SecurityStanceChanged(HighValueTarget{...})`, block until confirmed |
| `AgentPingFailed` (repeated) | Emit `AgentAlert { level: "warn" }` |

## Emitted Events
- `AppEvent::SecurityStanceChanged(SecurityStance)`
- `AppEvent::AgentAlert { agent: "sentinel", level, message }`

## GUI Integration
- Sentinel stance drives the global color tint in `thegrid-gui/src/theme.rs`.
  - `Active` → green tint (normal)
  - `AfkTacticalLock` → amber tint + blur overlay
  - `HighValueTarget` → red tint + confirmation dialog

## Implementation Location
- Agent struct: `thegrid-workspace/crates/thegrid-ai/src/agents.rs` → `Sentinel`
- Spawn point: `thegrid-workspace/crates/thegrid-runtime/src/runtime.rs` → `AppRuntime::start()`
- Event handler: `thegrid-workspace/crates/thegrid-gui/src/app.rs` → `AppEvent::SecurityStanceChanged`

## AI Policy
- **local_only** — no LLM calls, only rule-based heuristics.
- Future: may use local Ollama to classify anomaly severity from event log summaries.

## Acceptance Criteria
- [ ] AFK lock fires after 10 min idle, GUI shows amber tint
- [ ] HVT gate shows red tint + blocks destructive action pending confirmation
- [ ] Repeated `AgentPingFailed` (≥3 in 5 min) emits `AgentAlert { level: "warn" }`
- [ ] `cargo check --workspace` passes after any change
