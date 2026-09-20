# Design: Border Colors — Single Horizontal Strip

## Technical Approach

Replace the N × `BorderColorSlot` vertical card stack with a single horizontal strip of numbered chips (1–8) and a shared in-flow picker card. The change touches 5 files: `BordersSection.slint` (major), `BorderColorSlot.slint` (deleted), `PanelRoot.slint` (minor dispatch), `callbacks.rs` (stop-count formula), and `ui_tests.rs` (9 tests rewritten + new ones). The engine is untouched.

The approach preserves the existing `editing-slot` index space (0–7 slots, 8 inactive, 9 glow, 10 glow-inactive, –1 closed) and the picker command-channel architecture (`picker-cmd` / `picker-cmd-kind`). The strip is a single keyboard stop with internal sub-navigation (←→ cycles chips), not N separate stops.

## Architecture Decisions

### Decision: Strip = single stop with internal sub-navigation

**Choice**: The entire chip strip is stop index 3 in the tune-local index space. Left/Right arrows cycle the active chip *within* the strip (wrapping). Enter opens the picker. The strip is never "engaged" (no slider engagement).

**Alternatives considered**:
1. Strip as N separate stops (like old slots) — rejected: defeats the purpose (same vertical cost in keyboard nav).
2. Strip with engaged mode + arrows for chip selection — rejected: conflicts with the engaged-slider paradigm (arrows adjust values). The strip is a selection widget, not an adjustment widget.

**Rationale**: Sub-navigation is self-contained in the strip component. PanelRoot only sees stop index 3. The `engaged-slider` property is never set for the strip, so Left/Right always do sub-navigation (never value adjustment). Enter on strip → `tune-enter()` returns 1 (handled internally, strip opens picker). PanelRoot never engages/disengages for this stop.

### Decision: No scale animation on the picker card in software renderer

**Choice**: Animate `opacity` (0→1) and `y` (–8px→0px) on the picker card using `animate`. Declare `transform-scale-x` and `transform-scale-y` (0.95→1.0) for the production app. In software-renderer headless tests, scale will not render (Slint limitation), so the render test asserts on opacity + y + content presence.

**Alternatives considered**:
1. Skip scale entirely — rejected: the spec requires it and the production app uses the GPU renderer where it works.
2. Use an `Image`-based workaround — rejected: too complex, not worth the effort for a visual polish item.

**Rationale**: Slint's software renderer does not support `transform-scale-x`/`transform-scale-y` (confirmed via docs: "These features are not supported by the software renderer"). The project's render tests use `renderer_name: "software"`. The scale animation is a visual polish for the GPU-rendered production app; the headless test confirms the critical states (opacity, position, content) without scale.

### Decision: Picker card height stays at 344px, placed above the strip

**Choice**: The picker card is a `Rectangle` with `height: 344px` positioned in the `VerticalLayout` *before* the strip. When `editing-slot < 0`, it is wrapped in `if` and takes 0 layout space.

**Alternatives considered**:
1. Dynamic height based on content — rejected: the current picker is already 344px and fits all content (tokens + HSV + header). Dynamic height adds complexity for no benefit.
2. Place the picker below the strip — rejected: spec requires it above; scrolling to show the strip + picker above is cleaner.

**Rationale**: Matches the existing picker pattern (lines 231–302 of current `BordersSection.slint`). The `if` conditional ensures no dead gap when closed. The existing `follow-focus()` already scrolls to `viewport-y = 0px` when `editing-slot >= 0`, which positions the strip at the bottom of the viewport with the picker above.

### Decision: Deprecated file deleted, not reduced to chip reusables

**Choice**: `BorderColorSlot.slint` is **deleted entirely**. Its internal token-swatch row and chip+label pattern are inlined into the strip component and the compact rows. No re-usable sub-component is extracted.

**Alternatives considered**:
1. Reduce to a `BorderColorChip` (16×16 chip + label) — rejected: the chip is 4 lines of Slint (Rectangle + Text). Extracting a component for 4 lines adds an import and a file for no reuse benefit (each usage site binds different properties).

**Rationale**: The strip's chips are in a `for` loop with inline properties (resolved-color, index, focus state). The compact rows each have unique callback wiring. Inlining keeps the code local and avoids the import-cleanup trap.

## Data Flow

```
PanelRoot.borders-focused-index
    │
    ├─ < list-count → BordersListPane (card focus)
    │
    └─ ≥ list-count → BordersTunePane (tune stop)
         │
         ├─ local-focus == 0 → Size slider (engaged-slider = 0)
         ├─ local-focus == 1 → Angle control
         ├─ local-focus == 2 → Inactive colour row (engaged-slider = 2)
         ├─ local-focus == 3 → Strip (NEVER engaged)
         │    ├─ ←→ cycles strip-active-chip (internal)
         │    ├─ Enter → editing-slot = strip-active-chip → picker opens
         │    └─ Esc  → editing-slot = -1 → picker closes
         ├─ local-focus == 4 → Add slot
         ├─ local-focus == 5 → Remove slot
         ├─ local-focus == 6 → Glow toggle (if !glow)
         │   or 6..9+glow range/power/color/inactive (if glow)
         ├─ base-anim..tail → Animation leaves
         ├─ tail+1 → Rule toggle
         ├─ tail+2 → Save name
         └─ tail+3 → Save button
```

### State Map

| Property | Owner | Description |
|----------|-------|-------------|
| `editing-slot` | BordersSection ↔ PanelRoot (two-way) | 0–7 gradient slots, 8 inactive, 9 glow, 10 glow-inactive, –1 closed. **Preserved exactly.** |
| `strip-active-chip` | BordersTunePane (internal) | Index 0..slot-count–1 of the focused chip within the strip. Wraps on ←→. Resets to 0 when strip first focused. |
| `tune-active-colors` | Model from Rust | Array of "p:token" or "c:hex8" strings, one per gradient slot. |
| `tune-colors-resolved` | Model from Rust | Array of `color` values, same index space as tune-active-colors + inactive/glow. |

### editing-slot Interaction with Picker Card

```
editing-slot >= 0  →  picker card visible (if conditional)
                    →  HveColorPicker seeded from tune-colors-resolved[clamp(editing-slot, 0, 10)]
                    →  Header shows channel-name(editing-slot)
editing-slot == -1 →  picker card hidden (0 layout space)
```

Clicking a chip or "Custom…" button calls `request-edit-slot(idx)`, which sets `editing-slot` on the section. The picker card is the ONLY picker at any time. Clicking a different chip while open transitions `editing-slot` to the new index (same card, different content).

## File Changes

| File | Action | Description |
|------|--------|-------------|
| `ui/panel/sections/BordersSection.slint` | **Modify** (~200 lines changed) | Remove `BorderColorSlot` import. Replace `for` loop over `BorderColorSlot` cards with horizontal strip. Replace inactive/glow `BorderColorSlot` instances with compact rows (chip + label + "Custom…" button). Add picker card animation bindings (opacity, y, transform-scale-x/y). Add `strip-active-chip` property. Update `idx-add`, `idx-remove`, and all downstream index calculations. Update `adjust-slider()` to handle new strip index. Update `tune-enter()` to handle strip-as-single-stop. Update keyboard hint text. |
| `ui/panel/sections/BorderColorSlot.slint` | **Delete** | Deprecated — logic absorbed into strip and compact rows. No other file imports it. |
| `ui/panel/PanelRoot.slint` | **Modify** (~5 lines) | No structural changes needed. The `borders-engaged-slider` is never set for the strip (the strip is never engaged). PanelRoot already handles Escape → `tune-editing-slot = -1` and the picker-open/adjust branches. The strip's ←→ sub-navigation is internal. The only change: the strip stop (index 3) should NOT engage `borders-engaged-slider` when Enter is pressed. This is already the case if `tune-enter()` returns 1 for the strip. |
| `src/callbacks.rs` | **Modify** (~30 lines) | Change `borders_tune_stop_count` formula: replace `3 + n + 2 + 1` with `3 + 1 + 2 + 1` (strip = 1 stop, not n). Update 5 unit test assertions with new expected values. |
| `src/shell/ui_tests.rs` | **Modify** (~400 lines) | Rewrite 6 tests: `borders_tune_pane_renders_with_dynamic_slots`, `borders_tune_full_focus_reaches_last_and_middle`, `borders_tune_glow_color_cards_render_inside_their_box`, `borders_tune_custom_picker_opens_for_one_channel`, `borders_list_pane_scrolls_with_the_mouse_wheel`, `borders_list_wheel_reaches_the_end`, `borders_tune_renders_eight_slots`. Keep 2 tests: `borders_mouse_never_touches_focus_by_construction`, `panel_scopes_ignore_mouse_focus_by_construction`. Add 5 new tests for strip rendering, keyboard sub-navigation, picker-above-strip, stop-count formula, and compact row height. |

### Exact Changes in BordersTunePane (Borrowed from Current Code)

**Lines removed** (current):
- Line 25: `import { BorderColorSlot } from "BorderColorSlot.slint";` — REMOVED
- Lines 475–498: `for i in root.tune-active-colors.length : Rectangle { ... BorderColorSlot ... }` — REPLACED with strip
- Lines 440–451: `BorderColorSlot` inside inactive-wrap — REPLACED with compact row
- Lines 780–793: `BorderColorSlot` inside glow color card — REPLACED with compact row
- Lines 819–831: `BorderColorSlot` inside glow inactive card — REPLACED with compact row

**Lines added** (new):
- Horizontal strip component (see "Strip Component" below)
- Compact row component for inactive/glow/inactive-glow
- Animation bindings on the picker card
- `strip-active-chip` property and sub-navigation logic

**Index calculations changed**:

| Property | Old Formula | New Formula |
|----------|-------------|-------------|
| `first-slot-idx` | 3 | 3 (unchanged — strip occupies 1 stop at index 3) |
| `idx-add` | 3 + slot-count | 4 (fixed — add is always stop 4) |
| `idx-remove` | 4 + slot-count | 5 (fixed — remove is always stop 5) |
| `idx-glow-en` | 5 + slot-count | 6 (fixed — glow-enable is always stop 6) |
| `idx-glow-range` | 6 + slot-count | 7 |
| `idx-glow-power` | 7 + slot-count | 8 |
| `idx-glow-color` | 8 + slot-count | 9 |
| `idx-glow-inactive` | 9 + slot-count | 10 |
| `base-anim` | glow ? 10+n : 6+n | glow ? 11 : 7 |

**Critical**: All `slot-count`-dependent offsets become fixed constants. The slot-count is now only used for: (1) how many chips the strip renders, (2) bounds on add/remove, (3) the `tune-count` output property.

## Interfaces / Contracts

### BordersTunePane — New Properties

```slint
// Strip sub-navigation state (internal to the tune pane).
// Index of the active chip within the strip (0..slot-count-1).
// Wraps circularly on ←→. Resets to 0 when the strip gains focus.
in-out property <int> strip-active-chip: 0;

// Animation state for the picker card.
// Driven by editing-slot transitions.
property <bool> picker-visible: root.editing-slot >= 0;
```

### BordersTunePane — Strip Component (new, replaces for-loop)

```slint
// ── Gradient colour strip (replaces N × BorderColorSlot cards) ──
if root.slot-count > 0 : VerticalLayout {
    // Picker card — appears ABOVE the strip when a chip is selected
    if root.picker-visible : Rectangle {
        height: 344px;
        opacity: root.picker-visible ? 1.0 : 0.0;
        y: root.picker-visible ? 0px : -8px;
        transform-scale-x: root.picker-visible ? 1.0 : 0.95;
        transform-scale-y: root.picker-visible ? 1.0 : 0.95;
        animate opacity { duration: 250ms; easing: ease-in-out-back; }
        animate y { duration: 250ms; easing: ease-in-out-back; }
        animate transform-scale-x { duration: 250ms; easing: ease-in-out-back; }
        animate transform-scale-y { duration: 250ms; easing: ease-in-out-back; }
        border-width: 1px;
        border-color: HveColors.accent-cyan;
        border-radius: SkwdTokens.radius-8;
        background: HveColors.bg-surface;
        clip: true;

        VerticalLayout {
            padding: 16px;
            spacing: 8px;
            // Header: channel name + Done button (same as current picker)
            HorizontalLayout { /* ... header ... */ }
            // 6 token swatches in a compact row
            HorizontalLayout { /* ... tokens ... */ }
            // HveColorPicker HSV area
            col-picker := HveColorPicker { /* ... same as current ... */ }
        }
    }

    // Chip strip — always visible
    HorizontalLayout {
        spacing: 6px;
        alignment: start;
        padding-left: 16px;
        padding-right: 16px;

        for i in root.tune-active-colors.length : Rectangle {
            property <bool> is-active-chip: i == root.strip-active-chip && root.has-focus;
            width: 40px;
            height: 32px;
            border-radius: SkwdTokens.radius-4;
            border-width: self.is-active-chip ? 2px : 1px;
            border-color: self.is-active-chip ? #8fd8ff : HveColors.border;
            background: self.is-active-chip ? HveColors.bg-hover : HveColors.bg-surface;

            HorizontalLayout {
                spacing: 4px;
                alignment: center;

                Rectangle {
                    width: 16px;
                    height: 16px;
                    border-radius: SkwdTokens.radius-4;
                    border-width: 1px;
                    border-color: HveColors.border;
                    background: root.tune-colors-resolved[i];
                }

                Text {
                    text: (i + 1);
                    font-family: SkwdTokens.font-body;
                    font-size: SkwdTokens.size-11;
                    font-weight: 600;
                    color: HveColors.text;
                    vertical-alignment: center;
                }
            }

            TouchArea {
                width: 100%;
                height: 100%;
                mouse-cursor: pointer;
                clicked => {
                    root.strip-active-chip = i;
                    root.request-edit-slot(i);
                }
            }
        }
    }
}
```

### Compact Row Component (replaces inactive/glow BorderColorSlot)

```slint
// ── Compact colour row (chip + label + "Custom…" button) ──
// Replaces the 110px BorderColorSlot with a ~40-56px row.
// Used for inactive (index 8), glow colour (9), glow inactive (10).
Rectangle {
    property <bool> lit: root.local-focus == ROOT_STOP_INDEX && root.has-focus;
    height: self.lit ? 56px : 48px;
    animate height { duration: 250ms; easing: ease-in-out-back; }
    border-width: 1px;
    border-color: self.lit ? #8fd8ff : HveColors.border;
    border-radius: SkwdTokens.radius-8;
    background: self.lit ? HveColors.bg-hover : HveColors.bg-card;

    HorizontalLayout {
        padding: 12px;
        spacing: 8px;
        alignment: space-between;

        HorizontalLayout {
            spacing: 8px;
            alignment: center;

            Rectangle {
                width: 16px;
                height: 16px;
                border-radius: SkwdTokens.radius-4;
                border-width: 1px;
                border-color: HveColors.border;
                background: RESOLVED_COLOR;
            }

            Text {
                text: "LABEL";
                font-family: SkwdTokens.font-body;
                font-size: SkwdTokens.size-13;
                font-weight: 600;
                color: HveColors.text;
                vertical-alignment: center;
            }
        }

        Rectangle {
            width: 84px;
            height: 22px;
            border-radius: SkwdTokens.radius-4;
            border-width: 1px;
            border-color: root.editing-slot == EDITING_INDEX ? HveColors.accent-cyan : HveColors.border;
            background: root.editing-slot == EDITING_INDEX ? HveColors.accent-cyan
                : (custom-ta.has-hover ? HveColors.bg-hover : HveColors.bg-surface);

            Text {
                text: root.editing-slot == EDITING_INDEX ? "Editing…" : "Custom…";
                color: root.editing-slot == EDITING_INDEX ? HveColors.bg-dark : HveColors.text;
                font-family: SkwdTokens.font-body;
                font-size: SkwdTokens.size-11;
                font-weight: 600;
                horizontal-alignment: center;
                vertical-alignment: center;
            }

            custom-ta := TouchArea {
                width: 100%;
                height: 100%;
                mouse-cursor: pointer;
                clicked => { root.request-edit-slot(EDITING_INDEX); }
            }
        }
    }
}
```

### BordersTunePane tune-enter() Changes

```slint
// NEW: strip stop (index 3) — never engages, always returns 1 (handled internally)
if (local == 3 && root.slot-count > 0) {
    // Enter on strip opens the picker for the active chip
    root.editing-slot = root.strip-active-chip;
    return 1;
}

// MODIFIED: inactive stop (index 2) — engage path unchanged,
// but Enter on engaged opens picker (existing behavior, unchanged)
// if (local == 2 && root.engaged-slider == local) { root.editing-slot = 8; return 1; }

// REMOVED: old per-slot engage+enter paths (3..3+n-1).
// The strip at index 3 is NEVER engaged, so Left/Right always do sub-navigation.
```

### BordersTunePane adjust-slider() Changes

```slint
// REMOVE the old per-slot branch:
// if (e >= 3 && e < 3 + n) { ... next-token ... }

// The strip is never engaged (engaged-slider is never set to 3),
// so adjust-slider() is never called for index 3. No replacement needed.
```

### callbacks.rs — New Formula

```rust
pub fn borders_tune_stop_count(
    _slot_count: i32,  // kept for API compat, ignored in formula
    glow_enabled: bool,
    anim0: bool,
    anim1: bool,
    anim2: bool,
) -> i32 {
    // OLD: let n = slot_count.clamp(0, 8); let mut total = 3 + n + 2 + 1;
    // NEW: strip = 1 stop (not N stops)
    let mut total = 3 + 1 + 2 + 1;  // size, angle, inactive, strip, add, remove, glow
    if glow_enabled {
        total += 4;  // enabled + range + power + color + inactive
    }
    for enabled in [anim0, anim1, anim2] {
        total += 1;  // leaf enabled toggle
        if enabled {
            total += 3;  // speed + bezier + style
        }
    }
    total += 1 + 2;  // rule + save-name + save-button
    total
}
```

**New expected values:**

| slot_count | glow | anim0 | anim1 | anim2 | OLD | NEW |
|-----------|------|-------|-------|-------|-----|-----|
| 2 | off | off | off | off | 14 | **13** |
| 2 | on | off | off | off | 18 | **17** |
| 8 | on | on | on | on | 33 | **26** |
| 3 | on | off | off | off | 19 | **17** |
| 0 | off | off | off | off | 12 | **13** |
| 99 | off | off | off | off | 20 | **13** |

The savings: `N - 1` stops per configuration. For 8 slots with everything on, that's 8 fewer stops (33→26).

### BordersSection tune-count Property Change

```slint
// OLD:
out property <int> tune-count: 12 + root.tune-n + (root.tune-glow-enabled ? 4 : 0)
    + (root.tune-anim-borderangle-enabled ? 3 : 0)
    + (root.tune-anim-border-enabled ? 3 : 0)
    + (root.tune-anim-fadeshadow-enabled ? 3 : 0);

// NEW:
out property <int> tune-count: 13 + (root.tune-glow-enabled ? 4 : 0)
    + (root.tune-anim-borderangle-enabled ? 3 : 0)
    + (root.tune-anim-border-enabled ? 3 : 0)
    + (root.tune-anim-fadeshadow-enabled ? 3 : 0);
// Note: the "+ root.tune-n" is removed (strip = 1 stop, not N).
```

### PanelRoot.slint — Keyboard Dispatch Change

The strip stop (index 3) is never engaged. When `tune-enter()` returns 1 for the strip, PanelRoot must NOT set `borders-engaged-slider`. This is already the case: the current code only engages when `tune-enter() == 0`:

```slint
// PanelRoot line 510 (current, UNCHANGED):
if (borders.tune-enter() == 0) {
    root.borders-engaged-slider = root.borders-focused-index - root.borders-list-len;
}
```

Since `tune-enter()` returns 1 for the strip, `borders-engaged-slider` stays at -1. Left/Right then hit the non-engaged branch in PanelRoot (lines 484–500), which only jumps panels (list↔tune), not within the strip. The strip's internal sub-navigation is handled by `BordersTunePane` intercepting keys *before* PanelRoot — but wait, PanelRoot owns the single FocusScope.

**Correction**: PanelRoot owns the FocusScope and dispatches ALL keys. The strip cannot intercept keys internally. Instead:

**Sub-navigation is PanelRoot-level**: When `local-focus == 3` (strip) and the strip is NOT engaged, Left/Right arrows must be forwarded to the strip for chip cycling. This requires a small addition to PanelRoot:

```slint
// PanelRoot section == 1, non-engaged Left/Right:
// When focused on the strip (local == 3), arrows cycle chips instead of jumping panels.
if (event.text == Key.RightArrow) {
    if (root.borders-focused-index >= root.borders-list-len
        && (root.borders-focused-index - root.borders-list-len) == 3) {
        // Strip sub-navigation: cycle chip right
        borders.strip-cycle(1);
        return accept;
    }
    // ... existing panel jump logic ...
}
if (event.text == Key.LeftArrow) {
    if (root.borders-focused-index >= root.borders-list-len
        && (root.borders-focused-index - root.borders-list-len) == 3) {
        // Strip sub-navigation: cycle chip left
        borders.strip-cycle(-1);
        return accept;
    }
    // ... existing panel jump logic ...
}
```

**Strip sub-navigation exposed as public function:**

```slint
// BordersTunePane (or BordersSection — same pattern as picker-move):
public function strip-cycle(delta: int) {
    if (root.slot-count <= 0) { return; }
    root.strip-active-chip = mod(root.strip-active-chip + delta + root.slot-count, root.slot-count);
}
```

**Alternative (cleaner)**: Make the strip a "pseudo-engaged" state. When focused on stop 3, Enter engages it (but not as a slider — as a "strip focus" mode). Then Left/Right do chip cycling, and Enter again opens the picker. This requires a new `strip-engaged` boolean, which adds complexity. **Rejected** — the PanelRoot-forward approach is simpler and follows the existing picker-move pattern.

### PanelRoot.slint — Esc Behavior with Strip

The current Esc logic:
```slint
if (root.tune-editing-slot >= 0) { root.tune-editing-slot = -1; }
else if (root.borders-engaged-slider >= 0) { root.borders-engaged-slider = -1; }
else { root.back(); }
```

This already works correctly:
1. Picker open → Esc closes picker (editing-slot = -1)
2. Strip focused, picker closed → Esc goes back to rail (back())

The strip is never "engaged" so the middle branch is never hit for it.

## Testing Strategy

### Tests Rewritten (7 tests)

| Test Function | Change | Reason |
|--------------|--------|--------|
| `borders_tune_pane_renders_with_dynamic_slots` | Replace slot card assertions with strip chip rendering assertions. Count chip rectangles instead of slot cards. | Layout changed from vertical cards to horizontal strip |
| `borders_tune_full_focus_reaches_last_and_middle` | Update `borders_tune_stop_count` assertion (27→26 for 2 slots glow+3 anims). Recalculate `middle` and `last` indices. (stop-count assertion updated in PR 1 to keep the suite green) | Stop count formula changed |
| `borders_tune_glow_color_cards_render_inside_their_box` | Replace `BorderColorSlot` height assertions with compact row height assertions. Update stop count (18→17). Recalculate focus index. (stop-count assertion updated in PR 1 to keep the suite green) | Glow colour rows are now compact (48–56px not 168px) |
| `borders_tune_custom_picker_opens_for_one_channel` | Verify picker appears above the strip (not between slot cards). Adjust preview index for new layout. | Picker position changed (above strip) |
| `borders_list_pane_scrolls_with_the_mouse_wheel` | Minimal change: slot count stays the same, scroll logic unchanged. May need to adjust content pixel bounds. | Layout is different but scroll behavior is identical |
| `borders_list_wheel_reaches_the_end` | Minimal change: same as above. | Scroll behavior unchanged |
| `borders_tune_renders_eight_slots` | Replace "8 slot swatch rows must render" assertion with "8 chips must render in strip". | 8 slots → 8 chips in strip, not 8 slot cards |

### Tests Kept Unchanged (2 tests)

| Test Function | Reason |
|--------------|--------|
| `borders_mouse_never_touches_focus_by_construction` | Presentational: checks no FocusScope and no .focus() in BordersSection. Still valid — strip doesn't add a FocusScope. |
| `panel_scopes_ignore_mouse_focus_by_construction` | Checks all panel sections have no FocusScope. Still valid. |

### Tests Added (5 tests)

| Test Function | What It Verifies |
|--------------|------------------|
| `borders_strip_renders_correct_chip_count` | Strip renders N chips for slot-count ∈ {2, 3, 5, 8}. Each chip has a colored rectangle and numeric label. |
| `borders_strip_picker_opens_above_strip` | When editing-slot ≥ 0, the picker card is visible above the strip. Diff between closed/open is > 5000 pixels. |
| `borders_strip_keyboard_sub_navigation` | Verify strip-cycle function cycles chips (0→1→2→0). Verify Enter on strip sets editing-slot. Verify Esc closes picker. (Unit-test the public functions, not the FocusScope dispatch.) |
| `borders_tune_stop_count_strip_single_stop` | Verify the new formula: `borders_tune_stop_count(N, ...)` returns the same value regardless of N (N is ignored). Specific assertions for 2/4/8 slots × glow on/off × anims on/off. |
| `borders_strip_compact_rows_height` | Inactive/glow/glow-inactive rows render at 48–56px height (not 168px). Focus the inactive stop and verify the row height is compact. |

### Render Test Extension

Extend `slice_focus_flow_renders` (or add a sibling `borders_strip_flow_renders`) to produce PNGs of:

1. Strip closed (no picker, 3 chips visible)
2. Strip open (picker above strip, editing-slot = 1)
3. Strip with 8 chips (no overflow)
4. Compact inactive row (focused)

Save under `/tmp/opencode/borders_strip_*.png` and visually inspect for:
- Chips are evenly spaced and non-overlapping at 8 slots
- Picker card is fully visible above the strip
- Compact rows are ~48–56px tall
- Focus ring (icy-cyan #8fd8ff) appears on the focused chip

### TDD Steps for Apply

**PR 1 — Keyboard model (callbacks.rs + tests)**

1. **RED**: Update `test_borders_tune_count_covers_full_inventory` assertions to expect new values (13, 26, 17, 13, 13). Tests fail.
2. **GREEN**: Change `borders_tune_stop_count` formula to `3 + 1 + 2 + 1` base (strip = 1). Tests pass.
3. **REFACTOR**: Update `tune-count` expression in `BordersSection.slint` (remove `+ root.tune-n`).

**PR 2 — Strip UI + picker (BordersSection.slint)**

1. **RED**: Add `borders_strip_renders_correct_chip_count` test. Mount MainWindow, set 3 slots, assert strip renders 3 chips. Test fails (old layout has 3 slot cards, not strip).
2. **GREEN**: Replace the `for i in root.tune-active-colors.length` block with the horizontal strip. Test passes.
3. **RED**: Add `borders_strip_picker_opens_above_strip` test. Set editing-slot = 1, assert picker visible above strip. Test fails.
4. **GREEN**: Add picker card `if root.picker-visible` block above strip. Test passes.
5. **RED**: Add `borders_strip_compact_rows_height` test. Focus inactive stop, assert height < 60px. Test fails.
6. **GREEN**: Replace `BorderColorSlot` in inactive-wrap with compact row. Test passes.
7. **REFACTOR**: Remove `BorderColorSlot.slint` file, remove import.

**PR 3 — Keyboard dispatch + sub-navigation (PanelRoot.slint + BordersSection.slint)**

1. **RED**: Add `borders_strip_keyboard_sub_navigation` test. Verify `strip-cycle(1)` increments chip. Test fails (function doesn't exist).
2. **GREEN**: Add `strip-cycle()` public function to `BordersTunePane`.
3. **RED**: Verify PanelRoot forwards Left/Right to `strip-cycle` when local-focus == 3. Test fails (PanelRoot doesn't check for strip stop).
4. **GREEN**: Add strip sub-navigation branches in PanelRoot Left/Right dispatch.
5. **REFACTOR**: Update keyboard hint text.

**PR 4 — Visual render verification + test polish**

1. Run `cargo test` — all tests green.
2. Run `cargo test borders_strip_flow_renders` — PNGs produced.
3. Visually inspect PNGs.
4. Add `borders_tune_stop_count_strip_single_stop` test.

## Threat Matrix

N/A — no routing, shell, subprocess, VCS/PR automation, executable-file classification, or process-integration boundary.

## Migration / Rollout

No data migration required. The `editing-slot` index space is preserved exactly. No engine changes. No config format changes. No feature flags needed — the change is a visual layout replacement that applies immediately.

**Rollback**: `git revert` the chained PRs in reverse order (visual → keyboard → strip → callbacks). The `BorderColorSlot.slint` file returns from git history. `borders_tune_stop_count` returns to the old formula.

## PR Chaining Plan

Estimated total: ~800–1000 lines changed across 5 files. Split into 4 chained PRs:

| PR | Scope | Est. Lines | Files |
|----|-------|-----------|-------|
| **PR 1** | `borders_tune_stop_count` formula + unit tests + `tune-count` expression | ~50 | `callbacks.rs`, `BordersSection.slint` (1 line) |
| **PR 2** | Strip UI + picker card + compact rows + delete `BorderColorSlot.slint` + render tests | ~400 | `BordersSection.slint`, `BorderColorSlot.slint` (delete), `ui_tests.rs` |
| **PR 3** | Keyboard sub-navigation (PanelRoot dispatch + `strip-cycle`) + keyboard tests | ~100 | `PanelRoot.slint`, `BordersSection.slint`, `ui_tests.rs` |
| **PR 4** | Animation polish + render verification + test assertions update | ~50 | `BordersSection.slint`, `ui_tests.rs` |

**Dependency order**: PR 1 → PR 2 → PR 3 → PR 4. Each PR leaves `cargo test` green.

## Open Questions

- [ ] **`transform-scale-x/y` in software renderer**: Confirmed NOT supported. The render test must not assert on scale. In-app visual verification uses the GPU renderer. Is this acceptable for the visual verification gate?
- [ ] **Strip cycle forwarding from PanelRoot**: The sub-navigation must go through PanelRoot (owns FocusScope). Is the `strip-cycle(delta)` function + PanelRoot branch the cleanest pattern, or should we add a `strip-engaged` boolean to avoid PanelRoot needing to know about strip internals?
- [x] **Picker card exit animation**: RESOLVED 2026-09-20. The picker card is an in-flow child of the `picker-block` VerticalLayout, so its `y` is owned by the layout and the spec's `y: -8px → 0px` / `0px → -8px` cannot render. The implementation animates `height` instead — entry 0px → 380px over 250ms `ease-in-out-back`, exit 380px → 0px over 150ms `ease-in` — and that collapse is what moves the strip, smoothly instead of jumping. Decision: document the behaviour that exists and keep it (the keeper approves the current animation); do NOT add a `transform-translate-y` shim, which is GPU-only and therefore outside the reach of the software-renderer visual gate. R5 was corrected to match.
