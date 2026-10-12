# Feature: applying a theme refreshes BOTH surfaces

**Locator**: `odd/tasks/theme-apply-syncs-both-surfaces.md`
**Engram mirror**: topic `odd/theme-apply-syncs-both-surfaces/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: **CLOSED** — the keeper closed it on 2026-10-12 ("vale el 3 listo también"). Reported
2026-10-11: "cuando cambias un tema debe de sincronizarse tanto en el slider, como en los settings
en el apartado guardar... ahora he entrado y cada uno decía una cosa diferente". Delivered the same
day (commits `bd0aba9`, `bbb8ba0`); this file's status and work units were stale, corrected here.
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~80-140 authored changed lines → single PR, no chaining.
**Route**: delegated writer (one file, `src/main.rs`, plus tests), parent self-check.

## Objective

The gallery slider and the Settings → Save list must never disagree about which theme is
applied. Every apply path refreshes both, and entering Save can never show a stale list.

## Problem / proven evidence (read-only investigation, 2026-10-11)

There are TWO `ThemeManager` instances: the panel's (`theme_manager`, inside `AppState`)
and the gallery's (`gallery_tm`). The STATE converges on every apply — `ThemeManager::apply`
sets its own `last_applied` (`theme_manager.rs:644`) and the panel side sets the main
manager's (`app_state.rs:100-107`, `mark_theme_applied`). What diverges is the DRAWING:

1. `on_gallery_card_clicked` (the normal way to change theme, `main.rs:3547`, apply branch
   from `:3584`) applies through the slot, then rebuilds the gallery cards from
   `gallery_tm` (`:3637` `to_gallery_cards`, `:3650` `set_gallery_cards`) and **never calls
   `refresh_theme_list`** — verified: `refresh_theme_list` has no call site in that block
   (its call sites are `main.rs:3008, 4473, 4644, 4760, 4803, 4841, 4885, 4909`).
2. The panel's own apply path (`main.rs:4634`) DOES call `mark_theme_applied` +
   `st.refresh_theme_list` (`:4642-4645`).
3. Entering the Save section refreshes nothing: `on_panel_section_selected`
   (`main.rs:4305`) has a branch for Borders only, and none for Save.

Consequence: apply from the gallery → the slider marks the new theme, the Save list keeps
marking the previous one, until something else happens to refresh it. Two surfaces, two
answers, one state.

## Scope (authorized)

- **U1 — one shared sync helper.** Factor "mark this theme applied and refresh BOTH
  surfaces" into a single function (e.g.
  `pub(crate) fn sync_applied_theme(window: &MainWindow, state: &SharedState, gallery_tm: &Mutex<ThemeManager>, name: &str)`)
  that:
  1. marks the main manager + config through the existing `mark_theme_applied` (do NOT
     reimplement the reload-first discipline it documents),
  2. refreshes the panel list (`refresh_theme_list`) so the Save marks and the active index
     follow,
  3. rebuilds the gallery cards from `gallery_tm` (the existing in-place card sync that
     preserves baked thumbs — reuse it, do not invent a second one),
  4. respects the file's lock discipline: NEVER hold two locks at once and never call the
     engine while a lock is held (see the comments at `main.rs:3594-3612`).
- **U2 — wire it everywhere.** Use the helper in the gallery card-clicked apply branch and
  in the panel apply path, so both surfaces are refreshed by construction instead of by
  each path remembering. Keep the existing per-path work that is NOT about this
  (border tune pane, transition flag, focus restores).
- **U3 — the safety net.** Refresh the theme list when the Save section is entered
  (`on_panel_section_selected`), so a stale list can never be displayed even if a future
  apply path forgets the helper.
- **U4 — tests.** Pin what can be pinned without driving a UI: the helper converges both
  managers' `last_applied` and the panel's active index after an apply, and the Save-section
  entry decision refreshes. If the section-entry behaviour cannot be exercised without a
  window, extract the decision into a small pure predicate (e.g.
  `save_section_entry_needs_refresh(target)`) and test that instead, stating plainly that
  the wiring itself is verified by reading the diff.

## Out of scope

- No change to the apply itself, the providers, the colour pipeline, the preset bundling,
  or the gallery's card model.
- No new dependency.
- The "Listo" indicator and the preview-delivery work stay as they are.

## Acceptance criteria

1. `cargo test` green, `cargo check --all-targets` clean.
2. After applying from the gallery, the panel's Save list and the carousel mark the SAME
   theme, with no further interaction.
3. Entering Save always shows a list refreshed from the managers.
4. No lock is held across another lock or across an engine call (read the diff for this).
5. The keeper's live check: change theme from the gallery, then open Settings → Save and
   confirm both name the same theme.

## Checks

- `cargo test`
- `cargo check --all-targets`
- Keeper live check (criterion 5).

## Work units

- [x] **U1** — the shared sync helper. Done: `sync_applied_theme` (`src/main.rs:1194`), built on the
  existing `mark_theme_applied` + `refresh_theme_list` (`:651`) + the in-place card rebuild.
- [x] **U2** — wire both apply paths to it. Done: called from the gallery branch (`src/main.rs:3782`)
  and the panel apply path (`:4826`). Commit `bd0aba9`.
- [x] **U3** — the Save-section-entry refresh. Done: `save_section_entry_needs_refresh`
  (`src/main.rs:395`) wired in `on_panel_section_selected` (`:4538`). Commit `bbb8ba0`.
- [x] **U4** — tests + parent verification. Done:
  `sync_applied_theme_converges_both_surfaces_and_keeps_bakes` and
  `save_section_entry_needs_refresh_only_for_save` (`src/shell/ui_tests.rs`); full suite green.

## Progress log

- 2026-10-11: divergence reproduced in code (three findings above); contract written.
- 2026-10-11: implemented and committed — `bd0aba9` (U1 + U2) and `bbb8ba0` (U3), tests in
  `src/shell/ui_tests.rs` (U4). The contract's status and work units were never updated.
- 2026-10-12: **CLOSED by the keeper** ("vale el 3 listo también"). Verified before closing: the
  four units are in the tree, and `cargo test` (1616 passed) and `cargo check --all-targets` are
  green on this same code (checked when this round's binary was built from HEAD).
