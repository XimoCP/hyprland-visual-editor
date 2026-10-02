# Preset rename and delete dialogs (Borders + Motion)

## Objective

The rename button on a saved border/animation preset must open the same
floating dialog the Save section uses for themes, and the delete button must ask
for confirmation the same way. Extract the dialog into a shared, reusable
component instead of duplicating it.

## Problem

- `SavedPresetCard` emits `open-rename(name)` / `open-delete(name)`.
- `BordersSection.slint:2561,2664` and `MotionSection.slint:493,531` answer with
  `root.rename-border-preset(name, "")` / `root.delete-border-preset(name)` — the
  rename passes an EMPTY new name, so `PresetStore::validate_name` rejects it and
  only an error shows. Delete calls the Rust delete with no confirmation.
- The theme rename dialog is an inline block inside `SaveSection.slint:502-562`,
  bound to that section's own properties; `ConfirmDialog`
  (`SaveSection.slint:25-75`) is file-local and has no text input.

## Design (reuse, no duplication)

1. New file `ui/panel/sections/Dialogs.slint`, exporting two components:
   - `ConfirmDialog` — moved verbatim from `SaveSection.slint:25-75` (props
     `title`, `title-color`, `message`, `cancel-text`, `confirm-text`,
     `confirm-color`; callbacks `confirm()`, `cancel()`).
   - `RenameDialog` — extracted from the inline block
     `SaveSection.slint:502-562`. Props: `title`, `placeholder`, `cancel-text`,
     `confirm-text`, `confirm-color`, `error-text`, `in-out value`; callbacks
     `confirm(string)`, `cancel()`. Same 300x180 look.
2. `SaveSection.slint` imports both from `Dialogs.slint`, removes its local
   `ConfirmDialog`, and replaces the inline rename block with a `RenameDialog`
   instance. Behaviour MUST stay identical, including the guard
   `value != "" && value != dialog-target` before calling `rename-theme`.
3. `BordersSection.slint` and `MotionSection.slint` each import both dialogs and
   add local dialog state (e.g. `preset-dialog-mode`, `preset-dialog-target`,
   `preset-rename-input`). Rewire `open-rename`/`open-delete` to set that state
   instead of calling Rust with `""`. Add the two overlays at the section root:
   - rename confirm → `root.rename-border-preset(target, value)` (resp.
     `rename-animation-preset`), guarded like SaveSection.
   - delete confirm → `root.delete-border-preset(target)` (resp.
     `delete-animation-preset`).
4. Plumbing for the dialog strings (titles, message, cancel/confirm labels):
   add properties on the two sections, pass them from `PanelRoot.slint`
   (BordersSection ~`:762`, MotionSection ~`:820`), mirror in `ui/shell.slint`
   and `ui/main.slint` like the existing `panel-save-rename-title`
   (`shell.slint:141`, `main.slint:294`), set them in `src/main.rs` near
   `:4250`, and add the setters in `src/panel_i18n.rs` near `:142`.
   New i18n keys in `i18n/en.json` and `i18n/es.json`.

## Constraints

- Strict TDD is active. Runner: `cargo test`. Write/adjust the failing test
  first where a test can express the behaviour.
- Visual-only scope: do NOT touch the engine, providers, or preset storage
  logic. `PresetStore::rename/delete` already exist and are correct.
- Do NOT run `cargo fmt`. Do NOT switch branches.
- Engineered artifacts in English; UI strings via the i18n files.
- Keep the existing theme Save dialogs working exactly as before.

## Tasks

- **T1 — RED**: add/adjust UI tests (see `src/shell/ui_tests.rs`) asserting that
  clicking a user preset's rename opens the dialog and confirming calls
  `rename-border-preset`/`rename-animation-preset` with the typed name, and that
  delete opens a confirm and only then calls the delete. Confirm they fail.
- **T2**: create `Dialogs.slint` (ConfirmDialog moved + RenameDialog extracted).
- **T3**: rewire `SaveSection.slint` to the shared dialogs (behaviour identical).
- **T4**: wire `BordersSection.slint` rename + delete dialogs.
- **T5**: wire `MotionSection.slint` rename + delete dialogs.
- **T6**: plumbing + i18n keys (`PanelRoot`, `shell`, `main.slint`, `main.rs`,
  `panel_i18n.rs`, `i18n/en.json`, `i18n/es.json`).
- **T7 — GREEN**: full `cargo test`; report counts.
- **T8**: commit the work unit(s).

## Acceptance criteria

- Rename on a user border/animation preset opens a floating dialog prefilled
  with the current name; confirming a new name renames it; cancelling changes
  nothing.
- Delete on a user preset opens a confirmation; only confirming deletes.
- The theme Save rename/delete/refresh/overwrite dialogs behave exactly as
  before.
- Full suite green.

## Checks

- `cargo test` (full).
- Visual verification by the orchestrator: headless render + read the PNGs
  (`HVE_RENDER_DIR=... cargo test slice_focus_flow_renders -- --nocapture`), plus
  any render test extended to cover the preset dialog.
- Cross-model verification (UI + wiring → Tier 3): writer MiMo, verifier a
  different model.

## Progress

- [x] T1
- [x] T2
- [x] T3
- [x] T4
- [x] T5
- [x] T6
- [x] T7
- [x] T8

## Evidence

- T1 RED: `cargo test preset_rename_delete_use_shared_dialogs` →
  `FAILED ... ui/panel/sections/Dialogs.slint must exist` (before T2).
- T2/T3: new `ui/panel/sections/Dialogs.slint` exports `ConfirmDialog`
  (moved verbatim from SaveSection) and `RenameDialog` (extracted from the
  inline block). SaveSection imports both; behaviour identical (same guard
  `value != "" && value != root.dialog-target`).
- T4/T5: BordersSection (`preset-dialog-mode/target/rename-input`) and
  MotionSection wired the same way; `open-rename`/`open-delete` now open the
  dialogs and the confirm handlers fire the existing Rust callbacks.
- T6: window props plumbed `main.slint` → `shell.slint` → `PanelRoot.slint`
  → sections; strings set in `src/main.rs` from new i18n keys
  (`borders.rename_dialog.title`, `borders.delete_dialog.{title,message}`,
  `animations.rename_dialog.{title,placeholder}`,
  `animations.delete_dialog.{title,message}`); border rename placeholder
  reuses `BordersText.save-placeholder` (the section-label test forbids
  duplicating it as a literal).
- T7 GREEN: `cargo test` → **1239 passed; 0 failed; 0 ignored**.
- Visual verification: `HVE_RENDER_DIR=/tmp/opencode/render-verify-presetdialogs
  cargo test preset_dialogs_render_rename_and_delete -- --nocapture` → ok;
  PNGs read: border rename (prefilled "COCO", pink Rename), border delete
  (red confirm, `...delete "COCO"?`), motion rename (prefilled "EASE").
- T8: commit on `hve2-visual-rewrite` (see git log).

## Next step

Cross-model verification (Tier 3) by a different model.
