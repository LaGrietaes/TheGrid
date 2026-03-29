# Day 1 Execution Card - S1-C1 Endpoint Capability Gating

Objective:
- Enforce capability checks server-side for sensitive agent endpoints.
- Return explicit 403 responses when capability is disabled.

Chunk ID:
- S1-C1

Context scope (load only these first):
- `thegrid-workspace/crates/thegrid-net/src/agent.rs`
- `thegrid-workspace/crates/thegrid-core/src/config.rs`
- `thegrid-workspace/crates/thegrid-core/src/events.rs` (only if needed)

Out of scope for this chunk:
- Node CLI expansion
- GUI visual changes
- DB schema changes unrelated to access gating

Implementation targets:
1. Define/confirm capability flags used by agent routes.
2. Gate sensitive routes (file ops, terminal, remote control paths).
3. Standardize 403 response body for disabled capability.
4. Keep behavior unchanged when capability is enabled.

Required validation:
- `cargo check -p thegrid-node`
- `cargo check --workspace`
- Manual route sanity checks for enabled/disabled behavior

Evidence to record in day1/log.md:
- Files changed
- Endpoints gated
- Validation outputs (pass/fail)
- Rollback note

Rollback checkpoint:
- `git checkout crush-day0-20260329 -- thegrid-workspace/crates/thegrid-net/src/agent.rs thegrid-workspace/crates/thegrid-core/src/config.rs`

Conversation boundary rule:
- Keep same conversation only for closely related endpoint gating follow-ups.
- If task moves into sync DB or runtime worker behavior, open a new conversation.
