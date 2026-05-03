---
description: Courier agent — chunked resumable large-file transfers (VerteX protocol)
turbo-all: true
ai_policy: local_only
---

## Purpose
Courier handles large-file transfers between TheGrid nodes using the VerteX protocol.  
VerteX is a simple HTTP chunked upload with offset headers, supporting resume after interruption.  
**No AI involved.** Pure transport logic.

## Protocol (VerteX v1)
```
PUT /vertex/upload
Headers:
  X-VerteX-TransferId  : <uuid>
  X-VerteX-FileName    : <name>
  X-VerteX-Offset      : <byte_offset>
  X-VerteX-Total       : <total_bytes>
Body: raw chunk bytes (default 4 MiB)
```

Receiver responds:
- `200 OK` with `{"received": <offset + chunk_len>}` on success
- `409 Conflict` with `{"expected_offset": <n>}` if chunk is out of order (sender retries from n)

## Responsibilities
1. **Send** — chunk file into 4 MiB pieces, upload sequentially with resume on failure.
2. **Receive** — `AgentServer` handles `/vertex/upload`, writes chunks to staging dir, emits `FileReceived` on completion.
3. **Progress** — emit `CourierProgress` every chunk for real-time GUI bar.
4. **Retry** — on network error, back off 2s and retry same chunk up to 3 times before failing.

## Trigger Events (incoming)
| Event | Action |
|---|---|
| `AppEvent::FileReceived` from drag-drop (>50 MB) | Auto-route through Courier instead of simple upload |
| User selects "Send via VerteX" in file browser | Call `Courier::send()` |

## Emitted Events
- `AppEvent::CourierProgress { transfer_id, file_name, bytes_done, bytes_total, peer_device }`
- `AppEvent::CourierComplete { transfer_id, file_name, peer_device }`
- `AppEvent::CourierFailed { transfer_id, error }`
- `AppEvent::AgentAlert { agent: "courier", level: "warn" }` on retry

## Implementation Location
- Agent struct: `thegrid-workspace/crates/thegrid-ai/src/agents.rs` → `Courier`
- Receiver endpoint: add `/vertex/upload` to `thegrid-net/src/agent.rs` → `AgentServer`
- GUI progress: render `CourierProgress` in the transfers panel in `thegrid-gui/src/views/`

## AgentServer Receiver (to implement)
Add to `AgentServer::handle_request()` in `agent.rs`:
```rust
("/vertex/upload", Method::PUT) => self.handle_vertex_upload(req),
```
`handle_vertex_upload` writes chunk at reported offset, assembles file, emits `FileReceived`.

## Acceptance Criteria
- [ ] 1 GB file transfers successfully over Tailscale mesh between two nodes
- [ ] Transfer survives a 5s network dropout and resumes from last offset
- [ ] GUI shows live progress bar (% and MB/s)
- [ ] `cargo check --workspace` passes after any change
