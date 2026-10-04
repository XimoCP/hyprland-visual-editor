# Ctrl+Up / Ctrl+Down deploy the hidden drawers in BOTH gallery styles

**Opened**: 2026-10-04 (keeper's decision, replacing the "edge escape" option)
**Branch**: hve2-visual-rewrite
**Route**: delegated writer (2 files), sequential after the Motion payload fix
**Status**: planned

## Objective

Ctrl+Up deploys the top drawer (Settings) and Ctrl+Down the bottom drawer
(Slider/Mosaic), in BOTH gallery styles (Slider = `gallery-style == 0`,
Mosaic = `gallery-style == 2`), so the drawers have one keyboard convention
that reads the same in both.

## Context (verified mapping)

The drawers lost their keyboard path because `shell-kbd`
(`ui/shell.slint:842-899`) gives the Mosaic its own branch at line 867
(`mounted-screen == 1 && gallery-style == 2`) that consumes all four arrows and
routes them to `gallery-mosaic-nav`. Plain Up/Down therefore only reach
`nav-move` in the Slider, where `src/callbacks.rs:146-164` toggles the drawers.

The drawer state itself lives in Slint: `gallery-top-open` / `gallery-bottom-open`
(`ui/shell.slint:83-84`, two-way bound to `GalleryRoot` at `ui/shell.slint:536-537`).
The mouse already toggles them directly in Slint, with mutual exclusion, in the
`top-arrow` / `bottom-arrow` `ChromeButton`s
(`ui/gallery/GalleryRoot.slint:551-577`).

## Decisions (keeper's, then the engineer's one-liner)

1. **Ctrl+Up / Ctrl+Down** are the drawer keys, in both styles. Chosen by the
   keeper over the "edge escape" option.
2. **Implementation is Slint-only, in `shell-kbd`.** The branch goes AFTER the
   `is-panel-open` reject (line 862) and BEFORE the Mosaic branch (line 867), so
   it wins in the Mosaic too. It mirrors the existing mouse toggle (including
   mutual exclusion) instead of going through Rust: the drawer state already
   lives in Slint, and adding nothing to `nav-move` keeps the Rust callback and
   its tests untouched.
3. The branch is gated on `!root.is-mutating && root.mounted-screen == 1`. The
   drawers only exist in the gallery, and while a mutation is in flight the
   keyboard must stay inert, exactly like `nav-move`'s own guard
   (`src/callbacks.rs:132-136`).
4. **Plain Up/Down keep their current meaning everywhere.** In the Slider they
   keep toggling the drawers (existing, documented V6.1 behaviour: `Ctrl` is an
   ADDITION there, not a replacement), and in the Mosaic they keep moving the
   cursor. Rationale: nothing that works today stops working, and the Slider's
   plain Up/Down stays byte-identical. If the keeper later wants the Slider to
   lose its plain Up/Down, that is a separate change.
5. While a drawer is OPEN, `gallery-keys` (`ui/gallery/GalleryRoot.slint:303-342`)
   owns the arrows and they already operate the drawers; no change needed there.

## Scope

- `ui/shell.slint`: the Ctrl branch in `shell-kbd` + the callback/property
  plumbing it needs (it toggles the existing `gallery-top-open` /
  `gallery-bottom-open` properties, so no new plumbing is expected).
- Tests in `src/shell/ui_tests.rs`.
- Do NOT touch `src/callbacks.rs`, `ui/panel/PanelRoot.slint`, or the FilterBar.

## Acceptance criteria

- With the gallery mounted in style 2 (Mosaic): Ctrl+Up opens the top drawer,
  Ctrl+Down the bottom one, and each closes the other.
- With the gallery mounted in style 0 (Slider): the same.
- Plain Up/Down in the Mosaic still move the cursor and never open a drawer.
- Plain Up/Down in the Slider behave exactly as before the change.
- Ctrl+arrows inside the open Settings panel keep their plain meaning (the panel
  rejects first at line 862).
- Slint confirms `event.modifiers.control` is deliverable: verified in
  `i-slint-compiler-1.17.0/builtins.slint:854-865`, and the modifier state is
  tracked by the window from individual modifier key events
  (`i-slint-core-1.17.0/window.rs:1040-1066`), so a test can hold Control.

## Checks

- Strict TDD is ON. Runner: `cargo test`. RED FIRST, and the test must drive the
  REAL keyboard path: dispatch `KeyPressed{text: Key::Control}`, then the arrow,
  then release both — the existing `focus_press_key`
  (`src/shell/ui_tests.rs:5093`) carries no modifiers, so a helper is needed.
- The test must exercise BOTH styles, not just the Mosaic.
- Visual (mandatory, the change is in `.slint`): render into a per-run directory
  and READ the PNG with vision, showing the drawer deployed by Ctrl+Up.
- Do not reformat unrelated code (house style: no blanket `cargo fmt`).

## Tasks

- [x] **T1 (RED)** — Failing test: Ctrl+Up / Ctrl+Down open the drawers in
  Mosaic and in Slider; plain Up/Down still move the Mosaic cursor.
- [x] **T2 (GREEN)** — `ui/shell.slint`: add the gated Ctrl branch before the
  Mosaic branch.
- [x] **T3 (VISUAL)** — Render and read the PNG of the deployed drawer.
- [x] **T4 (COMMIT)** — One Conventional Commit with tests and change together;
  record the hash here.

## Evidence log

**Head before the change**: `45e92b5` (`fix(motion): the built-in card applies its file, not its title`).

**T1 (RED)** — `cargo test ctrl_arrows_deploy_the_drawers_in_both_gallery_styles -- --nocapture`
on `45e92b5` with only the test added:

```
thread 'shell::ui_tests::ctrl_arrows_deploy_the_drawers_in_both_gallery_styles' (76982) panicked at src/shell/ui_tests.rs:15999:5:
Ctrl+Up must deploy the top drawer in the Slider
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test shell::ui_tests::ctrl_arrows_deploy_the_drawers_in_both_gallery_styles ... FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1301 filtered out; finished in 0.05s
```

Fails for the right reason: on the current head Ctrl+Up leaves the top drawer
closed (the arrow only reaches `nav-move`, which the headless test does not
wire). The keyboard path is real: `focus_press_key_with_ctrl` dispatches a
`Key::Control` press, then the arrow, then releases both, so the window's
tracked modifier state makes the arrow carry `modifiers.control`.

**T2 (GREEN)** — the Ctrl branch added in `shell-kbd` after the `is-panel-open`
reject and before the Mosaic branch:

```
test shell::ui_tests::ctrl_arrows_deploy_the_drawers_in_both_gallery_styles ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1301 filtered out; finished in 0.06s
```

**T3 (VISUAL)** — `HVE_RENDER_DIR=/tmp/opencode/render-ctrl-drawer-t3 cargo test ctrl_drawer_deploy_renders -- --nocapture`:

```
render artifacts for this run: /tmp/opencode/render-ctrl-drawer-t3
test shell::ui_tests::ctrl_drawer_deploy_renders ... ok
```

PNGs read with vision from that per-run directory:

- `/tmp/opencode/render-ctrl-drawer-t3/ctrl_top_drawer_slider.png` — the top
  Settings drawer is deployed above the carousel: the Save/Borders/Motion/
  Filters/System pills are visible, "Save" highlighted, top arrow glyph `▾`.
- `/tmp/opencode/render-ctrl-drawer-t3/ctrl_bottom_drawer_slider.png` — the
  bottom style drawer is deployed below the carousel: Slider/Mosaic pills,
  "Slider" highlighted.
- `/tmp/opencode/render-ctrl-drawer-t3/ctrl_top_drawer_mosaic.png` — in the
  Mosaic the top Settings drawer is deployed too (the Ctrl branch wins over the
  Mosaic arrow branch).

**Full suite** — `cargo test`:

```
test result: ok. 1303 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 86.45s
```

Baseline on this head was 1301 passed / 0 failed; the two new tests
(`ctrl_arrows_deploy_the_drawers_in_both_gallery_styles`,
`ctrl_drawer_deploy_renders`) account for 1303.

**T4 (COMMIT)** — `feat(shell): Ctrl+Up/Down deploy the drawers in both gallery styles`,
hash: `e64d2d2` (tests + change in one commit).
