# Border Tune Pane Specification

## Purpose

Defines the borders tune pane: loading every parameter of the selected border preset, full-schema saving, per-control descriptions, dynamic colour slots, the dual-mode colour input, the glow group (its enable switch and its folding body), one delimited container per block, and the label-sourcing contract that keeps every pane string translated.

## Requirements

When the user clicks a border preset card, the tune pane MUST load ALL of that preset's parameters: every active-border color (2..8 slots), gradient angle, inactive border color, border size, glow state, the three border animation leaves, and the floating-window rule state. Switching cards MUST replace the full tune state (no stale slots or glow blocks from the previous preset). A preset without `border_size` (e.g. `13_the_joker.lua`) MUST keep the current size value instead of resetting it.

#### Scenario: Full load on card click

- GIVEN the tune pane showing preset A with 3 colors and glow on
- WHEN the user clicks preset B (2 colors, no glow)
- THEN the pane shows exactly 2 color slots, angle/inactive/size of B, and the glow switch OFF with its empty-state line shown, and B's leaves and rule state load into the tune state

#### Scenario: Missing size keeps current value

- GIVEN size slider at 3
- WHEN the user clicks `13_the_joker.lua` (no `border_size` key)
- THEN the size slider stays at 3 and all other params load from the file

Save MUST write a user preset containing the complete tuned schema: active colors + angle, inactive color, border size, glow block (only when present), the fixed `windowrulev2` pair (only when the rule state is on), file-local curve definitions for every referenced non-`default` bezier, and the three animation leaves. Re-selecting the saved preset MUST reproduce the tuned parameters (round-trip). Tokens MUST re-emit bare (`primary`); custom colors MUST emit `"rgba(rrggbbaa)"`.

#### Scenario: Save round-trips

- GIVEN a tuned state (3 custom colors, angle 45, glow on, rule on)
- WHEN the user saves as "My Glow" and clicks its card
- THEN the tune pane shows the identical 3 colors, angle, and glow fields, and the rule state survives the round-trip

#### Scenario: Toggles omit blocks

- GIVEN glow absent and rule state off
- WHEN the user saves
- THEN the file contains no `decoration.shadow` block and no `windowrulev2` block

EVERY tune control MUST carry a clear human-readable description rendered next to it: each color slot, angle, inactive color, size, each glow field, slot add/remove, and the live-vs-saved status. Descriptions MUST state what the control does in plain language (no bare parameter names as the only label).

#### Scenario: Descriptions present

- GIVEN the rebuilt tune pane
- WHEN inspected control by control
- THEN each of slots, angle, inactive, size, glow fields, add/remove, and status shows its description

Every block of the pane MUST live in its OWN container carrying the block's title and one description line saying what the block controls. Each container MUST paint its own surface (the pane's `bg-card` fill, a 1px `border` hairline, `radius-8` corners and 12px padding) so a reader can tell where a block ends and the next begins. A container MUST NOT add, remove or reorder a keyboard stop, and a container whose block is absent MUST paint nothing.

#### Scenario: Every block is delimited

- GIVEN the tune pane is open
- WHEN it is inspected block by block
- THEN Geometry, Gradient angle, Inactive colour, Active colours, Slot add/remove, Glow and Save each render inside their own surface with a title and a description

Every user-visible string in the tune pane MUST be read from the `BordersText` global, never hardcoded in the section. Each label MUST be registered in the text global, the `en` and `es` message maps, the panel i18n applier and the label table that pins the pane's strings, so the pane renders in English and neutral Spanish without a hardcoded fallback.

#### Scenario: Labels come from the text global

- GIVEN the tune pane with a language applied
- WHEN the section is inspected for a hardcoded label
- THEN every label is read from `BordersText` and switching the language repaints the pane

The pane MUST render one color slot per preset color (2..8). Add/remove controls MUST clamp to 2..8 and explain the range. Slot order MUST match file order.

#### Scenario: Slot counts match fixtures

- GIVEN `07_infinity.lua` (8 colors) and `03_duo.lua` (2 colors)
- WHEN each is clicked
- THEN the pane shows 8 slots and 2 slots respectively, in file order

Each slot MUST offer **Follow theme** (6 large palette-token swatches: primary, secondary, tertiary, error, surface, surface_lowest) and **Custom color** (linear hue slider + saturation/brightness 2D area + opacity slider + large live preview). No hex input may be required; an optional hex field is a power-user shortcut only, and invalid hex MUST keep the last valid color with a hint.

#### Scenario: Both modes edit the slot

- GIVEN a slot in Follow mode
- WHEN the user picks the tertiary swatch, then switches to Custom and drags hue
- THEN the slot value updates in both modes and the preview tracks live

The glow block MUST always be present, headed by its title and description. Its enable control MUST be a switch — the same pill switch the preset cards use, with its animated background and travelling knob — mounted in BOTH states so the row never jumps. Turning the switch ON MUST create the default glow state; turning it OFF MUST return to the empty state.

#### Scenario: Addable glow

- GIVEN `01_cascade.lua` loaded (no glow, the switch OFF and the empty-state line shown)
- WHEN the user turns the glow switch ON, sets range, and saves
- THEN the body folds open and the saved file contains a `decoration.shadow` block with that range

The glow body MUST be always mounted inside a wrapper whose height animates from 0 to the body's own content height, so enabling the glow unfolds the body (250ms `ease-in-out-back` in, 150ms `ease-in` out) instead of appearing at full size. With the glow on the body MUST carry range, render_power, color and color_inactive. The OFF-state line MUST fold the same way in BOTH directions. The fold MUST NOT change the keyboard stop count: a collapsed body has no reachable stops while the glow is off.

#### Scenario: Glow body folds open

- GIVEN the glow switch OFF
- WHEN the user turns it ON
- THEN the body unfolds to its content height instead of appearing at full size, and the OFF-state line folds closed

The pane MUST NOT expose an editing surface for the three animation leaves (`borderangle`, `border`, `fadeShadow`). Their enabled, speed, bezier and style values MUST still round-trip through the tune state and the saved preset unchanged. Curve names are file-local: any bezier list the tune state carries MUST come from the loaded preset, never a global list.

#### Scenario: Leaves round-trip without a surface

- GIVEN `12_neon_cyberpunk.lua` loaded
- WHEN the tune pane renders
- THEN it shows no animation-leaf controls, and saving the preset keeps that file's own leaves

The pane MUST NOT expose a floating-window rule control. The rule MUST remain a single boolean in the tune state that emits the fixed pair (`"noshadow, focus:0"` + `"dim_around, floating:1"`) when on and nothing when off. Free-text rule editing is FORBIDDEN.

#### Scenario: Rule round-trips without a surface

- GIVEN a preset without `windowrulev2` (`01`-`05` shape)
- WHEN its rule state is on and the preset is saved
- THEN the file contains exactly the fixed pair and nothing else

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

Carried from current behavior unchanged.

The list pane (built-in cards + user cards with rename/delete, active index, CUSTOM tags) MUST behave exactly as today. Rename/delete paths are untouched.

#### Scenario: List untouched (preserved)

- GIVEN user presets exist
- WHEN the panel opens
- THEN built-in and user cards, badges, and active highlight render as today

`engine.apply_border` (by filename) and the debounced `apply_geometry` live path MUST keep their current contracts. The engine and `Config` shapes are unchanged; the file-based apply path is reused for the live draft (see Live Apply via Hidden Draft). Geometry keeps its by-value debounced path.

#### Scenario: Apply paths (preserved)

- GIVEN a card clicked
- WHEN the engine applies it
- THEN the desktop border updates via the existing file-based path within the current latency
