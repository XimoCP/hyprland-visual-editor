# Composer Specification

## Purpose

This specification defines the `Composer` trait and the contract for compositor interactions in HVE. It abstracts the underlying compositor implementation (e.g., Hyprland) to allow deterministic testing of navigation and window state transitions.

## Requirements

### Requirement: Composer Trait Interface

The `Composer` trait SHALL provide the following operations to manage window visibility, focus, and workspace state.

#### Scenario: Successful workspace transition

- GIVEN a `Composer` instance
- WHEN `move_to_workspace(id)` is called
- THEN the current workspace is updated to `id`

#### Scenario: Toggle floating state

- GIVEN a window is currently tiled
- WHEN `toggle_float(window_id)` is called
- THEN the window's floating state is toggled

### Requirement: Window State Management (Show/Hide)

The `Composer` MUST handle the transition of special workspaces and window focus to ensure keyboard navigation remains consistent.

#### Scenario: Hiding a special workspace

- GIVEN a special workspace is active
- WHEN `hide_special_workspace()` is called
- THEN the special workspace is closed
- AND the previous workspace is re-asserted

#### Scenario: Showing a special workspace from a different window

- GIVEN a window is focused in the main workspace
- WHEN `show_special_workspace()` is called
- THEN the special workspace becomes visible
- AND the focus is re-asserted to the window that was active before the workspace change

#### Scenario: Deferred focus re-assertion (Edge Case)

- GIVEN a window is being re-shown via `show_special_workspace()`
- WHEN the composer transition completes
- THEN the system MUST re-assert focus to the previously active window after the internal timer (approx. 150ms) expires

### Requirement: Workspace and Window Discovery

The `Composer` SHALL provide read-only access to the current state of the compositor.

#### Scenario: Get current workspace

- GIVEN the compositor is on workspace 2
- WHEN `current_workspace()` is called
- THEN the returned value MUST be 2

#### Scenario: Handle invalid workspace request

- GIVEN the compositor is on workspace 2
- WHEN `move_to_workspace(999)` is called (where 999 does not exist)
- THEN the system SHOULD return an error or ignore the request without crashing
