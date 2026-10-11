# Feature: a saved preset is recorded, and a stored preset name always resolves

**Locator**: `odd/tasks/preset-identity-recorded-and-name-tolerant.md`
**Engram mirror**: topic `odd/preset-identity-recorded-and-name-tolerant/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: IN PROGRESS — started 2026-10-11 16:02, after the active-theme-hero work closed and released
`src/main.rs`. The keeper authorized it earlier the same day ("cuando terminemos lo que se está haciendo
lanzas a continuación esto que hemos hablado").
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~120–200 authored changed lines → single PR.
**Route**: delegated writer (`src/main.rs`, `src/presets.rs`, `src/theme_presets.rs`), parent verification
including the keeper's live look.
**Restore point before starting**: tag `restore-point/pre-preset-identity-2026-10-11` + a tarball of
`~/.config/hve/themes` and `~/.config/hve/presets` in `~/Proyectos/hve-backups/`.

## Objective

Two defects with one visible face: the keeper's Space theme carries a border preset that the live desktop
applies, the Settings → Borders pane does not mark, and the theme package does not ship. Both are real and
verified by reading the code, not inferred from the symptom.

## Bug 1 — saving a preset does not record it as the active preset

- `src/main.rs` `on_panel_save_border_preset` (~:5180): writes the file (`PresetStore::save` → `<name>.lua`),
  then applies it LIVE with the bare name (`engine.apply_border(&name_str)`) and clears `tune_dirty` — but
  never writes `cfg.active_border_file`.
- `src/main.rs` `on_panel_save_animation_preset` (~:5400): identical shape for `cfg.active_anim_file`.
- The ONLY writers of those two fields are the preset CARD click (`src/main.rs` ~:5104–5106 and
  ~:5154–5156, which stores the full `"<name>.lua"`) and `on_reset_presets` (`src/callbacks.rs:456–457`,
  which clears them).
- Consequence: saving a preset and then saving a theme records the OLD or EMPTY preset in the theme's
  `state.json` (`src/providers/hve_presets.rs` `save` reads `Config::load()` verbatim). The look is applied
  live and does not travel. This affects every preset saved from now on.

## Bug 2 — two conventions for one name

- Canonical form, verified: `assets/scripts/scan.sh` emits `"file": "<basename with extension>"`, so the
  window's preset lists hold `13_the_joker.lua`. `src/shell/ui_tests.rs` also pins `"14_looper.lua"`.
- `src/presets.rs` `resolve_active_index` / `reseed_active_border_index`: exact string equality against that
  list → a bare `"Space"` matches nothing, the index becomes `-1`, a warning is logged and the marker clears.
- `src/theme_presets.rs` `plan_bundle` / `bundle_theme_presets`: builds `user_presets_root/<category>/<stored
  name>` with no extension handling → a bare `"Space"` is not found, is warned, and is SKIPPED, so the theme
  package never gets its `presets/borders/Space.lua` and never installs it on another machine (T4).
- Yet `assets/scripts/utils.sh` `_hve_preset_in_dir` IS tolerant (a name without a dot probes `<name>.lua`),
  and `src/main.rs:286` `sync_border_tune_pane` strips `.lua` before `PresetStore::read_border_file`, which
  appends it. So the live border applies and the tune pane reads it, while the marker and the bundler do not.
- Consequence: a legacy, imported or hand-made theme (Space's `state.json` holds `"active_border_file":
  "Space"`) shows no selected border and carries no preset.

## Scope (authorized)

- **U1 — Bug 1**: after a successful preset save from the tune pane, record the new preset as the active one
  in the config, for BOTH borders and animations, so the live state and the recorded state agree. Decide and
  document whether the save also has to refresh the pane's marker (the section re-seeds on entry).
- **U2 — Bug 2**: one canonical form (full `"<name>.lua"`) enforced at ONE boundary, plus tolerant reading in
  the marker and the bundler, so a legacy bare name resolves instead of vanishing. Prefer canonicalising on
  write AND tolerating on read; do not add a third convention.
- **U3 — tests**: (a) saving a border preset records it as active; (b) same for an animation preset; (c) a
  stored bare name still resolves in the marker; (d) a stored bare name still bundles its `.lua` into the
  theme's `presets/`; (e) a full `"<name>.lua"` keeps working everywhere (no regression); (f) a name that
  matches no preset is still an explicit `-1` with a warning, never a silent blank.
- **U4 — parent**: `cargo test`, `cargo check --all-targets`, release build, backup + install
  `~/.local/bin/hve`, relaunch with the window shown for the keeper's live look.

## Out of scope

- JoKer's animation `"prueba1"`: the file does not exist anywhere, so tolerance cannot fix it. The keeper
  picks a real animation and re-saves; document it, do not "repair" it in code.
- The colour path (the border colour travels in the theme palette; not a defect).
- Any UI redesign of the Borders/Animations panes, the preset lists or the pagination.
- No new dependency.

## Acceptance criteria

1. `cargo test` green, `cargo check --all-targets` clean.
2. Save a border preset from the tune pane, then save a theme: the theme's `state.json` records that preset
   and bundles its `.lua` under `presets/borders/`.
3. Same for an animation preset.
4. With the existing Space theme applied: its border card appears SELECTED in Settings → Borders, and saving
   that theme bundles `Space.lua` into the package.
5. A shipped full name (`13_the_joker.lua`) still resolves exactly as today.
6. A name matching nothing stays an explicit `-1` with a warning.
7. The keeper sees it live: marker present, border applied, and a re-saved Space theme carrying its preset.

## Checks

- `cargo test`
- `cargo check --all-targets`
- Release build + backup + install `~/.local/bin/hve` + relaunch (keeper's live look).

## Work units

- [ ] **U1** — a saved preset is recorded as the active preset (borders + animations).
- [ ] **U2** — one canonical name form + tolerant read in the marker and the bundler.
- [ ] **U3** — the tests above.
- [ ] **U4** — the parent's build, install, relaunch and the keeper's live look.

## Progress log

- 2026-10-11: both defects found in a read-only review while the active-theme-hero writer ran; queued by the
  keeper to start right after it. NOT started.
