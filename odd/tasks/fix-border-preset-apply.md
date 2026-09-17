# Fix: applying a built-in border preset with the mouse sends the title, not the file

## Objective

Clicking a built-in border preset card in the Borders list must apply **that preset**,
the same preset the keyboard already applies from the same card.

## Problem

`BordersListPane` receives the **translated titles** (`builtin-names: root.border-titles`)
and its card fires `apply-preset(index, preset-name)` with that displayed title. The title
travels all the way to `assets/scripts/border.sh`, which resolves presets **by file name
only** (`$HVE_BORDERS_DIR/$name.lua`, `assets/scripts/utils.sh:37`). Resolution fails, so
the script's security fallback writes a flat generic border:

```lua
hl.config({ general = { ["col.active_border"] = primary } })
```

The script still exits 0, and the app only logs `Err`, so the failure is silent: the border
changes to a generic flat one, which reads as "the preset does not apply".

## Evidence (collected before the fix)

- `assets/fragments/border.lua` held exactly the 61-byte fallback line (mtime 16:50,
  matching the keeper's live report).
- `hve_resolve_preset "El Joker"` → FAIL; `"13_the_joker"` / `"13_the_joker.lua"` → OK
  (`assets/borders/13_the_joker.lua`).
- `~/.config/hve/config.json` stored `active_border_file: "Infinito"` — the Spanish title of
  `07_infinity` — which never matches `PresetInfo.file`, so the active highlight is lost too.
- Asymmetry proving the diagnosis: the keyboard path (`PanelRoot.slint:505`) passes
  `root.border-files[idx]` and works; the mouse path passes the title and fails.
- Origin: commit `e366392` (2026-09-13, BordersSection rework). Pre-existing; not introduced
  by the `border-colors-single-strip` work.

## Scope

In scope (authorized):

- Clicking a built-in border preset card applies the preset **file** (`NN_name.lua`).
- Both `BordersListPane` instantiations (wide two-column and narrow stacked).

Out of scope (flag, do not fix here):

- The same-class bug in `MotionSection.slint` (animation presets apply `anim-titles`).
- Saved user presets: they live in `~/.config/hve/presets/borders/` while `border.sh`
  resolves only inside `assets/borders/`.
- Any engine file (`src/engine.rs`, `src/config.rs`, `src/settings.rs`,
  `src/theme_manager.rs`, `src/app_state.rs`, `src/watcher.rs`, `src/utils.rs`,
  `src/providers/`).

## Constraints

- Strict TDD: the failing test comes first, and it must be observed failing for the right
  reason. Test runner: `cargo test`.
- Visual verification is mandatory for `.slint` changes: run the headless render tests and
  read the PNGs.
- Keep the title as the **display** text. Only the apply argument changes.
- Do not change the language or wording of any UI string.

## Tasks

- [x] B1 [RED] Add `borders_preset_card_click_applies_the_file_not_the_title` to
      `src/shell/ui_tests.rs`: mount the panel on the Borders section, set
      `border_titles = ["Cascada"]` and `border_files = ["01_cascade.lua"]`, dispatch a real
      mouse press+release over the first preset card, capture the `panel_apply_border`
      argument, and assert it is `"01_cascade.lua"` and NOT `"Cascada"`. Must fail before the
      fix, capturing `"Cascada"`. Observed RED: `got ["Cascada"]` vs `right: ["01_cascade.lua"]`.
- [x] B2 [GREEN] `ui/panel/sections/SavedPresetCard.slint`: add
      `in property <string> apply-value: root.preset-name;` and emit
      `apply-preset(root.index, root.apply-value)` for the click.
- [x] B3 [GREEN] `ui/panel/sections/BordersSection.slint`: add `in property <[string]> builtin-files;`
      to `BordersListPane`, bind `apply-value: root.builtin-files[i]` on the built-in card,
      and pass `builtin-files: root.border-files;` from BOTH instantiations.
- [x] B4 [REFACTOR] Full suite green (`cargo test`), including the 3 strip tests and the
      existing `panel_apply_border` tests. Result: `740 passed; 0 failed`.
- [x] B5 Visual verification: run the border render tests, read the PNGs, confirm the list and
      the tune pane still render unchanged. `panel_borders_two_col.png`,
      `panel_borders_stacked.png` and `borders_focus_card0.png` are byte-identical in size to
      the pre-fix renders and still show the translated titles.

## Acceptance criteria

1. Clicking the first built-in preset card applies `01_cascade.lua`.
2. The card still DISPLAYS the translated title.
3. `cargo test` is fully green.
4. No layout change is visible in the border renders.

## Checks

- `cargo test borders_preset_card_click_applies_the_file_not_the_title` (RED, then GREEN)
- `cargo test` (full suite)
- `cargo test slice_focus_flow_renders` + `cargo test borders_` (render PNGs, read them)

## Progress

- Diagnosis complete, evidence attached above.
- B1–B5 done. RED observed capturing `"Cascada"`; full suite `740 passed; 0 failed`;
  render PNGs regenerated and reviewed (no visual change).
- The interspersed `border-colors-single-strip` PR 2 work was closed and committed first
  (`17256b5` artifacts, `0d271a1` strip + tests) by reverting this fix's hunks, verifying the
  PR-2-only tree at `739 passed / 0 failed`, committing, and restoring these three files
  byte-identically (sha256 checked against `/tmp/opencode/fix-restore/SHA256SUMS`).
- This fix is committed as its own work unit immediately after that slice.

## Verification

- RED before the fix captured `["Cascada"]`; GREEN after. Parent re-ran the load-bearing test:
  `1 passed; 0 failed`.
- Shell: `hve_resolve_preset "$HVE_BORDERS_DIR" "01_cascade.lua"` → OK; the title → exit 1.
- `border.sh` has no `exit` of its own, so a resolve failure still surfaces as `Ok` from
  `run_script` — that is why the defect was silent.
- Independent read-only verifier (high-tier requirement): no counterexample. It confirmed the
  captured argument can only originate from the card's switch `TouchArea`, that the test is
  load-bearing (it fails if `apply-value` is reverted), and that no `SavedPresetCard` caller
  regressed. It also confirmed the trace UI → `main.rs:2853` → `engine.apply_border` →
  `border.sh`.
- "Byte-identical" renders were compared by file size plus visual inspection, not by hash.

## Known gaps (disclosed, not hidden)

- The new test covers only the wide two-column pane. The narrow instantiation (`:2515`) is
  plumbed the same way but has no test.
- The test stops at the callback: it never executes `border.sh`.
- Live state at close: `~/.config/hve/config.json` has `active_border_file: ""` (clicking the
  active card toggles it off) and `assets/fragments/border.lua` still holds the fallback line.
  The running process predates this fix, so a rebuild + restart is required to observe it.

## Next step

- Restart HVE (the binary was rebuilt after this fix) and re-apply the preset: click El Joker
  and confirm `assets/fragments/border.lua` now holds the joker content, not the fallback line.
- Not authorized yet, tracked follow-ups: the same-class animation bug in `MotionSection.slint`
  (built-in animation cards still send `anim-titles`), and saved user presets that cannot
  resolve because the shell scripts search only `assets/borders/`.
