# Week 1 - Day 2 Log

Date: 2026-03-29
Active chunks: S1-C2 (tombstone/delete conflict safety), S1-C3 (runtime worker/queue baseline)
Conversation IDs: current session

## Build/Validation
- cargo check --workspace: PASS
- cargo check -p thegrid-node: PASS
- Node smoke (help/devices/ping/history/update/quit): PARTIAL PASS (automated `help` + `quit` executed in forced TUI mode)

## Changes Delivered
- S1-C2 hardening: cross-platform tombstone conflict matching now checks path variants (Windows/Linux separators) when:
	- reading tombstone timestamps,
	- reading existing index timestamps,
	- applying remote tombstones (delete side).
- This closes a resurrection edge-case when remote path separators differ.
- S1-C3 verification (no code change needed):
	- hashing worker singleton guard confirmed (`hash_worker_running` AtomicBool),
	- queue dedupe confirmed (`UNIQUE(root_path, dir_path)` + `INSERT OR IGNORE` in runtime queueing),
	- skip-dir and fingerprint path confirmed in persistent queue processor.

## Issues Found
- Blocker: none
- High: none
- Medium: node smoke run showed `DB open failed (...) using in-memory fallback` in this local run path; needs follow-up under S3 verification to confirm environment-only vs reproducible bug
- Low: smoke was partial (help+quit only), full manual sequence pending

## Scope Drift Check
- New requests today: 0
- Cross-subsystem change?: no
- Re-plan triggered?: no

## Rollback Notes
- Rollback Day 2 code delta:
	- `git checkout d9dad3b -- thegrid-workspace/crates/thegrid-core/src/db.rs`

## Next Day Start Point
- Day 3 start with S2-C1 (node command registry scaffolding), then run Day-3 scope calibration event.
