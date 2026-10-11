# Feature: a saved preset is recorded, and a stored preset name always resolves

**Locator**: `odd/tasks/preset-identity-recorded-and-name-tolerant.md`
**Engram mirror**: topic `odd/preset-identity-recorded-and-name-tolerant/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: U1–U3 DONE pending review; **U5–U9 (round 2) DONE**; **U10–U12 (round 3, the second
verification's LOW residuals) DONE**, nothing pushed. The parent's U4 (release build, backup + install
`~/.local/bin/hve`, relaunch, the keeper's live look) is
outstanding. Was IN PROGRESS from 2026-10-11 16:02, after the
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

## Corrections — round 2 (the independent verification's findings)

An independent verification of U1–U3 (different model, fresh context) found two real defects — one of them a
REGRESSION U1 itself introduced — plus small ones. The scope is to fix them, not to redesign anything else.

- **U5 — MEDIUM, the regression U1 introduced**: the card-click deactivation toggle compared by EXACT equality
  (`st.cfg().active_border_file == file_str`, `src/main.rs` ~:5179 and ~:5229). U1 records the CANONICAL
  `<name>.lua` while a user-preset card still sends the bare STEM (`SavedPresetCard.slint:24` defaults
  `apply-value` to `preset-name`, and `PresetStore::list` returns stems), so clicking the ACTIVE card of a
  preset saved from the tune pane re-applied it and wrote the stem back into the config instead of
  deactivating it. Both halves fixed: a tolerant compare (`preset_files_match`) so the two spellings are the
  same preset, and canonicalising the recorded value (the click is where the bare stem came from), so the
  config stops flip-flopping between the two forms.
- **U6 — MEDIUM**: `PresetStore::canonical_preset_file` kept any name carrying a dot verbatim while
  `PresetStore::save` ALWAYS writes `format!("{}.lua", name)`, and `validate_name` allows interior dots
  (`src/preset_store.rs:75-78`). Saving `my.preset` therefore wrote `borders/my.preset.lua` and recorded
  `my.preset` — a name with no file: the marker warned and cleared, the bundler skipped it and the live
  re-apply failed (observed in the shell resolver, see the log).
- **U7 — LOW**: the record was written and persisted BEFORE the live re-apply (`src/main.rs` ~:5292-5306 and
  the Motion twin), so a failed apply left the config naming a preset the desktop never showed; and
  `let _ = st.cfg().save()` discarded the write error, so a config write failure silently reintroduced the
  Bug 1 symptom (the theme saved next recorded the OLD preset).
- **U8 — LOW**: the Borders save refreshed `user_border_preset_names` (stems, sorted) without re-deriving the
  index, so the untouched `active_border_index` could highlight a card that was never active until the section
  was re-entered.
- **U9 — trivial**: `src/preset_store.rs` had lost the newline inside the signature of the SECURITY test
  `save_rejects_traversal_and_writes_nothing_outside_base()`.

### Round-2 acceptance

1. `cargo test` green, `cargo check --all-targets` clean.
2. Clicking the ACTIVE card of a preset saved from the tune pane DEACTIVATES it — border and Motion.
3. A click on any other card records the canonical `<name>.lua`.
4. A preset saved as `my.preset` records `my.preset.lua`, and that record resolves in the marker and bundles.
5. A legacy `*.json` record is still honoured verbatim, and a bare `Space` still resolves to `Space.lua`.
6. A failed live apply leaves the config on the previous preset, and a config write failure is logged.
7. After a save the Borders marker marks the card that was just saved, not the previously active one.

## Corrections — round 3 (the second verification's LOW residuals)

The independent verification of round 2 returned `verified_with_findings`: `verified`, with four LOW
residuals. Three are closed here; the fourth is registered below as pre-existing residue and left alone.
The scope is to close them, not to redesign anything.

- **U10 — LOW**: `PresetStore::save` ALWAYS writes `<name>.lua`, so a name typed as `Space.lua` wrote
  `Space.lua.lua`. The canonical re-apply that follows resolves the OLD `Space.lua`, so the preset just saved
  was never applied and nothing warned — `border.sh` / `apply_animation.sh` exit 0 on a miss, so the shell
  treats "not found" as success with a safe fallback, and the record-only-on-success guard cannot bite
  because a miss is not a failure. The store now REFUSES a typed name that already ends in a container
  extension (`lua`, `frag`), names the stem to type instead, and the pane shows it in its existing save-error
  field. Nothing is written and nothing is recorded. The legacy record extensions (`json`, `ron`) are
  deliberately not refused: `canonical_preset_file` honours them verbatim on the read side by design.
- **U11 — LOW**: the Borders save refreshed `user_border_preset_names` in EVERY branch, while the marker
  re-derive U8 added lived only in the live-apply SUCCESS branch. A failed apply therefore left a refreshed
  list with an untouched index: since `PresetStore::list` sorts bytewise, a newly saved stem can sort before
  the card the index points at, and the board then highlighted a preset that is not active until the section
  was re-entered — the U8 defect surviving in a rarer branch. The re-derive now runs after the branch, from
  the value the config holds once the save is done, in both tune panes. Motion had no re-derive at all in any
  branch and gained the twin (`reseed_active_anim_index`, resolving against the panel's combined list).
- **U12 — LOW**: the U8 fixture stated an order the real list cannot produce. It claimed the refreshed user
  list keeps `aa_first` first after saving `Space`, but `PresetStore::list` sorts the stems bytewise, so
  `Space` (0x53) leads `aa_first` (0x61). The demonstrated scenario is real; the fixture now uses a genuinely
  sorted pair and covers both movements — a save that pushes the marker forward and one that pulls it back.

### Round-3 acceptance

1. `cargo test` green, `cargo check --all-targets` clean.
2. Typing `Space.lua` into a tune pane's save form refuses the save, shows the error in the pane, writes NO
   file, records NOTHING and leaves the live state untouched; typing `Space` still saves exactly as today.
3. A legacy `*.json` / `*.ron` record still resolves and still bundles.
4. A save whose live apply FAILED leaves the marker on the preset that is still active, in both tune panes.
5. After a save, the marker follows the card just saved whether that moves the index forward or back.

## Recorded, NOT fixed (out of this round's scope)

- The Motion pane HAD the U8 twin: `on_panel_save_animation_preset` refreshed `user_animation_preset_names`
  without re-deriving `active_anim_index`, and no animation equivalent of `reseed_active_border_index`
  existed. Closed in round 3 by U11, which added `reseed_active_anim_index` and the re-derive in both
  branches.
- `PresetStore::rename` carries the U10 trap: it appends `.lua` to the TYPED new name, so a rename to
  `New.lua` writes `New.lua.lua` — verified empirically (`rename("old_preset", "New.lua")` → `Ok(())`, and
  the only file in the store is `New.lua.lua`). U10 was scoped to `save` and the guard was NOT extended to
  `rename` unasked; the refusal belongs to the same rule when the rename form is next touched.
- The two card-click handlers still persist with `let _ = st.cfg().save()` — the same swallowed-error shape U7
  fixes, but U7 names only the two save handlers.
- A preset whose own NAME ends in a container extension (`keeper.json`, file `keeper.json.lua`) is read by the
  pure canonical rule as a legacy file record, and `save("keeper.json")` writes `keeper.json.lua` while
  `canonical_preset_file("keeper.json")` stays verbatim — so the record names no file. U10 does not refuse
  these names on purpose: the legacy record shapes are honoured verbatim on the read side by design, and the
  corner is documented in the rule's comment. Only a typed name can take that shape.

### Pre-existing residue — the RAW recorded value on the apply paths (registered, NOT fixed)

Not introduced and not worsened by rounds 1–3. Two apply paths hand the recorded preset name to the engine
WITHOUT canonicalising it, unlike the marker (`preset_files_match`) and the bundler (`plan_bundle`):

- `src/providers/hve_presets.rs:196-204` (border) and `:185-195` (animation): a theme apply runs
  `let _ = self.engine.apply_border(v)` / `apply_animation(v)` and SWALLOWS the result, then copies the raw
  `v` into the config.
- `src/main.rs:134-137` (`teardown_border_draft`): re-applies `st.cfg().active_border_file` raw; the error IS
  logged and surfaced as shell status text, so only the raw value is shared with the provider path.

**Trigger**: a theme whose record is an interior-dot stem — `my.preset`, `19_stylized2.5D`, the shape a save
BEFORE U6 could write, or the shape of the shipped `19_stylized2.5D` if a theme record drops its extension.
The tolerant marker canonicalises it to `my.preset.lua`, resolves it against the scanned list and marks the
card, and `plan_bundle` canonicalises it so the `.lua` ships in the package — while the live apply probes the
raw `my.preset`, which the shell resolver does not find (verified in U6: `my.preset` → NOT FOUND,
`my.preset.lua` → found). A theme whose record holds the canonical `<name>.lua`, or a bare stem with no
interior dot (`Space`), is unaffected: both resolve in the shell resolver.

Left alone because fixing it means changing two apply paths outside this round's authorized files, and
because the residue needs a record shape the current save can no longer produce.

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

### Round 2 (the verification's corrections)

- [x] **U5** — the card-click toggle compares tolerantly and records the canonical file (borders + animations).
  Commit `21cfee6`.
- [x] **U6** — the canonical rule names the file `save` writes when the name carries an interior dot.
  Commit `b8e1ce8`.
- [x] **U7** — apply before recording, and the persist error is logged instead of swallowed. Commit `a585f8a`.
- [x] **U8** — the Borders save re-derives the pane marker from the recorded value. Commit `a585f8a`.
- [x] **U9** — the lost newline in the traversal save test's signature. Commit `b0671f5`.

### Round 3 (the second verification's LOW residuals)

- [x] **U10** — a typed name that already carries a preset file extension is refused instead of saved as
  `<name>.lua.lua`. Commit `0ba3336`.
- [x] **U11** — the pane marker is re-derived in EVERY save branch, in both tune panes, with the Motion twin
  added (`reseed_active_anim_index`). Commit `4780b33`.
- [x] **U12** — the U8 fixture states the bytewise order `PresetStore::list` really produces, and covers the
  marker moving forward and back. Commit `bab5e21`.

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
- 2026-10-11 (**round 2 start**): the independent verification's findings queued as U5–U9; nothing else in the
  contract was touched, and the keeper's theme/preset data was left exactly as it is (no migration).
- 2026-10-11 (**U6**, commit `b8e1ce8`): RED observed first — 5 assertion failures, each for the right reason:
  `canonical_preset_file("my.preset")` returned `"my.preset"` (want `"my.preset.lua"`), the same on the
  filesystem-backed test (`save` really had written `my.preset.lua`), `populate_metadata` resolved the record to
  `-1` instead of `0` (border and animation halves), `plan_bundle` planned `file_name: "my.preset"` and the
  bundler copied nothing (`left: []`). Landed the deliberate rule: a reference that already ends in a preset
  FILE extension (`lua`, `frag`) or in a legacy record extension (`json`, `ron`) stays VERBATIM, everything else
  is a STEM and gains `.lua` — which is exactly what `save` writes. `PRESET_FILE_EXTENSIONS` names the set once.
  **Evidence for the rule** (empirical, the shell resolver `assets/scripts/utils.sh` sourced with a temp user
  store): `Space` → `…/borders/Space.lua`, `Space.lua` → `…/borders/Space.lua`, `my.preset` → NOT FOUND,
  `my.preset.lua` → `…/borders/my.preset.lua`, `13_the_joker` and `13_the_joker.lua` → both built-in. So the
  interior-dot stem really did miss in the live apply, and BOTH spellings of a `*.lua` file really do resolve.
  Renamed the U2 test to `canonical_preset_file_honours_a_file_extension_and_extends_a_stem` (its old name and
  doc stated the rule that was wrong). `cargo test` → 1578 passed / 0 failed.
- 2026-10-11 (**U5**, commit `21cfee6`): RED observed first — the two behaviour tests failed to COMPILE
  (`cannot find function preset_card_click`) and the construction pin failed with "…`window.on_panel_apply_border(`
  must decide through the shared card-click rule", because the handlers compared `== file_str`. Landed
  `preset_card_click(active, clicked) -> (is_deactivating, recorded_value)`: tolerant compare
  (`preset_files_match`) + canonical record, called by BOTH card-click handlers (Borders, Motion). The value
  handed to `engine().apply_border/apply_animation` is the canonical one, checked against the shell resolver
  above, and the tune-pane read-back receives it too (`sync_border_tune_pane` strips `.lua` and the reader
  appends it again). `cargo test` → 1581 passed / 0 failed.
- 2026-10-11 (**U7 + U8**, commit `a585f8a`): RED observed first — the applier pin failed with
  "`window.on_panel_save_border_preset(` must apply the preset BEFORE recording it" and the marker pin with
  "the save must re-derive the pane marker from the recorded value"; both handlers recorded and persisted
  before the live apply and used `let _ = st.cfg().save()`. Landed: apply first (the canonical file), record
  only on success, `if let Err(e) = st.cfg().save()` + `tracing::warn!` (the shape `src/main.rs` ~:4015 uses),
  and `reseed_active_border_index(&w, &recorded)` right after the list refresh. Added the behaviour test
  `reseed_after_a_save_moves_the_marker_to_the_newly_saved_card` (ui_tests.rs): with the stale marker the card
  that WAS active stays marked, and re-deriving from the recorded value marks the card just saved. `cargo test`
  → 1584 passed / 0 failed. Decision: U8 stays Borders-only — the Motion pane has the same shape but no
  equivalent re-seed helper, and inventing one is outside this round (recorded above).
- 2026-10-11 (**U9**, commit `b0671f5`): RED observed first — `cargo fmt --check` printed
  `-    fn save_rejects_traversal_and_writes_nothing_outside_base() {        let dir = temp_dir();`. Restored
  the newline, logic untouched. `cargo fmt --check` still FAILS repo-wide (pre-existing: 55 files, `src/main.rs`
  207 hunks, `src/shell/ui_tests.rs` 372 at the round's start commit; `openspec/config.yaml:31` even records
  "formatter: none installed"), so "clean" was never true at HEAD. Measured per file instead, from a detached
  worktree of `097b603` against the tree after U9: `src/main.rs` 207 → 202, `src/preset_store.rs` 33 → 32 (the
  U9 hunk, gone), `src/presets.rs` 7 → 7, `src/theme_presets.rs` 1 → 1, `src/shell/ui_tests.rs` 372 → 372 — no
  new fmt noise, one pre-existing defect fixed.
- 2026-10-11 (**round 3 start**): the second verification's four LOW residuals queued as U10–U12 plus the raw
  recorded value as registered residue. The keeper's theme and preset data still untouched (no migration).
- 2026-10-11 (**U10**, commit `0ba3336`): RED observed first —
  `save_refuses_a_name_that_already_carries_a_preset_file_extension` failed with
  "a name already carrying `.lua` must be refused, got: Ok(())": the store really did write `Space.lua.lua`
  and leave the existing `Space.lua` alone, so the preset just saved was unreachable and the canonical
  re-apply went to the OLD file, silently. Landed `PRESET_CONTAINER_EXTENSIONS` (`lua`, `frag`, deliberately
  NOT the legacy record shapes), one shared `preset_extension` reader, and
  `refuse_a_name_that_already_carries_the_extension` called by `save` after `validate_name` — the refusal
  names the stem to type instead and reaches the pane through the handler's existing `if let Err(e) = result`
  → `set_border_save_error` / `set_animation_save_error`. Because `save` returns `Err`, the whole
  `if r.is_ok()` block is skipped: no file, no record, no apply. Extra pin
  `both_tune_pane_save_handlers_gate_the_record_on_the_store_write_and_show_the_error` (the store write
  precedes the record, which it already did — the missing piece was the refusal itself, kept as a regression
  guard). Regression guard `save_still_accepts_a_stem_and_a_legacy_record_name`: `Space`, `my.preset` and
  `glow.json` still reach the store. `cargo test` → 1587 passed / 0 failed.
- 2026-10-11 (**U11**, commit `4780b33`): RED observed first — `the_motion_save_handler_re_derives_the_marker_
  in_every_branch` failed with "the Motion save must re-derive its marker from the recorded value", and
  `the_border_save_handler_re_derives_the_marker_outside_the_success_branch` failed with "the marker must be
  re-derived in EVERY branch … (re-derive at indent 24, failure branch at 20)" — the nested call it exists to
  catch. The two behaviour tests failed to COMPILE first (`cannot find function reseed_active_anim_index`).
  Landed: both save handlers read the value the config holds once the save is done and re-derive AFTER the
  apply/record branch (Borders keeps `reseed_active_border_index(&w, &recorded)`, so the U8 pin still holds),
  and `reseed_active_anim_index` resolves the recorded value against the Motion panel's combined list
  (`anim-files` + `user-animation-preset-names`; `PanelRoot.motion-list-len` and its card click use the same
  index space). New pins use `indent_of` (leading whitespace) — the only honest way to pin "sibling of the
  branch, not nested in it" for a handler no test can invoke. Decision: the Motion twin IS the cheap version
  of the same fix here (the helper mirrors a tested one and the index space is confirmed by the panel), so it
  is done rather than re-registered. `cargo test` → 1591 passed / 0 failed. `cargo fmt --check` re-measured per
  file against a detached worktree of `6f8ff4c`, so the new pins add NO fmt noise: `src/main.rs` 202 → 202,
  `src/preset_store.rs` 32 → 32, `src/presets.rs` 7 → 7, `src/shell/ui_tests.rs` 372 → 372 (repo-wide
  1544 → 1544); the three hunks the first draft of the pins produced were reworded/re-broken to rustfmt's own
  preferred shape instead of reformatting anything else.
- 2026-10-11 (**U12**, commit `bab5e21`): the fixture's stated order was impossible, so the RED was an
  arithmetic one: the first attempt at the second save used a stem sorting BETWEEN the two, and the assertion
  caught that the card keeps the index the previous save left ("re-deriving must follow the card just saved,
  not the index the previous save left": `left: 3, right: 2`). The fixture now saves `zz_saved` (marker
  FORWARD, 2 → 3) and then `Space` (marker BACK, 3 → 2), and the bytewise order is pinned against the real
  store by `list_sorts_the_stems_bytewise_so_an_uppercase_stem_leads` (`Space`, `aa_first`, `ab_saved`,
  `zz_saved`). `cargo test` → 1592 passed / 0 failed.
- 2026-10-11 (**U10–U12 closed for review**, nothing pushed): `cargo test` → 1592 passed / 0 failed;
  `cargo check --all-targets` → clean. Files touched in round 3: `src/preset_store.rs`, `src/main.rs`,
  `src/presets.rs`, `src/shell/ui_tests.rs`. No dependency added, no theme or preset data rewritten, no
  migration, no release build, no install. U4 still belongs to the parent. The U11 and U12 commit SHAs were
  rewritten by the `fixup!` that folded the fmt cleanup into U11 (nothing is pushed).
