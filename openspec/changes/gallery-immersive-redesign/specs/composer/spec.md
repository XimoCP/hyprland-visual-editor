# Delta for composer

Existing capability (`openspec/specs/composer/spec.md`). All existing requirements (trait interface, show/hide state management, workspace discovery, V4/V5 dispatch branches, Controller-owned state) remain unchanged. This delta only adds fullscreen lifecycle control.

## ADDED Requirements

### Requirement: Fullscreen Gallery Session

The `Composer` trait SHALL expose fullscreen control without leaking compositor types. Opening the Gallery MUST switch HVE to an undecorated fullscreen surface covering the target monitor before content shows. Closing the Gallery MUST restore the previous floating geometry and float mode. At startup, the system MUST run a sanity check that detects a stuck fullscreen state left by a crash and repairs it to floating.
(Reason: immersive skwd-wall presentation requires fullscreen-over-desktop launch; previously HVE only floated/hid/showed as a decorated window.)

#### Scenario: Enter fullscreen on open

- GIVEN HVE floating and the user expands Gallery
- WHEN the Gallery slot mounts
- THEN Composer enters undecorated fullscreen before content displays

#### Scenario: Restore floating on close

- GIVEN a fullscreen Gallery session active
- WHEN the user closes the Gallery
- THEN previous floating geometry and float mode are restored

#### Scenario: Startup sanity repair

- GIVEN a crash left HVE stuck fullscreen
- WHEN HVE starts
- THEN the sanity check restores the floating state before first show
