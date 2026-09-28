# Capability Routing Specification

## Purpose

Defines how HVE stays adaptable without force-unifying configuration
systems. The core states *what* it wants as a capability; a router sends
each capability to an independent backend. A backend never calls another
backend. Hyprland is the base HVE speaks natively, not an interchangeable
backend.

Each statement below is labelled. **Decided contract (the keeper's)** is
the rule every future change must respect. **Known debt** is today's
measured violation of that rule, pinned by `src/architecture_contract.rs`
and removed phase by phase (`odd/tasks/hve-capability-routing.md`).
**Open question** is undecided: do not invent an answer in code, ask first.

## Requirements

### Requirement: Hyprland Is the Base, Never a Backend (decided contract)

HVE is the Hyprland Visual Editor. Hyprland-specific knowledge MUST live
inside one module (`src/composer/hyprland.rs` plus the Hyprland-owned
guard/markers) and MUST NOT be scattered through core files. The
`Composer` trait encapsulates Hyprland; it does not make the compositor
interchangeable.

#### Scenario: Raw compositor calls leave core files

- GIVEN a core file outside the Hyprland module issues a compositor call
- WHEN the architecture contract test scans it
- THEN the test fails until the call moves behind `Composer`

**Known debt**: raw `hyprctl` still lives outside `Composer` in
`src/main.rs`, `src/settings.rs` and `src/providers/shell.rs`;
`src/hypr_ipc.rs` (event socket) and `src/config_guard.rs` (reads
`hyprland.lua` to gate mutations) are Hyprland-native but unnamed. Phase 3
and Phase 4 re-home them; the pins count every occurrence until then.

### Requirement: Backends Are Siblings, Never a Chain (decided contract)

A backend MUST NOT call, wait for, or configure another backend. Anything
that needs two backends is a capability the core routes, not a call
between them. Bidirectional calls between backends are forbidden.

#### Scenario: One backend reaches into another

- GIVEN backend A invokes, waits for, or writes configuration owned by
  backend B
- WHEN this is found in review or by a phase audit
- THEN it is treated as a defect: the shared need becomes a routed
  capability and the direct call is deleted

**Known debt**: `providers/noctalia.rs` calls the skwd engine and drives
mpvpaper while `providers/mpvpaper.rs` calls back into Noctalia
(bidirectional); `color_watcher.sh` runs `noctalia templates-apply` into
`assemble.sh` into the reload queue into `hyprctl reload`, four adapters
in sequence. Phase 2 breaks these chains.

### Requirement: Capability Vocabulary (decided contract)

The core may only ask for these capabilities, each with fixed semantics.
The "today" column is known debt, not design: it records who implements
each capability now, so phases know what to re-home.

- `window-state` — show, hide, focus, fullscreen and tray transitions of
  the HVE window. Today: `Composer` (`HyprlandComposer`); the one
  capability already flowing through its seam end to end.
- `reload-config` — re-read the compositor configuration after files
  changed. Today: raw `hyprctl reload` in core files plus the
  `assemble.sh` / reload-coalescer queue.
- `read-config-option` — query one live compositor option value. Today:
  raw `hyprctl getoption` and workspace queries in `src/main.rs`.
- `write-config-option` — change one live compositor option value. Today:
  raw `hyprctl dispatch` variants in `src/main.rs`.
- `apply-border` — write the border preset fragment and reload. Today:
  `assets/scripts/border.sh` via the engine.
- `apply-animation` — write the animation preset fragment and reload.
  Today: `assets/scripts/apply_animation.sh` via the engine.
- `apply-shader` — write the shader fragment and reload. Today:
  `assets/scripts/shader.sh` via the engine.
- `apply-geometry` — write gaps/border-size geometry and reload. Today:
  `assets/scripts/geometry.sh` via the engine.
- `set-colours` — resolve the active palette and paint every owned
  surface. Today: the `colors.sh` five-source chain plus the provider-side
  re-assert in `providers/noctalia.rs`.
- `apply-background` — paint the theme's wallpaper through the selected
  engine. Today: the Noctalia / mpvpaper / skwd delegation chain (see the
  siblings rule above).
- `read-desktop-preference` — read a system-wide desktop preference such
  as the light/dark theme. Today: `gsettings` / darkman calls in
  `src/theme.rs`.
- `yield-to-another-app` — hand control to another running app that owns
  part of the desktop. Today: dead code (`pgrep hyprmod` in
  `shell/gallery/slot.rs`).
- `read-preview-source` — provide the files a theme preview renders from.
  Today: hardcoded provider layouts in `src/shell/gallery/thumbs.rs`.

#### Scenario: Core wants something outside the vocabulary

- GIVEN the core needs a new integration behaviour with no matching
  capability
- WHEN the change is proposed
- THEN the capability is added to this spec first, with its semantics and
  its initial backend, before any code routes it

### Requirement: Backend Contract (decided contract)

Every backend is declarative: it states what it is and owns, and the core
plus the scripts only read those declarations. No commands live in files
the router reads; a declaration never executes.

A backend declares:

- `id` — its stable name (for example `noctalia-v5`).
- `capabilities` — the subset of the vocabulary above it implements.
- `files-owned` — the live config paths it manages, and the saved theme
  paths it restores from.
- `watch-paths` — the paths it wants watched for external changes; the
  central watcher reads this list instead of naming integrations.
- `deletable-artefacts` — what the core may delete when a theme that used
  this backend is removed.
- `refresh` — its re-assert / reload action, executed by the backend
  itself (Rust provider code or the backend's own script module), never by
  another backend's CLI inside a central script.

Colour authority is declared through a small descriptor file the
provider writes and the central scripts only read (which palette file,
which palette name, which backend).

#### Scenario: New shell arrives as files, not core edits

- GIVEN a new shell backend with its declaration, its colour module and
  its watch paths under its own directory
- WHEN it is registered
- THEN no core file and no central script changes behaviour for existing
  backends

**Known debt**: `colors.sh` hardcodes a five-source chain with cross-fill
paths (Noctalia v4 tertiary filling v5, kitty palette filling Noctalia)
and `color_watcher.sh` hardcodes every watch path. Phase 1 converts the
chain into a loader over backend colour modules and moves the watch paths
into backend declarations.

### Requirement: Definition of Done (decided contract)

Supporting a new shell or tool MUST require adding files under that
backend's own directory and at most one registry entry — never editing
`src/main.rs`, `src/settings.rs`, `src/theme_manager.rs`,
`src/shell/gallery/thumbs.rs`, `assets/scripts/colors.sh` or
`assets/scripts/color_watcher.sh`. A change that edits any of these files
to add a backend is rejected in review regardless of tests.

#### Scenario: Registry-only registration

- GIVEN a backend implemented entirely under its own directory
- WHEN it is added with a single registry entry
- THEN all existing tests pass unchanged and the architecture pins do not
  grow

### Requirement: Enforcement by Pinned Architecture Test (decided contract)

`src/architecture_contract.rs` counts today's couplings per core file
(integration tokens per file, production code only) and fails when any
count grows (new coupling) or when a pin exceeds its count (stale pin —
lower it). Each phase of `odd/tasks/hve-capability-routing.md` lowers
the pins until zero. Pins are measured by running the scanner, never
copied from documents.

#### Scenario: A new coupling leaks into a core file

- GIVEN a core file gains one more occurrence of a pinned token
- WHEN `cargo test architecture_contract` runs
- THEN it fails naming the file, the token, the actual count and the pin

#### Scenario: A phase removes a coupling

- GIVEN a re-homed coupling drops a file's count below its pin
- WHEN the suite runs
- THEN it fails as a stale pin until the author lowers the pin to the
  measured value

## Open Questions

- Where does the backend registry live (Rust registry module, a
  declarations directory on disk, or both), and what exactly counts as
  the single registry entry?
- What is the exact schema of the colour-authority descriptor file
  (field names, palette-name whitelist, failure mode when it is absent)?
- Does `disabled_providers` become the backend on/off switch the
  registry consumes (Phase 2 intent), or a separate mechanism?

## Out of Scope

- Rewriting the engine (`src/engine.rs`, `config.rs`, `settings.rs`,
  `theme_manager.rs`, `app_state.rs`, `watcher.rs`, `utils.rs`,
  `src/providers/` are reused, not reimplemented).
- Monitors, HDR and display layout (belongs to hyprmod).
- Making HVE compositor-agnostic (Hyprland is the base by design).
- Correcting `openspec/specs/composer/spec.md`'s framing of Composer as
  "abstracts the compositor implementation" (owned by Phase 0 alongside
  this spec, not by this spec).
