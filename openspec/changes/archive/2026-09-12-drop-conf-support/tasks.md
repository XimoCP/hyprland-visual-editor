# Tasks: Drop `.conf` Support (Lua-only)

Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: pending
400-line budget risk: High

### Suggested Work Units

| Unit | Goal | Likely PR | Focused test command | Runtime harness | Rollback boundary |
|---|---|---|---|---|---|
| 1 | RED fixtures and guard | PR 1 | `cargo test` | N/A: pure Rust | Rust test/module files |
| 2 | Lua-only Rust core | PR 2 | `cargo test settings providers` | N/A: settings backend | Rust core files |
| 3 | Gates and scripts | PR 3 | `cargo test` + shell assertions | `cargo test slice_focus_flow_renders` | main, i18n, scripts |
| 4 | Assets and proof | PR 4 | `cargo test` + asset scan | N/A: static assets | deleted assets/verification |

## Phase 1: RED Fixtures and Guard Contract

- [x] 1.1 Rewrite conf-locked fixtures in `src/settings.rs` and `src/providers/hyprland_settings.rs` to Lua; run focused `cargo test` and record RED (R3, R4, R11).
- [x] 1.2 Add four matrix plus prompt/ready RED tests in `src/config_guard.rs`; assert `LuaReady`, `PromptToEnable`, `ConfOnly`, `NothingToDo`; run `cargo test` (R6, R7).
- [x] 1.3 Test shared Lua marker constants and HVE marker in `src/config_markers.rs`; run `cargo test` (R4, R7).

## Phase 2: Rust Lua-only Core

- [x] 2.1 Implement `ConfigGuardDecision` and `evaluate_config_guard` in `src/config_guard.rs`, then make guard tests green (R6, R7).
- [x] 2.2 Add constants in `src/config_markers.rs`; simplify `src/settings.rs` to Lua templates, empty rules, keybinds, autostart, and idempotent writes; run focused tests (R3–R5, R11).
- [x] 2.3 Remove `hve_format`/cache branching in `src/config.rs`; always use `.lua`, fresh-generate stale `.conf`, and update `src/providers/hyprland_settings.rs`; run `cargo test` (R1, R2, R3, R11).

## Phase 3: Mutation Gates and Shell Integration

- [x] 3.1 Add failing policy assertions for refusal, prompt, bilingual text in `src/main.rs`, `i18n/en.json`, `i18n/es.json`; implement gates without closing the window (R7, R8).
- [x] 3.2 Add failing shell assertions for Lua-only resolver/setup and `init.sh enable` refusal; simplify `assets/scripts/utils.sh`, `init.sh`, `assemble.sh`, and `scan.sh`; run them plus `cargo test` (R1, R6, R8, R10).
- [x] 3.3 Simplify `assets/scripts/{border.sh,apply_animation.sh,shader.sh,geometry.sh}` to Lua-only fragments; run deterministic assertions (R9, R10).
- [x] 3.4 Remove conf parsing/watch paths from `assets/scripts/colors.sh` and `color_watcher.sh`; update `install.sh`; run assertions (R1, R8, R10).

## Phase 4: Assets and Verification

- [x] 4.1 Delete 14 `assets/borders/*.conf` and 18 `assets/animations/*.conf`; assert 14/19 Lua files remain and `19_stylized2.5D.lua` is unchanged (R9).
- [x] 4.2 Delete `assets/scripts/detect_format.sh` and `format_test.sh`; scan production paths for forbidden symbols, conf fixtures, markers, and globs (R1, R10, R11).
- [x] 4.3 Run full `cargo test`; run `cargo test slice_focus_flow_renders`, read `/tmp/opencode/slice_*.png`, and confirm the guard status window still renders/opens (R8, R11).

## Review Workload Forecast

Estimated changed lines: ~1,200–1,600 (Rust, scripts, tests, 32 assets)
Chained PRs recommended: Yes
400-line budget risk: High
Decision needed before apply: Yes

Suggested split: PR 1 RED fixtures/guard; PR 2 Rust core; PR 3 gates/scripts; PR 4 deletions/proof. Each slice is independently testable after its dependencies.

Recommended chain strategy: feature-branch-chain — Rust must land before integration and cleanup; a tracker keeps child diffs focused.
