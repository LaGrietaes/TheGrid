---
description: Node CLI expansion — one command group per PR, never bulk rewrites
turbo-all: true
ai_policy: local_only
---

## Rule (Non-Negotiable)
**One command group per change. Never bulk-rewrite the parser.**  
Each expansion must pass the TUI smoke test before merge.  
See `THEGRID_CONTINUATION.md` → "Blueprint rule going forward".

## TUI Smoke Test (run after every node CLI change)
```bash
cargo build -p thegrid-node
echo -e "help\ndevices\nping 0\nhistory\nupdate\nquit\n" | ./target/debug/thegrid-node --plain
```
All commands must produce clean output, no panics, no render noise.

## Expansion Queue (ordered by value)

### Group 1 — Mesh Status (next up)
Commands: `mesh status`, `mesh sync <all|node>`  
Backend: `GET /v1/sync/status` on each peer via `AgentClient`  
Files: `thegrid-node/src/main.rs` → add `CmdMesh` handler  
Gate: mesh status returns reachable node count and last sync age

### Group 2 — Node Selection
Commands: `node select <index>`, `node show`  
Backend: in-memory active-target state in CLI  
Files: `thegrid-node/src/main.rs`  
Gate: subsequent `ping`, `files`, `clip` commands default to selected node

### Group 3 — File Ops
Commands: `files list [node] [path]`, `files pull <node> <path>`, `files push <node> <path>`  
Backend: `AgentClient::list_directory`, `download_file`, `upload_file`  
Files: `thegrid-node/src/main.rs`  
Gate: round-trip a 1 MB file to/from a live node

### Group 4 — Clipboard
Commands: `clip send <node> <text>`, `clip get <node>`  
Backend: `AgentClient::send_clipboard`, `get_clipboard`  
Files: `thegrid-node/src/main.rs`

### Group 5 — Health / Observability
Commands: `health`, `logs tail [component]`  
Backend: `GET /v1/capabilities`, local log buffer  
Files: `thegrid-node/src/main.rs`

### Group 6 — Config
Commands: `config show`, `config set <key> <value>`  
Backend: read/write `config.json` via `Config::save()`  
Files: `thegrid-node/src/main.rs`  
Gate: `config set ai_policy local_only` persists and takes effect on next run

### Group 7 — AI / Semantic (after Phase 4 wiring complete)
Commands: `ai status`, `ai search <query> [k]`, `ai embed <text>`  
Backend: call local `SemanticSearch` or remote AI endpoint  
Files: `thegrid-node/src/main.rs`

### Group 8 — Device Lifecycle
Commands: `wol <node>`, `rdp enable <node>`, `adb enable <node>`  
Backend: existing `WolSentry`, RDP/ADB endpoints  
Files: `thegrid-node/src/main.rs`

## Implementation Pattern
```rust
// In main.rs command dispatch — add one arm at a time:
"mesh" => handle_cmd_mesh(&args, &state).await,
```
Each handler is a separate `fn handle_cmd_<group>()` — never inline complex logic in the match arm.

## Rollback Checkpoint
Before each group: commit the current working state as a checkpoint commit.  
Message format: `node-cli: checkpoint before <group_name> expansion`

## Acceptance Criteria (per group)
- [ ] All existing smoke test commands still pass
- [ ] New commands produce correct output on happy path
- [ ] New commands fail gracefully (no panic) on bad input / offline peer
- [ ] `cargo check -p thegrid-node` passes
