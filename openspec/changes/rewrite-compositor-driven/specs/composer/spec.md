# Delta for Composer

## MODIFIED Requirements

### Requirement: Composer Trait Interface

The `Composer` trait SHALL be `Send + Sync` and MUST manage window visibility, focus, and workspace state without leaking Hyprland types. It defines `hide(win) -> bool`, `show(win) -> bool`, `focus()`, and `toggle_float()`. (Previously: exposed only `move_to_workspace`/`toggle_float`.)

The trait MUST NOT expose `HyprMode` or compositor-specific types; mode detection stays internal.

#### Scenario: Successful show operation

- GIVEN a `Composer` instance bound to a window
- WHEN `show(win)` is called while HVE is on a special workspace
- THEN the fast path returns `true`

#### Scenario: Successful hide operation

- GIVEN a `Composer` instance with an active window
- WHEN `hide(win)` is called
- THEN the window moves to the special workspace and `true` is returned

#### Scenario: Toggle floating state

- GIVEN a window is currently tiled
- WHEN `toggle_float()` is called
- THEN the window's floating state is toggled

### Requirement: Window State Management (Show/Hide)

The `Composer` MUST handle special-workspace transitions and window focus so keyboard navigation stays consistent, including a deferred focus re-assertion after show. (Previously: named `show_special_workspace`/`hide_special_workspace`.)

#### Scenario: Hiding via special workspace

- GIVEN the compositor is on a main workspace
- WHEN `hide(win)` is called
- THEN the previous workspace is recorded and the window moves to `special:minimized`
- AND `toggle_special` closes the special workspace
- AND focus is returned to the main workspace

#### Scenario: Showing a window from another window

- GIVEN a window is focused in the main workspace
- WHEN `show(win)` is called
- THEN focus is asserted, the window moves to its recorded workspace
- AND `WindowActiveChanged(true)` is emitted to re-enable keyboard navigation

#### Scenario: Deferred focus re-assertion (Edge Case)

- GIVEN a window is being re-shown via `show(win)`
- WHEN the composer transition completes
- THEN focus MUST be re-asserted to the previously active window after ~150ms

### Requirement: Workspace and Window Discovery

The `Composer` SHALL provide read-only discovery via `active_workspace()` and `hve_in_special()`. (Previously: `current_workspace` returned a numeric id with no special-workspace membership query.)

#### Scenario: Get current workspace name

- GIVEN the Hyprland composer is on workspace named "2"
- WHEN `active_workspace()` is called
- THEN the returned value MUST be "2"

#### Scenario: Detecting HVE inside a special workspace

- GIVEN the HVE client is in a `special:minimized` workspace
- WHEN `hve_in_special()` is called
- THEN the returned value MUST be true

#### Scenario: Handle invalid workspace request

- GIVEN the compositor is on workspace "2"
- WHEN a move to workspace "999" (which does not exist) is dispatched
- THEN the system SHOULD return an error or ignore the request without crashing

## ADDED Requirements

### Requirement: V4/V5 Dispatch Branches

The composer implementation SHALL detect the Hyprland version once at init and branch every `hyprctl` dispatch on it. Detection MUST prefer V5 (`hl.dsp` no-op probe), fall back to V4 (`hyprctl version`), and cache the result exactly once.

- GIVEN Hyprland V5 is detected at init
- WHEN a dispatch is issued
- THEN the V5 dispatch path (`hl.dsp.*`) is used

- GIVEN Hyprland V4 is detected at init
- WHEN a dispatch is issued
- THEN the V4 `hyprctl dispatch` argument form is used

- GIVEN neither probe succeeds
- WHEN a dispatch is attempted
- THEN the composer returns an error without crashing

### Requirement: Controller Owned State

The system MUST encapsulate mutable state in a `Controller` struct holding `window_hidden: bool`, `tray_mode: bool`, `prev_workspace: Option<String>`, and a `Box<dyn Composer>`. Callers MUST interact with composer state exclusively through the `Controller`.

- GIVEN a `Controller` backed by a fake `Composer`
- WHEN `toggle_tray(win)` is called while the window is not hidden
- THEN the hide path runs and `window_hidden` flips to true

- GIVEN a `Controller` where the window is hidden
- WHEN `toggle_tray(win)` is called again
- THEN the show path runs and `window_hidden` flips to false
- AND `prev_workspace` is retained for restoration