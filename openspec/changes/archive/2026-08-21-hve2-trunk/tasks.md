# Tasks: HVE 2 Trunk — State-Driven Navigation Shell

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1400–1600 (additions + deletions) |
| 400-line budget risk | High |
| Chained PRs recommended | Yes |
| Suggested split | PR 1 → PR 2 → PR 3 → PR 4 → PR 5 |
| Delivery strategy | auto-forecast |
| Chain strategy | pending |

Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: pending
400-line budget risk: High

### Suggested Work Units

| Unit | Goal | Likely PR | Notes |
|------|------|-----------|-------|
| 1 | NavState pure state machine | PR 1 | standalone; base = main |
| 2 | SlotRegistry + SizePolicy | PR 2 | depends on PR 1 |
| 3 | SkwdTokens + shell.slint UI | PR 3 | depends on PR 2 |
| 4 | MainWindow rewiring + callbacks + i18n | PR 4 | depends on PR 3; ~390 lines, near budget |
| 5 | Headless integration tests | PR 5 | depends on PR 4 |

## Phase 1: Foundation — NavState (R1, R2)

- [x] 1.1 Create `src/shell/nav.rs`: `Screen`, `ExpansionState`, `NavCommand`, `NavTransition`, `NavState` (pure, no Slint).
- [x] 1.2 TDD: write failing tests first — initial Home+Collapsed (R1); expand happy path, no-op same target, unknown target, queue order (final=last), `Back`, `move_focus` (R2) — then implement; `cargo test` green.
- [x] 1.3 Create `src/shell/mod.rs` skeleton exposing `pub mod nav;` — `cargo test` green, baseline intact.

## Phase 2: Slot Contract + Size Policy (R3, R8)

- [x] 2.1 TDD: failing tests — `StubSlot` mount/unmount hooks fire, unknown target stays put (R3) — then create `src/shell/slots.rs`: `trait Slot`, `SlotRegistry`, `StubSlot` (`cfg test`); green.
- [x] 2.2 TDD: failing tests — `SizePolicy::target/clamp` for base 900×680, expanded 1200×800, min 800×580 (R8) — then create `src/shell/size.rs`: `SizePolicy`; green.

## Phase 3: Shell UI (R6)

- [x] 3.1 Create `ui/tokens.slint`: `export global SkwdTokens` (fonts, sizes, radii, spacing, anim incl. 350ms OutCubic, borders, opacity, colors, gallery geometry); MIT credit header.
- [x] 3.2 Create `ui/shell.slint`: chrome, slot area via `if`+enum mount, `KeyBinding` activate/back, animated `set_size` hooks; MIT credit header.

## Phase 4: Wiring + i18n (R4, R5, R6, R7, R9)

- [x] 4.1 Modify `ui/main.slint`: mount shell, remove panel clip-switch block.
- [x] 4.2 Modify `src/callbacks.rs`: `card-activated`, `back-activated`, `nav-move` → `NavCommand` (keyboard == mouse, R4).
- [x] 4.3 Modify `src/shell/mod.rs` + `src/main.rs`: `Shell` root (NavState, SlotRegistry, SizePolicy, Tr; dispatch, `sync_after_show`), register stubs, stepped 16ms `Timer` `set_size`, min-width/height, port `nav_logic` (R2, R6, R7, R9).
- [x] 4.4 Add `shell.*` keys to `i18n/en.json` + `i18n/es.json` (R5); `src/tr.rs` untouched.

## Phase 5: Headless Integration Tests (R2, R4, R6, R7, R9)

- [x] 5.1 Tests (`i-slint-backend-testing`, `--test-threads=1`): exactly one window at 900×680 on startup (R6).
- [x] 5.2 Tests: `card-activated` → `mounted-screen` + `set_size` growth; collapse shrinks to base (R2, R7).
- [x] 5.3 Tests: keyboard activation equals mouse; back/collapse key; Escape (R4).
- [x] 5.4 Tests: hide preserves state + focus restored on show (R9); `toggle_tray` restore.
- [x] 5.5 Full suite: `cargo test` — 244 passed, 0 failed, 1 ignored, zero regressions.

## Phase 6: Cleanup

- [x] 6.1 Remove dead `nav_logic`/clip-switch leftovers; drop unused imports.
- [x] 6.2 Verify MIT credit headers in translated files; update change docs.

## Requirement Traceability

R1→1.1-1.2 · R2→1.2/4.3/5.2 · R3→2.1 · R4→4.2/5.3 · R5→4.4 · R6→3.2/4.1/5.1 · R7→2.2/4.3/5.2 · R8→2.2 · R9→4.3/5.4