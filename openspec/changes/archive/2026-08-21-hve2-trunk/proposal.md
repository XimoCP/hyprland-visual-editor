# Proposal: HVE 2 Trunk — State-Driven Navigation Shell

## Intent

Current UI swaps panels via clip/width/height property switching (channel-changing), which cannot support HVE 2's expansion navigation: the gallery is the main screen; tapping a card/button expands the workshop inside the SAME window. The trunk replaces the shell with a state-driven navigation skeleton: one large base window, slot-based mounting for future modules (Gallery, Workshop/Settings), and a card-expansion transition. Future modules attach without rework.

## Scope

### In Scope
- Base window: single, starts large, no fixed sidebar
- Navigation state model: screen enum + expansion state
- Slots: Home, Gallery, Workshop/Settings mount/unmount contract
- Expansion transition: window grows via `set_size` with animation hook
- i18n shell wiring (ES/EN) reusing today's mechanism
- Keyboard + mouse shell navigation (keyboard-first preserved)
- Module seams (traits); strict TDD, all tests green

### Out of Scope
- Theme gallery + skwd-wall visual styles (parallelogram/grid/hexagon) — module 2
- Workshop/border/animation editors — module 4
- Engine changes (engine/config/settings/theme_manager/providers) — sealed
- Hyprland/monitor/HDR work — hyprmod's domain

## Capabilities

### New Capabilities
- `nav-shell`: state-driven navigation — screen enum, expansion transitions, slot mounting, keyboard+mouse input, shell i18n (ES/EN)
- `base-window`: single-window shell — starts large, `set_size` growth, min size, composer lifecycle wiring

### Modified Capabilities
None — engine and composer contracts preserved unchanged.

## Approach

1. Slint: `MainWindow` becomes a shell with a root screen slot; screens mount via state, not clip-swapping.
2. Rust: `NavState` + `ExpansionState` enums drive UI and window size from one mutable owner.
3. Card tap → `ExpansionState::Expanded(target)` → animate `set_size` → mount panel in slot.
4. Queue nav commands to avoid rapid-nav races; unit-test transitions headless (i-slint-backend-testing).
5. Slots as traits future modules implement; gallery/workshop ship stubs in trunk tests only.

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `ui/main.slint` | Modified | Becomes shell; panel clip-switching removed |
| `ui/shell.slint` | New | Shell chrome, slots, expansion animation |
| `src/shell/nav.rs` | New | NavState/ExpansionState + transitions |
| `src/shell/slots.rs` | New | Slot mount/unmount contract |
| `src/main.rs` | Modified | Composer lifecycle wired to shell |
| `src/tr.rs` | Modified | Shell strings ES/EN (mechanism as today) |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Slint layout jumpiness during `set_size` | Med | Animate in small steps; clamp min size; headless size tests |
| Rapid nav queues IPC commands | Med | Single state owner + command queue; fake-composer tests |
| Shell/module state drift | Med | State in one Rust owner; slots are read-only views |

## Rollback Plan

Additive: new `src/shell/` + `ui/shell.slint`; old panel logic preserved until module 2 lands. Revert commits to restore current UI; no config/engine/data-format changes.

## Dependencies

- Slint 1.17.1 `set_size`/animations (verified), existing `Composer` trait, `tr.rs` i18n, `cargo test` + i-slint-backend-testing.

## Success Criteria

- [ ] Single window starts large; no stacked windows
- [ ] Screen/expansion transitions unit-tested headless
- [ ] Slot stubs mount/unmount with zero engine changes
- [ ] Keyboard + mouse both navigate; ES/EN strings verified
- [ ] Existing tests green; gallery/workshop attach with no shell rework