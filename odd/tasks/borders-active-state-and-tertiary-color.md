# Fix: borders active state on entry + tertiary colour duplication

**Locator**: `odd/tasks/borders-active-state-and-tertiary-color.md`
**Engram mirror**: topic `odd/borders-active-state-and-tertiary-color/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: PLANNED (user authorized 2026-09-19: "Arregla los dos")
**TDD**: strict (project rule) — runner `cargo test`

## Objective

Two user-reported defects in the borders area, both reproduced from evidence:

1. **Entering the Borders section shows no border marked as loaded**, even though the app persists which border is applied.
2. **The colour strip paints three chips with two identical colours and never shows the third (violet) colour a preset declares.**

## Problem (verified evidence)

### Bug A — active border not marked on entry

- Persistence exists and is correct: `src/config.rs:35` (`active_border_file`), written on apply at `src/main.rs:2843-2846`, read at startup by `src/presets.rs:61` via `populate_metadata` (`src/presets.rs:34-38`).
- Live config on this machine: `~/.config/hve/config.json` → `"active_border_file": "14_looper.lua"` (a valid, exact-matching value).
- The seeder runs **only at startup and on apply**. Callers of `set_active_border_index`: `src/presets.rs:61` (startup), `src/main.rs:2866` (apply), `src/callbacks.rs:448` (manual reset). `sync_preset_indices` (`src/callbacks.rs:527`) is dead code (`#[allow(dead_code)]`).
- **The active index is never re-seeded when the Borders section is entered.**
- `populate_metadata` **fails silently** when the stored value matches no preset file: no explicit `-1`, no diagnostics, the card simply shows nothing. Behaviour pinned by `src/presets.rs:220-237`.
- Historical cause of desync (already fixed in the tree): the mouse click used to send the translated **title** instead of the file (`odd/tasks/fix-border-preset-apply.md:29-30,125-126`). No migration/repair exists for a value already persisted in that shape.
- The installed binary the user ran is **older than the current code**: `~/.local/bin/hve` is dated 2026-09-18 05:35, repo HEAD is 2026-09-19. Part of the user's live observation may come from that build; the un-verified part must be reproduced, not assumed.

### Bug B — duplicate colour / missing violet

- Strip chips paint `root.tune-colors-resolved[i]` (`ui/panel/sections/BordersSection.slint:725`), built by `refresh_resolved_colors` (`src/main.rs:150-164`) from theme tokens set in `src/theme.rs:227-229`.
- Chip count = preset's colour roles (`tune-color-count` ← `params.active_colors.len()`, `src/main.rs:2882-2883`); Looper is `colors = { primary, secondary, tertiary }` (`assets/borders/14_looper.lua:14`) → exactly 3 chips. The parser does **not** truncate (`src/border_preset.rs:679-696`).
- Root cause of the duplicate: `assets/scripts/colors.sh:349` — `: "${HVE_TERTIARY:=${HVE_SECONDARY}}"`. When the source defines no tertiary, tertiary silently becomes secondary.
- The active source (Noctalia **v5**, `~/.config/hypr/noctalia.lua`) defines primary/secondary/surface/error but **no tertiary**; `colors.sh:231-243` prefers v5 and returns before looking at the v4 path. Verified live: `noctalia msg color-scheme-get` → `wallpaper vibrant`, and the shell exposes no palette-printing command.
- Result on this machine: primary `#67abe4` (blue), secondary `#d6915c` (orange), tertiary **= orange** → `blue, orange, orange`.
- The coherent violet of the *current* scheme exists on the machine as `#9e70d6` (`~/.config/kitty/themes/noctalia.conf` color4/color12, same palette as the v5 primary/secondary). A stale violet `#9d00ff` also exists in `~/.config/hypr/noctalia/noctalia-colors.lua`, but that file belongs to an older scheme (its primary is green `#2ec436`) — **not** coherent with the current palette.
- Additional verified gap (out of scope): the `@Color: #cba6f7` line that `scan.sh:84` emits for Looper is dropped — `src/engine.rs:233-248` (`ScanEntry`) has no `color`/`icon` field and serde discards unknown fields. The preset's own declared colour can never reach the UI today.

## Why it matters

A board that silently shows nothing teaches the user to distrust the app's state, and a strip that paints the same colour twice looks like a rendering bug even when the underlying detection is the real culprit. Both defects make correct behaviour indistinguishable from broken behaviour.

## Scope

- In scope: the borders active-index seed path (`src/presets.rs`, `src/main.rs`, `src/callbacks.rs`) and the tertiary colour detection (`assets/scripts/colors.sh`).
- Out of scope: carrying `@Color`/`icon` through the scanner (recorded as a known gap), the picker `y` slide-in (parked, see `openspec/changes/border-colors-single-strip/design.md` Open Question #3), the strip focus-ring ownership risk inherited from PR 3, and the user's pending polish list (keyboard shortcuts, visual details).
- Constraints: `src/config.rs` is engine-sealed per `AGENTS.md` — keep changes minimal and additive; HVE is a visual-only editor; no GPL code; visual changes require the PNG render inspection.

## Authorized scope

User approved fixing both defects on 2026-09-19 ("Arregla los dos"), including the approach: check whether the shell can genuinely supply a third colour, and if it cannot, use a violet the system already declares instead of silently duplicating the second colour.

## Tasks

- [x] **B1** [RED] Write a failing test for the active-border marker contract: entering the Borders section must reflect the border actually loaded from `cfg.active_border_file`, including when the index was never seeded or was seeded before the preset list existed. [f: `src/shell/ui_tests.rs` or `src/presets.rs` tests] [t: `cargo test`] — RED observed: `test_populate_metadata_empty_input` (99 vs -1) and `test_populate_metadata_clears_stale_index_when_active_file_matches_nothing` (0 vs -1); `entering_borders_reseeds_active_index_by_construction` panicked on the missing main.rs marker.
- [x] **B2** [GREEN] Re-seed `active-border-index` from the persisted value when the section/panel opens (not only at startup), and stop failing silently when the stored value matches no preset: make the state honest and self-consistent. [f: `src/presets.rs`, `src/main.rs`, `src/callbacks.rs`] — added `resolve_active_index` + `reseed_active_border_index`; `populate_metadata` now always writes an explicit index and warns on mismatch; wired into `on_panel_section_selected` for Borders.
- [x] **B3** [RED] Add a test locking the "never duplicate" colour contract: when the theme provides no tertiary, the resolved tertiary must not equal the secondary. [f: shell-script test harness used by existing colour tests, or a Rust test invoking `colors.sh` with fixtures] [t: `cargo test`] — RED observed: 4 hermetic tests in `src/scripts_contract.rs`, all returning `#d6915c` for tertiary.
- [x] **B4** [GREEN] Fix `assets/scripts/colors.sh` so a missing tertiary is resolved from a real source in preference order (shell v5 palette → v4 palette → other shell-generated palette of the same scheme, e.g. the terminal template's 4th accent) and, only when nothing exists, falls back to a **documented, distinct** colour. `HVE_TERTIARY` must never silently become `HVE_SECONDARY`. Record the chosen fallback and why in the script. [f: `assets/scripts/colors.sh`] — added `_hve_complete_tertiary` (v4 coherence-gated → terminal template color4) and `HVE_TERTIARY_FALLBACK=#94e2d5`.
- [x] **B5** Verify on this machine that Looper's strip shows three distinct colours, with the violet from the scheme rather than a duplicate orange. [t: render/PNG inspection per the project visual rule] — live `get_colors.sh` → `#67abe4 / #d6915c / #9e70d6` (distinct); stale v4 palette skipped with a diagnostic. Renders read, but they do NOT prove the data fix (see Progress).
- [x] **B6** Reconcile docs/notes touched by the fix, run the full suite, and close with work-unit commits (tests and docs alongside behaviour, Conventional Commits, no AI attribution).

## Acceptance criteria

1. Entering Borders with `active_border_file = 14_looper.lua` marks the Looper card — deterministically, covered by an automated test that fails before the fix.
2. A stored value that matches no preset never leaves a silently blank board.
3. The strip for a 3-colour preset renders three **distinct** colours on this machine (blue / orange / violet), and no code path sets tertiary from secondary without an explicit, documented decision.
4. Full suite green, `cargo check` without warnings, and the visual state verified by reading rendered PNGs.

## Applicable checks

- `cargo test` (full suite) and `cargo check`
- focused tests per task
- visual verification per `AGENTS.md` for any `.slint`/render-visible change

## Progress

- 2026-09-19 — Plan created from a read-only diagnosis (two independent explorers) plus live verification of the persisted config, the shell palette files and the installed binary age. No source changed yet.
- 2026-09-19 — B1→B4 implemented with strict TDD by the delegated writer.
  - B1 RED (verbatim): `cargo test populate_metadata` → `test_populate_metadata_empty_input` failed `left: 99, right: -1`; `test_populate_metadata_clears_stale_index_when_active_file_matches_nothing` failed `left: 0, right: -1`. `cargo test entering_borders_reseeds_active_index_by_construction` → panicked `src/main.rs must call presets::reseed_active_border_index on section entry`.
  - B3 RED (verbatim): `cargo test tertiary` → 4 failed, all with `left: "#d6915c"` for the resolved tertiary (secondary aliasing).
  - B2/B4 GREEN: `cargo test populate_metadata` 5 passed; `cargo test reseed_active_border_index` 1 passed; `cargo test entering_borders_reseeds_active_index_by_construction` 1 passed; `cargo test tertiary` 4 passed.
  - B5 live: `bash assets/scripts/get_colors.sh` → `{"primary":"#67abe4","secondary":"#d6915c","tertiary":"#9e70d6",...}`; stderr shows `[HVE] Colors from: Noctalia v5 (lua)` + `[HVE] Skipping stale v4 palette (primary #2ec436 != #67abe4)`. The three colours are distinct.
  - B5 render honesty: NO `.slint` file changed. The headless renders (`slice_focus_flow_renders_settled_and_midflight`, `borders_strip_renders_correct_chip_count`) were run and their PNGs read with vision. They show the strip painting three distinct swatches, but their fixtures feed colours directly and never invoke `colors.sh`, so they do NOT prove the data fix. The data fix is proven by the hermetic `scripts_contract` tests + the live run.
  - B6: README.md and WIKI.md document the tertiary completion order; full suite `cargo test` → 748 passed, 0 failed; `cargo check` → no warnings.
- Fix detail (B4): preference order is v4 palette **coherence-gated** on `primary` (a stale v4 from another scheme is skipped and logged) → shell-generated terminal template `color4` → documented distinct fallback `#94e2d5`. This is why the machine picks `#9e70d6` (kitty color4) and not the stale violet `#9d00ff` from the older-scheme v4 file.

## Next step

Changes complete and verified locally. Parent decision: review the work-unit commits and, if approved, reinstall the binary so the fix reaches the user's live app (out of scope for this writer).
