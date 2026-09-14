# Design: Dynamic Border Tune Pane

Change: `dynamic-border-tune-pane` · Budget: <400 lines per work unit · Engine: SEALED (no touch)

## Architecture Overview

```
.lua preset file                Rust                              Slint (single mutating window)
┌──────────────────┐   ┌─────────────────────────────────┐   ┌──────────────────────────────────────────┐
│ assets/borders/  │   │ border_preset.rs NEW (pure)     │   │ MainWindow (ui/main.slint)               │
│  *.lua  (14)     │──▶│  parse_file() -> BorderParams   │──▶│  ↕ new tune properties + callbacks       │
│ ~/.config/hve/   │   │  local-alias resolution         │   │ ShellRoot (ui/shell.slint)  ↕ forward    │
│  presets/borders/│   │  defaults for absent blocks     │   │ PanelRoot                   ↕ forward    │
│  *.lua (user)    │   │                                 │   │ BordersSection/TunePane     ↕ render     │
│                  │   │ preset_store.rs EXTENDED        │   │  ├ dynamic color slots (2..8)            │
└──────────────────┘   │  generate_border_lua_full()     │   │  ├ angle / inactive / size               │
                       │  list/load/save/rename/delete   │   │  ├ glow group (present │ addable)        │
Config (sealed)        │  (unchanged, 5 tests green)     │   │  ├ 3 animation leaves                    │
active_border_file ───▶│ main.rs: apply-border loads     │   │  ├ floating-rule toggle                  │
  (filename only)      │  params → window props; save    │   │  └ dual-mode color input per slot        │
engine.scan (sealed)   │  writes tune state → file       │   │ theme.rs parse_hex + ColorScheme         │
  metadata only        │  engine.apply_* reused unchanged│   │  resolve tokens for DISPLAY only         │
└─────────────────────────────────┘   └──────────────────────────────────────────┘
```

*The engine stays intact. `Config` still stores only `active_border_file`; `engine.scan` still returns metadata only. The new reader bridges the gap: file content -> structured params -> window properties. The engine never sees the params; it keeps applying files by name exactly as today.*

## Component Inventory

**Create**: `src/border_preset.rs` (params struct + pure parse functions + tests); `ui/panel/sections/BorderColorSlot.slint` (dual-mode color input, reused per slot) — Custom mode adapts `NeonColorPicker` from `wilmercampagna/slint-ui-system` (MIT OR Apache-2.0, © 2026 Wilmer Campagna) with an added alpha slider and `SkwdTokens` remap, notice retained; headless render coverage for the new pane (extend `slice_focus_flow_renders` or a sibling test).
**Modify**: `src/preset_store.rs` (full-schema generation; CRUD untouched), `src/main.rs` (apply-border loads params; save-border-preset writes tune state), `src/callbacks.rs` (tune-state helpers if needed), `ui/main.slint`, `ui/shell.slint`, `ui/panel/PanelRoot.slint` (forward every new property/callback), `ui/panel/sections/BordersSection.slint` (tune pane rebuild).
**Untouched**: `src/engine.rs`, `src/config.rs`, `src/settings.rs`, `src/theme_manager.rs`, `src/app_state.rs`, `src/watcher.rs`, `src/utils.rs`, `src/providers/*`, animation presets, `SavedPresetCard.slint` (list pane unchanged).

## Decisions

### D1 — Line-pattern Lua reader, never eval
- **Context**: Preset files are Lua with `hl.config({...})`, `hl.animation({...})`, `hl.curve(...)` calls plus (in one file) `local` aliases. There is no Lua runtime in the dependency tree and adding one for UI parsing is disproportionate.
- **Decision**: `border_preset.rs` parses with line-oriented pattern matching: first collect `local <name> = "<literal>"` bindings, then substitute aliases, then extract `colors = {...}`, `angle = N`, `inactive_border = X`, `border_size = N`, the `decoration.shadow` block fields (`enabled`, `range`, `render_power`, `color`, `color_inactive`, `offset`), `windowrulev2` presence, `hl.animation({leaf, enabled, speed, bezier, style})` triples, and `hl.curve("name", ...)` names. Unknown or malformed forms resolve to documented defaults (D2), never panic, never `unwrap` on file content.
- **Alternatives rejected**: embedding a Lua interpreter (heavy dep for a settings form); extending the engine with a parse API (engine is sealed).
- **Risk**: a future preset using a genuinely new color form parses to the default — fixture tests over all 14 files pin current coverage; the fallback is visible (default + description), not silent corruption.

### D2 — Color value model
- **Decision**: each color is `BorderColor::Token(PaletteToken)` or `BorderColor::Custom(r, g, b, a)`. `PaletteToken` = one of `primary | secondary | tertiary | error | surface | surface_lowest`. Accepted literal forms: bare token; `"rgba(rrggbbaa)"`; `"0xffRRGGBB"` (alpha forced to `ff`, matching `10_golden.lua` usage); `"#rrggbbaa"` / `"#rrggbb"` (alpha defaults `ff`, matching `14_looper.lua`). Locals resolve through the alias table before classification; an unresolvable name falls back to `surface_lowest` (inactive) or `primary` (active slot). Anything unparseable falls back the same way.
- **Display resolution**: tokens resolve through `src/theme.rs` `parse_hex()` over the active `ColorScheme` (same path as `apply_theme`). See assumption A2 for the `error` token.
- **Risk**: `0xffRRGGBB` vs `rgba()` channel-order confusion — both observed forms are RRGGBBAA-ordered hex (`00fff7ff` = cyan, full alpha); tests pin byte order per fixture.

### D3 — Absent blocks have explicit defaults (from data survey)
- **Context**: verified across all 14 files. `windowrulev2` is ABSENT in `01`-`05` and the identical pair in `06`-`14`. `decoration.shadow` exists only in `12`, `13`, `14`. `hl.animation` is a single `borderangle/enabled=false` leaf in `01`-`05` and the full triple (`borderangle`, `border`, `fadeShadow`) plus file-local curves in `06`-`14`. `13_the_joker.lua` has NO `border_size` key and is the only file using locals.
- **Decision**: absent glow = `glow: None` (pane shows the addable empty state, toggle off). Absent `windowrulev2` = rule toggle off. Missing animation leaves = that leaf defaults to `enabled: false` (matching the `01`-`05` single-leaf shape). Missing `border_size` = keep the current tune value (do not reset the slider). Curve names are stored per-preset (file-local); the bezier dropdown lists the preset's own curves plus `default`.
- **Alternative rejected**: hiding controls for absent blocks (the frozen requirement says glow is ADDABLE and the rule is always a toggle).

### D4 — Everything applies live via a hidden draft preset (USER-APPROVED)
- **Context**: HVE's product vision states "everything instant" (`references/vision.md`: "Real-time apply ('everything instant' rule)"). The sealed engine applies borders by FILENAME (`apply_border`) and geometry by value (`apply_geometry`); there is no by-value path for colors/glow/animations.
- **Decision**: ALL edits apply live. Every tune change writes ONE hidden DRAFT preset file and applies it through the existing `apply_border` path, so color/glow/animation changes are visible immediately.
  - The draft lives OUTSIDE the user preset directory so it can never appear in the preset list (e.g. `~/.config/hve/draft/border.lua`).
  - **Debounce**: never write on every drag step. Use the picker's `committed` callback (fires on pointer-up) plus a short debounce; reuse the existing 80ms geometry debounce pattern.
  - On Save, the draft content becomes the named user preset.
  - Cleanup: the draft is removed when the user leaves the section or deselects the preset.
  - The pane labels live-vs-saved state so the user is never misled: working state applies live but is not persisted until Save.
- **Spike required (Unit 2)**: confirm how `apply_border` resolves the file it applies — a name looked up within the scanned directories, or an arbitrary path. If it accepts only names inside the scanned dirs, the fallback is a dot-prefixed file inside the user preset dir that `PresetStore::list()` filters out, plus a cleanup rule.
- **Alternative rejected**: staged edits — contradicts the product vision's "everything instant" rule.
- **Alternative rejected**: draft inside the user preset dir under a normal name — it would show up in the list and pollute rename/delete semantics.

### D5 — Full-schema generation replaces the fictional geometry-only emit
- **Decision**: `PresetStore` gains `generate_border_lua_full(name, &BorderParams)` emitting the complete schema: `general.col.active_border` (colors + angle), `general.col.inactive_border`, `general.border_size`, `decoration.shadow` block iff glow present, the fixed `windowrulev2` pair iff the rule toggle is on, file-local `hl.curve` definitions for every non-`default` bezier referenced, and the three `hl.animation` leaves. Custom colors emit as `"rgba(rrggbbaa)"`; tokens emit bare. The old `generate_border_lua` (rounding/gaps) is deleted in the same commit that switches the call site (zero-warning repo rule precedent), with its test migrated to the new fn.
- **Round-trip contract**: `parse(generate(params)) == params` for every representable value; tested headless.

### D6 — Property/callback chain through all 4 layers
- **Context**: verified convention — every panel property/callback is declared and forwarded `MainWindow` -> `ShellRoot` -> `PanelRoot` -> section.
- **Decision**: new surface (names mirror existing `border-*`/`panel-*` prefixes): tune properties `tune-active-colors: [string]` (canonical serialized form per slot, e.g. `token:primary` or `rgba:rrggbbaa`), `tune-color-count`, `tune-angle`, `tune-inactive`, `tune-size`, `tune-glow-*` (present flag + 6 fields), `tune-anim-*` (3 x enabled/speed/bezier/style), `tune-rule-enabled`, `tune-loaded-file`; callbacks `border-params-loaded` (Rust -> UI bulk set, or individual setters — implementer picks the smaller diff), `tune-changed(...)` per control group, `save-border-preset` reused with tune state. Descriptions ship as `*_desc` string properties so copy stays editable without touching layout. `PanelRoot` keyboard dispatch is recomputed for the new control inventory (focus order: slots -> angle -> inactive -> size -> glow -> animations -> rule -> save form).
- **Alternative rejected**: stuffing params through the existing 4 geometry properties (lossy by construction — the defect being fixed).

### D7 — Dual-mode color input built on an adapted MIT component
- **Context**: building an HSV picker from scratch is significant Slint work. Reuse research found a permissively licensed shared component.
- **Decision**: Custom mode is built by ADAPTING `NeonColorPicker` from `wilmercampagna/slint-ui-system` (`ui/components/color-picker.slint`), license **MIT OR Apache-2.0** (verified by fetching `Cargo.toml` — `license = "MIT OR Apache-2.0"` — and `LICENSE-MIT`, "Copyright (c) 2026 Wilmer Campagna"; the GitHub API reported `NOASSERTION` only because the file is named `LICENSE-MIT` rather than `LICENSE`). It already provides: a 2D Saturation×Value `TouchArea`, a 48-segment hue slider, a large preview swatch, HSV→RGB math via `sector`/`f`/`q`/`p`/`t` properties (Slint has no local `let`), a `nibble()` hex helper, `changed`/`committed` callbacks, and an RGB write-back pull pattern (`apply-rgb` + `set-from-rgb-r/g/b`).
- **Adaptation required BEFORE use** (all inside the new `ui/panel/sections/BorderColorSlot.slint`):
  1. **Add an opacity/alpha slider and an alpha output.** Upstream is RGB-only. HVE presets use alpha heavily (`rgba(ffffff44)`, `rgba(9d00ff88)`, `"#1a002655"`, inactive borders with alpha `55`/`00`), so alpha is mandatory, not optional.
  2. **Remap `Theme.*` to `SkwdTokens`.** Upstream imports its own `Theme` (`Theme.sp-*`, `Theme.radius-*`, `Theme.border`, `Theme.bg-surface`, `Theme.neon-cyan`, `Theme.font-*`, `Theme.text-primary`). Replace every reference with project tokens.
  3. **Preserve the MIT notice.** Retain the "Copyright (c) 2026 Wilmer Campagna" attribution in the component header and credit the source (MIT requires keeping the notice).
- **License-rejected during research**: Slint's own LSP picker (`tools/lsp/ui/components/widgets/floating-color-picker-widget.slint`) is **GPL-3.0-only OR Slint royalty-free** — incompatible with this MIT project (same GPL-contagion rule that forbids copying hyprmod). Do not use.
- `BorderColorSlot.slint` wraps the adapted picker: mode switch (Follow theme | Custom), 6 large token swatches in Follow mode (resolved via forwarded palette props), the adapted picker in Custom mode, plus an optional hex LineEdit (validated through the D2 byte-order tests; invalid input keeps the last valid color and shows the description hint). The tune pane instantiates 2..8 slots from `tune-color-count`; add/remove buttons clamp to 2..8 with descriptions explaining the Hyprland gradient range.
- **Alternative rejected**: circular HSV wheel (out of scope); per-slot hex as primary input (frozen: no hex required); copying the GPL Slint picker (license-blocked).

### D8 — Visual verification is mandatory
- **Decision**: Unit 3 gates on `cargo test slice_focus_flow_renders` (extended to mount the rebuilt pane, or a sibling render test if the harness cannot mount panels) with PNGs read under `/tmp/opencode/` and geometry confirmed by inspection: slot count matches fixture (e.g. 8 slots for Infinity, 2 for Duo), glow group visible/addable states, descriptions rendered. "Tests green + build clean" alone does not close Unit 3.

## Data Flow

```
card click (idx, file) ─▶ main.rs: read file text (assets dir or user dir)
  ─▶ border_preset::parse_file() -> BorderParams
  ─▶ set tune-* properties (bulk) + set_active_border_index + engine.apply_border(file) [unchanged]
  ─▶ tune pane renders: slots/angle/inactive/size/glow/animations/rule + descriptions
edit ─▶ tune-changed ─▶ update tune props (immediate)
  ─▶ generate_border_lua_full(draft, params_from_props) ─▶ write hidden draft file
  ─▶ apply draft via apply_border (debounced: picker `committed` + 80ms; NEVER per drag step)
  ─▶ user sees the change live; state is not persisted yet (D4)
save(name) ─▶ generate_border_lua_full(name, params_from_props) ─▶ store.save
  ─▶ refresh user list props + clear error (existing refresh pattern in main.rs)
  ─▶ remove the hidden draft
leave section / deselect ─▶ remove the hidden draft
user card click ─▶ same load path (user dir file) — round-trip closed
```

## Testing Strategy

| Layer | What | How |
|---|---|---|
| Unit (reader) | all 14 fixtures: slot count, token vs custom classification, byte order, angle, inactive, size, glow present/absent, rule present/absent, leaves, curves, joker locals + missing size | `cargo test border_preset` RED-first via `unimplemented!()` stubs |
| Unit (generation) | full-schema emit contains every block; toggle-off omits glow/rules; round-trip `parse(generate(p)) == p` | `cargo test preset_store` |
| Integration | click `07_infinity.lua` pushes 8 slots + angle 45 + rule on; click `01_cascade.lua` pushes 2 slots + glow absent + single leaf; save writes file that re-parses to tune state | headless `init_no_event_loop` pattern (PR1.1 precedent) |
| Visual | rebuilt pane geometry, slot counts, glow states, descriptions | `slice_focus_flow_renders` + read PNGs |

## Threat Matrix

No new subprocess TYPE: the live-draft path reuses the engine's existing `assemble.sh` script through `Engine::run_script`, the same script `border.sh` already ran on every border apply, writing and removing the same `assets/fragments/border.lua` fragment the border flow already owned. `preset_store.rs` stays pure file I/O (no `Command`/`spawn`; verified by inspection). No file writes outside the paths the border flow already used (`assets/fragments/` is gitignored), no network, no IPC change. The reader reads the same `.lua` files the engine already executes; it never executes them. Save path reuses `PresetStore::save` (existing user-consent flow with overwrite guard via rename-exists error). No new threat rows.

## Migration / Rollout

No data migration. Existing user presets (geometry-only files from the old generator) still list/apply/delete; when loaded, their absent blocks resolve via D3 defaults and re-saving upgrades them to the full schema. Old `generate_border_lua` removed with its call site in one commit.

## Open Questions

- [x] D4 staged-vs-live semantics — RESOLVED (user-approved): everything applies live via a hidden draft preset file. See D4.
- [x] A2 `error` token display fallback — RESOLVED: display-only fallback chain (`accent` value, else a fixed alert red) and the swatch always renders the token name; saved files always re-emit the bare `error` token. See A2.

## Assumptions

- **A1**: `border_size` range is 1..5 (data shows 1..2; frozen decision allows 1..5). Reader accepts any positive int; the slider clamps to 1..5.
- **A2 (RESOLVED)**: `error` is a valid palette token in preset files (used by `05`, `07`, `09`) but `engine::ColorScheme` has no `error` field (it has `accent`), and the engine is sealed. The shell side already extracts `HVE_ERROR` in `assets/scripts/colors.sh`, but the JSON the engine consumes (`get_colors.sh`) does not emit it, so the Rust side cannot read it without changing sealed code. DISPLAY therefore uses a documented fallback chain: the scheme's `accent` value, else a fixed alert red. The swatch always renders the token NAME (`error`) beside the preview so the user is never misled about which token is set, and saved files always re-emit the bare `error` token, so the fallback never leaks into a preset file. If the engine is ever unsealed, the real value can be read and the fallback removed.
- **A3**: `angle` is restricted to 30/45/90 in the UI (segmented control); the reader accepts any int but the tune control snaps to the nearest of the three (all observed values).
- **A4**: offset is always `{0, 0}` in the data; the glow group exposes no offset control (fixed pair emitted). If a future preset carries a nonzero offset, the reader preserves it in state but the UI shows it read-only with a description.
