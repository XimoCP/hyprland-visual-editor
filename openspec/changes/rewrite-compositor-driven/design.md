# Design: Extract Composer behind a `Composer` trait

## Technical Approach

Extract the Hyprland compositor driver from `src/ipc.rs` (692 lines) into a new `src/composer/` module. Mirror the existing `ThemeProvider` pattern (`Send + Sync` trait + `Box<dyn Composer>`) so the IPC callbacks and tray menus delegate to a `Controller` with owned state instead of module-level atomics. Behavior is preserved verbatim — pure refactor.

## Architecture Decisions

### Decision: Trait vs Concrete Type

| Aspect | Choice | Tradeoff |
|--------|--------|----------|
| Interface | `trait Composer: Send + Sync` | Enables fake in tests; slight vtable indirection (~1ns) |
| Concrete impl | `HyprlandComposer` — only spawner of `hyprctl` | All hyprctl calls flow through one place; grep-enforceable invariant |
| Storage | `Box<dyn Composer>` in `Controller` | Matches `ThemeProvider` pattern already in codebase |

### Decision: Where state lives

| Aspect | Choice | Tradeoff |
|--------|--------|----------|
| Owned state | `Controller` struct: `window_hidden: bool`, `prev_workspace: Option<String>`, `tray_mode: bool` | Eliminates `AtomicBool`/`Mutex<Option>` globals; single mutable owner |
| `HYPR_MODE` | `OnceLock<HyprMode>` inside `HyprlandComposer` | Lazy single init; no global leak to callers |
| Debounce | `Mutex<Option<Instant>>` stays in `ipc.rs` | Pure local concern; no cross-thread sharing needed |

### Decision: Timer placement

| Aspect | Choice | Tradeoff |
|--------|--------|----------|
| 150ms deferred focus | Inside `HyprlandComposer::show()`, uses `slint::Timer::single_shot` | Tests assert emitted sequence (focus → move → WindowActiveChanged), not wall-clock |

### Decision: V4/V5 seam

| Aspect | Choice | Tradeoff |
|--------|--------|----------|
| Detection | `hypr_mode()` runs once at init (V5: `hl.dsp.no_op()` → "ok"; V4: `hyprctl version` → "Hyprland") | Cached in `OnceLock`; no runtime dispatch cost |
| Dispatch | Two internal methods: `hypr_dispatch_v5(script)` / `hypr_dispatch_v4(args)` | Each `HyprlandComposer` method branches once on mode |

### Decision: Controller vs AppState naming

| Aspect | Choice | Tradeoff |
|--------|--------|----------|
| Name | `Controller` | Matches the proposal; avoids confusion with Slint's internal state management |

## Data Flow

```
toggle-tray command
    │
    ▼
ipc.rs: cmd_toggle_tray() ──→ delegate to controller.hide() or controller.show()
    │
    ▼
Controller::show()
    │── composer.hide()        → HyprlandComposer: special workspace move
    │── composer.focus()       → hyprctl dispatch (V5 or V4)
    │── composer.move_to_workspace() → hyprctl dispatch
    │── Timer(150ms)
    │       ├── composer.focus()         (re-assert)
    │       └── slint::WindowActiveChanged(true)  (re-enable keyboard nav)
```

## File Changes

| File | Action | Description |
|------|--------|-------------|
| `src/composer/mod.rs` | Create | `Composer` trait (Send + Sync), `HyprlandComposer` driver, `HyprMode` enum, `Controller` struct with owned state |
| `src/composer/hyprland.rs` | Create | V4/V5 dispatch, special workspace logic, `hve_in_special()`, `active_workspace()`, deferred focus timer |
| `src/ipc.rs` | Modify | Remove hyprctl/hide-show internals; replace `WINDOW_HIDDEN`/`TRAY_MODE` atomics with Controller delegate; keep IPC server, debouncer, `get_next_index`, `format_response` |
| `src/countdown.rs` | Modify | Replace 4 direct uses of `crate::ipc::WINDOW_HIDDEN` (lines 26, 88, 158, 261) with Controller state access. CRITICAL for compilation — atomics removed from ipc.rs |
| `src/main.rs` | Modify | Wire `Controller` at composition root; replace `ipc::toggle_float_hve()` / `ipc::focus_hve_window()` calls with controller delegates; remove startup `ipc::` calls |
| `src/tray.rs` | Modify | Tray menu reads/writes controller state instead of `crate::ipc::WINDOW_HIDDEN`; delegate show/hide to controller |

## Interfaces / Contracts

```rust
// src/composer/mod.rs — mirrors ThemeProvider pattern (Send + Sync + Box<dyn>)

#[derive(Clone, Copy, PartialEq)]
pub enum HyprMode {
    V5,
    V4,
    None,
}

pub trait Composer: Send + Sync {
    /// Hide window via special workspace (not Slint hide).
    fn hide(&self, win: &slint::Window) -> bool;

    /// Show window: focus → move_to_workspace → deferred focus assert.
    /// Returns true for fast path (special workspace), false for full remap.
    fn show(&self, win: &slint::Window) -> bool;

    /// Re-assert focus (used by deferred timer and direct calls).
    fn focus(&self);

    /// Toggle floating (for tiling mode).
    fn toggle_float(&self);
}

/// Owned state + driver. Single mutable owner — no atomics leak to callers.
pub struct Controller {
    window_hidden: bool,
    prev_workspace: Option<String>,
    tray_mode: bool,
    composer: Box<dyn Composer>,
}

impl Controller {
    pub fn new(composer: Box<dyn Composer>) -> Self { … }
    pub fn toggle_tray(&mut self, win: &slint::Window) -> bool { … }
    pub fn window_hidden(&self) -> bool { self.window_hidden }
    pub fn set_window_hidden(&mut self, val: bool) { … }
    pub fn composer(&self) -> &dyn Composer { self.composer.as_ref() }
}
```

`HyprlandComposer` implements `Composer` by spawning `hyprctl` — it is the ONLY place in the crate that does so.

## Testing Strategy

| Layer | What to Test | Approach |
|-------|-------------|----------|
| Unit | `Composer` contract — hide sequence | Fake `Composer` records ordered calls; assert hide emits `(record_workspace, move_to_special, toggle_special)` |
| Unit | `Composer` contract — show sequence | Fake `Composer`; assert show emits `(focus, move_to_workspace)` then timer fires `(focus, WindowActiveChanged)` |
| Unit | Controller state transitions | Build `Controller<FakeComposer>`; toggle_tray when hidden → show path; verify `window_hidden` flips |
| Unit | `get_next_index` | Existing tests in `ipc.rs` stay (unchanged logic) |
| Integration | Controller + real `HyprlandComposer` | Only runs live; skipped in CI via `#[cfg(not(test))]` guard on hyprctl spawner |
| Verify | `hyprctl` only in `HyprlandComposer` | `grep -r "hyprctl" src/` — must match only `src/composer/hyprland.rs` |

## Migration / Rollout

No migration required. This is an additive refactoring — the new `src/composer/` module plus a thin `Controller`. The prior `ipc.rs` behavior is preserved. Rollback = revert the commit. The `hve.sock` IPC protocol and `hve-settings` file are untouched.

## Open Questions

### Resolved: `HyprMode` stays internal

`HyprMode` is private to `HyprlandComposer` (detected once at init in `OnceLock`). Fakes don't report mode — the trait surface stays free of Hyprland types. Callers use `Controller`/`Composer`, never the enum.

### Resolved: debounce stays in `ipc.rs`

`LAST_TOGGLE` (400ms) stays as local state in `ipc.rs`. It's a transport concern (IPC command throttling), not composer behavior. `Controller::toggle_tray()` may be called without debounce — callers decide.
