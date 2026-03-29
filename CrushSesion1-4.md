# THE GRID - CRUSH SESSION 1-4

Purpose:
- Deliver fast without breaking stability.
- Build in the most efficient order: base first, scaffolding second, validation third, polish + beta fourth.

Success criteria for this crush block:
- Core stability and security are verified before adding risky features.
- All new structures needed for upcoming functionality are in place.
- UX and behavior match intended outcomes in both node and GUI flows.
- One-week local closed beta runs with logs and clear go/no-go release decision.

---

## Session 1 - Solid Base (Stability + Safety)

Goal:
- Freeze the foundation so future speed does not create regressions.

Scope:
1. Security and capability gating
- Enforce capability checks server-side for sensitive endpoints.
- Ensure unauthorized actions return explicit 403 + reason.

2. Data integrity and sync safety
- Validate tombstone/delete conflict behavior.
- Verify no stale resurrection after reconnect/sync replay.

3. Runtime baseline
- Confirm single-worker behavior for hashing/index paths.
- Confirm watch queue handling does not flood under burst events.

Required validation:
- cargo check --workspace
- cargo check -p thegrid-node
- Node TUI smoke: help, devices, ping, history, update, quit
- Sync safety checks: delete -> sync -> reconnect -> no resurrection

Exit criteria:
- No critical correctness bugs open.
- No security bypass in endpoint gating.
- Baseline considered stable for structure build-out.

---

## Session 2 - Build Structures for Upcoming Code

Goal:
- Create extension points and internal structures needed to scale features quickly.

Scope:
1. Command and execution structure
- Introduce a command registry approach for node CLI (one command group at a time).
- Keep parser logic and rendering logic separated.

2. Observability structure
- Add/prepare health model objects for:
  - sync age
  - tombstone count
  - sync failures
  - detection source distribution

3. Feature scaffolding
- Add interfaces/hooks for upcoming command groups (mesh, files, clipboard) without shipping all behavior in one shot.
- Reuse existing runtime/event APIs before creating new abstractions.

Required validation:
- Build checks pass.
- Existing command behavior unchanged.
- New structures are additive and backward-safe.

Exit criteria:
- Structure exists to ship future features in small slices.
- No behavior regressions introduced.

---

## Session 3 - Verify Intent (Works as Intended + Looks as Intended)

Goal:
- Ensure current implementation quality before final polish.

Scope:
1. Functional verification
- End-to-end checks for node core flows (commands + sync + transfers).
- GUI checks for state visibility and expected interactions.

2. Visual and UX verification
- Confirm UI states are understandable and operationally useful.
- Confirm node TUI remains stable and readable under live updates.

3. Reliability tests
- Rename storm test in watched trees.
- Recursive directory move test.
- Offline/online reconnect with delayed sync replay.
- High file-count indexing with queue pressure.

Required validation:
- All required smoke tests pass.
- No major UX confusion points remain unresolved.
- Performance remains acceptable under stress scenarios.

Exit criteria:
- Product behaves as intended.
- Product presentation/UX matches intended quality bar.

---

## Session 4 - Final Details + Local One-Week Closed Beta

Goal:
- Add final polish, run local beta, gather evidence, and decide release readiness.

Scope:
1. Final details
- Minor quality fixes and polish only.
- No major architecture changes during beta window.

2. Closed beta execution (7 days)
- Run local production-like usage every day.
- Keep structured logs for node/runtime/sync/transfer/AI paths.

3. Logging and review cadence
- Daily review:
  - critical errors
  - repeated warnings
  - sync anomalies
  - usability pain points
- Categorize findings by severity: block, high, medium, low.

4. Beta closeout
- Compile final issue list.
- Fix blockers and highs.
- Tag candidate as closed-beta build.

Required validation:
- Daily build health checks remain green.
- No unresolved blocker by end of week.
- Release checklist complete.

Exit criteria:
- Closed beta version approved with evidence.
- Go/No-Go decision documented.

---

## Execution Rules During Crush

1. One risk domain per change.
2. One CLI command group per change.
3. Always include rollback note in change summary.
4. Prefer smallest patches and avoid unrelated formatting churn.
5. Run focused checks first, full workspace checks at integration milestones.

---

## Initial Week Execution Plan (Ready to Run)

Day 0 (Setup - 60 to 90 min)
- Freeze current baseline and define starting branch/checkpoint.
- Confirm validation commands are working locally.
- Confirm log collection paths for node/runtime/sync/transfer.

Day 1 (Foundation Sprint A)
- Run S1-C1: endpoint capability gating.
- Run focused validations and record evidence.
- Stop if any blocker appears; fix before moving to Day 2.

Day 2 (Foundation Sprint B)
- Run S1-C2 and S1-C3: sync safety + runtime queue/worker baseline.
- Re-run node smoke sequence and workspace check.
- Capture known risks and rollback notes.

Day 3 (Check + Update Event)
- Run S2-C1 and start S2-C2 only if Day 1-2 are stable.
- Execute a formal scope calibration event at end of day.

Day 3 scope calibration rules:
- Count incoming tweaks/additions from users and internal findings.
- If tweak volume is low and same-context: continue current conversation and finish S2-C2.
- If tweak volume is high or cross-area: open a new conversation and re-slice chunks before continuing.
- If any blocker touches security/sync correctness: pause new feature work and return to Session 1 fixes.

Scope drift trigger (mandatory re-plan):
- More than 3 new requests in one day, or
- Any request touching a new subsystem not in the active chunk, or
- Validation failure in required smoke/build checks.

Day 4 (Intent Verification)
- Run S3-C1 and S3-C2 with explicit pass/fail checklist.
- Only proceed if behavior and UX match intended baseline.

Day 5 (Stress Verification)
- Run S3-C3 stress pack.
- Triage findings by severity and create fix list.

Day 6 (Polish Gate)
- Run S4-C1 for low-risk fixes only.
- Re-validate full baseline and node smoke.

Day 7 (Beta Week Launch)
- Start S4-C2 day-1 closed beta loop with structured logging.
- Publish a daily beta note with: blockers, highs, and next-day focus.

Post Week-1 continuation:
- Keep the closed beta daily loop for 6 more days.
- End with S4-C3 closeout and go/no-go release decision.

---

## Chunked Execution Plan (Context-Window Optimized)

Use small chunks so Gemini 3 Flash (or similar) can execute fast with low token use.

Model strategy:
- Use one conversation for chunks that share the same files and subsystem context.
- Start a new conversation when moving to a different subsystem or after a large context shift.
- Keep each chunk scoped to one deliverable and one validation gate.

### Session 1 Chunks (Solid Base)

S1-C1: Endpoint capability gating
- Area: `thegrid-net` + config capability checks.
- Expected duration: 1 focused run.
- Validation: endpoint auth/capability checks + build.
- Conversation rule: Keep same conversation for S1-C2 only if same endpoint files are touched.

S1-C2: Tombstone/delete conflict safety
- Area: `thegrid-core` sync and DB conflict handling.
- Expected duration: 1 focused run.
- Validation: delete -> sync -> reconnect safety scenario.
- Conversation rule: New conversation recommended if S1-C1 context was mostly network handlers.

S1-C3: Runtime worker/queue baseline
- Area: `thegrid-runtime` worker lifecycle and queue pressure.
- Expected duration: 1 focused run.
- Validation: no duplicate workers, queue burst stability.
- Conversation rule: New conversation (runtime context differs from DB/network).

### Session 2 Chunks (Structure Build)

S2-C1: Node command registry scaffolding
- Area: `thegrid-node` command dispatch structure.
- Expected duration: 1 focused run.
- Validation: existing commands unchanged + compile checks.
- Conversation rule: Keep same conversation for S2-C2 if touching same command files.

S2-C2: Parser/render decoupling prep
- Area: `thegrid-node` TUI state + parser separation.
- Expected duration: 1 focused run.
- Validation: TUI smoke set remains stable.
- Conversation rule: Same conversation as S2-C1 (shared context).

S2-C3: Observability model scaffolding
- Area: shared models/events for sync health metrics.
- Expected duration: 1 focused run.
- Validation: additive structures only, no behavior regression.
- Conversation rule: New conversation (cross-crate context shift).

### Session 3 Chunks (Verify Intent)

S3-C1: Functional verification pass
- Area: node + sync + transfer end-to-end checks.
- Expected duration: 1 verification run.
- Validation: command smoke + sync flow checks.
- Conversation rule: New conversation dedicated to verification output.

S3-C2: Visual/UX verification pass
- Area: GUI and node readability under live updates.
- Expected duration: 1 verification run.
- Validation: intended look/behavior checklist.
- Conversation rule: New conversation (GUI-focused context differs).

S3-C3: Reliability stress pack
- Area: rename storms, recursive moves, reconnect replay, high file count.
- Expected duration: 1 stress run.
- Validation: no data-loss regressions.
- Conversation rule: New conversation for clean test evidence.

### Session 4 Chunks (Polish + Closed Beta)

S4-C1: Final polish batch
- Area: low-risk fixes only.
- Expected duration: 1 focused run.
- Validation: full build + no blocker introduced.
- Conversation rule: Keep same conversation for S4-C2.

S4-C2: Daily beta logging loop (Day 1-7)
- Area: runtime logs, sync anomalies, usability notes.
- Expected duration: daily short runs.
- Validation: daily log triage and severity labeling.
- Conversation rule: Same conversation during one day; new conversation each new day.

S4-C3: Beta closeout and release decision
- Area: blocker/high fixes and go/no-go summary.
- Expected duration: 1 closeout run.
- Validation: zero unresolved blockers.
- Conversation rule: New conversation for final release decision artifact.

---

## Conversation Handoff Template (Use Between Chunks)

Copy this into the next conversation when switching context:

1. Chunk ID and objective
- Example: `S2-C1 - Node command registry scaffolding`

2. Files touched
- List exact files changed in prior chunk.

3. What was completed
- 3-6 bullet deltas only.

4. Validation evidence
- Commands run and pass/fail summary.

5. Risks and rollback note
- One-line risk + one-line rollback method.

6. Next chunk start point
- First file/symbol to open for the next chunk.

---

## Fast Model Operating Defaults (Gemini Flash Friendly)

1. Keep chunk scope to one subsystem and one acceptance gate.
2. Avoid loading unrelated files.
3. Prefer focused checks first, workspace checks at integration points.
4. Use short delta updates, no repeated summaries.
5. Start new conversation at subsystem boundaries.

---

## Deliverables

1. Stable baseline report (security + sync correctness).
2. Structural readiness report (command + observability scaffolding).
3. Validation report (functional + UX + stress outcomes).
4. Closed beta report (weekly logs, issues, go/no-go decision).
