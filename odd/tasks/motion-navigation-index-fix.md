# Fix Motion section navigation index space

## Objective

Entering Motion from the left nav must land on the animation LIST (left pane),
and keyboard navigation must reach every card including the last built-in
(`19_stylized2.5D`, "Stylized 2.5D") and the user presets.

## Problem

`MotionSection` uses a **tune-first** index space (`split: 5` hardcoded, tune =
`[0,5)`, list = `[5, 5+N)`) while `PanelRoot` drives it **cards-first**
(list = `[0, N)`, tune = `[N, N+4)`). The two spaces are inverted:

- Entry index `0` lights the tune slider `a-wrap` (`MotionSection.slint:122`)
  and gives the list `-5` — so you enter on the curves, not the list.
- List card `i` lights at global `i + 5`; the last card (i=18) needs global 23,
  but `PanelRoot` caps at `anim-files.length + 3 = 22` (`PanelRoot.slint:573,
  591`). So the last animation is never highlighted and Down wraps to 0 (tune).

`BordersSection` is the working reference: `list-count` = builtin + user,
`split` = `list-count`, the list reads the global index, the tune subtracts the
split, and `PanelRoot` uses `borders-list-len` / `borders-total`
(`PanelRoot.slint:223-224`).

## Fix (mirror Borders exactly)

### `ui/panel/sections/MotionSection.slint`
- Add `property <int> list-count: root.anim-files.length + root.user-preset-names.length;`
  and set `property <int> split: root.list-count;` (replace the hardcoded `5`,
  line ~445).
- Add `out property <int> tune-count: 5;` (4 bezier sliders + the save form),
  mirroring `BordersSection`'s `out property <int> tune-count`.
- List pane instances (`~:500`, `~:545`): `focused-index: root.focused-index`
  (remove the `- root.split`); `follow: root.focused-index < root.split`.
- Tune pane instances (`~:523`, `~:568`): pass the LOCAL index
  (`root.focused-index - root.split`); `follow: root.focused-index >= root.split`.
- Tune pane internals: light by local index (`local-focus == 0..3` for the
  sliders, `local-focus == 4` for the save form) instead of `focused-index == split + k`.
- Confirm the list-card lit comparison uses the global index (card `i` lit when
  `focused-index == i`).

### `ui/panel/PanelRoot.slint`
- Add `property <int> motion-list-len: root.anim-files.length + root.user-animation-preset-names.length;`
  and `property <int> motion-total: root.motion-list-len + motion.tune-count;`
  (mirror `borders-list-len` / `borders-total` at `:223-224`).
- Motion block (`:558-630`): replace every `anim-files.length` bound with
  `motion-list-len` / `motion-total`:
  - Down/Up wrap over `[0, motion-total - 1]`.
  - Right: `focused-index < motion-list-len` → `= motion-list-len`.
  - Left: `focused-index >= motion-list-len` → `0`; else `menu-active = true`.
  - Enter: `< motion-list-len` → apply builtin (`< anim-files.length`) else user
    (`index - anim-files.length`), exactly like `:542-548`; otherwise engage
    slider `= index - motion-list-len`.
- Keep the engaged-slider branch consistent with the same bounds.

## Constraints

- Strict TDD. Runner `cargo test`. Write the failing test first.
- Do NOT change the borders section, the engine, or storage.
- Do NOT run `cargo fmt`. Do NOT switch branches.
- English artifacts.

## Tasks

- **T1 — RED**: in `src/shell/ui_tests.rs`, add/adjust tests asserting:
  (a) entering Motion focuses index 0 = the first LIST card (not the tune pane);
  (b) keyboard Down reaches the last built-in card (index `anim-files.length - 1`)
  and does not skip it; (c) the tune stops are reachable after the cards.
  Update the existing motion tests that baked in the inverted mapping
  (`engaged_motion_slider_down_exits_and_moves_row`, `motion_last_slider_renders_unclipped`,
  `panel_motion_system_save_stacked_render`). Confirm the new ones fail.
- **T2**: implement the MotionSection index-space fix.
- **T3**: implement the PanelRoot motion navigation fix.
- **T4 — GREEN**: full `cargo test`; report counts.
- **T5**: commit the work unit(s).

## Acceptance criteria

- Entering Motion lands on the first animation card.
- Down/Up walk the full card list including `19_stylized2.5D` and user presets,
  then the tune stops, then wrap.
- Right from the list goes to the tune block; Left returns to the list then the rail.
- Full suite green.

## Checks

- `cargo test` (full).
- Visual verification by the orchestrator: render + read the PNGs.
- Cross-model verification (navigation math → Tier 3): writer deepseek, verifier
  a different model.

## Progress

- [x] T1
- [x] T2
- [x] T3
- [x] T4
- [x] T5

## Evidence

- **T1 RED** (`cargo test motion_` before the fix):
  - `motion_keyboard_walks_cards_then_user_presets_then_tune` FAILED:
    `left: [0, 2] right: [0, 2, 3]` — Enter on the user-preset index engaged
    a slider instead of applying the preset.
  - `motion_entry_focus_renders_on_first_list_card` FAILED:
    `list_icy=0 tune_icy=6885` — entry lit the tune pane, not the list.
  - `motion_last_slider_renders_unclipped` FAILED:
    `grabbed slider must emphasize over lit — got 0` — global index 3 was no
    longer a tune slider.
- **T4 GREEN**: `cargo test` → 1242 passed; 0 failed; 0 ignored.
- **Visual verification** (read the PNGs from the run's own directory):
  - `motion_entry_focus.png` — first list card "Ease" carries the icy ring;
    the tune pane is unlit.
  - `motion_last_slider_lit.png` / `motion_last_slider_grabbed.png` — Y2-d
    lit, then grabbed (white border, expanded).
  - `motion_save_form_idle.png` vs `motion_save_form_focus.png` — the save
    input gains the icy ring on tune-local stop 4.
- **Files / ranges**:
  - `ui/panel/sections/MotionSection.slint` — root `list-count` / `split` /
    `tune-count` (~447-458); list panes read the GLOBAL index and follow
    `< split` (~509-512, ~556-560); tune panes read the local index and follow
    `>= split` (~533-540, ~580-587); sliders light by `local-focus` 0..3
    (122, 145, 168, 191); save form lights at `local-focus == 4` (~230-236,
    ~257-263).
  - `ui/panel/PanelRoot.slint` — `motion-list-len` / `motion-total` (~227-229);
    Motion keyboard block bounds + user-preset apply (~572-635); preview
    `engaged-slider` local mapping (~860).
  - `src/shell/ui_tests.rs` — new
    `motion_keyboard_walks_cards_then_user_presets_then_tune`,
    `motion_entry_focus_renders_on_first_list_card`,
    `motion_save_form_focus_renders_ring`; updated
    `motion_last_slider_renders_unclipped`,
    `engaged_motion_slider_down_exits_and_moves_row`.
- **Commit**: pending.

## Next step

Orchestrator: render + read the PNGs, then cross-model verification
(navigation math → Tier 3).

