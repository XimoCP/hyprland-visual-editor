# Proposal: Dynamic Border Tune Pane

## Change ID: dynamic-border-tune-pane

## Summary

Rebuild the Borders section tune pane so that clicking a border preset card loads ALL of that preset's real parameters into an editable form (colors, angle, size, glow, border animations, floating-window rule), with a human-readable description on every control, and Save writes the edited result as a full-schema user preset. The static 4-slider form and the lossy Save go away.

## Problem

The tune pane (`BordersTunePane` in `ui/panel/sections/BordersSection.slint`) is a static form with 4 geometry sliders (`border_size` 1-5, `rounding` 0-100, `gaps_in`/`gaps_out` 0-30) plus a save form. Two defects follow:

1. **Nothing loads.** Clicking a preset card (`apply-border` in `src/main.rs`) applies the file through the engine and snaps the 4 geometry sliders, but no color, glow, animation, or rule value ever reaches the tune pane. There is nothing to edit.
2. **Save is lossy and fictional.** The save handler reads `border_size`/`border_radius`/`gaps_in`/`gaps_out` from `Config` and `PresetStore::generate_border_lua` emits `rounding`, `gaps_in`, `gaps_out` — parameters that exist in NONE of the 14 shipped presets (`assets/borders/*.lua`). A saved "preset" therefore captures neither what the user saw nor anything the engine's border path uses. Verified against all 14 files: the real schema is `general.col.active_border.colors` (2-8 entries) + `angle` (30/45/90) + `general.col.inactive_border`, `general.border_size`, optional `decoration.shadow.*`, the fixed `windowrulev2` pair, `hl.animation` leaves (`borderangle`, `border`, `fadeShadow`), and file-local `hl.curve(name)` definitions.

## Scope (In)

1. New Rust reader module that parses a border `.lua` file into a parameter struct (colors, angle, inactive, size, glow, window-rule presence, three animation leaves, file-local curves), handling every color form found in the data: bare palette tokens, quoted `"rgba(rrggbbaa)"`, quoted `"0xffRRGGBB"`, quoted `"#rrggbbaa"`, and Lua `local` aliases (only `13_the_joker.lua`).
2. Tune pane rebuild: dynamic color slots (2-8, one per preset color), gradient angle, inactive color, border size 1-5, glow group (shown when present, addable when absent), the three border animation leaves, floating-window rule as a boolean toggle. EVERY control carries a clear human-readable description.
3. Dual-mode color input per slot: **Follow theme** (6 palette tokens via large swatches) and **Custom color** (hue slider + saturation/brightness 2D area + opacity slider + large live preview; optional hex field as power-user shortcut only). Custom mode adapts the MIT-licensed `NeonColorPicker` (`wilmercampagna/slint-ui-system`, MIT OR Apache-2.0, © 2026 Wilmer Campagna) with an added alpha slider and a project-token remap; the copyright notice is retained. Slint's own LSP picker is rejected as GPL/royalty-free (license-incompatible).
4. Full-schema Save: extend `src/preset_store.rs` Lua generation to emit the complete border schema from tune state; existing list/load/save/rename/delete and its 5 unit tests stay green.
5. Full 3-layer property/callback plumbing (`MainWindow` -> `ShellRoot` -> `PanelRoot` -> `BordersSection`), keyboard navigation update in `PanelRoot` for the new control set, and headless render verification of the new pane.

## Scope (Out)

- Animation presets and their 20-leaf parameter surface (later change with progressive disclosure: basic controls + Advanced section).
- Circular HSV wheel picker (the linear hue slider + 2D SV area is the approved form).
- Engine changes of any kind: `src/engine.rs`, `src/config.rs`, `src/settings.rs`, `src/theme_manager.rs`, `src/app_state.rs`, `src/watcher.rs`, `src/utils.rs`, `src/providers/*` are sealed and untouched.
- Monitors/HDR/display layout (hyprmod territory, never HVE).

## Approach

Three chained work units (<400 changed lines each), strict TDD (`cargo test`, failing test first):

- **Unit 1 — Lua reader + full-schema generation**: new `src/border_preset.rs` (pure parse functions, headless-tested against the real 14 files as fixtures) + extended `PresetStore` generation. Zero UI.
- **Unit 2 — Rust plumbing**: card click reads the `.lua` file and pushes parameters into new window properties; **every edit applies live** through a hidden draft preset (written outside the user preset dir, debounced, invisible to the list) so the user sees changes instantly, honoring the vision rule "everything instant"; Save persists the draft as a full-schema user preset and refreshes the user list. Engine call paths (`apply_border`, `apply_geometry`) reused unchanged.
- **Unit 3 — Slint tune pane**: rebuild `BordersTunePane` (dynamic slots, glow group, animations, rule toggle, dual-mode color input, per-control descriptions), forward properties/callbacks through all 4 layers, update `PanelRoot` keyboard dispatch, headless render check (`cargo test slice_focus_flow_renders` + read the PNGs).

## Capabilities

> Contract for sdd-spec. No border-tune spec archived yet; lineage is the current `BordersSection` behavior (sliders + save form + card list).

### New Capabilities
- `border-tune-pane`: dynamic tune contract — parameter loading from preset files, per-control descriptions, dual-mode color input, glow/animation/rule controls, full-schema save.

### Modified Capabilities
- None (engine, composer, gallery specs untouched).

## Affected Areas

| Area | Impact |
|------|--------|
| `src/border_preset.rs` (new) | Lua reader: params struct + pure parse fns + unit tests over real fixtures |
| `src/preset_store.rs` | Extend `generate_border_lua` to the full border schema (CRUD untouched) |
| `src/main.rs` / `src/callbacks.rs` | Card click loads params into window props; Save persists tune state |
| `ui/main.slint`, `ui/shell.slint`, `ui/panel/PanelRoot.slint` | New properties/callbacks forwarded through all layers |
| `ui/panel/sections/BordersSection.slint` | Tune pane rebuild (dynamic slots, glow, animations, rule, color input, descriptions) |
| `src/engine.rs`, `src/config.rs`, providers, etc. | Sealed — no changes |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Lua parsing drifts from real files (new preset forms later) | Med | Reader is line-pattern based, never `eval`; unknown forms fall back to documented defaults; fixture tests pin all 14 current files |
| Tune pane exceeds 400-line review budget in one step | High | Unit 3 splits further (slots+basics, then glow+animations+rule, then color picker) if the forecast exceeds budget |
| Keyboard index math in PanelRoot breaks (control count changes) | Med | Focus indices recomputed from explicit pane layouts; existing slider tests updated in the same commit |
| Palette token `error` has no `ColorScheme` field for display | Low | Documented fallback in design (assumption A2); flagged at review |

## Rollback Plan

Each unit reverts independently via git revert. Unit 1 is additive (new module + new generation fn; old `generate_border_lua` kept until Unit 2 switches the call site). Unit 2 call sites are the existing `apply-border`/`save-border-preset` handlers — reverting restores the static-slider behavior. Unit 3 is Slint-only; reverting restores the 4-slider pane. No data migrations.

## Dependencies

- Slint skill installed; `src/theme.rs` `parse_hex()` + `ColorScheme` for token display resolution.
- `engine.scan("borders")` metadata flow and `PresetStore` CRUD (5 tests) stay green throughout.
- `slice_focus_flow_renders` headless harness for visual verification.

## Success Criteria

- [ ] Clicking any of the 14 border cards loads its exact colors (2-8 slots), angle, inactive color, size, glow state, 3 animation leaves, and rule state into the tune pane
- [ ] Every control shows a clear human-readable description
- [ ] Color slots offer Follow-theme swatches and Custom picker (hue + 2D SV + opacity + preview); hex is optional only
- [ ] Glow group appears when the preset has it and is addable when it does not
- [ ] Every edit applies live: color/glow/animation changes are visible immediately without saving (via a hidden draft preset that never appears in the preset list)
- [ ] Save writes a user preset that round-trips: re-selecting it reproduces the tuned parameters
- [ ] `cargo test` fully green; headless PNGs visually confirm the new pane geometry
