# Archive Report: dynamic-border-tune-pane

**Archived**: 2026-09-22
**Task state**: 27/27 complete (read from `tasks.md` and confirmed by `gentle-ai sdd-status`).

**Live spec created**: `openspec/specs/border-tune-pane/spec.md` — 11 requirements, 16 scenarios, generated from this change's delta (`specs/border-tune-pane/spec.md`). No live spec existed for this capability before, so the delta's MODIFIED and ADDED requirements became its initial content. The delta's editorial intro and its `MODIFIED`/`ADDED Requirements` section headers were dropped (they describe the delta, not the contract); the requirement and scenario text is verbatim.

**Evidence of completion**: the tune pane loads every parameter of the selected preset, saves the full schema, shows per-control descriptions, dynamic colour slots, the dual-mode colour input, the glow group, the animation leaves and the floating-window toggle; the suite on this branch is green (893 tests). Later work on the same pane landed as `30c9100` (populate the tune pane when a theme applies its border) and the strip work archived as `29407ad`.
