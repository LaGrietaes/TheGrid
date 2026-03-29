# Week 1 - Day 1 Log

Date: 2026-03-29
Active chunks: S1-C1 (Endpoint capability gating)
Conversation IDs: current session

## Build/Validation
- cargo check --workspace: PASS
- cargo check -p thegrid-node: PASS
- Node smoke (help/devices/ping/history/update/quit): NOT RUN (interactive/manual; queued for Day 2 manual pass)

## Changes Delivered
- Added explicit capability flags in config: `enable_terminal_access`, `enable_ai_access`, `enable_remote_control`.
- Added centralized capability gate helpers in agent server (`capability_enabled`, standardized 403 capability response).
- Applied capability gating to sensitive endpoints:
	- Remote control: `/v1/config`, `/adb/enable`, `/clipboard`, `/v1/rdp/enable`
	- File access: `/v1/sync`, `/filelist`, `/files/*`, `/upload`, `/v1/browse`, `/v1/read`, `/v1/preview`, `/v1/files*`
	- Terminal: `/v1/terminal/session`, `/v1/terminal/input`, `/v1/terminal/output`
	- AI: `/v1/ai/embed`, `/v1/ai/search`

## Issues Found
- Blocker: none
- High: none
- Medium: none
- Low: minor compile fix during implementation (Request ownership in forbidden helper), resolved same session

## Scope Drift Check
- New requests today: 0 (within active chunk)
- Cross-subsystem change?: no
- Re-plan triggered?: no

## Rollback Notes
- Rollback this chunk only:
	- `git checkout crush-day0-20260329 -- thegrid-workspace/crates/thegrid-core/src/config.rs`
	- `git checkout crush-day0-20260329 -- thegrid-workspace/crates/thegrid-net/src/agent.rs`

## Next Day Start Point
- S1-C2: tombstone/delete conflict safety in `thegrid-core` sync/DB paths.
