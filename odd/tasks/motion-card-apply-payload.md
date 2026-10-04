# Motion: a built-in card must apply its FILE, not its translated title

**Opened**: 2026-10-04 (keeper's live test)
**Branch**: hve2-visual-rewrite
**Route**: delegated writer (2 files, sequential with the Ctrl+arrows task)
**Status**: planned

## Objective

Clicking a built-in animation card in Settings -> Motion must send the preset's
FILE name (`01_relampago.lua`) to `apply-animation`, exactly like Borders does,
so the preset really applies AND the tune pane on the right reads that preset's
curve/speed/style back.

## Problem (verified, not guessed)

`ui/panel/sections/MotionSection.slint:531-541` builds `SavedPresetCard` from
`builtin-names`, which is fed with `anim-titles` — the TRANSLATED TITLES
(`src/presets.rs:69-70`) — and never sets `apply-value`. `SavedPresetCard`
defaults that property to its own display name (`SavedPresetCard.slint:24`), so
the Apply switch sends the title (e.g. `"Lightning"`). Three consequences:

1. `src/main.rs:409-412` `read_animation_file("Lightning")` fails, so
   `sync_motion_tune_pane` returns through its silent `return` and the pane keeps
   the previous values. This is the symptom the keeper reported.
2. `engine.apply_animation("Lightning")` fails, so the desktop never changes.
3. The card still lights up as active and `cfg.active_anim_file` persists the
   bogus title into the config.

Borders is correct: `BordersSection.slint:1973` sets
`apply-value: root.builtin-files[i]`. The keyboard path is also correct
(`ui/panel/PanelRoot.slint:630` passes `anim-files`), and user presets are fine
because their name IS the file stem — which is why the existing tests missed
this.

## Scope

- Fix the mouse payload for built-in Motion cards, mirroring Borders.
- Do NOT touch: the Motion tune pane layout, the keyboard path, user presets,
  the parser (`src/animation_preset.rs`), or the engine.

## Acceptance criteria

- A real pointer click on the Apply switch of the first built-in Motion card
  emits `apply-animation(idx, "<file>.lua")`.
- The click never emits the displayed title.
- With the correct file, `sync_motion_tune_pane` already reads the preset back
  into the pane (existing behaviour, merely unreachable until this fix).

## Checks

- Strict TDD is ON. Runner: `cargo test`. RED FIRST: the new test must fail on
  the current head for the right reason (payload is the title, not the file).
- The new test must drive the REAL input path (pointer press/release at the
  switch coordinates), modelled on
  `borders_preset_card_click_applies_the_file_not_the_title`
  (`src/shell/ui_tests.rs:15045`). A property-fixing test would NOT see this
  class of defect.
- The Motion list pane sits lower than Borders' (it has a title + hint above the
  panes), so the switch coordinate must be taken from a real rendered frame, not
  copied from the Borders test.
- Visual (mandatory, the change is in `.slint`): run the headless render into a
  per-run directory and READ the PNG with vision, confirming the tune pane shows
  the clicked preset's values.
- Do not reformat unrelated code (house style: no blanket `cargo fmt`).

## Tasks

- [x] **T1 (RED)** — New test in `src/shell/ui_tests.rs`: click the Apply switch
  of the first built-in Motion card, assert the payload is the file name and is
  never the title. Run it on the current head and record the failure output.
- [x] **T2 (GREEN)** — `MotionSection.slint`: give `MotionListPane` a
  `builtin-files` property, pass `anim-files` from both of its instantiations,
  and set `apply-value: root.builtin-files[i]` on the built-in card loop.
  Re-run T1: green.
- [x] **T3 (VISUAL)** — Render and read the PNG proving the pane reflects the
  clicked preset.
- [x] **T4 (COMMIT)** — One Conventional Commit with the test and the fix
  together; record the hash here.

## Evidence log

- T1 RED (`cargo test motion_preset_card_click_applies_the_file_not_the_title
  -- --nocapture`, head `1a9b0df`): the click landed on the switch and the
  payload was the TRANSLATED TITLE, exactly the reported defect:

  ```
  thread 'shell::ui_tests::motion_preset_card_click_applies_the_file_not_the_title' panicked at src/shell/ui_tests.rs:7254:5:
  assertion `left == right` failed: clicking the built-in Motion card's switch must apply its FILE — got ["Lightning"]
    left: ["Lightning"]
   right: ["01_relampago.lua"]
  test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1299 filtered out; finished in 0.80s
  ```

  Coordinate derivation: the Motion list sits lower than Borders', so the
  Borders point (989, 986) would miss. The test renders a frame and finds the
  switch by its off-state ink (`HveColors.border` #30363d) with
  `first_color_band_bbox(BORDER_INK, ...)`, scanning below the header's 1px
  chrome line (y0=300). Detected switch bbox `(995, 975, 1034, 996)`, click at
  its centre `(1014, 985)`; the click genuinely fired the callback with the
  title, proving it landed on the Apply switch.
- T2 GREEN (`MotionSection.slint`: `MotionListPane.builtin-files`, wired from
  `root.anim-files` in both instantiations, `apply-value: root.builtin-files[i]`
  on the built-in loop):
  `cargo test motion_preset_ -- --nocapture` → both new tests `ok`
  (`test result: ok. 2 passed; 0 failed; ... 1299 filtered out`).
- T3 VISUAL — render dir `/tmp/opencode/render-motion-payload-t3`, PNG
  `motion_preset_apply_pane.png` (1920x1080). Test
  `motion_preset_apply_reflects_the_file_in_the_tune_pane` wires the production
  callback (`on_panel_apply_animation` → `sync_motion_tune_pane`), clicks the
  switch at `(1014, 985)`, and asserts the pane values. Read with vision: the
  card still DISPLAYS "Stylized 2.5D" while the tune pane shows
  `X1 — a 0.4`, `Y1 — b -0.3`, `X2 — c 0.2`, `Y2 — d 1.15`, `Speed 4.5`, and
  `Popin` highlighted — the applied file's values, not the stale defaults
  (0.25 / 0.1 / 0.25 / 1.0 / 2 / Slide).
- Full suite `cargo test`: `test result: ok. 1301 passed; 0 failed; 0 ignored`
  (baseline 1299 + the 2 new tests). No unrelated failures.
- T4 COMMIT — `45e92b5` `fix(motion): the built-in card applies its file, not
  its title` (2 files: `src/shell/ui_tests.rs`,
  `ui/panel/sections/MotionSection.slint`; test and fix together).
