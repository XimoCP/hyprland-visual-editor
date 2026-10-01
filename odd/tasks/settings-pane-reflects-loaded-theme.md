# ODD — Settings panes must reflect the loaded theme

Feature name: `settings-pane-reflects-loaded-theme`
Branch: `hve2-visual-rewrite` (current; not the default branch)
Status: open — implemented by the T1..T4 batch, verification pending

## Objective

Everything the Settings panel displays must be what the loaded theme actually has, the moment the
panel is opened: border preset, border colours, glow, and geometry. The System section is explicitly
out of scope (keeper's exclusion). Animations are out of scope for this change because they are going
to be rewritten like Borders; the requirement is captured under **Deferred** so the rewrite inherits it.

## Problem (verified 2026-10-01, read-only investigation)

1. **The tune pane is filled only on apply paths.** `sync_border_tune_pane` (`src/main.rs:188-302`) is
   the only function that parses the active border preset and fills `tune-color-count`,
   `tune-active-colors`, `tune-angle`, `tune-inactive-color`, `tune-glow*`, `tune-colors-resolved`.
   Its only three call sites are applies: gallery theme apply (`src/main.rs:3115`), saved-theme apply
   (`:3916`), manual border card click (`:4308`). It never runs at startup and never on section entry,
   so a fresh launch leaves the Slint defaults: no colour chips, glow switch off.
2. **Section entry only writes the card index.** `on_panel_section_selected` (`src/main.rs:3577`,
   Borders block `:3611-3619`) calls only `presets::reseed_active_border_index`
   (`src/presets.rs:112-137`). The active card *is* marked at startup (`src/presets.rs:83`), which is
   why the panel reads as "applied but empty".
3. **Geometry comes from a hard-coded table that never matches.** `preset_geometry_for`
   (`src/callbacks.rs:849-857`) is a literal table keyed by `thin-rounded.ron`, `sharp.ron`,
   `thick.ron`, `01_cascade.conf`. Every shipped border preset is `*.lua` (`assets/borders/*.lua`), so
   the lookup always misses and falls back to `BorderGeometry::default()` = `2/32/5/5`.
4. **Consequence A — the pane lies about the desktop.** Applying a theme whose geometry is e.g.
   `5/20/10/10` writes those into `Config` and Hyprland, then the same apply sets the sliders to
   `2/32/5/5`. Pane != desktop != config. After a restart the sliders show the true values again, so
   the same state displays differently before and after a restart.
5. **Consequence B — a click silently discards your geometry.** `on_panel_apply_border`
   (`src/main.rs:4287-4295`) treats the table value as truth: when it differs from `cfg` it overwrites
   `cfg` and applies it to Hyprland. Picking any border card therefore resets size/radius/gaps to
   `2/32/5/5`.
6. **Clicking the active card cannot repopulate the pane**: it is treated as deactivation
   (`src/main.rs:4281`).
7. **A section-entry-only fix misses the reopen path.** Reopening the panel onto the same section emits
   no `section-selected` (`ui/panel/PanelRoot.slint:230-242` — `init`/`changed open` only touch
   internal state), so a startup seed is required as well.

## Source of truth (decided)

| What | Reads from |
|------|------------|
| Geometry (size, radius, gaps in/out) | Persisted state `Config.border_size/border_radius/gaps_in/gaps_out` (`src/config.rs:27-33`) |
| Border identity | `Config.active_border_file` |
| Border colours + glow | The preset `.lua` via `PresetStore::read_border_file` + `border_preset::parse` (`src/main.rs:238-239`) — already correct inside the sync function |

Why persisted state for geometry and not the theme's `state.json`: applying a theme writes the theme's
geometry into `Config` (`src/providers/hve_presets.rs:107-145` from `state.json` at `:40-62`), so
reading `Config` *is* "what the loaded theme has" right after load or apply, and it is also what
Hyprland is rendering. Reading the theme file instead would visually revert the user's unsaved in-session
edits every time the section is re-entered.

Ordering constraint: seed strictly AFTER `refresh_visual_state` (`src/main.rs:3525`), because
`refresh_resolved_colors` (`src/main.rs:156-170`) bakes concrete `slint::Color`s from `token-*`;
seeding earlier bakes the default accent.

## Scope

**In:** seeding the Borders tune pane (colours, angle, glow, geometry) at startup and on Borders entry;
geometry sourced from persisted state; the card-click geometry overwrite.

**Out:** System section (keeper's exclusion); new animation storage; monitors/HDR; anything in skwd.

## Decisions accepted

- **D1** — Geometry source is persisted state; the hard-coded `preset_geometry_for` table is retired
  because its keys match no shipped preset. Its pinning tests go with it.
- **D2** — The optional `border_size` inside a preset `.lua` stays unread (only 12 of 14 carry it, and no
  preset carries radius or gaps). Recorded as future work, not part of this change.
- **D3** — A border card click must never write geometry to `cfg` from a lookup table; it preserves the
  current geometry.

## Tasks

- [x] **T1 — Tests first (RED).** Add failing tests before any production edit:
      (a) entering Borders seeds the tune pane from persisted state (colour chips count > 0, glow true,
      angle from the preset) — not from Slint defaults;
      (b) the geometry shown equals the persisted values, not `2/32/5/5`;
      (c) a startup seed exists and runs after the token refresh;
      (d) clicking a border card leaves `cfg` geometry untouched.
      Update or replace the tests that pin the hard-coded default
      (`src/shell/ui_tests.rs:11794-11798`, `src/main.rs:5353-5360`, `src/callbacks.rs:1541-1559`).
- [x] **T2 — Geometry from persisted state.** Make `sync_border_tune_pane` take geometry from
      state/`Config` instead of `preset_geometry_for`, and stop the card-click path from writing the
      table value into `cfg`.
- [x] **T3 — Seed at entry and at startup.** Call the sync in the Borders block of
      `on_panel_section_selected` (`src/main.rs:3611-3619`) next to the existing reseed, and once at
      startup after `refresh_visual_state` (`src/main.rs:3525`) guarded on a non-empty
      `active_border_file` (an empty file clears the pane, `src/main.rs:198-228`).
- [x] **T4 — Retire the table.** Remove `preset_geometry_for` (`src/callbacks.rs:849-857`) and its
      dead-key tests; keep the default geometry only where it is genuinely a default.
- [ ] **T5 — Cross-model verification** (independent model, not the writer's) over the whole batch.
- [ ] **T6 — Keeper's live check**: start HVE with a theme loaded, open Settings -> Borders and confirm
      the pane matches the desktop; pick a border card and confirm the geometry does not jump.

## Acceptance criteria

- Opening Settings -> Borders right after launch shows the loaded theme's border, its colours, its glow
  state and its geometry.
- Geometry shown equals the geometry persisted for that theme, and no restart is needed to make them agree.
- Clicking a border card never overwrites persisted geometry with a hard-coded default.
- Reopening the panel straight into Borders shows the same correct values (not only a fresh section switch).
- No ping-pong: seeding does not itself trigger an apply, a config write, or a watcher burst.
- Nothing in skwd is modified; the engine modules are reused, not rewritten.

## Checks (resolved)

- TDD: **strict** (project rule + hve2 skill). Write the failing test first. Runner: `cargo test`.
- Evidence commands: `cargo test` (full suite, expected all green), `cargo build` (warning-free).
- Never `cargo fmt` in this repo (it once reformatted 54 files); never `git checkout -- .`.
- Visual: if any `.slint` changes, the headless render run plus reading the PNGs is mandatory. Expected:
  no `.slint` change, because the properties already exist and are bound (`ui/main.slint:456`,
  `ui/shell.slint:604`, `ui/panel/PanelRoot.slint:762-819`).
- Commit as one reviewable work unit, conventional commit message, no AI attribution.

## Delivery

- Forecast: ~150-260 authored lines (Rust + tests) — under the ~400-line review budget, so a single PR
  with the default `ask-on-risk` strategy; no split decision needed. Chain strategy: not applicable.
- Route: T1-T4 delegated to one bounded writer (two or more non-trivial files); T5 is an independent
  delegated verification on a different model, not the writer's.

## Progress / evidence

Writer: one bounded batch (T1-T4), strict TDD, single work-unit commit. Commit SHA is recorded in
the ODD completion report: a commit cannot contain its own hash, so the doc states the unit and the
report states the identity. Rollback boundary is this commit alone — `src/main.rs`,
`src/callbacks.rs`, `src/shell/ui_tests.rs` and this doc; no engine module and no `.slint` file is
part of it.

### T1 — RED before any production edit

`cargo test --no-run` on the test-only change failed with 5 errors, all of them the intended ones:

```
error[E0425]: cannot find function `seed_borders_pane_from_persisted` in the crate root
error[E0599]: no function or associated item named `from_config` found for struct `BorderGeometry`
error[E0061]: this function takes 3 arguments but 4 arguments were supplied     (x3, the sync call sites)
error: could not compile `hve` (bin "hve" test) due to 5 previous errors
```

A second RED pass covered the re-select guard
(`borders_entry_seed_skips_a_reselect_of_the_active_section`): 4x
`cannot find function borders_entry_should_seed in module super`.

Tests added or replaced:

- `shell::ui_tests::borders_entry_seed_fills_pane_from_persisted_state` (new, a) — real
  `13_the_joker.lua`: 3 chips, angle 45, glow enabled, marker index 1, geometry 5/20/10/10.
- `shell::ui_tests::theme_border_sync_replaces_stale_tune_pane_like_manual_pick` (updated, b) — pins
  the persisted geometry 5/20/10/10 and asserts it is not `2/32/5/5`.
- `shell::ui_tests::theme_empty_border_clears_tune_pane_like_manual_deselect` (updated) — deselect
  clears the border props and leaves the geometry sliders alone.
- `shell::ui_tests::startup_seeds_borders_pane_after_token_refresh_by_construction` (new, c) — the
  seed call sits between `refresh_visual_state` and `AppState::new`, is guarded on a non-empty
  `active_border_file`, and the funnel body cannot apply or persist.
- `shell::ui_tests::border_card_click_preserves_persisted_geometry_by_construction` (new, d) — the
  `on_panel_apply_border` body contains neither `preset_geometry_for` nor `apply_geometry`.
- `tests::borders_entry_seed_skips_a_reselect_of_the_active_section` (new) — pure predicate guard.
- `callbacks::geometry_tune_tests::border_geometry_reads_persisted_config` (new) — `from_config`.
- `shell::ui_tests::entering_borders_reseeds_active_index_by_construction` (updated) — the entry
  funnel is invoked in the handler, the funnel performs the reseed, and the handler uses the
  re-select guard.
- Deleted (they pinned the retired table): `callbacks::geometry_tune_tests::test_pick_snaps_sliders`,
  `tests::test_pick_snaps_sliders` (main.rs).

### T2 — geometry source

`sync_border_tune_pane` gained a `geometry: BorderGeometry` parameter and sets the four sliders from
it; `BorderGeometry::from_config` (`src/callbacks.rs`) is the single conversion from
`Config.border_size/border_radius/gaps_in/gaps_out`. All three apply paths read it from `cfg` under
the short state lock they already held. The card-click path no longer compares/writes geometry or
calls `apply_geometry` (D3): a card carries identity only.

### T3 — entry and startup

`seed_borders_pane_from_persisted` (`src/main.rs`) is the one funnel: reseed the card marker, then
sync the pane. The Borders block of `on_panel_section_selected` calls it, and `main` calls it once
after `refresh_visual_state` and before `AppState::new`, guarded on a non-empty `active_border_file`.

Ordering is asserted, not assumed: `startup_seeds_borders_pane_after_token_refresh_by_construction`
checks the call sits in the window between the visual-state push and `AppState::new`, so the seeded
`slint::Color`s are baked from the loaded `token-*` values and not from the default accent.

### T4 — table retired

`preset_geometry_for` is gone; `BorderGeometry::default()` remains only where it genuinely is an
initial value (the debouncer tests and the funnel test fixture). No `.slint` change was needed, so
the headless render check does not apply to this change.

### Evidence

- `cargo test` — `test result: ok. 1209 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;
  finished in 74.83s`.
- `cargo build` — `Finished dev profile ... in 24.61s`, zero warnings (`cargo build 2>&1 | grep -c
  ^warning` → `0`).
- `grep -rn preset_geometry_for src/` — only an assertion string in a test and a retirement note in
  a doc comment; no production reference.
- Branch untouched otherwise; nothing in `skwd/` touched; `src/config.rs` untouched.

### Review workload

`git diff --cached --numstat` for this commit:

```
266    0   odd/tasks/settings-pane-reflects-loaded-theme.md
 37   34   src/callbacks.rs
145   51   src/main.rs
185   13   src/shell/ui_tests.rs
```

That is **465 authored Rust lines** (367 additions + 98 deletions), above the ~400 review advisory
and above this doc's 150-260 forecast. The task doc shows 266 additions because it enters the index
as a new file; ~104 of those lines are this change's evidence section, the rest was already authored
with the task. The overage is tests (three new construction/behaviour tests, four updated tests, two
deleted table pins) plus the comment density this repository already enforces. No production logic was
padded to reach a number and nothing was shrunk to hide it. The change is one inseparable behaviour
(pane seeded from persisted state) and does not slice into coherent work units, so it ships as one
commit and the overage is recorded instead of hidden.

### Decision the doc did not cover (writer)

PanelMenu fires `selected` on **every** click of a rail item (`ui/panel/PanelMenu.slint:106`), so
re-clicking the already-open Borders section re-emits `section-selected(1)`. Seeding there would have
cleared the pane's dirty marker and repainted persisted values while the user's live draft fragment
was still assembled — a silent draft wipe, a regression the doc's "entering Borders seeds" wording
did not anticipate. Guarded with `borders_entry_should_seed(is_closed, prev_section)`: a seed runs
when the panel is opening onto Borders or when the previous section was anything else, and is
skipped on a plain re-select. Covered by a pure-predicate test plus a construction assertion.

### Honestly not done / pending

- **T5 (cross-model verification) and T6 (keeper's live check) are not claimed here.** They need a
  different model and the real desktop, so they remain open in this doc.
- The within-session `Esc`-close-then-reopen-onto-Borders gap (the pane keeps its last in-memory
  values, and `teardown_border_draft` does not reset `tune-dirty`) is **pre-existing and untouched**
  by this change; the doc scopes the reopen requirement to the process-start case, which the startup
  seed covers.
- D2 stands as recorded: a `border_size` inside a preset `.lua` stays unread.

## Deferred — keeper's requirement for the animations rewrite

When animations are rewritten the way Borders is being rewritten now, the Motion pane must load the
animation's real values from the loaded theme. Today only the identity is recoverable:

- The theme record stores only `active_anim_file` (`src/providers/hve_presets.rs:40-62`; confirmed live in
  `~/.config/hve/themes/MiniJoker/providers/hve-presets/state.json`).
- `Config` has no bezier or duration field (`src/config.rs:23-46`).
- `bezier-a/b/c/d` are pure Slint defaults (`ui/panel/sections/MotionSection.slint:423-426`) and the only
  production writes are reads for saving (`src/main.rs:4576-4579`); every `set_bezier_*` outside that is
  in tests (`src/shell/ui_tests.rs:3237-3240`). The apply path documents the gap itself
  ("Future: per-animation bezier map", `src/main.rs:4343-4344`).
- The values exist in the animation `.lua` (`assets/animations/12_rebote.lua:7-23`) and in
  `PresetStore::generate_animation_lua` (`src/preset_store.rs:229-256`), but nothing reads curve points
  back: `preset_store.rs:176-178` and `border_preset.rs:516-549` recover curve *names* only, and the
  existing parser targets the border leaf vocabulary, not animation leaves.

So the rewrite needs both new storage (effective bezier/duration/style in the theme record + `Config`)
and an animations-category parser. Filters (shaders) needs none: it displays identity only, though it
also lacks an entry-time reseed.

## Relevant files

- `src/main.rs` — `sync_border_tune_pane` (`:188-302`), startup push (`:2474-2485`), token refresh
  (`:3525`), section-entry funnel (`:3577-3619`), card-click apply (`:4281-4295`).
- `src/callbacks.rs` — `preset_geometry_for` (`:849-857`), `BorderGeometry::default` (`:838-842`).
- `src/config.rs` — geometry + active-file storage (`:23-58`).
- `src/providers/hve_presets.rs` — theme record read/write (`:40-62`, `:107-145`).
- `src/presets.rs` — `populate_presets` / `reseed_active_border_index` (`:63-137`).
- `ui/panel/sections/BordersSection.slint` — pane properties (`:136-137`, `:173`) and the glow switch.
- `src/shell/ui_tests.rs` — construction guards and the tests that pin the hard-coded default.
