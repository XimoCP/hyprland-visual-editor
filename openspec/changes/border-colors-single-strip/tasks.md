# Tasks: Border Colors — Single Horizontal Strip

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | 600–750 (across 4 PRs) |
| 400-line budget risk | High (PR 2 alone ~400 lines) |
| Chained PRs recommended | Yes |
| Suggested split | PR 1 → PR 2 → PR 3 → PR 4 |
| Delivery strategy | auto-chain |
| Chain strategy | feature-branch-chain |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: High

### Suggested Work Units

| Unit | Goal | Likely PR | Focused test command | Runtime harness | Rollback boundary |
|------|------|-----------|----------------------|-----------------|-------------------|
| 1 | Stop-count formula + unit tests + tune-count expression | PR 1 | `cargo test test_borders_tune_count_covers_full_inventory` | N/A (pure Rust unit test) | `callbacks.rs` + 1 line in `BordersSection.slint` |
| 2 | Strip UI + picker card + compact rows + delete BorderColorSlot | PR 2 | `cargo test borders_strip_renders_correct_chip_count` | `cargo test borders_strip_flow_renders` | `BordersSection.slint` + `BorderColorSlot.slint` (delete) + `ui_tests.rs` |
| 3 | Keyboard sub-navigation (PanelRoot dispatch + strip-cycle) | PR 3 | `cargo test borders_strip_keyboard_sub_navigation` | N/A (unit test the public functions) | `PanelRoot.slint` + `BordersSection.slint` + `ui_tests.rs` |
| 4 | Animation polish + render verification + final test assertions | PR 4 | `cargo test borders_strip_flow_renders` | `cargo test borders_strip_flow_renders` (PNG inspection) | `BordersSection.slint` + `ui_tests.rs` |

## Phase 1: Keyboard Model — Stop-Count Formula (PR 1)

- [x] 1.1 [RED] Update `test_borders_tune_count_covers_full_inventory` assertions in `src/callbacks.rs` to expect new values: `(2, off, off, off, off) → 13`, `(8, on, on, on, on) → 26`, `(3, on, off, off, off) → 17`, `(0, off, off, off, off) → 13`, `(99, off, off, off, off) → 13`. Tests MUST fail. [f: `src/callbacks.rs`] [t: `cargo test test_borders_tune_count_covers_full_inventory`]

- [x] 1.2 [GREEN] Change `borders_tune_stop_count` formula in `src/callbacks.rs` from `3 + n + 2 + 1` to `3 + 1 + 2 + 1` (strip = 1 stop, not N). Keep `_slot_count` parameter for API compat but ignore it in the formula. Tests MUST pass. [f: `src/callbacks.rs`] [t: `cargo test test_borders_tune_count_covers_full_inventory`]

- [x] 1.3 [REFACTOR] Update `tune-count` expression in `ui/panel/sections/BordersSection.slint` line 1753: remove `+ root.tune-n` from the formula. Old: `12 + root.tune-n + ...` → New: `13 + ...` (strip = 1 stop, not N; base moves 12→13 because the strip adds exactly one stop). Verify: `cargo test` still passes. [f: `ui/panel/sections/BordersSection.slint`] [t: `cargo test`]

- [x] 1.4 Update tune-local index comments in `BordersTunePane` (lines 113–128) to reflect new fixed indices: `idx-add = 4`, `idx-remove = 5`, `idx-glow-en = 6`, etc. No functional change — comments only. [f: `ui/panel/sections/BordersSection.slint`]

- [x] 1.5 [FIX] Align ui_tests stop-count assertions (27→26, 18→17) and glow-en index (local 7→6) so PR 1 closes green. [f: src/shell/ui_tests.rs]

## Phase 2: Strip UI + Picker Card + Compact Rows (PR 2)

- [ ] 2.1 [RED] Add `borders_strip_renders_correct_chip_count` test in `src/shell/ui_tests.rs`. Mount `MainWindow`, set `slot-count = 3`, assert strip renders exactly 3 chip rectangles with numeric labels "1", "2", "3". Test MUST fail (old layout has 3 slot cards, not strip). [f: `src/shell/ui_tests.rs`] [t: `cargo test borders_strip_renders_correct_chip_count`]

- [ ] 2.2 [GREEN] Add `strip-active-chip` and `picker-visible` properties to `BordersTunePane` in `BordersSection.slint`. Replace the `for i in root.tune-active-colors.length : Rectangle { ... BorderColorSlot ... }` block (lines 475–498) with the horizontal strip component: `HorizontalLayout` with `for i in root.tune-active-colors.length : Rectangle { ... }` containing a 16×16 colour chip, numeric label, focus ring (icy-cyan #8fd8ff on active chip), and `TouchArea` that sets `strip-active-chip` and calls `request-edit-slot(i)`. Test MUST pass. [f: `ui/panel/sections/BordersSection.slint`] [t: `cargo test borders_strip_renders_correct_chip_count`]

- [ ] 2.3 [RED] Add `borders_strip_picker_opens_above_strip` test in `src/shell/ui_tests.rs`. Mount `MainWindow`, set `slot-count = 3`, set `editing-slot = 1`, assert picker card is visible above the strip. Diff between closed/open pixel count > 5000. Test MUST fail. [f: `src/shell/ui_tests.rs`] [t: `cargo test borders_strip_picker_opens_above_strip`]

- [ ] 2.4 [GREEN] Move the existing picker card block (lines 231–302) from the top of `tune-col` to AFTER the strip, inside a `VerticalLayout` wrapper that contains: (1) the picker card `if root.picker-visible : Rectangle { ... }` with `height: 344px`, and (2) the chip strip `HorizontalLayout { ... }`. Update `picker-visible` binding to `root.editing-slot >= 0`. Test MUST pass. [f: `ui/panel/sections/BordersSection.slint`] [t: `cargo test borders_strip_picker_opens_above_strip`]

- [ ] 2.5 [RED] Add `borders_strip_compact_rows_height` test in `src/shell/ui_tests.rs`. Mount `MainWindow`, set `editing-slot = 8`, assert inactive row height < 60px. Test MUST fail (old height is 168px). [f: `src/shell/ui_tests.rs`] [t: `cargo test borders_strip_compact_rows_height`]

- [ ] 2.6 [GREEN] Replace `BorderColorSlot` instances in inactive-wrap (lines 440–451), glow color card (lines 780–793), and glow inactive card (lines 818–831) with compact row pattern: `Rectangle { height: self.lit ? 56px : 48px; ... HorizontalLayout { ... 16×16 chip + Text label + "Custom…" button ... } }`. Update the inactive-wrap container height from `168px/184px` to `48px/56px`. Test MUST pass. [f: `ui/panel/sections/BordersSection.slint`] [t: `cargo test borders_strip_compact_rows_height`]

- [x] 2.7 [REFACTOR] Delete `ui/panel/sections/BorderColorSlot.slint`. Remove `import { BorderColorSlot } from "BorderColorSlot.slint";` from `BordersSection.slint` line 25. Run `cargo check` — no dangling imports. [f: `ui/panel/sections/BorderColorSlot.slint`, `ui/panel/sections/BordersSection.slint`] [t: `cargo check`]

- [x] 2.8 (4 of 7 rewritten; 3 deferred — see the note below) Rewrite 7 existing border tests in `src/shell/ui_tests.rs` to match new layout: `borders_tune_pane_renders_with_dynamic_slots` (strip chips instead of slot cards), `borders_tune_full_focus_reaches_last_and_middle` (stop-count 27→26 already done in PR 1 — PR 2 only recalculates the `middle`/`last` focus indices if the layout shifts), `borders_tune_glow_color_cards_render_inside_their_box` (stop-count 18→17 already done in PR 1 — PR 2 only replaces the `BorderColorSlot` height assertions with compact-row height assertions), `borders_tune_custom_picker_opens_for_one_channel` (picker above strip), `borders_list_pane_scrolls_with_the_mouse_wheel` (adjust pixel bounds), `borders_list_wheel_reaches_the_end` (adjust pixel bounds), `borders_tune_renders_eight_slots` (8 chips in strip). Keep 2 unchanged: `borders_mouse_never_touches_focus_by_construction`, `panel_scopes_ignore_mouse_focus_by_construction`. [f: `src/shell/ui_tests.rs`] [t: `cargo test`]

> **2.8 closure note**: four tests were genuinely strengthened and each was observed RED
> against the old layout (`borders_tune_pane_renders_with_dynamic_slots`,
> `borders_tune_glow_color_cards_render_inside_their_box`,
> `borders_tune_custom_picker_opens_for_one_channel`, `borders_tune_renders_eight_slots`).
> Three were deliberately left untouched: `borders_tune_full_focus_reaches_last_and_middle`
> encodes the tune-local index map, which is still slot-dependent and is fixed by 3.5-3.7
> (the test currently asserts only that *some* stop is focused, so it is green but hollow —
> making it meaningful requires Phase 3); `borders_list_pane_scrolls_with_the_mouse_wheel` and
> `borders_list_wheel_reaches_the_end` only exercise the list pane, which PR 2 did not change
> (measured wheel ink identical before/after), so they had no stale tune assertion to rewrite.

## Phase 3: Keyboard Sub-Navigation (PR 3)

- [ ] 3.1 [RED] Add `borders_strip_keyboard_sub_navigation` test in `src/shell/ui_tests.rs`. Verify `strip-cycle(1)` increments `strip-active-chip` from 0→1→2→0 (wraps). Verify `strip-cycle(-1)` decrements. Verify Enter on strip sets `editing-slot`. Verify Esc closes picker. Test MUST fail (function doesn't exist). [f: `src/shell/ui_tests.rs`] [t: `cargo test borders_strip_keyboard_sub_navigation`]

- [ ] 3.2 [GREEN] Add `strip-cycle(delta: int)` public function to `BordersTunePane` in `BordersSection.slint`:
  ```slint
  public function strip-cycle(delta: int) {
      if (root.slot-count <= 0) { return; }
      root.strip-active-chip = mod(root.strip-active-chip + delta + root.slot-count, root.slot-count);
  }
  ```
  Test MUST pass. [f: `ui/panel/sections/BordersSection.slint`] [t: `cargo test borders_strip_keyboard_sub_navigation`]

- [ ] 3.3 [RED] Verify PanelRoot forwards Left/Right to `strip-cycle` when `local-focus == 3`. Add test assertion in `borders_strip_keyboard_sub_navigation` that checks PanelRoot dispatch. Test MUST fail (PanelRoot doesn't check for strip stop). [f: `src/shell/ui_tests.rs`] [t: `cargo test borders_strip_keyboard_sub_navigation`]

- [ ] 3.4 [GREEN] Add strip sub-navigation branches in `ui/panel/PanelRoot.slint` Left/Right dispatch (lines 484–500). When `borders-focused-index >= borders-list-len` and `(borders-focused-index - borders-list-len) == 3`, forward arrows to `borders.strip-cycle(1)` (Right) or `borders.strip-cycle(-1)` (Left) instead of jumping panels. Test MUST pass. [f: `ui/panel/PanelRoot.slint`] [t: `cargo test borders_strip_keyboard_sub_navigation`]

- [ ] 3.5 Update `BordersTunePane tune-enter()` (lines 2028–2091): add strip stop handling at index 3 — when `local == 3 && root.slot-count > 0`, set `root.editing-slot = root.strip-active-chip` and return 1. Remove old per-slot engage+enter paths (`local >= 3 && local < 3 + n && root.engaged-slider == local`). The strip stop is NEVER engaged. [f: `ui/panel/sections/BordersSection.slint`]

- [ ] 3.6 Update `adjust-slider()` (lines 1894–2023): remove the old per-slot branch `if (e >= 3 && e < 3 + n) { ... next-token ... }` — the strip is never engaged, so this path is dead. Update glow index references from `6 + n` / `7 + n` / `8 + n` / `9 + n` to fixed constants `7` / `8` / `9` / `10` (matching new index map). [f: `ui/panel/sections/BordersSection.slint`]

- [ ] 3.7 Update all tune-local index properties (lines 118–146) to fixed constants:
  ```
  idx-add = 4, idx-remove = 5, idx-glow-en = 6, idx-glow-range = 7,
  idx-glow-power = 8, idx-glow-color = 9, idx-glow-inactive = 10,
  base-anim = glow-enabled ? 11 : 7
  ```
  All downstream `idx-a0-*`, `idx-a1-*`, `idx-a2-*`, `base-tail`, `idx-rule`, `idx-save-name`, `idx-save-btn` recompute from fixed base. [f: `ui/panel/sections/BordersSection.slint`]

- [ ] 3.8 Update keyboard hint text in `BordersTunePane` (if any references `N slots` or per-slot descriptions). Strip description: "←→ select gradient colour, Enter to edit, Esc to close." [f: `ui/panel/sections/BordersSection.slint`]

## Phase 4: Animation + Render Verification (PR 4)

- [ ] 4.1 Add entry animation bindings on the picker card in `BordersSection.slint`: `animate opacity { duration: 250ms; easing: ease-in-out-back; }`, `animate y { duration: 250ms; easing: ease-in-out-back; }`, `animate transform-scale-x { duration: 250ms; easing: ease-in-out-back; }`, `animate transform-scale-y { duration: 250ms; easing: ease-in-out-back; }`. Bind `opacity: 0→1`, `y: -8px→0px`, `scale-x/y: 0.95→1.0` on `picker-visible`. [f: `ui/panel/sections/BordersSection.slint`]

- [ ] 4.2 Add exit animation: `opacity: 1→0` at 150ms `ease-in` only (NO y-shift — prevents 8px strip jump). When picker closes, the card fades out without moving, so the strip stays in place. Document this in the component comment. [f: `ui/panel/sections/BordersSection.slint`]

- [ ] 4.3 [RED] Add `borders_tune_stop_count_strip_single_stop` test in `src/shell/ui_tests.rs`. Verify `borders_tune_stop_count(N, ...)` returns the same value regardless of N for specific combos: 2 slots no glow → 13, 4 slots glow → 17, 8 slots glow+all anims → 26. Test MUST fail (old formula varies with N). [f: `src/shell/ui_tests.rs`] [t: `cargo test borders_tune_stop_count_strip_single_stop`]

- [ ] 4.4 [GREEN] Verify the formula produces consistent results across slot counts. Test MUST pass after Phase 1 formula change is in place. [f: `src/shell/ui_tests.rs`] [t: `cargo test borders_tune_stop_count_strip_single_stop`]

- [ ] 4.5 Add `borders_strip_flow_renders` render test (or extend `slice_focus_flow_renders`) in `src/shell/ui_tests.rs` to produce PNGs of: (1) strip closed — 3 chips visible, (2) strip open — picker above strip with `editing-slot = 1`, (3) strip with 8 chips — no overflow, (4) compact inactive row — focused. Save under `/tmp/opencode/borders_strip_*.png`. [f: `src/shell/ui_tests.rs`] [t: `cargo test borders_strip_flow_renders`]

- [ ] 4.6 VISUAL VERIFICATION: Read the PNGs saved under `/tmp/opencode/borders_strip_*.png` and confirm: chips are evenly spaced and non-overlapping at 8 slots, picker card is fully visible above the strip, compact rows are ~48–56px tall, focus ring (icy-cyan #8fd8ff) appears on focused chip. Note: software renderer does NOT support `transform-scale-x/y`, so scale assertions are omitted from render tests (GPU-only polish). [f: PNG inspection]

- [ ] 4.7 Run `cargo test` — all tests green. No skipped tests. All 9 original border tests rewritten and passing. All new tests passing. [f: full test suite] [t: `cargo test`]

- [ ] 4.7b Tighten three loose assertions found in the PR 2 verification (quality, not behavior):
  (1) the `span < 600` bound in `borders_tune_renders_eight_slots` is documented as the wrap
  guard, but wrapping *shrinks* the measured span — the real wrap guard is the shared-row
  assertion, so either drop the loose bound or re-document it honestly;
  (2) the picker card height is asserted as `330..=430` in one test and `360..=400` in its
  sibling — a card 50px off passes the loose one while its message claims "full height, not
  clipped";
  (3) the swatch count band `100..=400` does not pin the 16×16 swatch (the measured exact-colour
  bbox is 13×13 through the 1px border + radius), so the message overstates the precision.
  [f: `src/shell/ui_tests.rs`] [t: `cargo test borders_`]

- [ ] 4.8 Final `cargo check` — no warnings, no dangling imports, `BorderColorSlot.slint` deleted, no references to it anywhere in the codebase. [f: full build] [t: `cargo check && grep -r BorderColorSlot src/ ui/`]
