# Design: Drop `.conf` Support (Lua-only)

## Technical Approach

Collapse format handling to Lua, retaining writes and Composer state. Add a pure Rust decision function plus adapters. This is the negotiated HVE2 sealed-engine exception: config/settings/provider seams only.

## Architecture Decisions

| Decision | Choice | Alternatives / rationale |
|---|---|---|
| Guard API | New `src/config_guard.rs`; pure `evaluate_config_guard(Option<&str>, bool)`. | Reject filesystem-coupled tests; preserve one exact R6/R7 matrix. |
| Markers | New `src/config_markers.rs` owns six Lua settings constants and the HVE pair; settings/provider import them. | Reject duplicated literals and provider format state; shell mirrors literals across its process boundary. |
| Legacy settings | Always return `hve-settings.lua`; rename old `.lua` once, never copy `.conf`. `ensure_settings_file()` fresh-generates Lua and leaves stale `.conf` orphaned. | Byte migration could poison Lua; settings are regenerable (D4). |
| Mutation gate | `LuaReady` permits; `PromptToEnable` blocks applies but permits `init enable`; `ConfOnly` blocks both; `NothingToDo` preserves behavior. | Enable is remediation for valid Lua without HVE; conf-only must never receive ignored writes. |
| UI exception | Retain `MainWindow`/`shell-status-text`; add bilingual `shell.lua_migration` and `shell.lua_enable`. | No new Slint surface; existing window still opens. |

## Data Flow

`main()` path adapter → pure guard → status/policy → existing callbacks

`init.sh enable` → `utils.sh` shell guard → Lua-only setup/injection → reload

## File Changes

| File | Action | Description |
|---|---|---|
| `src/config_guard.rs` | Create | Decision enum, pure predicate, path adapter. |
| `src/config_markers.rs` | Create | Shared Lua settings/HVE marker constants. |
| `src/config.rs` | Modify | Remove `hve_format()`/cache; Lua path and legacy handling. |
| `src/settings.rs` | Modify | Lua template/constants/signatures/fixtures. |
| `src/providers/hyprland_settings.rs` | Modify | Remove format state/branch; use constants; Lua fixtures. |
| `src/main.rs` | Modify | Evaluate before `ensure_settings_file()` 895; status 912; gate gallery apply 1480–1510, saved apply 2247–2255, engine callbacks 2522–2628, startup writes 2663–2699. No early return: window opens. |
| `i18n/{en,es}.json` | Modify | Bilingual migration and enable messages. |
| `assets/scripts/utils.sh:34–73` | Modify | Resolve only `$dir/$name.lua`; retain path helpers. |
| `init.sh:7–171` | Modify | Remove detection/CONF injection; Lua setup only; retain `clean_hyprland_conf` 52–58 and preserve `HVE_COLORS_BASE` block without format branch; guard before `setup_files` 129. |
| `assemble.sh:9–150` | Modify | One `overlay.lua`, Lua emission/validation, symlink/reload; remove CONF arms. |
| `scan.sh:17–64` | Modify | Remove detection/dedup; list only Lua. |
| `border.sh:7–59` | Modify | Lua fragment/resolver/fallback; remove cache/opposite cleanup. |
| `apply_animation.sh:7–61` | Modify | Lua fragment/resolver/fallback; remove cache/opposite cleanup. |
| `shader.sh:7–100` | Modify | Lua wrapper/target; retain preset-name security. |
| `geometry.sh:9–127` | Modify | Lua fragment; retain numeric validation. |
| `colors.sh:50–61,240–344` | Modify | Remove conf parser/call sites; retain Lua/format-agnostic sources. |
| `color_watcher.sh:30–48,89–94,183–200` | Modify | Remove `.conf` watches and rendered-file entries. |
| `detect_format.sh`, `format_test.sh` | Delete | Obsolete detector/cache and dead transpiler test. |
| `get_colors.sh` | Keep | No format handling. |
| `hve_watchdog.sh:9–24` | Keep | Conf and Lua cleanup for one release. |
| `install.sh:397–401` | Modify | Add preflight before build; correct stale comments; do not change uninstall. |
| `assets/borders/*.conf` (14), `assets/animations/*01–18*.conf` (18) | Delete | Retain every `.lua`, including untouched `19_stylized2.5D.lua`. |

## Interfaces / Contracts

```rust
pub(crate) enum ConfigGuardDecision { LuaReady, PromptToEnable, ConfOnly, NothingToDo }
pub(crate) fn evaluate_config_guard(lua: Option<&str>, conf_exists: bool)
    -> ConfigGuardDecision;
```

Validity is `hl.` or `require(`. Matrix: valid Lua + HVE start marker → `LuaReady`; valid Lua without marker → `PromptToEnable`; absent/invalid Lua + conf → `ConfOnly`; absent/invalid Lua without conf → `NothingToDo`. Marker: `-- >>> HYPRLAND VISUAL EDITOR START <<<`.

## Testing Strategy

1. RED first: rewrite settings fixtures `test_set_keybinds_noop_when_disabled_and_markers_absent` 580–598 and `marker_block_needs_update_detects_changes` 700–719, plus provider fixtures 229–317, to Lua with no-conf assertions.
2. Add RED guard tests for four matrix rows plus prompt/ready; add stale settings path tests. Implement modules, then Lua production.
3. Add deterministic shell assertions for resolver, absent scripts, Lua globs, init refusal/injection, and install preflight; simplify scripts and delete assets.
4. Run `cargo test`; verify R8 via `cargo test slice_focus_flow_renders`, then read `/tmp/opencode/slice_*.png` to confirm the status-bearing window renders/opens. No new dependency.

## Threat Matrix

| Boundary | Applicability | Design response / RED tests |
|---|---|---|
| Documentation-like paths | N/A — no executable-file classification change. | None |
| Git repository selection | N/A — no Git automation. | None |
| Commit state | N/A — no commit automation. | None |
| Push state | N/A — no push automation. | None |
| PR commands | N/A — no PR automation. | None |

## Migration / Rollout

No data migration. Generate fresh Lua settings, leave stale `.conf` files untouched, and document re-saving old theme settings. No feature flag; rollback is `git revert`. Deferred items remain deferred: `state.json` tooling, `HVE_COLORS_BASE` removal, `HyprMode::V4`, post-release conf cleanup, and `uninstall.sh` marker cleanup.

## Open Questions

None.
