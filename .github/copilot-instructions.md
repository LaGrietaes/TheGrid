# TheGrid Copilot Crunch Instructions

These instructions optimize for fast delivery with quality gates.

## Scope
- Follow this workflow for all implementation work in this repository.
- Do not change behavior outside the active task scope.

## Delivery Order
1. Security and access gating
2. Observability and operator visibility
3. Reliability/performance hardening
4. Feature expansion

## PR/Change Slicing
- Keep one risk domain per change.
- For node CLI, add one command group per change only.
- Include rollback note in each change summary.

## Required Validation
- Run: `cargo check --workspace`
- Run: `cargo check -p thegrid-node` for node changes
- For node TUI changes, manually verify: `help`, `devices`, `ping`, `history`, `update`, `quit`
- For sync/index changes, validate tombstone/delete conflict behavior.

## Edit Hygiene
- Prefer smallest possible patch.
- Avoid reformatting unrelated code.
- Reuse existing runtime/event APIs before adding new abstractions.

## Communication Style
- Provide concise progress updates with deltas only.
- Report blockers early with one concrete fallback.
