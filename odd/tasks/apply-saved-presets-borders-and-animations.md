# ODD — Saved presets must apply (borders and animations)

Feature name: `apply-saved-presets-borders-and-animations`
Branch: `hve2-visual-rewrite` (current; not the default branch)
Status: implemented (T1-T3) by the writer 2026-10-01 — awaiting T4 cross-model verification and the
T5 keeper live check. Originally authorized by the keeper ("haz el arreglo para que se apliquen los
preajustes tanto en bordes como en animaciones"). Queued behind `settings-borders-visual-polish`
because two writers must never share one worktree.

## Objective

Applying a preset the keeper saved must actually apply the values they set — for borders AND for
animations. Today the preset is saved correctly and the panel shows the values, but the desktop
receives a generic fallback.

## Root cause (CONFIRMED, read-only investigation 2026-10-01)

The save path is correct and the file format round-trips (`PresetStore::generate_border_lua_full`,
`src/preset_store.rs:116`, called from `on_panel_save_border_preset` `src/main.rs:4429`; values from
`tune_params_from_window` `src/main.rs:348-364`; round-trip tests `src/preset_store.rs:501`, `:701`,
`:746`).

The failure is in **resolution**, not serialization:

- `border.sh` resolves the preset with `hve_resolve_preset "$HVE_BORDERS_DIR" "$PRESET_NAME"`
  (`assets/scripts/border.sh:18`).
- `HVE_BORDERS_DIR` is hard-coded to `$HVE_ASSETS_DIR/borders` (`assets/scripts/utils.sh:24`) — the
  BUILT-IN directory only. Nothing sets an override.
- A saved preset lives in `~/.config/hve/presets/borders/<name>.lua` (`src/preset_store.rs:23-29`), so
  the lookup fails and `border.sh:25-27` takes its security fallback:
  `hl.config({ general = { ["col.active_border"] = primary } })`. The desktop gets a plain `primary`
  border: no angle, no gradient, no inactive colour, no glow, no leaves.
- The telling asymmetry: the panel DOES show the values, because the Rust read-back
  (`PresetStore::read_border_file`, `src/preset_store.rs:261-278`) checks the user directory after the
  built-in one. The engine path never reads at all — it delegates resolution to the shell.

The same gap fires at save time: `on_panel_save_border_preset` re-applies with
`st.engine().apply_border(&name_str)` (`src/main.rs:4460`), so the live preview is replaced by the
fallback border the moment the keeper saves.

**Animations carry the identical gap**: `assets/scripts/apply_animation.sh:18` uses the same resolver
against the built-in animations directory only. The keeper asked for both.

## Decisions

- **D1 — Fix it in the shell resolver, not by copying files.** The user preset directory is the
  documented location; materializing a copy into the built-in plugin directory would pollute it and
  diverge from that documented layout. The resolver is the right place.
- **D2 — Built-in FIRST.** A user preset must never shadow a built-in of the same name. Search order:
  built-in directory, then the user directory — mirroring `read_border_file`'s order
  (`src/preset_store.rs:261-278`), so the shell and Rust agree.
- **D3 — The fallback must survive.** An unknown name must still produce the safe fallback
  (`border.sh:25-27`); that behaviour is a security property, not an accident.
- **D4 — Both scripts, one change.** Borders and animations share the resolver; fix them together, and
  keep the fix in `utils.sh` so a third caller cannot inherit the bug.
- **D5 — Scope.** Only what the keeper asked: the preset applies. The secondary defect (a user preset's
  active card marker is lost on section re-entry, because `reseed_active_border_index`
  `src/presets.rs:112-137` searches only the built-in model) is recorded as a follow-up, not fixed here
  unless it turns out to be a one-liner with an honest test.

## Tasks

- [x] **T1 — Failing test first, at the script contract level.** Extend `scripts_contract` so a preset
      written into a USER directory is resolved and its content reaches the fragment (borders and
      animations), and so an unknown name still produces the fallback. This is the honest place for the
      assertion: the existing contract test only walks built-ins (`scripts_contract.rs:1212`,
      `preset_names` reads `assets/borders` at `:1046`). DONE — four tests, RED before the fix.
- [x] **T2 — Extend the resolver** (`assets/scripts/utils.sh`) to accept the user directory as a second
      location, keeping built-in first, and update the two callers (`border.sh:18`,
      `apply_animation.sh:18`). DONE.
- [x] **T3 — Prove the end-to-end path**: a saved border preset applies its angle, gradient colours,
      inactive colour and glow; a saved animation preset applies its leaves; and the save-time
      re-apply (`src/main.rs:4460`) no longer replaces the preview with the fallback. DONE at the
      script-contract level (the UI click-through is T5).
- [ ] **T4 — Cross-model verification** (Tier 3: this is path/resolution handling that writes files).
- [ ] **T5 — Keeper's live check**: save a preset, apply it, and confirm the desktop takes the values —
      borders and animations.

## Acceptance criteria

- Applying a saved border preset applies the values the keeper set (angle, gradient colours, inactive
  colour, glow), not a plain `primary` border.
- The same holds for a saved animation preset.
- A built-in preset still wins over a user preset with the same name.
- An unknown name still yields the safe fallback.
- The panel and the desktop agree: what the pane shows is what gets applied.
- Nothing else in the scripts changes behaviour.

## Writer assignment and evaluation criteria (keeper's choice, 2026-10-01)

- **Writer: `opencode/mimo-v2.6-flash-free`** (free model, first run on real work — the keeper asked to
  evaluate it and wants a verdict on how it works).
- **Verifier: `glm-5.3-flash`** — never the writer's model (project rule).
- The verdict is reported with evidence, not impressions. Record, for the run:
  1. Did it follow STRICT TDD (a failing test seen RED before the fix)?
  2. Did it respect the file-ownership boundary (no `ui/` and no `src/shell/ui_tests.rs`)?
  3. Did it fix BOTH borders and animations, and keep built-ins first and the fallback intact?
  4. Full-suite and build results, exactly as observed.
  5. How many correction rounds the independent verification needed, and what it caught.
  6. Honest limits: what it could not do, and whether the result needed a second writer.

## Checks (resolved)

- TDD: **strict**. Failing test first. Runner: `cargo test` (the script contracts run from the Rust
  suite). No `cargo fmt`; no `git checkout -- .`/`reset`/`stash`/`clean`; no branch switching.
- **File ownership (avoids a writer collision):** touch only `assets/scripts/utils.sh`,
  `assets/scripts/border.sh`, `assets/scripts/apply_animation.sh`, `src/scripts_contract.rs` (this
  document names it `src/shell/scripts_contract.rs`; that path does not exist — the contract tests
  live at `src/scripts_contract.rs`, whose line refs 1046/1212 match the ones quoted above), and this
  document. Do NOT touch `src/shell/ui_tests.rs` or anything under `ui/` — another writer owns those
  for the card-centring item.
- Commit as one reviewable work unit, Conventional Commit, no AI attribution.

## Follow-ups (not this change)

- A user preset's active marker is lost on section re-entry (`src/presets.rs:112-137` searches only the
  built-in `border_files` model, not `user_border_preset_names`).
- The watcher cannot tell "asked" from "ran" for the palette defence
  (`~/.cache/hve/assert-color-authority.last`).

## Progress / evidence (writer: `opencode/mimo-v2.6-flash-free`, 2026-10-01)

T1-T3 done. T4 (cross-model verification) and T5 (keeper live check) are NOT the writer's to run and
remain open.

### What changed

- `src/scripts_contract.rs` — the contract tests (see the corrected path note in *Checks* above):
  - `OverlaySandbox` now also stages `apply_animation.sh` and the real `assets/animations/` set,
    pins `XDG_CONFIG_HOME` to `<root>/xdg` (deliberately DISTINCT from `$HOME/.config`, so a resolver
    that hardcoded the home path would fail instead of passing by accident), and gains
    `write_user_preset` (writes where `PresetStore::save` writes) and `apply_animation_preset`.
  - Four new tests, listed below.
- `assets/scripts/utils.sh` — new `HVE_USER_PRESETS_DIR`
  (`${HVE_USER_PRESETS_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/hve/presets}`, mirroring
  `PresetStore::new`/`dirs::config_dir()`); the resolver is split into `_hve_preset_in_dir` (one
  directory) and `hve_resolve_preset <built-in dir> <name> [<user dir>]` — built-in searched FIRST,
  user directory second, `return 1` when neither matches, so the callers' safe fallback still fires.
  Two-argument callers keep working unchanged.
- `assets/scripts/border.sh`, `assets/scripts/apply_animation.sh` — pass
  `"$HVE_USER_PRESETS_DIR/borders"` / `"$HVE_USER_PRESETS_DIR/animations"` as the third argument.
  No other behaviour changed.

### New test names

- `scripts_contract::saved_user_border_preset_reaches_the_overlay`
- `scripts_contract::saved_user_animation_preset_reaches_the_overlay`
- `scripts_contract::unknown_preset_name_still_takes_the_safe_fallback`
- `scripts_contract::built_in_preset_wins_over_user_preset_with_the_same_name`

### RED (strict TDD, before the fix)

`cargo test --bin hve -- saved_user_border_preset_reaches_the_overlay saved_user_animation_preset_reaches_the_overlay unknown_preset_name_still_takes_the_safe_fallback built_in_preset_wins_over_user_preset_with_the_same_name`

- Run 1 (helpers wrote the user preset under `$HOME/.config`), against the unmodified scripts:
  `test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 1224 filtered out`.
  The border failure showed the overlay holding
  `hl.config({ general = { ["col.active_border"] = primary } })`; the animation failure showed
  `hl.config({ animations = { enabled = true } })` — the two fallbacks, i.e. the defect itself.
- Run 2 (helpers re-pointed at a distinct `XDG_CONFIG_HOME`, originals restored from `HEAD`):
  `test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 1224 filtered out`.
  Logs: `/tmp/opencode/hve-presets-fix/red.log`, `red-xdg.log`.
- The fallback and built-in-first tests passed BEFORE and AFTER the fix — they pin behaviour that
  must never change (D2, D3), so they are guards, not the RED signal.

### GREEN (after the fix)

- Focused (same command): `test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 1224 filtered out`.
- Full suite: `cargo test` → `test result: ok. 1228 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 74.64s` (exit 0).
- `cargo build` → `Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.68s`, zero warnings
  (the test log has 0 lines starting with `warning` too).

### Commit

One work-unit commit on `hve2-visual-rewrite`, Conventional Commit, no AI attribution. A commit cannot
embed its own SHA inside the document that commit carries, so the SHA is recorded in the closing
verification report instead of here; `git log -1` on that commit shows it.

### Not done / not verified by the writer

- T4 cross-model verification and T5 keeper live check — open.
- `shellcheck` is not installed on this machine (`which shellcheck` finds nothing): the three scripts
  were executed end to end by the test suite under `bash`, but never linted.
- The save-time re-apply (`src/main.rs:4460`) is proven only at the script-contract level: it calls
  `border.sh <name>`, exactly the call the new tests exercise. The click-through inside the Slint
  window is T5 (and `src/shell/ui_tests.rs` is another writer's file).
- No visual/render verification: nothing `.slint` or under `ui/` was touched.
- Ownership respected: `git status` shows changes only in `assets/scripts/{utils,border,apply_animation}.sh`,
  `src/scripts_contract.rs` and this document.
