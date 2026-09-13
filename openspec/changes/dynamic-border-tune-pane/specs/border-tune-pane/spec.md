# Delta for border-tune-pane

New capability — no archived border-tune spec exists. MODIFIED supersedes the current `BordersSection` tune behavior (static 4-slider form + geometry-only save). PRESERVED restates still-valid contracts (list pane, CRUD, engine apply paths).

## MODIFIED Requirements

### Requirement: Tune Pane Loads the Selected Preset
(Previously: static `border_size`/`rounding`/`gaps_in`/`gaps_out` sliders that never reflected any preset file.)

When the user clicks a border preset card, the tune pane MUST load ALL of that preset's parameters: every active-border color (2..8 slots), gradient angle, inactive border color, border size, glow state, the three border animation leaves, and the floating-window rule state. Switching cards MUST replace the full tune state (no stale slots or glow blocks from the previous preset). A preset without `border_size` (e.g. `13_the_joker.lua`) MUST keep the current size value instead of resetting it.

#### Scenario: Full load on card click

- GIVEN the tune pane showing preset A with 3 colors and glow on
- WHEN the user clicks preset B (2 colors, no glow)
- THEN the pane shows exactly 2 color slots, angle/inactive/size of B, glow in the addable empty state, B's leaves and rule state

#### Scenario: Missing size keeps current value

- GIVEN size slider at 3
- WHEN the user clicks `13_the_joker.lua` (no `border_size` key)
- THEN the size slider stays at 3 and all other params load from the file

### Requirement: Full-Schema Save
(Previously: Save captured `border_size` + `rounding`/`gaps_in`/`gaps_out`, parameters present in no preset.)

Save MUST write a user preset containing the complete tuned schema: active colors + angle, inactive color, border size, glow block (only when present), the fixed `windowrulev2` pair (only when the rule toggle is on), file-local curve definitions for every referenced non-`default` bezier, and the three animation leaves. Re-selecting the saved preset MUST reproduce the tuned parameters (round-trip). Tokens MUST re-emit bare (`primary`); custom colors MUST emit `"rgba(rrggbbaa)"`.

#### Scenario: Save round-trips

- GIVEN a tuned state (3 custom colors, angle 45, glow on, rule on)
- WHEN the user saves as "My Glow" and clicks its card
- THEN the tune pane shows the identical 3 colors, angle, glow fields, and rule state

#### Scenario: Toggles omit blocks

- GIVEN glow absent and rule toggle off
- WHEN the user saves
- THEN the file contains no `decoration.shadow` block and no `windowrulev2` block

## ADDED Requirements

### Requirement: Per-Control Descriptions

EVERY tune control MUST carry a clear human-readable description rendered next to it: each color slot, angle, inactive color, size, each glow field, each animation leaf field, the rule toggle, slot add/remove, and the live-vs-saved status. Descriptions MUST state what the control does in plain language (no bare parameter names as the only label).

#### Scenario: Descriptions present

- GIVEN the rebuilt tune pane
- WHEN inspected control by control
- THEN each of slots, angle, inactive, size, glow fields, leaf fields, rule toggle, add/remove, and status shows its description

### Requirement: Dynamic Color Slots

The pane MUST render one color slot per preset color (2..8). Add/remove controls MUST clamp to 2..8 and explain the range. Slot order MUST match file order.

#### Scenario: Slot counts match fixtures

- GIVEN `07_infinity.lua` (8 colors) and `03_duo.lua` (2 colors)
- WHEN each is clicked
- THEN the pane shows 8 slots and 2 slots respectively, in file order

### Requirement: Dual-Mode Color Input

Each slot MUST offer **Follow theme** (6 large palette-token swatches: primary, secondary, tertiary, error, surface, surface_lowest) and **Custom color** (linear hue slider + saturation/brightness 2D area + opacity slider + large live preview). No hex input may be required; an optional hex field is a power-user shortcut only, and invalid hex MUST keep the last valid color with a hint.

#### Scenario: Both modes edit the slot

- GIVEN a slot in Follow mode
- WHEN the user picks the tertiary swatch, then switches to Custom and drags hue
- THEN the slot value updates in both modes and the preview tracks live

### Requirement: Glow Group Present-or-Addable

Presets with `decoration.shadow` MUST show the full glow field set (enabled, range, render_power, color, color_inactive). Presets without it MUST show an addable empty state; adding creates default glow state, removing returns to the empty state.

#### Scenario: Addable glow

- GIVEN `01_cascade.lua` loaded (no glow)
- WHEN the user adds glow, sets range, and saves
- THEN the saved file contains a `decoration.shadow` block with that range

### Requirement: Border Animation Leaves

The pane MUST expose the three leaves (`borderangle`, `border`, `fadeShadow`) with enabled, speed, bezier (the preset's own curves plus `default`), and style. Curve names are file-local: the bezier list MUST come from the loaded preset, never a global list.

#### Scenario: File-local curves

- GIVEN `12_neon_cyberpunk.lua` loaded
- WHEN the borderangle bezier list is inspected
- THEN it offers `nv_neon_flow` and `default`, not curves from other files

### Requirement: Floating-Window Rule Toggle

The rule MUST be a single boolean toggle emitting the fixed pair (`"noshadow, focus:0"` + `"dim_around, floating:1"`) when on and nothing when off. Free-text rule editing is FORBIDDEN.

#### Scenario: Rule toggle

- GIVEN a preset without `windowrulev2` (`01`-`05` shape)
- WHEN the toggle is on and the preset is saved
- THEN the file contains exactly the fixed pair and nothing else

### Requirement: Live Apply via Hidden Draft

Every tune edit MUST apply live, honoring the product vision rule "everything instant". Edits MUST be written to a single hidden draft preset and applied through the existing file-based `apply_border` path. The draft MUST be invisible to the preset list at all times, MUST be written debounced (never on every drag step), and MUST be removed when the user saves, leaves the section, or deselects the preset. The pane MUST label working state as live but not persisted until Save.

#### Scenario: Edit is visible before saving

- GIVEN a loaded preset with the tune pane open
- WHEN the user changes a border color
- THEN the desktop border updates without saving, and the preset list shows no extra entry

#### Scenario: Draft is transient

- GIVEN the user edited a border and left the draft unwritten as a named preset
- WHEN the user leaves the section or deselects the preset
- THEN the hidden draft file is gone and the preset list is unchanged

#### Scenario: Save promotes the draft

- GIVEN a live-edited tune state
- WHEN the user saves it under a name
- THEN the named preset contains the tuned parameters and the hidden draft is removed


The reader MUST correctly parse all 14 shipped presets: token/custom classification, `rgba()`/`0xff…`/`#…` byte order, joker locals, angles 30/45/90, glow present (`12`, `13`, `14`) vs absent, rule present (`06`-`14`) vs absent (`01`-`05`), single-leaf (`01`-`05`) vs triple-leaf animations, and file-local curves. Unparseable input MUST resolve to documented defaults, never panic.

#### Scenario: Full fixture coverage

- GIVEN the 14 files under `assets/borders/`
- WHEN the reader test suite runs
- THEN every file parses to its documented params (pinned by test)

## PRESERVED Requirements

Carried from current behavior unchanged.

### Requirement: Preset List and CRUD

The list pane (built-in cards + user cards with rename/delete, active index, CUSTOM tags) MUST behave exactly as today. Rename/delete paths are untouched.

#### Scenario: List untouched (preserved)

- GIVEN user presets exist
- WHEN the panel opens
- THEN built-in and user cards, badges, and active highlight render as today

### Requirement: Engine Apply Paths

`engine.apply_border` (by filename) and the debounced `apply_geometry` live path MUST keep their current contracts. The engine and `Config` shapes are unchanged; the file-based apply path is reused for the live draft (see Live Apply via Hidden Draft). Geometry keeps its by-value debounced path.

#### Scenario: Apply paths (preserved)

- GIVEN a card clicked
- WHEN the engine applies it
- THEN the desktop border updates via the existing file-based path within the current latency
