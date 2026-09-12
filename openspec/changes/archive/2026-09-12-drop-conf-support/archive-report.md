# Archive Report: drop-conf-support — Drop `.conf` Support (Lua-only)

## Change Summary

**Change**: drop-conf-support
**Description**: Remove dual-format (`.conf`/`.lua`) config support and collapse HVE to a Lua-only settings/overlay contract, plus a bilingual migration guard that blocks mutations on conf-only systems while still opening the window.
**Branch**: hve2-visual-rewrite
**Archive Date**: 2026-09-12
**Artifact Store**: hybrid (OpenSpec + Engram)

## Final-State Facts

Per the Final-State Authority hierarchy, the following facts represent the state of the change at close. Task artifact (13/13 checked) and the orchestrator's final-state facts outrank the intermediate `apply-progress`/`verify-report` snapshots; where they agree, both are cited.

### Verification

- **Test suite**: 669 passed, 0 failed (`cargo test`, full suite)
- **Build**: `cargo build` exit 0
- **Spec compliance**: 11/11 requirements, 14/14 scenarios — verdict `pass_with_warnings`
- **Critical findings**: 0
- **Blockers**: 0
- **Focused runs**: `config_guard` 13 passed, `config_markers` 3 passed, `scripts_contract` 7 passed, `slice_focus_flow_renders` 1 passed
- **Shell syntax**: `bash -n` over all touched scripts + `install.sh` — all OK

### Delivered Capability: `lua-only-config`

- No runtime format detection: `hve_format()` and the `hve_format` cache are gone; zero production reads of `HVE_FORMAT`.
- `hve_settings_path()` always resolves to `hve-settings.lua`; stale `.conf` is orphaned, never byte-migrated.
- All emitted content is Lua (`hl.bind(`, `hl.on("hyprland.start"`, comment-only window-rules block); all markers are `-- >>> … <<<`.
- Pure guard predicate `evaluate_config_guard` with a four-row decision matrix (`LuaReady`, `PromptToEnable`, `ConfOnly`, `NothingToDo`); valid `hyprland.lua` never fires the guard even with a stray `hyprland.conf`.
- Guard blocks mutations (preset apply, `init.sh enable`) with a bilingual ES/EN message; the window always opens.
- 32 `.conf` asset twins deleted (14 borders + 18 animations); all `.lua` twins retained including untouched `19_stylized2.5D.lua`.
- Scripts resolve Lua only; `detect_format.sh` and `format_test.sh` deleted.
- Suite carries zero conf-syntax fixtures.

### Work Units and Commits

| Unit | Commit | Scope |
|---|---|---|
| PR 1 — guard contract + marker constants | `ffe3b4f` | `src/config_guard.rs`, `src/config_markers.rs` created (11 tests) |
| PR 2 — Lua-only Rust core | `120eb91` | `config.rs`, `settings.rs`, `hyprland_settings.rs` collapsed to Lua |
| PR 3 — mutation gates + scripts | `0c70f57` | `main.rs` gates, i18n keys, 10 scripts Lua-only, `install.sh` preflight |
| PR 4 — assets + proof | `b89fb73` | 32 `.conf` twins + 2 dead scripts deleted (1,578 deletions, 0 insertions) |

PR 3 (1,133 changed lines, deletion-heavy, net −74) and PR 4 (deletion-only) carried maintainer-accepted `size:exception`.

### Engine Verification

Change limited to the negotiated sealed-engine seams (config/settings/provider/markers/guard, i18n, scripts, assets). `src/engine.rs`, `src/theme_manager.rs`, `src/app_state.rs`, `src/watcher.rs` untouched by all four commits. No `.slint` visual change; the R8 render is a status-bearing window check, not a new visual surface.

## Task Completion Gate

- **Total tasks**: 13 (1.1–4.3)
- **Completed**: 13/13 (all checked `[x]`)
- **Source**: `openspec/changes/archive/2026-09-12-drop-conf-support/tasks.md`
- **Gate**: PASS — no unchecked implementation tasks (`grep '- [ ]'` returns zero matches)

## Native Review Receipt Gate

- `reviewGate`: structurally absent (review mode disabled for this repository)
- **Gate**: PASS — ordinary repository policy applies, no receipt to validate

## Specs Synced

| Domain | Action | Details |
|--------|--------|---------|
| lua-only-config | Created | 11 requirements, 14 scenarios — copied as main spec (no prior main spec existed) |

The delta spec was a full spec (ADDED-only, no archived spec covers config format). No merge was needed; it was copied mechanically via shell `cp` to `openspec/specs/lua-only-config/spec.md` with an empty `diff -r` readback. No `rules.archive` exist in `openspec/config.yaml` (no `rules` section), so no destructive-merge warning applied.

## Main Specs Updated

- `openspec/specs/lua-only-config/spec.md` — created (source of truth for lua-only-config domain)

## Archive Contents

- `exploration.md` ✅
- `proposal.md` ✅
- `specs/lua-only-config/spec.md` ✅
- `design.md` ✅
- `tasks.md` ✅ (13/13 tasks complete)
- `apply-progress.md` ✅ (4/4 work units closed)
- `verify-report.md` ✅
- `archive-report.md` ✅ (this file, additive-only)

## Archived To

`openspec/changes/archive/2026-09-12-drop-conf-support/`

## Artifacts Read (OpenSpec paths)

- `openspec/changes/drop-conf-support/exploration.md`
- `openspec/changes/drop-conf-support/proposal.md`
- `openspec/changes/drop-conf-support/specs/lua-only-config/spec.md` (11 req / 14 scenarios)
- `openspec/changes/drop-conf-support/design.md`
- `openspec/changes/drop-conf-support/tasks.md` (13/13)
- `openspec/changes/drop-conf-support/apply-progress.md` (4/4 closed)
- `openspec/changes/drop-conf-support/verify-report.md` (`pass_with_warnings`, 0 CRITICAL)
- Precedent: `openspec/changes/archive/2026-08-21-hve2-trunk/` (layout + mechanical-copy convention)

## Carried Follow-ups (NOT actioned — recorded for future changes)

Coverage gaps (behavior correct by inspection, no dedicated test):

1. **R2 (`settings-always-.lua`)** has no dedicated test: neither the always-`.lua` path scenario nor the stale-`.conf`-not-migrated scenario has an automated assertion.
2. **R3 (`set_autostart(true)` emission)** has no dedicated test: only the disable path is exercised; the positive `hl.on("hyprland.start"` emission path is unasserted.
3. **R8 guard status** is not rendered guard-specifically: the headless render test shows the default `Ready` status, not the bilingual guard message.

Cosmetic follow-ups (not spec requirements, intentionally untouched per archive constraints):

4. `README.md`/`WIKI.md` still document `detect_format.sh` and the `hve_format` cache as live behavior.
5. `install.sh` ES/EN prompts still say "exec-once" while the code emits `hl.on("hyprland.start", …)`.
6. Dead `preset_geometry_for("01_cascade.conf")` key at `src/callbacks.rs:854` (`#[allow(dead_code)]`, never called).

Deferred by design (explicit scope-out, unchanged):

7. `clean_hyprland_conf` + watchdog conf cleanup removal (after one release).
8. Theme `state.json` migration tooling / Lua-shape validation in `apply()`.
9. `HVE_COLORS_BASE` dead-code removal.
10. `HyprMode::V4` dispatch fallbacks (IPC versioning, not config format).
11. `uninstall.sh` marker cleanup.

Out-of-scope pre-existing issue noted at verify time: noctalia env-lock flake (`delegate_noop_when_socket_absent` mutates `XDG_RUNTIME_DIR` without `ENV_LOCK`); did not reproduce in verification runs.

## SDD Cycle Status

**Formally closed.** The change has been fully planned, implemented, verified, and archived. Ready for the next change.
