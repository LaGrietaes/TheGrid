# Crush Session Day 0 Readiness

Date: 2026-03-29
Branch: main
Baseline tag: crush-day0-20260329
Baseline commit: 5f9f5b5

## Baseline Freeze
- Annotated tag created: `crush-day0-20260329`
- Rollback anchor: `git checkout crush-day0-20260329`

## Validation Results
- `cargo check --workspace`: PASS
- `cargo check -p thegrid-node`: PASS

## Day 0 Log Collection Paths
- Node logs folder: `crush-logs/day0/node/`
- Runtime logs folder: `crush-logs/day0/runtime/`
- Sync logs folder: `crush-logs/day0/sync/`
- Transfer logs folder: `crush-logs/day0/transfer/`

## Week 1 Daily Log Templates
- `crush-logs/week1/day1/log.md`
- `crush-logs/week1/day2/log.md`
- `crush-logs/week1/day3/log.md`
- `crush-logs/week1/day4/log.md`
- `crush-logs/week1/day5/log.md`
- `crush-logs/week1/day6/log.md`
- `crush-logs/week1/day7/log.md`

## Scope Drift Policy Reminder (Day 3)
Re-plan is mandatory when any of the following is true:
- More than 3 new requests in one day
- A request touches a new subsystem outside active chunk context
- Required validation/smoke checks fail

## Notes
- Local workspace contains one unrelated modified file: `thegrid-workspace/Cargo.lock`.
- This file was left untouched during Day 0 setup.
