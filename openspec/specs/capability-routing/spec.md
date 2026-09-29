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
`src/providers/shell.rs` (1× — its declared `reload_command`, no
production caller) and `src/providers/wallpaper_authority.rs` (2× —
read-only `hyprctl -j layers` / `-j monitors` queries);
`src/hypr_ipc.rs` (event socket) and `src/config_guard.rs` (reads
`hyprland.lua` to gate mutations) are Hyprland-native but unnamed.
`src/main.rs` and `src/settings.rs` measure 0 since Phase 3 (the
settings reload goes through `Composer::reload_config`); Phase 4
re-homes what remains, and the pins count every occurrence until then.

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

**Closed in Phase 2 and after**: `providers/noctalia.rs` no longer calls the
skwd engine adapter (`providers/skwd_engine.rs`) nor the mpvpaper plugin API
(`providers/mpvpaper.rs`); `providers/mpvpaper.rs` no longer calls back into
Noctalia — its supervisor IPC moved to the neutral
`providers/noctalia_runtime.rs` seam. `apply-background` is routed by
`providers/background.rs`, which the Noctalia provider invokes as its backend
role rather than calling its siblings, and the colour-authority yield (the
engine-config flip that holds the skwd engine off while another backend
applies colours) moved behind that same router: `background::hold_color_authority`
/ `ColourAuthorityHold` / `release_color_authority`. Noctalia names the
mechanism no longer; the router delegates to `providers/skwd_policy.rs`,
which remains the owner of the engine's own config file.

### Requirement: Capability Vocabulary (decided contract)

The core may only ask for these capabilities, each with fixed semantics.
The "today" column is known debt, not design: it records who implements
each capability now, so phases know what to re-home.

- `window-state` — show, hide, focus, fullscreen and tray transitions of
  the HVE window. Today: `Composer` (`HyprlandComposer`); the one
  capability already flowing through its seam end to end.
- `reload-config` — re-read the compositor configuration after files
  changed. Today: the app side through `Composer::reload_config`
  (`src/settings.rs`); the script side through `assemble.sh`, which
  queues `hve_reload_queue` and lets `reload_coalescer.sh`'s
  single-owner drainer fire the one reload per change burst; plus
  `init.sh` enable/disable's one-shot structural reload.
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

**Not a chain — decided 2026-09-29** (read from the code, pinned by
`colour_pipeline_reloads_only_through_the_coalescer`): the sequence
`color_watcher.sh` / an apply script → `assemble.sh` →
`reload_coalescer.sh` → `hyprctl reload` was recorded as "four adapters
in a row". The RELOAD suffix holds no backend: `assemble.sh` writes HVE's
own overlay, the coalescer is that pipeline's own helper, and `hyprctl`
talks to the base — so "a backend never calls another backend" cannot
apply to it, and there is nothing for a router to re-home. The watcher
pass around it DOES execute a backend — each colour module's own declared
refresh (unit 1d4), which is the core asking a backend to do its own work,
the permitted direction. The coalescer
stays the script side's sanctioned seam: its marker file plus
single-owner drainer are what make a change burst cost at most one
reload with none lost, a guarantee an IPC hop through the app cannot
make while the socket is down — and the app already learns of colour
changes separately (`_write_color_signal` plus `hve-ipc
refresh-theme`), which repaints HVE's UI and never was the reload
path. The one genuinely chained step this sequence once held — a
backend's CLI run from inside the watcher — left `color_watcher.sh`
when each declared refresh moved into its own colour module (unit
1d4). Genuine debt left under this capability: the raw fires in
`init.sh` enable/disable (one-shot, outside any burst) and the
uncalled `reload_command` in `src/providers/shell.rs`.

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

**Closed in Phase 1**: `colors.sh` is a loader over the colour modules in
`assets/scripts/color_sources.d/` (priority 10 stays inline as the
applied-theme snapshot), the cross-fill paths are gone, and
`color_watcher.sh` reads the watch paths each module declares instead of
naming sources. What remains is the priority-10 step's Noctalia-shaped
palette knowledge (`dark` / `mPrimary` — residual R7) and the empty-list
fallback that still names `manual_hypr.sh` by id.

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
