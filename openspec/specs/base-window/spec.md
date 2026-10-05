# Base Window Specification

## Purpose

Defines HVE's single-window shell: one large base window that mutates instead of stacking, grows via `set_size`, enforces a minimum size, and stays consistent with the existing `Composer` lifecycle. Engine and composer contracts are untouched; existing tests stay green.

## Requirements

### Requirement: Single Large Base Window

The system SHALL open exactly one window at startup. The window MUST start at a large base size and MUST NOT create additional windows during navigation; all screens render inside the same window.

#### Scenario: Startup

- GIVEN the application launches
- WHEN the shell initializes
- THEN exactly one window exists at the large base size

#### Scenario: No stacked windows

- GIVEN an expansion to Gallery
- WHEN the transition completes
- THEN the same single window shows the Gallery slot and no new window was created

### Requirement: Window Growth via set_size

The shell SHALL grow the window to the target size via `set_size` when a slot expands and shrink it back on collapse. Size changes SHALL be animated and MUST NOT drop below the minimum size.

#### Scenario: Expand grows the window

- GIVEN the window at base size
- WHEN a slot expands
- THEN `set_size` grows the window to the expanded target size

#### Scenario: Collapse shrinks the window

- GIVEN an expanded slot
- WHEN the slot collapses
- THEN the window returns to the base size

#### Scenario: Target below minimum (Edge Case)

- GIVEN an expansion target smaller than the minimum size
- WHEN the transition computes the target
- THEN the target is clamped to the minimum size

### Requirement: Minimum Size Enforcement

The window MUST NOT shrink below the configured minimum size on any axis, whether by user resize or programmatic `set_size`.

#### Scenario: User drag below minimum

- GIVEN the user drags a window edge toward a smaller size
- WHEN the drag would go below the minimum
- THEN the window is clamped at the minimum size

#### Scenario: Programmatic resize below minimum

- GIVEN a `set_size` call below the minimum
- WHEN the size is applied
- THEN the window ends at the minimum size, not below

### Requirement: Composer Lifecycle Wiring

The shell SHALL wire the existing `Composer` lifecycle (show/hide/focus) to the base window without modifying the `Composer` trait. When the composer hides or shows the window, shell navigation state MUST be preserved and restored.

#### Scenario: Hide preserves state

- GIVEN an expanded slot while the composer hides the window
- WHEN the window is re-shown
- THEN the same screen and expansion state are restored

#### Scenario: Focus restored on show

- GIVEN the window is re-shown by the composer
- WHEN the show completes
- THEN keyboard navigation is re-enabled for the restored screen