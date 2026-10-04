# The gallery remembers Slider vs Mosaic across a restart

**Opened**: 2026-10-04 (keeper's call: "si alguien quiere mosaico para siempre
no quiero que cuando inicie otra vez el pc tenga que volver a cambiar")
**Branch**: hve2-visual-rewrite
**Route**: direct inline (3 files, small additive change; no 4-file mapping needed)
**Status**: done (implemented, tests green, visual verified; commit recorded below)

## Objective

The presentation style chosen in the gallery must survive closing HVE and
rebooting the machine. Today the choice lives only in memory: every start forces
the Slider, so a keeper who prefers the Mosaic re-picks it every session.

## Diagnosis (verified, with evidence)

- The style is `gallery-style` (`in-out property <int>` on `MainWindow`,
  `ui/main.slint:271`). Values: `0` = Slider/Slice, `2` = Mosaic. `1` = Hexagon
  exists in the Rust enum but is NOT reachable from the chrome: the only two
  style entries offered are `style-selected(0)` and `style-selected(2)`
  (`ui/gallery/FilterBar.slint:154-190`, `ui/gallery/GalleryRoot.slint:290-531`).
- Exactly two writers in the whole binary:
  - `src/main.rs:3633` — `window.set_gallery_style(0)` at startup. Hardcoded.
  - `src/main.rs:3653` — `w.set_gallery_style(style)` inside the
    `on_gallery_style_selected` handler. In memory only; never persisted.
- Every chrome path (mouse drawer, FilterBar, keyboard) funnels into that ONE
  callback, so a single writer closes the hole. No second chokepoint to patch.
- `Config` (`src/config.rs`, persisted at `~/.config/hve/config.json`) already
  carries the UI preferences of the same kind — `language`, `theme`,
  `tiling_mode`, `minimize_seconds`, `auto_minimize_enabled`, `keybinds_enabled`
  — and has a versioned migration chain. `CONFIG_VERSION = 7`.
- `cfg` is loaded at `src/main.rs:2682`, so the value is in scope at the startup
  write site (line 3633). No extra load needed.
- The `gallery-style-selected` handler is wired BEFORE `AppState` exists
  (created at `src/main.rs:4032`), so it cannot capture `state`. It reaches the
  shared state through `gallery_state_slot` (`src/main.rs:2856`, filled at
  `4040`) — the same idiom already used at `src/main.rs:3391`.

## Scope

- `src/config.rs` — add `gallery_style: i32` (`#[serde(default)]`, default `0`),
  migration v7 → v8, `CONFIG_VERSION = 8`, tests.
- `src/main.rs` — read the persisted value at startup; dedupe the
  style-plus-cursor application into one helper shared by startup and the
  handler; persist inside the handler.
- Tests: `src/config.rs` (round-trip + migration) and `src/main.rs`
  (sanitizer + the shared apply helper).
- Do NOT touch: `ui/**` (the property already exists and is already wired),
  the Mosaic geometry/pagination model, `refresh_mosaic_page`, or the Hexagon
  engine.

## Decisions

1. **Where it is persisted**: `~/.config/hve/config.json`, next to `theme` and
   `language`. Same kind of preference, same store, same migration discipline.
   A separate file would fragment the app's own preferences for no gain.
2. **Only `0` and `2` are honored on load.** Anything else (a hand-edited `1`,
   a negative, a future value) opens the Slider. Rationale: the chrome cannot
   ENTER Hexagon, so honouring it would strand a keeper on a style the UI has no
   path back from — a persistence bug must not be able to do that. Stated as a
   deliberate narrowing, not an accident.
3. **Migration v7 → v8 writes `0`** for every existing install: behaviour for
   an upgrade is byte-identical to today (Slider), and nothing is inferred.
4. **A save failure is not fatal**: the style still changes on screen for this
   session; the write is best-effort, like every other `cfg().save()` on the
   event loop. Logged, never panics — a read-only config dir must not kill the
   gallery.
5. **The Mosaic cursor is seeded on start-up too**, not only on a manual switch:
   otherwise a persisted Mosaic opens with cursor `-1` and the first arrow press
   has no ring to move. Both paths now share one helper, so they cannot drift.

## Acceptance criteria

- Pick the Mosaic, quit, start again: the app opens on the Mosaic with the
  keyboard cursor seeded on the first tile.
- Pick the Slider, quit, start again: the app opens on the Slider (unchanged
  behaviour, and the migration default for existing installs).
- The persisted value is not writable by any other path: the single
  `gallery-style-selected` callback is the only writer.
- A `config.json` whose `gallery_style` is `1`, `-3` or absent opens the Slider.
- Upgrading an existing v7 `config.json` keeps the style at Slider and stamps
  `config_version: 8`.
- No `.slint` file changes, so no geometry can move.

## Checks

- Strict TDD is ON. Runner: `cargo test`. RED FIRST: paste the failing run
  before the implementation.
- `cargo test` full suite green at the end; report the pass count against the
  pre-change baseline.
- VISUAL: start-up in the Mosaic is a visual claim. Render into a per-run
  directory (`HVE_RENDER_DIR=/tmp/opencode/render-gallery-style-persist`) and
  READ the PNG with vision to confirm the Mosaic is the painted style, not the
  Slider. "Tests green" alone is not verification for this one.
- Do not reformat unrelated code; no blanket `cargo fmt`.

## Tasks

- [x] **T1 (RED)** — Failing tests: `Config::default().gallery_style == 0`;
  v7 → v8 migration stamps `8` and keeps `0`; a saved `2` round-trips; the
  sanitizer maps `2 → 2` and everything else `→ 0`.
- [x] **T2 (GREEN)** — `src/config.rs` field + migration + version;
  `src/main.rs` shared apply helper, start-up read, handler persist.
- [x] **T3 (VISUAL)** — Start-up render in the Mosaic, PNG read.
- [x] **T4 (COMMIT)** — One Conventional Commit, tests with the change; record
  the hash here.

## Evidence log

### T1 — RED on the pre-change head

`cargo test` (pre-change): 20 compile errors, `could not compile hve (bin "hve"
test)`. The new symbols did not exist yet:

```
error[E0425]: cannot find function `sanitized_gallery_style` in this scope
error[E0425]: cannot find function `gallery_cursor_for_style` in this scope
error[E0560]: struct `Config` has no field named `gallery_style`   (x2 sites)
error[E0609]: no field `gallery_style` on type `Config`
```

Rust RED is a compile error here: the behaviour under test cannot run before the
field and the helpers exist. The two `Config` struct literals in the tree are
`src/config.rs` (the round-trip test) and the one fixed in the tests; the
production literal in `src/providers/mod.rs:201` uses `..Default::default()` and
needed no change.

### T2 — change

- `src/config.rs`: `CONFIG_VERSION` 7 → 8; new `pub gallery_style: i32`
  (`#[serde(default)]`) documented as "0 = Slider, 2 = Mosaic, anything else
  falls back"; `Config::default()` sets `0`; migration
  `v7 → v8` writes `0` with the comment stating why an upgrade must not change
  which style is painted.
- `src/main.rs`: added `SLIDER_STYLE` / `MOSAIC_STYLE`, `sanitized_gallery_style`
  (pure), `gallery_cursor_for_style` (pure) and `apply_gallery_style` (the one
  place a style lands on the window, seeding the Mosaic cursor). Start-up now
  calls `apply_gallery_style(&window, sanitized_gallery_style(cfg.gallery_style))`
  instead of `set_gallery_style(0)`. The `on_gallery_style_selected` handler
  applies through the same helper (its inline cursor seed is gone, deduped) and
  persists `gallery_style` through `gallery_state_slot`, with a warn-on-failure
  save so a read-only config dir cannot kill the gallery.

### GREEN after the change

```
cargo test
test result: ok. 1317 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 88.61s
```

Baseline on the pre-change head: 1311 passed / 0 failed. The six new tests are
the two in `src/config.rs` (migration, on-disk value), the three in `src/main.rs`
(sanitizer, cursor rule, the two wiring pins) and the render test in
`src/shell/ui_tests.rs`. No existing test encoded the old behaviour; none was
updated or deleted.

### T3 — visual

Run directory: `/tmp/opencode/render-gallery-style-persist`

- `gallery_style_mosaic_startup.png` — persisted `2`: the Mosaic wall paints,
  three justified rectangles side by side (x ≈ 40–1900, y ≈ 215–910).
- `gallery_style_slider_startup.png` — persisted `0`: the Slice carousel paints,
  the expanded parallelogram card labelled "Theme 0" with "Theme 5" and
  "Theme 1" as reflowed neighbours on either side.

Both frames come from ONE window holding ONE card list, ONE wall model and ONE
carousel-tile model; the persisted value is the only thing that differs, and the
test asserts >5000 changed pixels between them. Read both PNGs with vision:
confirmed the wall in the first and the carousel in the second.

### T4 — commit

Hash: `a1023a8389e8d53a2497abe08b2d76855fc939b5`
(`feat(config): the gallery style survives a restart`). Contains
`src/config.rs`, `src/main.rs` and the tests in `src/shell/ui_tests.rs` together.
No AI attribution. The ficha itself lands in a follow-up `docs(odd)` commit.

### Deviations / tensions observed

- **`src/config.rs` is on the skill's "reuse, never rewrite" list.** Flagged, not
  hidden: the change is additive (one field, one migration step, one default) and
  the field is the same kind of UI preference already living there (`theme`,
  `language`, `tiling_mode`, `minimize_seconds`). No engine behaviour — borders,
  animations, shaders, assembly — is touched, and no existing field or migration
  step changed. If the keeper wants UI preferences out of `config.rs` entirely,
  that is a separate decision this task does not take.
- **The first render had a blank Slider frame.** The test built the Mosaic wall
  model but not the carousel model, so the persisted `0` frame painted nothing
  and the diff only proved "the wall is Mosaic-only" — weaker than the claim.
  Fixed by also setting `gallery_slice_tiles` / `slice_delta_base` /
  `slice_focus_pos`, so each persisted style paints its own view. Reported
  because the first passing run looked like evidence and was not.
- **`init_test_platform()` cannot be used here.** It installs
  `init_no_event_loop`, whose window adapter has no `take_snapshot`
  (`WindowAdapter::take_snapshot is not implemented by the platform`). The test
  follows the existing visual tests instead and gets its platform (and its
  snapshot support) from `gallery_with_production_callbacks()`.
