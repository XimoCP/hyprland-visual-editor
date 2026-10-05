# Nav Shell Specification

## Purpose

Defines HVE's state-driven navigation shell: one Rust-owned navigation state, slot-based mounting for future modules, expansion transitions, keyboard+mouse input, and ES/EN shell strings. The UI is a read-only view; panel clip-swapping is removed.

## Requirements

### Requirement: Navigation State Model

The shell SHALL keep all navigation state in a single Rust-owned owner composed of a screen enum (`Home`, `Gallery`, `Workshop`) and an `ExpansionState` (`Collapsed` | `Expanded(target)`). The UI MUST render from this state and MUST NOT hold navigation state of its own.

#### Scenario: Initial state

- GIVEN the shell starts
- WHEN the state owner initializes
- THEN the screen is `Home` and `ExpansionState` is `Collapsed`

#### Scenario: UI is a read-only view

- GIVEN the state owner reports `Expanded(Gallery)`
- WHEN the UI renders
- THEN the UI shows only the Gallery slot

### Requirement: Expansion Transition

The shell SHALL transition `ExpansionState` to `Expanded(target)` on activation, drive window growth via `set_size`, and mount the target slot. Transitions MUST be deterministic and unit-testable headless (i-slint-backend-testing). Rapid nav commands MUST be queued and processed in order so the final state matches the last command.

#### Scenario: Happy path expansion

- GIVEN a card on Home is activated
- WHEN the transition runs
- THEN `ExpansionState` becomes `Expanded(Gallery)`, the window grows, and the Gallery slot mounts

#### Scenario: Rapid navigation queueing (Edge Case)

- GIVEN two nav commands arrive before the first transition completes
- WHEN the queue processes them in order
- THEN the final state matches the last command

#### Scenario: No-op on same target

- GIVEN `Expanded(Gallery)` is active
- WHEN a nav command targets Gallery again
- THEN no transition or size change occurs

### Requirement: Slot Mounting Contract

The shell SHALL mount and unmount module slots behind a trait contract. Mounting SHALL show the slot view; unmounting SHALL remove it. The contract MUST require no engine or composer changes, and registering a module MUST NOT change core navigation logic. Trunk tests MAY register stub slots.

#### Scenario: Mount a stub slot

- GIVEN a registered stub slot
- WHEN the shell mounts it
- THEN the slot view is shown and its mount hook fires

#### Scenario: Unmount an active slot

- GIVEN a mounted slot
- WHEN the shell unmounts it
- THEN the slot view is removed and its unmount hook fires

#### Scenario: Unknown target (Edge Case)

- GIVEN a nav command targets an unregistered slot
- WHEN the command is processed
- THEN the shell stays on the current screen

### Requirement: Keyboard and Mouse Navigation

The shell SHALL accept both keyboard and mouse input for shell-level actions (activate, expand, back/collapse); keyboard activation MUST equal mouse activation.

#### Scenario: Mouse activation

- GIVEN the Home screen
- WHEN the user clicks a card
- THEN the expansion transition for that target runs

#### Scenario: Keyboard activation

- GIVEN keyboard focus on the same card
- WHEN the user presses the activation key
- THEN the same expansion transition runs

#### Scenario: Keyboard back/collapse

- GIVEN `Expanded(Gallery)`
- WHEN the user presses back/collapse
- THEN `ExpansionState` returns to `Collapsed` and the window shrinks to base size

### Requirement: Shell i18n (ES/EN)

The shell SHALL provide all chrome strings (titles, nav labels, back affordance) in Spanish and English, reusing the existing `tr.rs` mechanism (embedded JSON, locale detection at startup, English fallback). The active language MUST determine the rendered strings.

#### Scenario: Spanish shell

- GIVEN the startup locale is Spanish
- WHEN the shell renders chrome
- THEN all chrome strings are Spanish

#### Scenario: English shell

- GIVEN the startup locale is English
- WHEN the shell renders chrome
- THEN all chrome strings are English

#### Scenario: Unknown locale fallback (Edge Case)

- GIVEN a locale other than `es`
- WHEN the shell resolves a chrome string
- THEN the English string is used