# Tasks: Dynamic Border Tune Pane (dynamic-border-tune-pane)

Tags: [RED] failing test first · [GREEN] minimal impl · [WIRE] connect UI/Rust · [V] visual check (`cargo test slice_focus_flow_renders` + READ the PNGs under `/tmp/opencode/`). Size S<50 / M≤80 changed lines. Dep = prerequisite. Runner: `cargo test`; `.slint`-only tasks gate on `cargo check` + the render test. Engine files are sealed — any task touching them is out of scope.

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1100–1400 across 3 units |
| 400-line budget risk | High (Unit 3 especially) |
| Chained PRs recommended | Yes |
| Suggested split | U1 reader+generation → U2 Rust plumbing → U3a slots+basics → U3b glow+anims+rule → U3c color picker+keyboard+visual |
| Delivery strategy | auto-chain |
| Chain strategy | stacked-to-main |

Decision needed before apply: A2 `error` fallback only (D4 RESOLVED and user-approved: everything applies live via a hidden draft preset)
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: High

### Suggested Work Units

| Unit | Goal | Likely PR | Focused test command | Runtime harness | Rollback boundary |
|------|------|-----------|----------------------|-----------------|-------------------|
| 1 | Lua reader + full-schema generation, zero UI | U1 | `cargo test border_preset && cargo test preset_store` | headless only (no UI) | revert = delete new module + restore old generator |
| 2 | Card click loads params; edits apply live (hidden draft); Save persists | U2 | `cargo test` (full) | `cargo run`: click card → edit → see live change; save → file round-trips | revert = old 4-slider handlers return |
| 3a | Tune pane basics: dynamic slots + angle/inactive/size + descriptions | U3a | `cargo check` + render test | PNGs: slot counts per fixture | revert → 4-slider pane |
| 3b | Glow group + animations + rule toggle | U3b | `cargo check` + render test | PNGs: glow present/addable, leaves, toggle | revert → U3a pane |
| 3c | Color picker + keyboard + save wiring + visual sign-off | U3c | `cargo test` (full) + render test | PNGs: picker modes, focus states | revert → U3b pane |

### Unit 1 — Lua Reader + Full-Schema Generation (1.1–1.10)

- [x] 1.1 [RED] [f: src/border_preset.rs tests] [t: cargo test border_preset] [dep:-] [S] Failing contracts vs `unimplemented!()` stubs: parse `01_cascade.lua` → 2 token slots (primary, surface), angle 90, inactive token surface_lowest, size 1, glow None, rule off, single borderangle leaf disabled. Fixtures read from `assets/borders/`.
- [x] 1.2 [GREEN] [f: src/border_preset.rs] [t: cargo test border_preset] [dep:1.1] [M] Implement core reader: local-alias table, `colors`/`angle`/`inactive_border`/`border_size` extraction, bare-token vs quoted classification (`rgba()`, `0xff…`, `#…`).
- [x] 1.3 [RED] [f: src/border_preset.rs tests] [t: cargo test border_preset] [dep:1.2] [S] Failing joker contracts: `13_the_joker.lua` → 3 custom slots resolving locals (`39ff14ff`, `1a0026ff`, `9d00ffff` byte order pinned), glow present (range 20, power 4, `9d00ff88`/`9d00ff00`), NO `border_size` key → size falls back to keep-current sentinel.
- [x] 1.4 [GREEN] [f: src/border_preset.rs] [t: cargo test border_preset] [dep:1.3] [S] Implement local resolution + missing-size sentinel.
- [x] 1.5 [RED] [f: src/border_preset.rs tests] [t: cargo test border_preset] [dep:1.2] [S] Failing form contracts: `10_golden.lua` → 5 custom slots from `0xff…` (alpha ff, byte order pinned); `14_looper.lua` → inactive `#1a002655` parses with alpha 55; `08_neon.lua` → 6 token slots; `07_infinity.lua` → 8 slots incl. `error` token preserved as token.
- [x] 1.6 [GREEN] [f: src/border_preset.rs] [t: cargo test border_preset] [dep:1.5] [S] Implement remaining literal forms + `error` token preservation (A2 fallback lives in display resolution, not the reader).
- [x] 1.7 [RED] [f: src/border_preset.rs tests] [t: cargo test border_preset] [dep:-] [S] Failing block contracts: `12_neon_cyberpunk.lua` → glow present (range 20, power 4, colors, offset 0,0), rule on, triple leaves (borderangle enabled speed 30 bezier nv_neon_flow style loop; border + fadeShadow speeds), curves [nv_neon_flow]; `05_spectrum.lua` → glow None, rule off, single disabled leaf, no curves.
- [x] 1.8 [GREEN] [f: src/border_preset.rs] [t: cargo test border_preset] [dep:1.7] [M] Implement shadow/rule/animation/curve extraction + D3 absent-block defaults.
- [x] 1.9 [RED] [f: src/preset_store.rs tests] [t: cargo test preset_store] [dep:1.8] [S] Failing generation contracts: `generate_border_lua_full` emits colors+angle+inactive+size, glow block iff present, fixed rule pair iff on, file-local curves + 3 leaves; round-trip `parse(generate(p)) == p` for a token preset and a glow preset.
- [x] 1.10 [GREEN] [f: src/preset_store.rs] [t: cargo test preset_store] [dep:1.9] [M] Implement `generate_border_lua_full`; old geometry-only `generate_border_lua` + its test KEPT (deleted at U2 call-site switch per zero-warning rule). All 5 existing tests green.

### Unit 2 — Rust Plumbing: Load on Click, Save from Tune (2.1–2.6)

- [x] 2.1 [RED] [f: src/main.rs tests or src/callbacks.rs tests] [t: cargo test] [dep:1.10] [S] Failing headless contract (`init_no_event_loop` precedent): applying `07_infinity.lua` loads 8 tune color slots + angle 45 + rule on into window properties; applying `01_cascade.lua` loads 2 slots + glow absent + single leaf.
- [x] 2.2 [GREEN][WIRE] [f: src/main.rs `on_panel_apply_border`] [t: cargo test] [dep:2.1] [M] Read the `.lua` file (built-in dir or user dir by active source), `parse_file`, bulk-set tune properties; engine `apply_border` + geometry snap paths unchanged; missing `border_size` keeps current slider (D3).
- [x] 2.3 [RED] [f: src/preset_store.rs tests] [t: cargo test preset_store] [dep:2.2] [S] Failing: saved tune state for a 3-slot glow preset re-parses to identical params (file round-trip through `store.save` + `parse_file` on a temp dir).
- [x] 2.4 [GREEN][WIRE] [f: src/main.rs `on_panel_save_border_preset`] [t: cargo test] [dep:2.3] [M] Save builds params from tune properties → `generate_border_lua_full` → `store.save` → existing user-list refresh; delete old `generate_border_lua` + migrate its test here; remove the hidden draft. Live-vs-saved status line property set (D4).
- [x] 2.5 [WIRE] [f: ui/main.slint, ui/shell.slint, ui/panel/PanelRoot.slint] [t: cargo check] [dep:2.2] [M] Declare + forward every new tune property/callback through all 4 layers (D6); descriptions as `*_desc` string properties. No visual change yet (pane rebuild is U3).
- [x] 2.6 [RED][GREEN][WIRE] [f: src/main.rs live-edit path + src/preset_store.rs draft helpers] [t: cargo test] [dep:2.2] [M] Live draft (D4, user-approved): SPIKE first — confirm whether `apply_border` resolves a name inside the scanned dirs or an arbitrary path. Then implement: write ONE hidden draft OUTSIDE the user preset dir, apply it through `apply_border`, debounced (picker `committed` + 80ms; NEVER per drag step), invisible to `PresetStore::list()`, cleaned on save/leave/deselect. Headless test: an edit writes+applies the draft and the user preset list is unchanged.
- [x] 2.7 [t: cargo test] [dep:2.1–2.6] [S] Full suite green: reader + store + plumbing + draft + all pre-existing tests, zero regressions.

### Unit 3 — Slint Tune Pane Rebuild (3.1–3.10)

- [x] 3.1 [f: ui/panel/sections/BordersSection.slint] [t: cargo check] [dep:2.5] [M] Tune pane basics: dynamic slot placeholders (count from `tune-color-count`, 2..8) + angle segmented control (30/45/90) + inactive color row + size slider 1..5. Every control gets its description text. Old 4-slider block removed.
- [x] 3.2 [f: ui/panel/sections/BorderColorSlot.slint (new)] [t: cargo check] [dep:3.1] [M] Follow-theme mode: 6 large token swatches (primary, secondary, tertiary, error, surface, surface_lowest) with descriptions; selection writes the slot value. Palette colors forwarded as properties from the 4-layer chain.
- [x] 3.3 [WIRE] [f: BordersSection.slint, BorderColorSlot.slint] [t: cargo check] [dep:3.2] [S] Instantiate one slot component per color (2..8); slot add/remove clamped with descriptions (why 2..8).
- [x] 3.4 [f: ui/panel/sections/BordersSection.slint] [t: cargo check] [dep:3.3] [M] Glow group: full field set (enabled, range, render_power, color, color_inactive) with descriptions when present; addable empty state (Add glow button + description) when absent; removing returns to empty state.
- [x] 3.5 [f: ui/panel/sections/BordersSection.slint] [t: cargo check] [dep:3.4] [M] Three animation leaves (borderangle, border, fadeShadow): enabled + speed + bezier (preset curves + default) + style, each with descriptions; floating-rule boolean toggle with description (fixed pair, never free text).
- [x] 3.6 [f: ui/panel/sections/BorderColorSlot.slint] [t: cargo check] [dep:3.2] [M] Custom-color mode: ADAPT `NeonColorPicker` from `wilmercampagna/slint-ui-system` (MIT OR Apache-2.0, © 2026 Wilmer Campagna) — RETAIN the header notice, ADD an opacity/alpha slider + alpha output (upstream is RGB-only), REMAP `Theme.*` to `SkwdTokens` — then wrap with the hue slider + SV 2D TouchArea + large live preview + optional hex LineEdit (invalid keeps last valid + hint). Mode switch preserves the color across modes where representable. Rejected: Slint LSP picker (GPL/royalty-free, license-incompatible).
- [x] 3.7 [WIRE] [f: ui/panel/PanelRoot.slint] [t: cargo check] [dep:3.1–3.6] [M] Keyboard dispatch recomputed for the new control inventory (focus order slots → angle → inactive → size → glow → animations → rule → save); engaged-slider model extended or replaced with per-group engagement; wrap/clamp behavior preserved.
- [x] 3.8 [WIRE] [f: ui/panel/sections/BordersSection.slint] [t: cargo check] [dep:3.7] [S] Live-vs-saved status line (D4: working state applies live, not persisted until Save) + save form rewired to tune state; error-text path reused.
- [x] 3.9 [V] [t: cargo test slice_focus_flow_renders] [dep:3.1–3.8] [M] Extend the render harness to mount the rebuilt pane (or add a sibling render test); run it, then READ every PNG under `/tmp/opencode/` and confirm: slot counts (2 for Duo, 8 for Infinity), glow present/addable states, descriptions rendered, picker modes, no clipped geometry.
- [x] 3.10 [t: cargo test] [dep:3.9] [S] Full suite green: all pre-existing + new tests, zero regressions; visual sign-off recorded.

## Traceability

Reader core → 1.1–1.2 · Joker locals + missing size → 1.3–1.4 · Literal forms → 1.5–1.6 · Glow/rule/animations/curves → 1.7–1.8 · Generation + round-trip → 1.9–1.10 · Load on click → 2.1–2.2 · Save from tune → 2.3–2.4 · 4-layer forwarding → 2.5 · Full-suite gate → 2.6 · Slots + basics → 3.1–3.3 · Glow → 3.4 · Animations + rule → 3.5 · Picker → 3.2+3.6 · Keyboard → 3.7 · Status + save form → 3.8 · Visual proof → 3.9 · Final gate → 3.10 · Preserved (list pane, card CRUD rename/delete, engine apply paths, animation presets) → untouched, guarded by 2.6 + 3.10.
