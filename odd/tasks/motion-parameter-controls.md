# Motion: editable animation parameters, save like Borders, always-visible curve

## Objective

Give the Motion (animations) section the same working shape as Borders:
editable parameters (curve + style family + speed), saving a preset that appears
as a user card on the left with apply/rename/delete, read-back when applying, and
a curve indicator that is always visible and comfortably sized.

## Current reality (verified)

- Saving already creates a user card: `src/main.rs:4700` refreshes
  `user-animation-preset-names`, and `MotionSection.slint:377-388` renders user
  cards with apply/rename/delete. The rename/delete dialogs already shipped.
- What is missing vs Borders:
  1. The saved preset is incomplete: `PresetStore::generate_animation_lua`
     (`src/preset_store.rs:265-292`) writes only 4 bezier floats and a hardcoded
     `speed = 0`; no style, no named curve, no `hl.animation` leaves.
  2. No read-back on apply: `on_panel_apply_animation` (`src/main.rs:4426-4454`)
     never parses the file; Borders syncs its pane from the file on every apply
     (`sync_border_tune_pane`, `src/main.rs:193-309`).
  3. No parser for animation presets (only `src/border_preset.rs` exists).
  4. No re-apply after save (Borders does at `src/main.rs:4492`).
  5. No dirty/live-draft state for Motion.
  6. The UI model is only 4 bezier floats (`MotionBezier`, `src/callbacks.rs:791-827`).
- Curve indicator: `ui/panel/CurvePreview.slint:10-78`, used at
  `MotionSection.slint:111-118`. It is INSIDE the tune pane's `ScrollView`, so it
  scrolls away; its control-point dots are fixed 6x6px (`:63-64`, `:71-72`).

## Key design decision (needs the keeper's call)

Built-in presets are per-leaf: each `hl.animation` leaf has its OWN `speed` (1-5)
and a reference to a named `hl.curve`. There is no single global speed in real
files. Two options:

- **A (recommended): a user-facing global model.** The UI exposes one curve, one
  style family and one speed. Applying/saving builds a user preset that sets
  those on a canonical set of leaves (windowsIn / windowsOut / workspaces, the
  three visible transitions). Built-in presets stay read-only: opening one shows
  its primary curve and a representative speed/style; editing and saving produces
  a USER preset in our global model. Simple, matches "not overwhelming".
- **B: per-leaf editing.** Faithful to the files but explodes into dozens of
  controls. Rejected as the default (see the earlier parameter analysis).

Plan assumes **A**. Confirm before apply.

## Phases (each closes with tests green and a work-unit commit)

### Phase 1 — Animation preset model + parser (Rust)
- New `src/animation_preset.rs`: parse a user preset (curve points, speed, style)
  and read a built-in's primary curve + representative speed/style.
- Define the canonical user schema the writer emits and the parser reads, in the
  shape `apply_animation.sh` copies verbatim into the fragment.
- Round-trip test: `parse(generate(model)) == model`.

### Phase 2 — Motion controls
- Add a speed slider and a style-family dropdown (short enum: slide / fade /
  popin) next to the existing 4 bezier values.
- Wire them into the Motion model + a `motion-dirty` flag mirroring Borders'
  `tune-dirty`.

### Phase 3 — Save like Borders
- On save, generate the COMPLETE user preset from the model, write it, refresh
  the left list (already works), re-apply it, clear dirty and error — mirroring
  `src/main.rs:4461-4492`.

### Phase 4 — Read-back on apply
- When applying a built-in or user preset, parse it and sync the pane's curve /
  speed / style, mirroring `sync_border_tune_pane`. User presets round-trip
  exactly; built-ins show their primary curve.

### Phase 5 — Always-visible, bigger curve indicator
- Move `CurvePreview` OUT of the tune pane's `ScrollView` into the fixed section
  header (same idiom as the Borders dirty indicator, `BordersSection.slint:2529-2547`).
- Enlarge it (proposal: fill the header width up to ~260px, height ~120px) and
  scale the two control-point dots proportionally instead of the fixed 6px.

### Phase 6 — Verification
- Parser unit tests, UI tests (controls, save creates the card, read-back),
  render tests + read the PNGs, cross-model verification (Tier 3).

## Constraints

- Strict TDD, runner `cargo test`. English artifacts.
- Do NOT touch the engine/providers or the Borders section.
- Do NOT run `cargo fmt`. One writer at a time.
- This is large (>400 lines): plan for chained work units / PR slices.

## Open questions for the keeper

1. Confirm option A (global model) vs B (per-leaf). — **ANSWERED: A.**
2. Style family list: is slide / fade / popin enough, or add slidefade?
3. Curve indicator size/placement: header width, or a dedicated always-visible strip.

## Phase 1 spec — animation preset model + parser

Built-in schema to match (`assets/animations/07_lineal.lua`, `19_stylized2.5D.lua`):
`hl.curve("name", { type = "bezier", points = { { x1, y1 }, { x2, y2 } } })` plus
`hl.animation({ leaf = "...", enabled = true, speed = N, bezier = "name", style = "..." })`.

New file `src/animation_preset.rs`:
- `pub enum AnimationStyle { Slide, Fade, Popin }` with `as_lua()` -> `"slide"|"fade"|"popin"`
  and `from_lua(&str)` that matches the family prefix and ignores any `%`
  (`"popin 70%"` -> `Popin`, `"slide right"` -> `Slide`).
- `pub struct AnimationParams { pub bezier: [f32; 4], pub speed: f32, pub style: AnimationStyle }`
  (derive `Debug, Clone, PartialEq`).
- `pub fn generate(name: &str, p: &AnimationParams) -> String` emitting the built-in
  schema: header (`-- @Title`, `-- @Source: user`, `-- @Tag: CUSTOM`, `-- @Desc`,
  `---@diagnostic disable: undefined-global`), the `hl.curve("hve_user", ...)` with the
  4 points, `hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })`,
  then for leaves `["windowsIn", "windowsOut", "windowsMove", "workspaces"]`:
  `hl.animation({ leaf = "<leaf>", enabled = true, speed = <speed>, bezier = "hve_user", style = "<style>" })`.
- `pub fn parse(content: &str) -> Option<AnimationParams>`: resolve the curve referenced by
  `windowsIn` (fallback: the first `hl.curve`), read its 2 points into 4 floats, and read
  `windowsIn`'s speed + style (fallback: the first `hl.animation` with a style). `None` when
  no curve/animation is found.
- Register `mod animation_preset;` in `src/main.rs`.
- Round-trip test: `parse(generate(p)) == Some(p)` for several params, plus a
  `parse` test against a real built-in (`assets/animations/19_stylized2.5D.lua`).

## Progress

- [x] Phase 1
- [ ] Phase 2
- [ ] Phase 3
- [ ] Phase 4
- [ ] Phase 5
- [ ] Phase 6

## Evidence

### Phase 1 — animation preset model + parser

- Files: `src/animation_preset.rs` (new), `src/main.rs:1` (`mod animation_preset;`).
- RED: `cargo test animation_preset` failed to compile — 24 errors
  (`undeclared type AnimationStyle`, `cannot find function generate/parse`).
- GREEN: `cargo test animation_preset` -> 7 passed, 0 failed (5 new + 2
  matching `scripts_contract` tests).
- Full suite: `cargo test` -> 1247 passed, 0 failed, 0 ignored.
- Round-trip: `parse(generate(p)) == Some(p)` for three param sets
  (popin/4.5, slide/2.0, fade/3.0), including integral floats (`0`, `1`).
- Built-in read: `assets/animations/19_stylized2.5D.lua` ->
  bezier `[0.4, -0.3, 0.2, 1.15]`, speed `4.5`, style `Popin`.
- `from_lua` matches the leading family token and ignores parameters:
  `"popin 70%"` -> Popin, `"slide right"` -> Slide, `"slidefade 15%"` -> None.
- Runtime harness: N/A — pure model/parser with no runtime boundary; consumers
  land in Phase 2 (Motion controls).
- Rollback: delete `src/animation_preset.rs` and its `mod` line in
  `src/main.rs`; no engine/provider/UI/preset_store files were touched.

## Next step

Phase 1 committed. Phase 2 (Motion controls: speed slider + style dropdown,
wired into the Motion model with a `motion-dirty` flag) needs the keeper's
approval before launch.
