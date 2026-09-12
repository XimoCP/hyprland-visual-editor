# Proposal: Drop `.conf` Support (Lua-only)

## Intent

Hyprland defaults to Lua (0.55+); `.conf` is deprecated. HVE still carries a
dual-format matrix across Rust and 14 shell scripts. **Why now:** `hve_format()`
defaults to `conf` when its cache is missing, and its process-wide `OnceLock`
forces the whole `cargo test` suite into conf mode today — removing it kills the
top silent-breakage and test-hazard vector.

## Scope — In

- `src/config.rs`: delete `hve_format()`; `hve_settings_path()` → always
  `hve-settings.lua` (+ one-way migration).
- `src/settings.rs`: 5 functions Lua-only, shared markers; update 2 conf-locked
  tests + comments.
- `src/providers/hyprland_settings.rs`: drop `format` field/`markers()` branch;
  rewrite 2 conf-locked tests.
- Scripts: delete `detect_format.sh`/`format_test.sh`; Lua-only `utils.sh`,
  `init.sh`, `assemble.sh`, `scan.sh`, `border.sh`, `apply_animation.sh`,
  `shader.sh`, `geometry.sh`, `colors.sh`, `color_watcher.sh`.
- Assets: delete the 32 `.conf` twins (14 borders + 18 animations).
- Guard: startup check + `install.sh` preflight + bilingual ES/EN message.
- Stale-comment sweep in `config.rs`, `settings.rs`, `callbacks.rs`, `install.sh`.

## Scope — Out (deferred, explicit)

- `clean_hyprland_conf`/watchdog conf cleanup removal (after one release).
- Theme `state.json` migration / Lua-shape validation in `apply()`.
- `HVE_COLORS_BASE` dead-code removal.
- `HyprMode::V4` dispatch fallbacks (IPC versioning, not config format).
- `uninstall.sh` cleaning the HVE block from `hyprland.lua`.

## Confirmed Decisions (final)

- **D1** DELETE the 32 `.conf` twins; git retains them.
- **D2** Guard warns + blocks mutations, never silent. Window opens with the
  message; preset apply and `init enable` refused. `hyprland.lua`
  present-and-valid ⇒ Lua user, guard NEVER fires (both-files user is on Lua).
  Guard verifies the HVE marker block `-- >>> HYPRLAND VISUAL EDITOR START <<<`,
  not mere file existence.
- **D3** REMOVE `hve_format()`; hardcode Lua — also removes the
  `OnceLock`+`TempEnv` contamination.
- **D4** No byte-migration of a stale `hve-settings.conf`; generate a fresh
  `hve-settings.lua`, orphan the `.conf`; document re-save.

## Capabilities

- **New**: `lua-only-config` — Lua-only settings/overlay contract + migration
  guard.
- **Modified**: None (no archived spec covers config format).

## Approach

Collapse format branches to Lua, strict TDD: (1) rewrite conf-locked tests to
Lua fixtures (RED), (2) simplify production to green, (3) add guard tests
(conf-without-lua fires; both-files does not), (4) implement guard.

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Conf-only users silently break | High | Guard blocks mutations + loud message |
| Conf-locked tests mask breakage | High | Rewrite fixtures first (RED) |
| Lua user without HVE block no-ops | Med | Guard checks marker block |
| Stale snapshots poison Lua | Med | Fresh-generate (D4) |

## Rollback Plan

`git revert` restores conf paths and the 32 twins (git history). D4 generates
fresh files, so nothing to reverse.

## Success Criteria

- [ ] `cargo test` green with zero `.conf` fixtures.
- [ ] `hve_format()`, `detect_format.sh`, `format_test.sh` gone; no runtime
      `HVE_FORMAT` branch.
- [ ] Conf-only system shows ES/EN message, refuses mutations; both-files
      never guarded.
- [ ] 32 `.conf` assets removed; Lua twins untouched.
- [ ] Guard covered by new tests.
