# Fix: a theme must not own system state (autostart, keybinds, window rules)

## Objective

Applying an aesthetic theme must never touch the keeper's system/runtime state. The
`hyprland-settings` provider currently treats three blocks of `hve-settings.lua` as theme
payload and overwrites them on every apply, which is why HVE stopped starting at login and
why the system settings looked "reset".

## Problem

`HyprlandSettingsProvider` (`src/providers/hyprland_settings.rs`) declares
`ProviderCapabilities::KEYBINDS | AUTOSTART | WINDOW_RULES` and:

- `save()` captures those three marker blocks from `hve-settings.lua` into
  `<theme>/providers/hyprland-settings/state.json` (absent markers -> `None`).
- `apply()` writes `Some(x)` back, and **removes the block entirely when the captured value is
  `None`** (`remove_between_markers`).

A theme is an aesthetic snapshot; those three blocks are system configuration that lives in
`~/.config/hve/config.json` and is written by `settings.rs` at startup. The provider model's
premise — "the theme owns everything it captured" — is true for colours and wallpapers and
false for runtime state.

## Evidence

- Keeper's saved snapshots (`~/.config/hve/themes/*/providers/hyprland-settings/state.json`),
  all stale: 5 keybinds (the set removed on 2026-09-07; HVE now wants only SUPER+H), window
  rules in the old `hl.window_rule({...})` form (HVE manages its own window state and needs
  none), and `autostart` pointing at the dead dev path `/home/ximo/hve/target/debug/hve`
  (the repo lives at `/home/ximo/Proyectos/hve`). The newest one (ThemasAnimados,
  2026-09-06 22:24) has `autostart: null` -> applying it DELETES the autostart block.
- Result observed on disk: `~/.cache/hve/hve-settings.lua` has the current single SUPER+H
  bind but NO autostart block, while `config.json` says `auto_start: true`. Hyprland 0.56.2
  loads that file (`Using lua config found at .../hyprland.lua`), so nothing launches HVE.
- `ThemeManager::apply` runs every provider listed in the theme's `meta.json` and then fires
  `hyprctl reload` (`src/theme_manager.rs:242-292`), so the overwrite takes effect immediately.
- Provider tests encode the wrong intent: `test_apply_removes_block_when_state_none` and
  `test_save_with_absent_markers_then_apply_removes_active_block` literally assert that an
  apply removes the autostart block.

## Scope

In scope:

- Remove the `hyprland-settings` provider from the product (module, both registrations) so no
  theme can ever touch system state.
- Remove the system-state capabilities from `ProviderCapabilities` so the model cannot express
  the confusion again.
- Keep the runtime settings writers in `src/settings.rs` as the only owners of those blocks.
- Clean the keeper's stale provider snapshots (backup first).
- Refresh the binary autostart points at, and retire the dead `.desktop` mechanism.

Out of scope:

- Wallpaper/colour providers (genuinely aesthetic).
- `~/.config/hypr/hyprland.conf` (created 2026-09-18 04:28 with a `source =` line and old
  commented HVE rules): not HVE's file, harmless today because Hyprland loads the `.lua`.
  Reported, not touched.
- The engine files and the shell scripts.

## Tasks

- [x] A1 [RED] Test the invariant: applying a saved theme must not modify the
      `WINDOW RULES`, `KEYBINDS` or `AUTOSTART` blocks of `hve-settings.lua`. Needs a testable
      seam: extract the provider registration in `src/main.rs` into a function (e.g.
      `theme_manager::register_default_providers(tm, engine)`) and call it from both
      registration sites. The test seeds a temp HOME whose settings file carries the three
      blocks (autostart pointing at the dead path) plus a theme dir whose
      `providers/hyprland-settings/state.json` has `autostart: null` and the 5 stale binds.
      Must FAIL before the provider is removed.
      Done: seam is `providers::register_default_providers(tm, engine)` in
      `src/providers/mod.rs` (concrete providers own their registration; `theme_manager`
      stays generic). Called from both sites in `main.rs`. RED observed: the provider
      rewrote/removed all three blocks.
- [x] A2 Remove `HyprlandSettingsProvider`: delete
      `src/providers/hyprland_settings.rs`, drop `pub mod hyprland_settings;` from
      `src/providers/mod.rs`, and remove both `register_provider(...)` calls.
      Done: file deleted; module declaration and the single registration line inside the
      new seam removed (the two `main.rs` call sites now go through the seam).
- [x] A3 Remove `KEYBINDS`, `AUTOSTART` and `WINDOW_RULES` from `ProviderCapabilities` if
      nothing else consumes them (check every provider and test; report what you find).
      Done: removed. Grep for `ProviderCapabilities::(KEYBINDS|AUTOSTART|WINDOW_RULES)`
      across `src/` returns no matches after A2; the removed provider was the only consumer.
- [x] A4 Full suite green; no new dead-code warnings (`edit_between_markers` / `extract_block`
      lose their consumer — report whether they are now unused).
      Done: `cargo test` → 738 passed / 0 failed (was 740; −3 provider tests +1 new).
      `cargo check` reports `edit_between_markers` and `extract_block` (src/utils.rs) as
      `never used` in production. NOT deleted (task says report if uncertain); their unit
      tests still exercise them.
- [x] A5 Report only: does the theme UI tolerate a `meta.json` that lists the removed provider
      id (themes are ignored when unregistered)? Do not change user data in this task.
      Done: `ThemeManager::list()` filters out any theme whose `providers` contains an
      unregistered id, so such a theme is hidden from the gallery entirely. The `providers`
      list does reach Slint (`GalleryCardData.providers: [string]`) but
      `ThemeCardDelegate.slint` never renders it. Nothing is drawn — no blank icon, no
      broken label. `apply()` also silently ignores unknown ids.
- [x] A6 [RED] Migrate retired provider ids out of `meta.json` so themes saved before the
      removal reappear, while preserving the capability filter for genuinely unavailable
      providers. Add `REMOVED_PROVIDER_IDS` (`["hyprland-settings"]`) and a one-way,
      idempotent migration that rewrites a `meta.json` ONLY when it lists a removed id,
      preserving `saved_at`, `description` and the order of the remaining ids. Run it inside
      `ThemeManager::new` so it always precedes any `list()`; log at info what it rewrote.
      Done: RED observed on `theme_manager::tests::migration_lists_a_theme_that_lists_a_removed_provider`
      (theme hidden because the stale id survived the filter). GREEN after the migration.
      Invariant test `theme_with_unavailable_provider_outside_the_removed_list_stays_hidden`
      stays green: `mpvpaper` (not in the removed list) keeps its theme hidden and its
      `meta.json` byte-identical.
- [x] A7 Delete the now-unused `edit_between_markers` and `extract_block` (+ their unit tests)
      from `src/utils.rs` — the removed provider was their only production consumer.
      Done: repo-wide grep confirms no consumer outside the deleted tests; functions and
      tests removed; `cargo check` is warning-free.
- [x] A8 Make the invariant test load-bearing instead of conditionally vacuous: assert
      structurally that `register_default_providers` never registers
      `hyprland-settings`, and keep the byte-identity check as behavioural evidence.
      Prove the structural assertion fails when a provider with that id is registered.
      Done: assertion added on `ThemeManager::provider_ids()` in
      `providers::tests::applying_a_theme_does_not_touch_settings_system_blocks`.
      Teeth proven with a temporary registration: the test FAILED on the structural
      assertion, not on byte identity (verbatim RED in Progress); reverted → GREEN.
- [x] A9 Harden `migrate_removed_provider_ids`: preserve unknown `meta.json` fields
      (operate on `serde_json::Value`, not the typed `ThemeMeta`), write atomically
      (temp file + `fs::rename` in the same directory), and scan exactly what `list()`
      scans (skip `.`/`_`-prefixed dirs). Add tests.
      Done: all three. Two new tests are RED-first (verified against the old
      implementation); the no-mtime-churn assertion is pure hardening.
- [x] A10 Remove stale `providers/hyprland_settings.rs` mentions from `README.md`,
      `WIKI.md`, `i18n/en.json`, `i18n/es.json`, and
      `openspec/specs/lua-only-config/spec.md`. Archive stays untouched.
      Done: all tree/table/listing mentions removed; JSON files remain valid; no live
      mention remains outside the intentional `REMOVED_PROVIDER_IDS` / test references
      and this tracking document.
- [ ] B1 (parent) Back up and remove the stale `providers/hyprland-settings/` directories from
      the keeper's themes.
- [ ] C1 (parent) Refresh `~/.local/bin/hve` with a current build so
      `resolve_autostart_exe()` points at a binary whose `CONFIG_VERSION` matches HEAD
      (the installed one knows v3; HEAD is v7).
- [ ] C2 (parent) Back up and remove the dead `~/.config/autostart/hve.desktop`
      (jul 5, points at the stale binary; the generated systemd unit never runs because
      `xdg-desktop-autostart.target` is not started in this session).
- [ ] D1 (keeper) Restart HVE, then confirm `hve-settings.lua` regains the AUTOSTART block
      with a live path, and that the next login starts HVE.

## Acceptance criteria

1. Applying any saved theme leaves the three system blocks of `hve-settings.lua` byte-identical.
2. `cargo test` fully green, with the new invariant test failing before the removal.
3. The keeper's stale snapshots and the dead `.desktop` are gone, with backups kept.
4. The autostart block HVE writes points at a binary built from the current source.

## Checks

- `cargo test` (full suite)
- `cargo test <new invariant test>` (RED before, GREEN after)
- `cargo build --release` + comparison of the installed binary against the current build
- Post-restart readback of `~/.cache/hve/hve-settings.lua`

## Progress

- Scope recon done: no spec requires the provider, `capabilities()` has no production
  consumer, and `set_autostart(false)` is only called by the toggle.
- A1 done: `providers::register_default_providers` seam added; RED invariant test
  `providers::tests::applying_a_theme_does_not_touch_settings_system_blocks` pinned the bug
  (provider rewrote/removed all three blocks), then went green after A2.
- A2 done: `src/providers/hyprland_settings.rs` deleted, module declaration and registration
  removed.
- A3 done: `KEYBINDS` / `AUTOSTART` / `WINDOW_RULES` removed from `ProviderCapabilities`.
- A4 done: `cargo test` 738 passed / 0 failed; `cargo check` clean except `edit_between_markers`
  and `extract_block` now unused in production (kept, reported).
- A5 done: unregistered provider id in `meta.json` hides the whole theme from the gallery;
  the providers list is never rendered by the delegate.
- A6 done: `ThemeManager::new` now runs `migrate_removed_provider_ids` (guarded by
  `REMOVED_PROVIDER_IDS = ["hyprland-settings"]`). Regression test went RED first
  (`...must still be listed`), then GREEN. The filter in `list()` is untouched; only the
  retired id is stripped. Real keeper data verified unchanged by the test run:
  all 42 metas still list the old id and their mtimes are untouched (parent owns cleanup).
- A7 done: `edit_between_markers` / `extract_block` and their 4 unit tests deleted from
  `src/utils.rs`. `cargo check` → clean, no warnings. Full suite:
  `736 passed; 0 failed` (738 − 4 removed utils tests + 2 new migration tests).
- A8 done: the invariant test now has two halves. Structural prevention: the ids from
  `register_default_providers` must not contain `hyprland-settings`. Behavioural evidence:
  the three system blocks stay byte-identical after `apply`. Teeth proven by temporarily
  registering a `hyprland-settings` provider and running the test. Verbatim RED:
  `thread 'providers::tests::applying_a_theme_does_not_touch_settings_system_blocks' panicked at src/providers/mod.rs:128:9:`
  `no provider may own system state: 'hyprland-settings' must stay unregistered by register_default_providers, got ["hyprland-settings", "noctalia", "hve-presets"]`
  — the failure is on the structural assertion, not on byte identity. After reverting the
  temporary registration: `test result: ok. 1 passed; 0 failed; 737 filtered out`.
- A9 done: `migrate_removed_provider_ids` now edits `serde_json::Value` in place (unknown
  fields survive; `providers` order and `saved_at`/`description` preserved), writes through
  `write_atomic` (temp file + `fs::rename`, same directory), and skips `.`/`_`-prefixed
  directories exactly like `list()`. New tests `migration_preserves_unknown_meta_fields`
  and `migration_ignores_hidden_and_backup_directories` were verified RED against the old
  implementation (old typed round-trip returned `Null` for the unknown field; `_`/`.`
  dirs were rewritten). The existing no-removed-id test now also asserts the mtime does
  not change (pure hardening — the old code already skipped, but a same-content rewrite
  would have been invisible to the byte check).
- A10 done: stale `providers/hyprland_settings.rs` mentions removed from `README.md`
  (tree + table), `WIKI.md` (tree + table), `i18n/en.json`, `i18n/es.json` (tree lists;
  sibling `└──` connectors fixed) and `openspec/specs/lua-only-config/spec.md`. JSON files
  re-validated with `python3 -m json.tool` equivalent (`json.load`); no live mention
  remains outside `openspec/changes/archive/` (historical, kept on purpose) and the
  intentional `REMOVED_PROVIDER_IDS` / test references.
- Full suite after A8–A10: `738 passed; 0 failed`. `cargo check`: clean, no warnings.

## Next step

B1/C1/C2 belong to the parent. A6–A10 closed the source-side consequences of removing the
provider: the gallery regression is fixed by migration (A6), the dead-code warnings are gone
(A7), the invariant test is now load-bearing (A8), the migration preserves unknown fields and
writes atomically (A9), and the documentation no longer advertises the deleted file (A10).
Remaining work is the parent's on-disk cleanup and autostart refresh.
