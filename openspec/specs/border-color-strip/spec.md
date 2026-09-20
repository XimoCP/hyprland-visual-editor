# Border Color Strip Specification

## Purpose

Replace the N × `BorderColorSlot` vertical card layout with a single horizontal strip of numbered colour chips, an in-flow picker card, and compact inactive/glow/inactive-glow rows. This is the visual replacement for the dynamic gradient colour section of the Borders tune pane.

---

## Requirements

### R1 — Horizontal Chip Strip

The Borders tune pane MUST render a single `HorizontalLayout` containing one chip per gradient colour slot (2..8). Each chip MUST display:

1. A 16×16 colour rectangle filled with the resolved colour (token or custom).
2. A numeric label (1-indexed, matching the slot position: "1", "2", … "8").

The strip MUST fit within the available tune-pane width with 8 chips without horizontal overflow or truncation. Chips outside the current gradient colour count (e.g. chip 3 when `slot-count` is 2) MUST NOT render.

#### Scenario: Strip renders 2–8 chips

- GIVEN a preset with `slot-count = 3`
- WHEN the tune pane renders the border colours section
- THEN a horizontal row shows exactly 3 chips labelled "1", "2", "3"
- AND each chip's rectangle is filled with the resolved colour of the corresponding slot

#### Scenario: Strip scales to 8 slots

- GIVEN a preset with `slot-count = 8` (e.g. `07_infinity.lua`)
- WHEN the tune pane renders the border colours section
- THEN the strip shows exactly 8 chips, all visible and non-overlapping within the pane width

#### Scenario: Strip shows no chips when slot-count is 0

- GIVEN `slot-count = 0`
- WHEN the tune pane renders
- THEN no chip strip is rendered

---

### R2 — Chip Identity and Hover/Focus

Each chip MUST be visually distinguishable by its resolved colour and numeric label. The active chip (when the strip has keyboard focus) MUST show an icy-cyan (#8fd8ff) border highlight. On mouse hover, chips MUST show a subtle background highlight (consistent with `HveColors.bg-hover`).

#### Scenario: Active chip highlight

- GIVEN the strip has keyboard focus and the active chip index is 1
- WHEN the tune pane renders
- THEN chip "2" (index 1) displays an icy-cyan border highlight
- AND other chips show the default border

#### Scenario: Hover highlight

- GIVEN the user hovers over chip "3"
- WHEN the tune pane renders
- THEN chip "3" shows a subtle background highlight

---

### R3 — In-Flow Picker Card (Open)

When a chip is clicked (mouse) or selected via Enter (keyboard), a picker card MUST open **above** the strip, in the normal document flow. The picker card MUST contain:

1. A header row with the channel name (e.g. "gradient colour 2 of 5") and a "Done" button.
2. Six token swatch rectangles (primary, secondary, tertiary, error, surface, surface_lowest) in a compact horizontal row, each showing the resolved palette colour and a label.
3. The `HveColorPicker` HSV custom colour area (hue slider + saturation/brightness 2D + opacity) below the tokens.

The picker MUST be the ONLY picker open at any time. Clicking a different chip while a picker is already open MUST close the current picker and open the picker for the clicked chip (transition `editing-slot` to the new index). The picker MUST be seeded with the resolved colour of the target slot.

#### Scenario: Click chip opens picker

- GIVEN the picker is closed (`editing-slot = -1`) and 3 gradient chips are visible
- WHEN the user clicks chip "2"
- THEN the picker card opens above the strip
- AND the header reads "Custom colour — gradient colour 2 of 3"
- AND the 6 token swatches are visible
- AND the HSV area shows the resolved colour of slot 1 (index 1)

#### Scenario: Only one picker open at a time

- GIVEN the picker is open for slot 0 (chip "1")
- WHEN the user clicks chip "3"
- THEN the picker transitions to editing slot 2 (chip "3")
- AND the header updates to "Custom colour — gradient colour 3 of N"
- AND the HSV area re-seeds with slot 2's resolved colour

#### Scenario: Picker closes via Done button

- GIVEN the picker is open
- WHEN the user clicks "Done"
- THEN the picker closes (`editing-slot = -1`)
- AND the strip remains visible

---

### R4 — In-Flow Picker Card (Close)

The picker MUST close when:

1. The user clicks the "Done" button.
2. The user presses `Escape`.
3. The user clicks outside the picker card and the strip (if applicable to the focus model).

When the picker closes, the viewport MUST scroll so the strip remains visible.

#### Scenario: Escape closes picker

- GIVEN the picker is open for slot 2
- WHEN the user presses `Escape`
- THEN the picker closes and `editing-slot` returns to -1
- AND the strip is visible in the viewport

#### Scenario: Picker state preserved on close

- GIVEN the user changed slot 2 to "tertiary" via a token swatch while the picker was open
- WHEN the picker closes
- THEN the chip "3" rectangle reflects the new "tertiary" resolved colour

---

### R5 — Picker Entry Animation

The picker card is always mounted inside the `picker-block` VerticalLayout: it collapses and expands by animating its own `height`. Because it is an in-flow layout child, its `y` is owned by the layout and MUST NOT be animated; the expanding height is what moves the strip below, smoothly instead of jumping.

When `editing-slot` transitions from -1 to ≥ 0 (picker opens), the picker card MUST animate in with:

- `height`: 0px → 380px, duration 250ms, easing `ease-in-out-back` (unfold)
- `opacity`: 0 → 1, duration 250ms, easing `ease-in-out-back`
- `transform-scale-x`: 0.95 → 1.0, duration 250ms, easing `ease-in-out-back`
- `transform-scale-y`: 0.95 → 1.0, duration 250ms, easing `ease-in-out-back`

When `editing-slot` transitions from ≥ 0 to -1 (picker closes), the picker card MUST animate out with:

- `height`: 380px → 0px, duration 150ms, easing `ease-in`
- `opacity`: 1 → 0, duration 150ms, easing `ease-in`

**Constraints:**

- The animation MUST NOT animate `viewport-y` of any ScrollView.
- The picker card MUST remain fully visible when the user scrolls while it is open.
- The animation MUST NOT cause layout thrash or overflow the render test bounds.
- `transform-scale-x/y` is a GPU-renderer effect: the headless software renderer used by the render tests does not implement transforms (Slint documents no rotations, no scaling, no `drop-shadow-*`, and no `border-radius` with `clip: true`), so no render assertion depends on scale. Accepted 2026-09-20 (change design Open Question #1): the scale stays as GPU-only polish. The declaration is guarded by the construction test `picker_scale_animation_stays_declared_by_construction` in `src/shell/ui_tests.rs`, which fails if the declaration or either endpoint value (`0.95` / `1.0`) is removed or changed; the rendered pixels of the scale remain verified by eye on the GPU desktop.

#### Scenario: Entry animation visible

- GIVEN the picker is closed
- WHEN the user clicks a chip
- THEN the picker card fades in from opacity 0 to 1 over ~250ms
- AND the card unfolds from height 0px to 380px
- AND the card scales from 0.95 to 1.0 on both axes

#### Scenario: Exit animation visible

- GIVEN the picker is open
- WHEN the user clicks "Done"
- THEN the picker card fades out from opacity 1 to 0 over ~150ms
- AND the card collapses from height 380px to 0px

#### Scenario: Animation does not affect viewport-y

- GIVEN the picker is open and the ScrollView has a non-zero `viewport-y`
- WHEN the picker closes (exit animation)
- THEN `viewport-y` of the ScrollView remains unchanged during the animation

---

### R6 — Compact Inactive Colour Row

The inactive border colour row MUST be a compact single-row element (chip + label + "Custom…" button), NOT a full-height `BorderColorSlot` card. It MUST show:

1. A 16×16 colour chip with the resolved inactive colour.
2. A text label (e.g. "Inactive").
3. A "Custom…" button that opens the shared picker for editing-slot index 8.

The row height MUST be approximately 40–56px (consistent with other compact tune controls), not the 110px of the old `BorderColorSlot`.

#### Scenario: Inactive row renders compact

- GIVEN the tune pane is open
- WHEN the border colours section is inspected
- THEN the inactive colour row is a compact row (~40–56px tall) with chip, label, and "Custom…" button
- AND clicking "Custom…" opens the picker for editing-slot 8

---

### R7 — Compact Glow Colour Rows

When glow is enabled, the glow colour and inactive glow colour rows MUST each be compact single-row elements (chip + label + "Custom…" button), matching the inactive row pattern. Each MUST open the shared picker for its respective editing-slot index (9 for glow colour, 10 for inactive glow colour).

#### Scenario: Glow colour row compact

- GIVEN glow is enabled
- WHEN the glow section is inspected
- THEN the glow colour row is a compact row with chip, label, and "Custom…" button
- AND clicking "Custom…" opens the picker for editing-slot 9

#### Scenario: Inactive glow row compact

- GIVEN glow is enabled
- WHEN the glow section is inspected
- THEN the inactive glow colour row is a compact row with chip, label, and "Custom…" button
- AND clicking "Custom…" opens the picker for editing-slot 10

---

### R8 — Keyboard: Strip as Single Stop

The tune pane keyboard navigation MUST treat the entire chip strip as a single focus stop (index 3 in the tune-local index space). The stop count formula for `borders_tune_stop_count` MUST change from:

```
3 + n + 2 + 1  (where n = slot_count, i.e. N stops for N slots)
```

to:

```
3 + 1 + 2 + 1  (where 1 = the strip stop, regardless of slot count)
```

Concretely: `size (0) + angle (1) + inactive (2) + strip (3) + add (4) + remove (5) + glow... + anim... + rule + save-name + save-button`.

The `slot_count` parameter MUST still be accepted for backward compatibility but MUST be ignored in the count formula (the strip always contributes exactly 1 stop).

#### Scenario: Stop count with 2 slots no glow

- GIVEN `slot_count = 2`, glow disabled, no animations
- WHEN `borders_tune_stop_count(2, false, false, false, false)` is called
- THEN the return value is 13 (3 + 1 + 2 + 1 + 1 [rule] + 1 [save-name] + 1 [save-button] = … verify exact count)
- AND the strip occupies exactly 1 stop in the sequence

#### Scenario: Stop count with 8 slots glow + all animations

- GIVEN `slot_count = 8`, glow enabled, all 3 animations enabled
- WHEN `borders_tune_stop_count(8, true, true, true, true)` is called
- THEN the return value reflects the strip as 1 stop (not 8)
- AND the total is significantly lower than the old formula's 33

#### Scenario: Stop count with 0 slots

- GIVEN `slot_count = 0`
- WHEN `borders_tune_stop_count(0, false, false, false, false)` is called
- THEN the strip still contributes 1 stop (the strip stop is always present)

---

### R9 — Keyboard: Sub-Navigation Within Strip

When the keyboard focus is on the strip stop (index 3), LEFT and RIGHT arrow keys MUST navigate the active chip within the strip. This is internal sub-navigation, not a PanelRoot concern — PanelRoot only sees stop index 3.

**Dispatch note (2026-09-20, change design Open Question #2).** `PanelRoot` owns the keyboard map, so it is `PanelRoot` that recognises "tune-local stop 3, with colours present" and calls `strip-cycle`; the chip cycling itself stays inside the section. A `strip-engaged` boolean — letting the section decide and report whether the strip stop holds the keyboard — was considered and rejected: it would move a navigation decision into the layer that `PanelRoot.slint` declares presentational ("sections ... own no FocusScope and no key handling"). The coupling between `PanelRoot`'s literal `3` and this section's stop order is guarded end to end by `borders_strip_keyboard_sub_navigation` in `src/shell/ui_tests.rs`, which drives real key events through the production `FocusScope`. The stop number still has no named constant while the section names its other stops; that is a known minor legibility item, not a correctness gap.

- LEFT arrow: move active chip index left (wraps from 0 to `slot-count - 1`).
- RIGHT arrow: move active chip index right (wraps from `slot-count - 1` to 0).
- `Enter`: open the picker for the currently active chip (set `editing-slot` to the active chip index).
- `Escape`: close the picker if open (set `editing-slot` to -1).

#### Scenario: Arrow navigation within strip

- GIVEN focus is on the strip stop with 3 chips, active chip index is 0
- WHEN the user presses RIGHT
- THEN the active chip index becomes 1
- AND chip "2" receives the icy-cyan highlight

#### Scenario: Arrow wraps within strip

- GIVEN focus is on the strip stop with 3 chips, active chip index is 2
- WHEN the user presses RIGHT
- THEN the active chip index wraps to 0
- AND chip "1" receives the icy-cyan highlight

#### Scenario: Enter opens picker

- GIVEN focus is on the strip stop, active chip index is 1
- WHEN the user presses Enter
- THEN the picker opens for editing-slot 1
- AND the picker header shows "gradient colour 2 of N"

#### Scenario: Escape closes picker from strip

- GIVEN the picker is open (editing-slot = 2)
- WHEN the user presses Escape
- THEN the picker closes (editing-slot = -1)
- AND focus returns to the strip stop

---

### R10 — Scroll-to-Strip When Picker Opens

When the picker opens (editing-slot transitions from -1 to ≥ 0), the ScrollView MUST scroll to position the strip at the top of the visible area, ensuring the picker card above the strip is fully visible. This MUST use `follow-focus()` or equivalent scroll positioning. The viewport MUST NOT animate during scroll positioning.

#### Scenario: Picker opens at top of scroll

- GIVEN the tune pane is scrolled down past the strip
- WHEN the user clicks a chip to open the picker
- THEN the viewport scrolls to show the picker card and strip at the top
- AND the picker card is fully visible (no clipping)

---

### R11 — Shared Picker State via editing-slot

The existing `editing-slot` index space (0..7 = slots, 8 = inactive, 9 = glow, 10 = glow-inactive, -1 = closed) MUST be preserved exactly. The picker card in BordersTunePane MUST check `editing-slot >= 0` to determine visibility. The compact rows and the strip MUST all write to the same `editing-slot` index via `request-edit-slot()`.

#### Scenario: editing-slot index space preserved

- GIVEN the picker is open for the inactive colour (editing-slot = 8)
- WHEN the user clicks chip "1"
- THEN editing-slot transitions to 0
- AND the picker re-seeds with slot 0's resolved colour
- AND the picker header updates to "gradient colour 1 of N"

#### Scenario: editing-slot -1 means closed

- GIVEN editing-slot = -1
- WHEN the tune pane renders
- THEN no picker card is visible

---

### R12 — BorderColorSlot Deprecation

The `BorderColorSlot.slint` component MUST be removed from the project. Its import in `BordersSection.slint` MUST be removed. The token swatch row and chip + label logic from `BorderColorSlot` MUST be absorbed into:

1. The chip strip (for gradient colour slots).
2. The compact inactive/glow/inactive-glow rows (for channels 8, 9, 10).

No other file imports `BorderColorSlot` — the removal MUST NOT break any file outside `BordersSection.slint`.

#### Scenario: No dangling imports

- GIVEN `BorderColorSlot.slint` is deleted
- WHEN `cargo check` is run
- THEN the project compiles without errors related to `BorderColorSlot`

#### Scenario: No other file references BorderColorSlot

- GIVEN the codebase is grepped for `BorderColorSlot`
- THEN only `BordersSection.slint` (now removed import) and the deleted file itself match

---

### R13 — Visual Render Verification

After implementation, a headless render test (e.g. `slice_focus_flow_renders` or a sibling test) MUST produce PNG screenshots of:

1. The strip with all gradient chips visible (closed picker state).
2. The strip with the picker open for one of the chips.
3. The strip with 8 chips to verify overflow/fit.

These PNGs MUST be saved under `/tmp/opencode/` and visually inspected by the agent. The render test MUST confirm:

- Chips are evenly spaced and non-overlapping at 8 slots.
- The picker card is fully visible above the strip.
- Compact inactive/glow rows are the correct height (~40–56px).
- The picker entry animation frame (opacity, position) is correct.

#### Scenario: Render test produces PNGs

- GIVEN the implementation is complete
- WHEN `cargo test slice_focus_flow_renders` (or sibling) is run
- THEN PNGs are saved under `/tmp/opencode/`
- AND the PNGs show the strip with correct chip count, spacing, and picker layout

---

### R14 — Test Rewrite

All 9 affected `ui_tests` (lines ~4647–5920 in `ui_tests.rs`) MUST be rewritten to match the new layout. Tests MUST verify:

1. The strip renders the correct number of chips for various `slot-count` values (2, 3, 5, 8).
2. The picker opens above the strip when a chip is clicked.
3. Compact inactive/glow rows render at the correct height.
4. Keyboard navigation: strip is 1 stop; sub-navigation cycles chips; Enter opens picker; Escape closes.
5. `borders_tune_stop_count` returns correct values for the new formula.

No test may be skipped — all must pass or be rewritten.

#### Scenario: All border tests pass

- GIVEN the implementation is complete
- WHEN `cargo test` is run
- THEN all previously-passing tests pass
- AND all 9 rewritten border tests pass
- AND no tests are skipped

---

### R15 — Picker Content: Tokens + HSV

The in-flow picker card MUST contain:

1. **Header**: "Custom colour — {channel-name}" with a "Done" button.
2. **Token swatches**: 6 rectangles in a horizontal row, each showing the resolved palette colour and a short label (primary, secondary, tertiary, error, surface, lowest). The currently selected token MUST have an icy-cyan border highlight.
3. **HSV area**: The `HveColorPicker` component providing hue slider, saturation/brightness 2D area, opacity slider, and live preview.

Tapping a token swatch MUST immediately apply that token to the slot (via `slot-changed`). Dragging in the HSV area MUST update the preview live; pointer-up MUST commit the custom colour.

#### Scenario: Token selection applies immediately

- GIVEN the picker is open for slot 1
- WHEN the user taps the "tertiary" token swatch
- THEN the slot value changes to "p:tertiary"
- AND chip "2" in the strip updates to show the tertiary resolved colour
- AND the picker remains open

#### Scenario: HSV custom colour commits on pointer-up

- GIVEN the picker is open for slot 0
- WHEN the user drags in the saturation/brightness area and releases
- THEN the custom colour is committed (not per-drag-step)
- AND the chip "1" rectangle updates to the new colour
