# Delta for lua-only-config

New capability — no archived main spec covers config format. All requirements below are ADDED; each is verifiable by `cargo test` or a deterministic shell assertion.

## ADDED Requirements

### Requirement: No Runtime Format Detection

HVE MUST NOT detect config format at runtime. `hve_format()` and the `~/.cache/hve/hve_format` cache file MUST cease to exist, and no production code path in `src/` MAY read `HVE_FORMAT` or branch on `"lua"` versus `"conf"`.

#### Scenario: Symbols and cache removed

- GIVEN the refactored `src/`
- WHEN a repository scan searches for `hve_format`, `HVE_FORMAT`, and writes to the `hve_format` cache path
- THEN zero production matches remain and no cache file is written

### Requirement: Settings File Is Always `.lua`

`hve_settings_path()` MUST always resolve to `~/.cache/hve/hve-settings.lua`. A stale `hve-settings.conf` MUST NOT be byte-migrated; HVE MUST generate a fresh Lua file and leave the `.conf` orphaned.

#### Scenario: Path is always Lua

- GIVEN any cache state
- WHEN `hve_settings_path()` is called
- THEN the returned path ends with `hve-settings.lua`

#### Scenario: Stale `.conf` is not migrated

- GIVEN `~/.cache/hve/hve-settings.conf` exists with conf-syntax content
- WHEN `ensure_settings_file()` runs
- THEN `hve-settings.lua` is created
- AND no byte from the `.conf` is copied into it

### Requirement: Emitted Content Is Lua Syntax

All emitted settings content MUST be Lua. `set_keybinds(true)` MUST emit `hl.bind("SUPER + H", hl.dsp.exec_cmd("hve-ipc toggle-tray"))`; `set_autostart(true)` MUST emit an `hl.on("hyprland.start", …)` block; the window-rules block MUST stay empty/commented (the Composer owns window state). Conf syntax MUST never be written.

#### Scenario: Lua keybind and autostart emitted

- GIVEN a fresh settings file with keybinds and autostart enabled
- WHEN `set_keybinds(true)` and `set_autostart(true)` run
- THEN the file contains `hl.bind(` and `hl.on("hyprland.start"`
- AND it never contains `bind = SUPER, H, exec,` or `exec-once =`

#### Scenario: Window-rules block stays empty

- GIVEN the settings file after `set_tiling_window_rules(false)`
- WHEN its window-rules block is inspected
- THEN the block contains only comments, no rule lines

### Requirement: Markers Are Lua Comments

All three marker pairs (window rules, keybinds, autostart) MUST use `-- >>> … <<<` syntax. HVE MUST NOT emit any `#` marker.

#### Scenario: Comment prefix

- GIVEN generated settings content
- WHEN marker lines are scanned
- THEN every marker starts with `--`
- AND no `# >>>` marker is present

### Requirement: Idempotent Writes

Re-applying identical settings MUST NOT rewrite the file nor fire `hyprctl reload` (existing steady-state guard preserved).

#### Scenario: Same-state apply is a no-op

- GIVEN keybinds already emitted
- WHEN `set_keybinds(true)` runs again
- THEN the file mtime is unchanged and no reload fires

### Requirement: Migration Guard Fires Only For Conf-Only Systems

HVE MUST expose a pure, testable guard predicate over (`hyprland.lua` content, `hyprland.conf` existence). A present-and-valid `hyprland.lua` (containing `hl.` or `require(`) MUST be treated as a Lua user and MUST NOT fire the guard, even when `hyprland.conf` also exists. The guard MUST fire only when `hyprland.lua` is absent or present-but-not-valid-Lua AND `hyprland.conf` exists.

#### Scenario: Guard decision matrix

- GIVEN the four filesystem states below
- WHEN the predicate evaluates
- THEN it returns the listed decision

| `hyprland.lua` | `hyprland.conf` | Guard fires |
|---|---|---|
| valid Lua | present | no |
| absent | present | yes |
| absent | absent | no |
| invalid stub | present | yes |

### Requirement: Guard Also Requires The HVE Block

When `hyprland.lua` is valid Lua but lacks the HVE marker block `-- >>> HYPRLAND VISUAL EDITOR START <<<`, the guard MUST signal prompt-to-enable rather than silently treating the system as ready.

#### Scenario: Valid Lua without the HVE block

- GIVEN valid `hyprland.lua` with no HVE marker block
- WHEN the guard runs
- THEN the result is prompt-to-enable, not a silent ready/apply

### Requirement: Guard Blocks Mutations, Never Silent

When the guard fires, mutating paths (preset apply, `init.sh enable`) MUST be refused and a bilingual (ES/EN) message surfaced. The window MUST still open.

#### Scenario: Preset apply refused

- GIVEN a guard-fired state
- WHEN a preset apply is requested
- THEN the mutation is refused and the bilingual message is returned
- AND the window still opens

#### Scenario: `init.sh enable` refused

- GIVEN a guard-fired state
- WHEN `init.sh enable` runs
- THEN it exits without injecting any Lua block and prints the ES/EN message

### Requirement: Assets Are Lua-Only

The 32 `.conf` twins (14 borders + 18 animations) MUST be deleted; every `.lua` twin MUST remain; the Lua-only `19_stylized2.5D.lua` MUST be untouched.

#### Scenario: Asset counts

- GIVEN the refactored assets
- WHEN files are counted
- THEN `assets/borders/` holds 14 `.lua` and 0 `.conf`
- AND `assets/animations/` holds 19 `.lua` and 0 `.conf`, including `19_stylized2.5D.lua`

### Requirement: Scripts Resolve Lua Only

`hve_resolve_preset` MUST resolve only `$dir/$name.lua`; `assemble.sh` MUST target `overlay.lua`; `scan.sh` MUST list only `*.lua`; `detect_format.sh` and `format_test.sh` MUST no longer exist.

#### Scenario: Script assertions

- GIVEN `assets/scripts/`
- WHEN the resolver, assembler, and scanner are inspected
- THEN no `HVE_FORMAT`, conf fallback, or `*.conf` glob remains
- AND `detect_format.sh` and `format_test.sh` are absent

### Requirement: Tests Carry No `.conf` Fixtures

The conf-locked tests in `settings.rs` and `providers/hyprland_settings.rs` MUST be rewritten to Lua fixtures, and `cargo test` MUST pass with zero conf-syntax fixtures.

#### Scenario: Suite green with Lua fixtures

- GIVEN the refactored suite
- WHEN `cargo test` runs
- THEN it passes
- AND fixture scans find no `# >>>` markers and no `bind = `/`exec-once =` syntax
