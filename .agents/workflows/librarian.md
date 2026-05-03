---
description: Librarian agent — distributed semantic search across the mesh
turbo-all: true
ai_policy: local_first
---

## Purpose
Librarian coordinates semantic search across all reachable TheGrid nodes.  
It fans out a query to peer agents, collects `(file_id, score)` pairs, merges and re-ranks results.  
Uses local Ollama for query expansion if available; degrades gracefully without it.

## Responsibilities
1. **Query expansion** — optionally expand terse queries using local inference before embedding.
2. **Fan-out search** — call `/v1/ai/search` on each reachable peer via `AgentClient`.
3. **Result merge** — merge peer results with local index results, re-rank by score.
4. **Emit results** — fire `AppEvent::LibrarianSearchResult` for GUI to render.

## Trigger Events (incoming)
| Event | Action |
|---|---|
| `AppEvent::SearchResults` (from FTS5) | Also trigger semantic fan-out in parallel |
| `AppEvent::RequestIdleWork` | Pre-embed any unembedded files in local index |
| Peer reconnect | Refresh peer's index slice into local memory |

## Emitted Events
- `AppEvent::LibrarianSearchResult { query, results, peer_results }`
- `AppEvent::AgentAlert { agent: "librarian", level: "warn", message }` on peer failure

## Implementation Location
- Agent struct: `thegrid-workspace/crates/thegrid-ai/src/agents.rs` → `Librarian`
- Peer search trait: `agents.rs` → `PeerSearchClient`
- Wire `AgentClient` as `PeerSearchClient` impl in `thegrid-net/src/agent.rs`
- Spawn point: `thegrid-runtime/src/runtime.rs`

## AI Provider Priority
1. **Local Ollama** for query expansion (default).
2. **Gemini Flash** if Ollama unavailable and policy = "local_first".
3. **Claude** — never for routine search; only if explicitly enabled.
4. **No inference** — fall back to raw query if all providers fail.

## Vector Index Sync Protocol
1. On node connect: `GET /v1/ai/index_stats` → confirm compatible dims.
2. On idle: `GET /v1/ai/embeddings?after=<ts>` to pull new vectors.
3. Store in local `VectorIndex` partition tagged by device_id.

## Acceptance Criteria
- [ ] Fan-out reaches all online peers (verified by mesh smoke test)
- [ ] Results merge correctly (no duplicate file_ids across nodes)
- [ ] Query expansion runs locally; never calls cloud under `local_only` policy
- [ ] Peer failure is logged as `AgentAlert`, search still returns local results
- [ ] `cargo check --workspace` passes after any change
