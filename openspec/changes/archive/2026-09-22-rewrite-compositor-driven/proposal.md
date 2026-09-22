# Proposal: Extract ComposerDriver behind a Composer trait

## Intent

The keyboard-navigation bug (silent nav after SUPER+H re-show) lives entirely in
`src/ipc.rs`: the show/hide special-workspace dance, the `WindowActiveChanged(true)`
re-emission, the 150ms deferred-focus timer, and the V4/V5 dispatch split. This
logic is untestable because it depends on global atomics (`HYPR_MODE`, `WINDOW_HIDDEN`,
`PREV_WORKSPACE`) and live `hyprctl` calls. Goal: isolate it behind a `Composer` trait
so the nav contract is a deterministic, unit-testable sequence (focus → move → emit),
without rewriting the working UI (Slint) or domain (engine/theme/providers).

## Scope

### In Scope
- Define `trait Composer: Send + Sync` with the operations HVE currently needs:
  `show/hide/focus/current_workspace/move_to_workspace/toggle_float`.
- Extract `HyprlandComposer` from `src/ipc.rs` — the ONLY component allowed to spawn
  `hyprctl`; moves the V4/V5 quirks, special-workspace logic, and the deferred
  focus re-assertion into it.
- Replace the module-level atomics with owned state on a `Controller`/`AppState`
  so tests can inject a fake `Composer`.
- Keep all 152 existing tests green; add unit tests with a fake `Composer` proving
  the hide→show→(focus re-assert) command sequence.
- Wire the IPC socket (`hve.sock`) and tray to the controller instead of touching
  `hyprctl` inline.

### Out of Scope
- Splitting `src/main.rs` into composition modules (deferred to a second work unit).
- Moving keyboard navigation out of `ui/main.slint` (the nav logic is not the bug cause).
- Full clean-architecture rewrite.
- Multi-compositor support (driver is Hyprland-only; the trait enables a fake in tests).

## Capabilities

### New Capabilities
None — this is a pure refactor; behavior must remain identical.

### Modified Capabilities
None — no spec-level requirement changes. The existing behavior (show/hide/nav
recovery) is preserved, only relocated and made testable.

## Approach

1. Define `Composer` trait in a new `src/composer/mod.rs` (pure interface, no Hyprland
   types leak to callers).
2. Move `HyprlandComposer` impl from `ipc.rs` verbatim; it owns all `hyprctl`
   spawning and the V4/V5 mode detection (kept as the `HyprMode` enum inside it).
3. Introduce `Controller` holding owned state (`window_hidden`, `prev_workspace`,
   tray mode) and the `Box<dyn Composer>`; `ipc.rs` callbacks delegate to it.
4. Tests: fake `Composer` records the ordered calls; assert the show contract
   (`focus → move_to_workspace → emit_window_active`) and hide contract
   (`record_workspace → move_to_special → close_special`).
5. Keep the 150ms deferred-focus timer inside the driver; tests assert the emitted
   sequence, not wall-clock timing.

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `src/ipc.rs` | Modified | Loses hyprctl/hide-show internals; becomes thin transport + controller delegate |
| `src/composer/mod.rs` | New | `Composer` trait + `HyprlandComposer` driver + mode detection |
| `src/composer/hyprland.rs` | New | V4/V5 dispatch, special workspace, deferred focus re-assertion |
| `src/main.rs` | Modified | Uses `Controller` instead of direct ipc/hyprctl calls |
| `src/tray.rs` | Modified | Menus call controller, not globals |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Deferred-focus timer timing is hard to test deterministically | Med | Test ordered sequence, not wall-clock; keep timer small and isolated |
| Cross-thread atomic reads (`WINDOW_HIDDEN`) race if ownership changes | Med | Single mutable owner in `Controller`; keep atomic only where threads genuinely share |
| Behavior drift during extraction | Med | Keep 152 tests green as the safety net; extract verbatim first, refactor after |

## Rollback Plan

Changes are additive: the new `src/composer/` module plus a thin `Controller`.
The prior `ipc.rs` behavior can be restored by reverting the commit — no data or
config format changes; the hve.sock protocol and hve-settings file are untouched.

## Dependencies

- `slint 1.17.1` (unchanged), `hyprctl` at runtime (unchanged), existing crate deps.

## Success Criteria

- [ ] All 152 existing tests pass after extraction.
- [ ] New fake-`Composer` tests assert the hide/show command contract.
- [ ] `hyprctl` is spawned only from `HyprlandComposer` (verified by grep).
- [ ] Live check: SUPER+H hide → re-show from another window → keyboard nav works
      without touching the mouse (re-validated on the running instance).
- [ ] hve.sock IPC + tray commands behave identically to before.
