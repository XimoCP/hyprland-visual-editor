# Drawers are Ctrl-only: plain arrows stop touching them

**Opened**: 2026-10-04 (keeper's call, after noticing the Slider accepted both keys)
**Branch**: hve2-visual-rewrite
**Route**: delegated writer (3 files), sequential BEFORE the shortcuts footer
**Status**: done (implemented, tests green, visual verified; commit recorded below)

## Objective

In the gallery, ONLY Ctrl+Up / Ctrl+Down operate the hidden drawers. Plain
Up/Down must do nothing there, so they stay free for a future use (the keeper
has another idea for them). No key may mean two different things in one screen.

## Decisions (keeper's, with the one consequence flagged)

1. Plain Up/Down in the gallery do NOTHING: no drawer, no carousel step. They
   are reserved.
2. Consequence, stated out loud and accepted: the drawer's own arrows stop
   CLOSING an open drawer too. Escape still closes it, and the mouse arrow
   button still closes it. Without this, Ctrl+Up would open and plain Up would
   close — exactly the "two meanings" the keeper is removing.
3. Home is untouched: there Up/Down still move the vertical focus
   (`src/callbacks.rs:165-173`, the `"down" | "up" if !is_gallery` arm).
4. Left/Right are untouched everywhere, including the Slider's carousel step and
   the Mosaic cursor.
5. Ctrl+Up / Ctrl+Down keep working exactly as they do today, including while a
   drawer is open (Ctrl is now the ONLY drawer key).

## Scope

- `src/callbacks.rs` — remove the V6.1 gallery drawer-toggle block from
  `nav-move` (the `if is_gallery { ... "up" ... "down" ... }` early-return at
  lines 143-164), and rewrite the now-false V6.1 comment above it. Leave the
  delta match and the `is_gallery` branch below intact: with the block gone, the
  gallery's up/down delta is already 0, which is exactly "do nothing".
- `ui/gallery/GalleryRoot.slint` — `gallery-keys` (lines 317-324): the plain
  Up/Down branches that close/cross-toggle an open drawer must require
  `event.modifiers.control`, mirroring `shell-kbd`. Escape (311-314) keeps
  closing.
- Tests in `src/shell/ui_tests.rs`.
- Do NOT touch: `shell-kbd`'s Ctrl branch, `ui/panel/**`, the mouse
  `ChromeButton` toggles, `nav-move`'s Home path, or `mosaic_neighbors`.

## Acceptance criteria

- Gallery, both styles: Ctrl+Up opens the top drawer, Ctrl+Down the bottom one,
  each closing the other. Unchanged from today.
- Gallery, both styles: plain Up and plain Down change NOTHING — no drawer opens,
  no carousel/`animate_nav` step, no mosaic cursor move on the vertical axis.
- With a drawer open: plain Up/Down do nothing; Ctrl+Up/Ctrl+Down still toggle;
  Escape still closes; the mouse arrow button still closes.
- Home: plain Up/Down still move the vertical focus.
- The stale "V6.1: Up/Down no longer move the carousel — they deploy the drawers"
  comment no longer claims something false.

## Checks

- Strict TDD is ON. Runner: `cargo test`. RED FIRST: a test asserting that plain
  Up in the gallery opens NO drawer fails on the current head (today it opens
  one) — paste that failure.
- The tests must drive the REAL keyboard path (`focus_press_key`,
  `focus_press_key_with_ctrl`, `src/shell/ui_tests.rs:5093` and the Ctrl helper
  added in `e64d2d2`).
- VISUAL: the change touches `.slint`, so render into a per-run directory and
  READ the PNG: after a plain Up in the gallery the drawer must NOT be painted
  (assert by pixel sampling, the way this project already proves "not painted"),
  and after Ctrl+Up it must be. "Tests green" alone is not verification here.
- Existing tests that encode the removed behaviour must be UPDATED, not deleted
  silently: report every one you changed and why it encoded the old contract.
- Do not reformat unrelated code; no blanket `cargo fmt`.

## Tasks

- [x] **T1 (RED)** — Failing test(s): plain Up/Down in the gallery do nothing;
  Ctrl+Up/Down still open the drawers in both styles.
- [x] **T2 (GREEN)** — `src/callbacks.rs` block removal + comment rewrite;
  `gallery-keys` Ctrl gate. Re-run T1 green.
- [x] **T3 (VISUAL)** — Render + read the PNG proving the drawer is absent after
  a plain Up and present after Ctrl+Up.
- [x] **T4 (COMMIT)** — One Conventional Commit with tests and change together;
  record the hash here.

## Evidence log

### T1 — RED on the pre-change head

Harness note: the headless suite normally registers only the callbacks a test
asserts on, so the Rust `nav-move` drawer logic was invisible to it. The new
`window_with_production_callbacks` / `gallery_with_production_callbacks`
helpers wire the REAL `callbacks::setup_callbacks`, so a plain arrow reaches
the production handler exactly as in the app. Without that seam the Rust block
removal is untestable.

`cargo test plain_arrows_do_not` (pre-change):

```
running 2 tests
test shell::ui_tests::plain_arrows_do_not_touch_an_open_gallery_drawer ... FAILED
test shell::ui_tests::plain_arrows_do_not_deploy_gallery_drawers ... FAILED
---- plain_arrows_do_not_touch_an_open_gallery_drawer ----
panicked: plain Up with the drawer open must do nothing (the drawer stays open)
---- plain_arrows_do_not_deploy_gallery_drawers ----
panicked: plain Up in the Gallery must not deploy a drawer
test result: FAILED. 0 passed; 2 failed
```

`HVE_RENDER_DIR=/tmp/opencode/render-ctrl-only-red cargo test plain_up_paints_no_drawer_but_ctrl_up_does -- --nocapture`
(pre-change): FAILED at `plain Up must not open a drawer before the snapshot`
(the plain arrow opened the drawer, so the visual assertion could not run).

### T2 — change

- `src/callbacks.rs`: deleted the V6.1 gallery drawer-toggle early-return from
  `on_nav_move`; rewrote the comment to describe the Ctrl-only behaviour.
  `is_gallery`'s up/down delta is now 0 (do nothing); Home's
  `"down" | "up" if !is_gallery` arm and the `is_gallery` Left/Right branch are
  untouched.
- `ui/gallery/GalleryRoot.slint`: `gallery-keys` plain Up/Down branches now
  require `event.modifiers.control` (mirrors `shell-kbd`). Escape and
  Left/Right untouched.

### GREEN after the change

```
cargo test plain_arrows_do_not
test result: ok. 2 passed; 0 failed

cargo test plain_arrows_still_move_home_vertical_focus
test result: ok. 1 passed; 0 failed

HVE_RENDER_DIR=/tmp/opencode/render-ctrl-only-final \
  cargo test plain_up_paints_no_drawer_but_ctrl_up_does -- --nocapture
render artifacts for this run: /tmp/opencode/render-ctrl-only-final
test result: ok. 1 passed; 0 failed

cargo test
test result: ok. 1307 passed; 0 failed; 0 ignored
```

Baseline on the pre-change head was 1303 passed / 0 failed; the four new tests
bring it to 1307. No existing test encoded the removed behaviour, so none was
updated or deleted.

### T3 — visual

Run directory: `/tmp/opencode/render-ctrl-only-final`

- `ctrl_only_plain_up_slider.png` — after a plain Up: NO drawer painted; the
  frame is byte-identical to the baseline (asserted, not eyeballed).
- `ctrl_only_ctrl_up_slider.png` — after Ctrl+Up: the top Settings drawer is
  painted (Save / Borders / Motion / Filters / System pills), 26393 pixels
  changed vs the baseline.
- `ctrl_only_baseline_slider.png` — reference frame.

Read all three PNGs with vision: confirmed the drawer is absent after plain Up
and present after Ctrl+Up.

### T4 — commit

Hash: `7841d16b58f2ffb023b4f8f9a985e7937a8e57a8`
(`refactor(shell): the drawers are Ctrl-only, plain arrows stay free`).
Contains `src/callbacks.rs`, `ui/gallery/GalleryRoot.slint` and the tests in
`src/shell/ui_tests.rs` together. No AI attribution.

### Deviations / tensions observed

- The task's acceptance line "no mosaic cursor move on the vertical axis" is
  inconsistent with its own scope rule "do not touch the Mosaic's plain-arrow
  cursor movement". The explicit scope won: in the Mosaic, plain Up/Down still
  move the cursor via `shell-kbd`'s Mosaic branch → `gallery-mosaic-nav`
  (handled in `main.rs`, separate from `nav-move`). Removing the `nav-move`
  block does not touch that path, and the existing
  `ctrl_arrows_deploy_the_drawers_in_both_gallery_styles` test still asserts
  it. Flagged, not silently changed.
- The software renderer's snapshot is largely transparent (the gallery window
  is), so `count_buffer_diff_region`'s `alpha < 10` gate skipped every differing
  drawer pixel and returned 0. The visual assertions copy snapshot bytes into
  owned buffers and compare RGB without the alpha gate; the plain-Up case is
  asserted as exact byte equality.
