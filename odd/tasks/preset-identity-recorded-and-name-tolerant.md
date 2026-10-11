# Feature: a saved preset is recorded, and a stored preset name always resolves

**Locator**: `odd/tasks/preset-identity-recorded-and-name-tolerant.md`
**Engram mirror**: topic `odd/preset-identity-recorded-and-name-tolerant/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: U1–U3 DONE pending review; the parent's U4 (release build, backup + install `~/.local/bin/hve`,
relaunch, the keeper's live look) is outstanding. Nothing pushed. Was IN PROGRESS from 2026-10-11 16:02, after the
active-theme-hero work closed and released `src/main.rs`. The keeper authorized it earlier the same day ("cuando
terminemos lo que se está haciendo lanzas a continuación esto que hemos hablado").
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

- [x] **U1** — a saved preset is recorded as the active preset (borders + animations). Commit `3c1f9dd`.
- [x] **U2** — one canonical name form + tolerant read in the marker and the bundler. Commit `6f1cc7f`.
- [x] **U3** — the tests above (a)–(f): RED observed first for every behaviour change; see the log.
- [ ] **U4** — the parent's build, install, relaunch and the keeper's live look.

## Progress log

- 2026-10-11: both defects found in a read-only review while the active-theme-hero writer ran; queued by the
  keeper to start right after it. NOT started.
- 2026-10-11 (**U1**, commit `3c1f9dd`): RED observed first — the construction pin on both save-handler bodies
  failed with "must record the just-saved preset as the active one" while the extracted handler body showed no
  recording at all; the two behaviour tests then failed to compile against the missing helper. Landed
  `record_saved_preset_active` + `SavedPresetSlot` in `src/main.rs`, called by both handlers just before the live
  re-apply, followed by `cfg().save()`. Both record the canonical `<name>.lua`, whose form is defined once as
  `PresetStore::canonical_preset_file` (the mirror of `_hve_preset_in_dir`, applied on write).
  **Decision (U1)**: the save does NOT refresh the pane marker. The marker is a view derived from the recorded
  config value against the window's lists, and the Borders section already re-derives it from the persisted value
  on every entry (`seed_borders_pane_from_persisted`); recording once keeps a single source of truth instead of a
  second, save-local copy that could drift. Handlers verified by construction, the rule behaviourally.
  `cargo test` → 1563 passed / 0 failed.
- 2026-10-11 (**U2**, commit `6f1cc7f`): RED observed first — 4 assertion failures against the pre-fix code:
  `plan_bundle` planned `file_name: "Space"`, the bundle copied nothing (`left: []` vs `[ {borders, Space.lua} ]`),
  the stale cleanup removed the file the plan still listed, and `reseed_active_border_index("Space")` returned `-1`
  instead of the user card's combined index `3`. Landed `PresetStore::preset_files_match` (canonical equality; an
  empty reference never matches), canonicalisation inside `plan_bundle` (once, at the boundary that decides the
  destination name) and a tolerant `reseed_active_border_index`.
  **Finding 1**: `reseed_active_border_index` resolved against the scanned BUILT-IN files alone, so tolerance by
  itself would NOT have marked the keeper's card: `Space.lua` is a USER preset, and user cards continue the same
  index space in `BordersListPane` (`builtin-names.length + i == active-index`). The resolution now covers the
  panel's combined list (built-in files first, then the user stems offset by the built-in count) — the same index
  space a card click reports, so click and re-entry can never disagree.
  **Finding 2**: `remove_stale_bundled_presets` compares the on-disk name with the plan name, so canonicalising
  only the source path would have written `Space.lua` and then deleted it as "stale" in the same run. The canonical
  form had to reach the PLAN, which is where it now lives.
  **Finding 3**: the card click stores a USER preset as a bare stem (`PanelRoot` passes
  `user-border-preset-names[i]`, a stem) — that is where the bare `Space` came from. The canonical form on write
  would have to land in the click handler too; left untouched deliberately (out of the authorized scope, and the
  tolerant read makes the stem harmless), and recorded here so the next pass can decide it with the evidence.
  `cargo test` → 1573 passed / 0 failed.
- 2026-10-11 (**U3**): the tests the contract lists, all with a RED observed before the change that makes them
  pass. (a) + (b) `saving_a_border_preset_records_it_as_the_active_border` /
  `saving_an_animation_preset_records_it_as_the_active_animation` (commit `3c1f9dd`), plus the construction pin
  `both_tune_pane_save_handlers_record_the_saved_preset_as_active`. (c)
  `test_populate_metadata_resolves_a_bare_stem_against_the_scanned_file` and
  `reseed_marks_a_user_border_card_from_a_legacy_bare_stem` (the combined user-card index).
  (d) `plan_bundle_canonicalises_a_legacy_bare_stem`, `bundle_resolves_a_legacy_bare_stem_to_its_lua_file`,
  `bundle_keeps_a_bundled_file_for_a_legacy_bare_stem` and, end to end through the real provider save path,
  `save_bundles_a_legacy_bare_stem_preset_into_the_package`. (e)
  `test_populate_metadata_keeps_the_full_name_and_clears_an_unmatched_one` (full names unchanged) and
  `canonical_preset_file_adds_the_extension_only_to_a_bare_stem` (a `*.json` record is left verbatim).
  (f) `preset_files_match_tolerates_the_legacy_stem_and_stays_strict_otherwise` and the unmatched half of the
  marker tests — an explicit `-1`, never a silent blank. Extra guard:
  `plan_bundle_never_bundles_a_bare_stem_that_canonicalises_to_a_shipped_preset`.
  Out of scope, unchanged: JoKer's `active_anim_file: "prueba1"` names no file anywhere, so tolerance cannot fix
  it — the keeper picks a real animation and re-saves.
- 2026-10-11 (**U1–U3 closed for review**, nothing pushed): `cargo test` → 1573 passed / 0 failed;
  `cargo check --all-targets` → clean. Files touched: `src/main.rs`, `src/preset_store.rs`, `src/presets.rs`,
  `src/theme_presets.rs`, `src/providers/hve_presets.rs`, `src/shell/ui_tests.rs`. No dependency added, no theme
  data rewritten, no migration. U4 (build, backup + install, relaunch, the keeper's live look) belongs to the
  parent.
