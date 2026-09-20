# Proposal: Border Colors — Single Horizontal Strip

## Intent

The current BordersSection tune pane renders each gradient colour as a separate `BorderColorSlot` card (~110px tall each, 2-8 cards). Each card duplicates the same 6 token swatches (primary, secondary, tertiary, error, surface, surface_lowest). The result is:

- **Vertical bloat**: 2 slots = 260px; 8 slots = 1040px of near-identical content. The tune pane is almost entirely duplicated swatch grids.
- **No gradient preview**: the user cannot see the gradient at a glance; the colours are isolated in separate cards.
- **UX inconsistency**: every other controls in the tune pane use compact inline rows (angle, inactive, glow, animation leaves); the colour slots are the only full-card treatment.

This change replaces the N × `BorderColorSlot` cards with a single horizontal strip of chips (one per gradient colour, numbered 1-8). Tapping a chip opens a shared in-flow picker card **above** the strip, showing 6 token swatches + HSV custom colour. The gradient becomes scannable at a glance; the vertical cost drops to one compact row.

## Scope

### In Scope

1. **Horizontal chip strip**: a `HorizontalLayout` of 1-per-colour chips (colour chip + number label), replacing the vertical stack of `BorderColorSlot` cards.
2. **In-flow picker card**: when a chip is tapped (or keyboard-selected + Enter), the picker card expands **above** the strip — it is NOT a floating popover. It pushes content down. The picker contains:
   - 6 token swatches (primary/secondary/tertiary/error/surface/surface_lowest) in a compact row.
   - HSV custom colour area (hue slider + saturation/brightness 2D + opacity) below the tokens.
   - "Custom…" / "Editing…" toggle (reuses the existing `editing-slot` mechanism).
3. **Compact inactive/glow/inactive-glow rows**: each gets its own compact row (chip + label + "Custom…" button) with the same shared picker pattern. These are NOT full cards.
4. **Picker entry animation**: card slides in with `fade + slide-y + scale` using `ease-in-out-back` (~250ms), consistent with the project's visual language (height expansion, background transitions).
5. **Keyboard rework**: the tune pane reduces from N slot stops to 1 "strip" stop with sub-navigation (←→ across chips within the strip). `Enter` opens the picker; `Escape` closes it. The formula `borders_tune_stop_count` changes from `3 + n + ...` to `3 + 1 + ...` (one stop for the strip, not N).
6. **Deprecate/remove `BorderColorSlot`**: the component is either eliminated or reduced to a bare chip-only element reused by the strip.
7. **Test rework**: all 9 affected `ui_tests` (lines 4647-5920 in `ui_tests.rs`) rewritten + extended for the new layout.
8. **Visual render verification**: headless render test (`cargo test slice_focus_flow_renders` or sibling) producing PNGs of the new strip + open-picker states.

### Out of Scope

- Engine changes (`src/engine.rs`, `src/config.rs`, `src/settings.rs`, `src/theme_manager.rs`, `src/app_state.rs`, `src/watcher.rs`, `src/utils.rs`, all `src/providers/`).
- Other sections (gallery, animations, geometry, etc.).
- Floating popover picker — the picker is always in-flow.
- Changes to the `Composer` trait, nav-shell, or base-window specs.
- Preset list pane changes.

## Capabilities

> Contract for sdd-spec. No existing archived spec covers border section internals (`dynamic-border-tune-pane` is still in `changes/`, not yet in `openspec/specs/`).

### New Capabilities

- `border-color-strip`: horizontal chip strip that replaces N × BorderColorSlot cards, with in-flow picker card (tokens + HSV), shared picker state via `editing-slot`, and compact inactive/glow/inactive-glow rows.

### Modified Capabilities

- None (no archived spec covers border section internals).

## Approach

### Layout: Strip + In-Flow Picker

Replace the vertical `for` loop over `BorderColorSlot` with:

```
VerticalLayout {
    // Picker card — visible only when editing-slot >= 0
    if root.editing-slot >= 0 : Rectangle {
        // fade + slide-y + scale animation
        // Header: channel-name(root.editing-slot)
        // 6 token swatches in a row (compact, same as current BorderColorSlot)
        // HSV custom area (reuse existing HveColorPicker content)
    }

    // Chip strip — always visible
    HorizontalLayout {
        for color[idx] in root.tune-active-colors : Rectangle {
            // 16x16 colour chip + "idx+1" label
            // TouchArea: root.request-edit-slot(idx)
            // Focus ring when focused
        }
    }
}
```

The picker card sits above the strip and pushes content down. When `editing-slot` changes from -1 to >= 0, the card animates in. When it returns to -1, it animates out.

### Keyboard Model

**Current** (tune-local, N=slot count):
```
0=size, 1=angle, 2=inactive, 3..3+N-1=slots, 3+N=add, 4+N=remove, ...
```

**New** (tune-local):
```
0=size, 1=angle, 2=inactive, 3=strip, 4=add, 5=remove, 6=glow...
```

- Stop 3 = "strip" (one focusable stop for the entire chip row).
- Left/Right arrows navigate chips *within* the strip (sub-navigation).
- Enter on strip opens the picker; Escape closes it.
- Add/Remove remain at 4/5 (shifted down by N-1).

**Formula**: `borders_tune_stop_count` becomes:
```rust
let n = 1; // strip, not N slots
let mut total = 3 + n + 2 + 1; // size, angle, inactive, strip, add, remove, glow
// ... rest unchanged
```

### `editing-slot` Reuse

The existing `editing-slot` index space (0..7 = slots, 8 = inactive, 9 = glow, 10 = glow-inactive, -1 = closed) is preserved. The picker card checks `editing-slot >= 0` to show/hide. This avoids touching the Rust plumbing for picker state.

### `BorderColorSlot` Fate

The `BorderColorSlot.slint` component is **deprecated and deleted**. Its token swatch row and chip + label logic move into the strip (for gradient slots) and the compact rows (for inactive/glow). The file is removed; its import in `BordersSection.slint` is removed.

### Affected Files

| File | Impact | Description |
|------|--------|-------------|
| `ui/panel/sections/BordersSection.slint` | Modified | Strip replaces N × BorderColorSlot; picker card added above strip; compact rows for inactive/glow; animation bindings |
| `ui/panel/sections/BorderColorSlot.slint` | Removed | Deprecated — logic absorbed into strip and compact rows |
| `src/callbacks.rs` | Modified | `borders_tune_stop_count` formula changes; 5 unit tests rewritten |
| `src/shell/ui_tests.rs` | Modified | 9 border tests rewritten for new layout |
| `ui/panel/PanelRoot.slint` | Modified | Keyboard dispatch update for new stop count and strip sub-navigation |

## Animation Specification

### Picker Entry Animation

When `editing-slot` transitions from -1 to >= 0:

```
opacity: 0 → 1          (250ms, ease-in-out-back)
y: -8px → 0px           (250ms, ease-in-out-back) — slide down from above
scale-x: 0.95 → 1.0     (250ms, ease-in-out-back) — subtle horizontal scale
scale-y: 0.95 → 1.0     (250ms, ease-in-out-back) — subtle vertical scale
```

When `editing-slot` transitions from >= 0 to -1:

```
opacity: 1 → 0          (180ms, ease-in)
y: 0px → -8px           (180ms, ease-in)
```

Entry is slower and bouncy (ease-in-out-back); exit is crisp and fast (ease-in).

### Constraints (from project visual language)

- **Never animate `viewport-y`**: the ScrollView's viewport-y is imperative (wheel-driven); animations must not touch it.
- **Picker must stay visible when scrolled**: when `editing-slot >= 0`, the strip scrolls into view and the picker card above it must be fully visible. Use `follow-focus()` to ensure this.
- **No floating overlays**: the picker is a normal layout element that pushes content. No absolute positioning, no z-index hacks, no popup windows.

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Keyboard sub-navigation (←→ within strip) adds complexity to PanelRoot dispatch | Medium | Sub-navigation is self-contained in the strip component; PanelRoot only sees one stop (index 3) for the strip. Sub-navigation is internal state, not a PanelRoot concern. |
| 9 broken ui_tests have subtle regression surface | Medium | Rewrite tests to match new layout; run `cargo test` after each test rewrite. No test should be "skipped" — all must pass or be rewritten. |
| Picker card animation jank on low-end GPU | Low | `ease-in-out-back` is already used elsewhere in the project (height expansion). The animation is CSS-like opacity/transform only — no layout thrash. |
| `BorderColorSlot` removal breaks something outside BordersSection | Low | Grep confirms `BorderColorSlot` is only imported in `BordersSection.slint`. No other file imports it. |
| Surface exceeds ~400 lines for single PR | High | The change spans 5 files (~800-1200 lines estimated). **Split into chained PRs**: (1) keyboard model + callbacks tests, (2) strip UI + picker card, (3) BorderColorSlot removal + compact rows + animation, (4) visual render verification. |

## Rollback Plan

1. `git revert` the PR(s) in reverse order (visual → strip → keyboard).
2. The `BorderColorSlot.slint` file returns from git history.
3. `borders_tune_stop_count` returns to the old formula.
4. No data migrations, no engine changes — pure UI + callback rollback.
5. All 9 original `ui_tests` return from git history.

## Dependencies

- `HveColorPicker` in `ui/panel/ColorPicker.slint` (366 lines) — the HSV custom area is reused inside the picker card.
- `HveColors` token definitions (`token-primary`, `token-secondary`, etc.) in `ui/theme.slint`.
- `SkwdTokens` for spacing, font, and radius constants.
- `slice_focus_flow_renders` headless test harness for visual verification.

## Success Criteria

- [ ] `cargo test` fully green — all 9 rewritten border tests pass, no skipped tests.
- [ ] `borders_tune_stop_count` formula verified with updated unit tests (new expected values for various slot/glow/anim combinations).
- [ ] Keyboard: single stop for strip; ←→ navigates chips within strip; Enter opens picker; Escape closes picker.
- [ ] Visual: chip strip shows 1 chip per gradient colour (numbered 1..8); strip is compact (~40px height).
- [ ] Visual: picker card appears above strip with fade+slide+scale animation; tokens + HSV visible inside picker.
- [ ] Visual: inactive/glow/inactive-glow rows are compact (chip + label + "Custom…"), not full cards.
- [ ] `BorderColorSlot.slint` deleted, no dangling imports.
- [ ] Render test PNGs produced and visually confirmed (geometry, spacing, animation states).
