# Exploration: HVE rewrite — modularity & reliable keyboard navigation

## Current State

HVE is a Rust (2021) + Slint 1.17.1 single binary (`src/`, `ui/*.slint`). The UI
is 3 executable layers that grew organically:

- **UI (Slint)**: `ui/main.slint` (~749 lines) holds ALL keyboard navigation
  logic inline: Root `FocusScope` with `forward-focus: root-kbd`, an
  auto-repeat Timer, manual `_nav-*` held-key flags, `nav-ready` gate, and
  `step-down/step-up` tab cycling. This logic does NOT belong in the view layer.
- **Glue (Rust)**: `src/main.rs` (~1675 lines) wires ~20 Slint callbacks with
  heavy closure captures, calls `hyprctl` directly 4x, hosts `prewarm_tabs` and
  `warmup_navigation`, and mixes config/theme/providers wiring into `main()`.
- **Driver-ish**: `src/ipc.rs` (~692 lines) owns `show_window`/`hide_window`,
  the show/hide special-workspace dance, `WindowActiveChanged(true)` emissions,
  the 150ms deferred-focus Timer, and a V4/V5 dispatch split. This is the
  fragile core (the navigation-after-re-show bug lived here).

**Coupling finding**: only `ipc.rs` and `main.rs` call `hyprctl` (plus
`providers/shell.rs` for detection). The domain (UI + engine + presets) does not
know about the compositor. That is the JOIN: the driver concerns are concentrated,
but they are embedded in the same files as UI/glue and use global atomics +
`OnceLock` (`HYPR_MODE`, `WINDOW_HIDDEN`, `TRAY_MODE`, `PREV_WORKSPACE`) that are
untestable in isolation.

**Existing decoupling precedent**: `src/theme_manager.rs` already defines a
`ThemeProvider` trait (`id`, `save`, `apply`, capabilities), and `providers/*`
implement it — the driver-for-compositor should mirror THIS pattern, not break new
ground.

## Affected Areas

- `src/ipc.rs` — show/hide, focus, V4/V5 dispatch, global atomics, IPC socket.
  The core fragility; must be extracted behind a `Composer` trait.
- `src/main.rs` — all Slint callback wiring + `hyprctl` calls + warmup helpers.
  Must shrink to composition root; `hyprctl` must move into the driver.
- `src/hypr_ipc.rs` — event listener (configreloaded). Low risk; keep or fold.
- `ui/main.slint` — keyboard nav + nav-ready/FocusScope live in the view.
  Should move to a reusable component/controller and hydrate state via the driver.
- `src/engine.rs`, `src/theme.rs` (theme * colors), `theme_manager.rs`,
  `providers/*` — domain; NOT directly affected but must not leak compositor.

## Approaches

1. **Extract a `Composer` trait + driver, keep structure** (recommended)
   - Define `trait Composer: Send + Sync { show/hide/focus/workspace/toggle_float }`.
   - `HyprlandComposer` implements it (moves the special-workspace + toggle + timer
     logic out of `ipc.rs` into a testable unit; `IPC` becomes a thin transport).
   - Replace `OnceLock`/atomics globals with fields on a `Controller`/`AppState`.
   - UI keeps rendering; a `controller` module maps Slint callbacks → driver calls.
   - Pros: least invasive to working code; isolates the exact buggy area; testable
     driver with fake `Composer` in unit tests; preserves 151 green tests.
   - Cons: `main.rs` still large if not also split; requires touching callbacks.
   - Effort: **High** (multi-file refactor) but surgical.

2. **Feature/component split of `main.rs`** (parallel concern): carve `main.rs`
   into `app/wiring.rs` (callbacks), `app/bootstrap.rs`, `app/state.rs`.
   - Pros: solves headless-testability; smaller reviewable units.
   - Cons: pure indirection when done alone; needs the trait to be meaningful.
   - Effort: **Medium**, do together with Approach 1.

3. **Classical clean-architecture rewrite** (UI→domain→driver, DIP everywhere):
   - Pros: maximum rigor, matches "modular" goal fully.
   - Cons: highest effort and risk; Slint callback wiring and Slint state getters
     are intertwined with the UI anyway; high churn for a codebase that largely
     WORKS except for the navigation bug. Overkill for the goal.
   - Effort: **Very High**.

## Recommendation

**Approach 1, plus the `main.rs` split (Approach 2) as a secondary deliverable.**

The rewrite decision is justified by the fragile navigation code, but the fix is a
**disciplined decoupling**, not a blank-slate rewrite. The codebase already has a
Trait-based provider pattern (theme_manager.rs) to mirror. Concrete plan:

1. Define `Composer` trait (pure interface, no Hyprland types leak into UI).
2. Extract `HyprlandComposer` from `ipc.rs` verbatim (hide/show/focus/toggle +
   the V4/V5 quirks + deferred-focus timer). Make it the ONLY thing that spawns
   `hyprctl`.
3. Introduce a `controller` (or slim `App`) that wires Slint callbacks →
   `&dyn Composer`, replacing the `OnceLock` globals with owned state so a fake
   `Composer` can be used in tests.
4. Keep all 151 passing tests green; add driver tests with a fake `Composer`
   proving hide→show→(focus re-assertion) contract.
5. During `apply`, also split `main.rs` into composable modules as deliverable
   work units (that is the modularity the user asked for).

The navigation bug is then testable as a deterministic unit contract, not a
live-compositor race.

## Risks

- Timer/async ordering of the deferred focus emission is hard to unit-test
  deterministically; test the sequence the driver issues (focus→move→emit), not
  wall-clock timing.
- The V4/V5 dispatch split adds a conditional seam — keep it in the composer.
- The atomics (`WINDOW_HIDDEN`) are read from multiple threads (tray, IPC); the
  refactor must keep a single mutable owner to avoid a new race.
- `main.rs` functional split must not silently change config save/reload order
  (theme apply relies on it).

## Ready for Proposal

**Yes.** Recommendation is clear: extract `ComposerDriver` + split `main.rs`, keep
the working UI, rewrite only the fragile driver seam. The orchestrator should tell
the user: this is a *decoupled refactor*, not a blank rewrite — the user gets
modularity AND keeps the 152 green tests.