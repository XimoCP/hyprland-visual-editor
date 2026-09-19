# Fix: strip quality findings from the deepseek-v4-flash review

**Locator**: `odd/tasks/borders-strip-quality-fixes.md`
**Engram mirror**: topic `odd/borders-strip-quality-fixes/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: PLANNED (user authorized 2026-09-19: "arregla y instala el binario")
**TDD**: strict — runner `cargo test`

## Source

Single-pass quality review of the frozen revision `476a442` by `opencode-go/deepseek-v4-flash` (a different model from the writers, which ran on `muse-spark-1.3-contributor-free`), persisting as Engram #1002. It found two **must-fix** defects plus comment rot, dead code and one weakly-asserting test. The reviewer's verdict on the rest of the strip work was positive: the always-mounted animated card, the two-way `strip-active-chip` mirror, the index map's internal consistency, the "strip is never engaged" contract, and tests that measure pixels and behaviour rather than tautologies.

## Defects to fix

### Q1 — must-fix: a keyboard-reachable control that silently does nothing
`strip-active-chip` is only kept in range by `strip-cycle` (`ui/panel/sections/BordersSection.slint:2360-2363`), which runs only while focus sits on the strip. Shrinking the slot count never clamps it: with 8 chips and the ring on chip 8, removing a slot leaves `strip-active-chip = 7` out of range. The ring disappears, and Enter on the strip sets `editing-slot = 7`: `channel-name` falls back to `"this colour"` (`:283`), `editing-value()` returns `""` (`:288-296`), `choose-token` matches no branch and writes nothing (`:301-311`), and `set-custom` hits a silent `else { return; }` (`:2063-2064`). The token row's HSV `changed`/`committed` route to the same dead end, and the `slot-color-changed` binding (`:2508-2511`) can index `tune-active-colors[7]` out of bounds.
**Required behaviour**: shrinking the slot count clamps the ring, and no picker can ever open on a chip that no longer exists. Clamp where the count changes and bound again at the point of use.

### Q2 — must-fix: ghost stops and a keyboard trap when there are no colours
`tune-count` (`BordersSection.slint:1989`) unconditionally counts the strip (3), add (4) and remove (5) stops, but those controls only mount behind `if root.slot-count > 0` (`:505, :698, :767`). PanelRoot's `== 3` dispatch (`ui/panel/PanelRoot.slint:489-493, 504-508`) therefore consumes both arrows on an invisible stop when `tune-n == 0`, and `tune-enter` (`BordersSection.slint:2285`) can fire `add-color-slot` while the `+` control is not rendered.
**Required behaviour**: the counted stops match the stops that exist; at zero colours the arrows are not swallowed by an invisible stop and the add action cannot fire from an unmounted control.

### Q3 — comments that describe code that no longer exists
- `src/callbacks.rs:1013, 1016` — still says "N slots" and "N is clamped to 0..8"; `e91f269` removed both the clamp and the slot dependency.
- `ui/panel/sections/BordersSection.slint:203-204` — describes the picker card as `visible`-toggled; it is now always-mounted with `states`/`transitions` (`63f64ad`).
- `src/shell/ui_tests.rs:6064-6066` — the header says "slots" and "glow range" while the body selects the glow-**enable** toggle at global 7.
Fix each to describe what the code actually does. Do not delete comments to make them stop lying.

### Q4 — dead code
`ui/panel/sections/BordersSection.slint:216` — `property <int> first-slot-idx: 3;` is the only occurrence of that identifier in the file. Remove it.

### Q5 — a test that does not verify what it claims
`borders_tune_full_focus_reaches_last_and_middle` (`src/shell/ui_tests.rs`) selects the glow-enable stop but only asserts `count_icy_pixels(&mid) > 200` plus a buffer-diff threshold (`:6134-6137`), so any lit stop at that index passes and the "middle identity" lives only in a comment. Strengthen it so drifting the ring to the neighbouring glow stop fails — keep the existing hard `assert_eq!(tune, 26)` lock.

## Out of scope (recorded debt, do NOT fix here)

- Consolidating the index map that is duplicated across the pane constants, `tune-enter`/`adjust-slider` literals, PanelRoot's `list-len`-derived `== 3`, and the Rust mirror in `src/callbacks.rs:1018-1038`. Also unasserted: the Slint `tune-count` `out property` has no Rust-side test.
- The parked picker `y` slide-in; the scanner dropping `@Color`/`icon`; `sync_preset_indices` dead code in `src/callbacks.rs:527`; the strip ring owning focus whenever the pane has focus.
- No `.slint` visual redesign: only the state/behaviour fixes above.

## Acceptance criteria

1. Shrinking the slot count can never leave the ring or an open picker on a non-existent chip; a test fails before the fix and passes after.
2. With zero colours, arrow keys are not swallowed by an invisible stop and the add action cannot fire from an unmounted control; covered by a test.
3. The three lying comments describe the current code, and `first-slot-idx` is gone (`cargo check` clean, no dangling references).
4. `borders_tune_full_focus_reaches_last_and_middle` fails if the ring drifts to the neighbouring glow stop.
5. Full suite green, `cargo check` without warnings, and the render PNGs inspected for any render-visible change.

## Delivery

Work-unit commits, one per defect group, Conventional Commit messages, tests and docs alongside the behaviour, no AI attribution. Parent then rebuilds and installs `~/.local/bin/hve` (backup first) so the user can see today's work live.

## Progress

- 2026-09-19 — Created from review #1002. No source changed yet.
- 2026-09-19 — Q1→Q5 implemented with strict TDD by the bounded writer. RED
  observed before GREEN for Q1 (`left: None, right: Some(1)` — the ring
  vanished), Q2 (`Enter on a tune stop must never fire the unmounted add
  control`) and Q5 (mutation: preview index drifted one stop down; the new
  assertion failed with "the ring drifted to the neighbouring Range stop").
  Full suite green: `750 passed; 0 failed; 0 ignored`. `cargo check
  --all-targets` clean. `first-slot-idx` gone (`grep` returns none).
  Renders read and confirmed: ring clamped to chip 2 after shrinking to 2
  colours; zero-colour tune-local 3 is the ringed glow toggle; the focused
  middle stop is the glow-enable toggle (no slider in its ring) while the
  neighbouring Range stop paints the white knob inside its ring.

### Commits

- `5f91b27` `fix(borders): clamp the strip ring and picker when the slot count shrinks` — Q1.
- `4363e7a` `fix(borders): drop the ghost strip/add/remove stops at zero colours` — Q2 (+ Q4 dead `first-slot-idx` in the same map block).
- `e9aed85` `test(borders): pin the glow-enable middle stop against ring drift` — Q5 (+ the ui_tests header comment from Q3).
- `0747f2c` `docs(borders): correct the picker-card comment to the always-mounted card` — Q3 (picker comment; the callbacks.rs doc rode with Q2's function rewrite).

### Notes for the parent

- The noctalia provider test `delegate_noop_when_socket_absent` is a
  pre-existing flaky env-var race when the module runs in parallel (it fails
  identically with these changes stashed; passes in isolation and serialized).
- Out-of-scope debt untouched: the duplicated index map, the Rust-side
  `tune-count` assertion gap, the parked `y` slide-in, the scanner
  `@Color`/`icon` gap and `sync_preset_indices`.

## Next step

Parent gate on commits `5f91b27`→`0747f2c`, then rebuild + install.
