```yaml
schema: gentle-ai.verify-result/v1
evidence_revision: sha256:4924adc9b8b90233b03129dec6b2e415af8cafeafa404e26364b0e6916f694a3
verdict: pass_with_warnings
blockers: 0
critical_findings: 0
requirements: 11/11
scenarios: 14/14
test_command: cargo test
test_exit_code: 0
test_output_hash: sha256:e9b186485713731e6618e06de24d0aace69014fa0b94466ac17b797b8e360e7f
build_command: cargo build
build_exit_code: 0
build_output_hash: sha256:0f7b5c95c2898c66233083a8db292cc162e152df1e9dfc4ec6232d800aba07ed
```

# Verification Report: drop-conf-support

**Change**: drop-conf-support — Drop `.conf` support (Lua-only config)
**Mode**: Strict TDD verify (`strict_tdd` active; runner `cargo test`)
**Branch**: `hve2-visual-rewrite`
**Verification Date**: 2026-09-13
**Verifier**: sdd-verify sub-agent (independent re-run; read-only except this report)
**Evidence Revision**: `sha256:4924adc9b8b90233b03129dec6b2e415af8cafeafa404e26364b0e6916f694a3` (HEAD `b89fb73c98ed79d4b4725347a4708abc800f3bd0`)
**Spec Source**: `openspec/changes/drop-conf-support/specs/lua-only-config/spec.md` — 11 requirements (R1–R11), 14 scenarios (counted, not copied)
**Apply Progress**: all 4 work units CLOSED (PR1 `ffe3b4f`, PR2 `120eb91`, PR3 `0c70f57`, PR4 `b89fb73`); tasks 1.1–4.3 checked

## Verdict

**PASS WITH WARNINGS** — every requirement is implemented and confirmed by direct re-execution or source inspection. Zero critical findings, zero blockers. Three scenarios carry a reduced verification strength (inspection-backed rather than dedicated-test-backed), recorded as WARNINGs. The `.conf` support is genuinely gone from the Rust core, the guard layer, the script layer, and the assets.

Nothing in this verification was taken from `apply-progress.md` claims: the suite, the greps, the asset counts, the guard matrix, and the headless render were all re-run independently in this session.

## Build & Tests Execution

### `cargo test` (full suite)
```
test result: ok. 669 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 19.79s
```
**Exit code**: 0
**Test output hash**: `sha256:e9b186485713731e6618e06de24d0aace69014fa0b94466ac17b797b8e360e7f` (captured at `/tmp/opencode/hve_verify_test.log`)
**Stability**: run twice in this session (20.10s and 19.79s), both 669/669 green. The known pre-existing noctalia env-lock flake did **not** reproduce.

### `cargo build`
```
   Compiling hve v1.0.0 (/home/ximo/Proyectos/hve)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 44.76s
```
**Exit code**: 0 (also implicitly type-checked by `cargo test` compiling the test binary)
**Build output hash**: `sha256:0f7b5c95c2898c66233083a8db292cc162e152df1e9dfc4ec6232d800aba07ed` (captured at `/tmp/opencode/hve_verify_build.log`)

### Focused runs (independently re-executed)
```
$ cargo test config_guard     -> 13 passed; 0 failed
$ cargo test config_markers   ->  3 passed; 0 failed
$ cargo test scripts_contract ->  7 passed; 0 failed
$ cargo test slice_focus_flow_renders -> 1 passed; 0 failed
```

### Shell syntax
```
$ bash -n assets/scripts/{utils,init,assemble,scan,border,apply_animation,shader,geometry,colors,color_watcher}.sh install.sh
all OK
```

## Per-Requirement Verdict Table

| Req | Verdict | Concrete Evidence | Gap |
|---|---|---|---|
| **R1** No runtime format detection | ✅ PASS | `rg 'hve_format\|HVE_FORMAT' src/` → only `src/scripts_contract.rs` (a `#![cfg(test)]` file, lines 102/142/148–149, the contract's own search needles + stale-env injection). Zero production matches. `rg '== "conf"\|== "lua"' src/` → zero production branches. `assets/scripts/detect_format.sh` and `format_test.sh` absent. No writer of `~/.cache/hve/hve_format` remains. | none |
| **R2** Settings file is always `.lua` | ⚠️ PARTIAL | `src/config.rs:238-257` `hve_settings_path()` unconditionally returns `hve_cache_dir().join("hve-settings.lua")`; the only legacy rename is `hve-windowrules.lua → hve-settings.lua` (Lua→Lua, never reads `.conf`). `src/settings.rs:14-31` `ensure_settings_file()` writes a fresh Lua template and never reads any `.conf`. `rg 'hve-settings\.conf' src/` → none. | **No covering test.** Neither scenario (path ends with `hve-settings.lua`; stale `.conf` not byte-migrated) has an automated assertion, despite the spec intro promising each requirement is verifiable by `cargo test` or a shell assertion. Behavior confirmed by inspection only. |
| **R3** Emitted content is Lua syntax | ⚠️ PARTIAL | `src/settings.rs:228-236` emits `hl.bind("SUPER + H", hl.dsp.exec_cmd("hve-ipc toggle-tray"))`; `src/settings.rs:343-353` emits `hl.on("hyprland.start", function() … hl.exec_cmd("{exe} --tray") end)`; `src/settings.rs:146-153` `window_rules_block()` is comment-only. `rg 'bind = SUPER\|exec-once *=' src/` → zero. | `set_keybinds(true)` is exercised (`test_set_keybinds_writes_and_removes` asserts `hve-ipc`/`toggle-tray`), but **`set_autostart(true)` is never called in any test** (`rg 'set_autostart\('` → tests only call `set_autostart(false)`). The exact spec literals `hl.bind(` / `hl.on("hyprland.start"` are not directly asserted; window-rules "comment-only" is asserted only via `assert_floating_content` (`Composer trait` present, `tile = true` absent). |
| **R4** Markers are Lua comments | ✅ PASS | `src/config_markers.rs:8-28` all six settings markers + HVE pair are `-- >>> … <<<`; test `every_marker_is_a_lua_comment` asserts `starts_with("-- >>> ")`, `ends_with(" <<<")`, `!starts_with('#')` for all 8. `rg '# >>>' src/` → zero. | none |
| **R5** Idempotent writes | ✅ PASS | `keybinds_skip_rewrite_when_already_enabled` (mtime unchanged on re-enable), `autostart_skip_rewrite_when_state_matches` (mtime unchanged), `tiling_rules_skip_rewrite_when_{floating,tiling}_already_applied` (content+mtime unchanged). Guard is `marker_block_needs_update` before write/reload. | none |
| **R6** Guard fires only for conf-only | ✅ PASS | `src/config_guard.rs:30-49` pure `evaluate_config_guard(Option<&str>, bool)`; 8 guard tests cover the full matrix: valid Lua+conf → `LuaReady` (test `valid_lua_with_hve_marker_is_lua_ready`) / `PromptToEnable` (`valid_lua_user_is_never_guarded_even_with_conf`), absent+conf → `ConfOnly`, absent+absent → `NothingToDo`, invalid stub+conf → `ConfOnly`. Valid Lua is never treated as conf-only even with a stray `hyprland.conf`. | none |
| **R7** Guard also requires the HVE block | ✅ PASS | `HVE_BLOCK_START = "-- >>> HYPRLAND VISUAL EDITOR START <<<"` (`config_markers.rs:26`) is required for `LuaReady`; `valid_lua_without_hve_marker_prompts_to_enable` → `PromptToEnable`. `hve_marker_only_matters_for_valid_lua` proves the marker alone on non-Lua content is `ConfOnly`. | none |
| **R8** Guard blocks mutations, window opens | ✅ PASS | `src/main.rs:902` evaluates the guard before `ensure_settings_file()` (922-924) and before `MainWindow::new()` (926); **no early return**. 16 `guard_permits` usages gate gallery apply (1513-1523), saved-theme apply (2289-2299), geometry/border/animation/shader callbacks (2573-2720), and startup writes (2750, 2790). Bilingual EN/ES keys `shell.lua_migration` / `shell.lua_enable` exist (`i18n/en.json:408-409`, `i18n/es.json:408-409`; `tr.rs:264-284`). `init.sh:116-123` refuses a conf-only system with ES+EN text and injects no Lua block (contract test `init_enable_refuses_conf_only_system`). Window-open confirmed by inspection (unconditional `MainWindow::new()`/`window.show()` at 926/2798) + headless render below. | Scenario 1 (preset apply refused) has **no direct UI-callback test**; the guard-status window is not rendered guard-specific (render test shows the default `Ready` status). See "Untestable As Specified". |
| **R9** Assets are Lua-only | ✅ PASS | `assets/borders/`: 14 `.lua`, 0 `.conf`. `assets/animations/`: 19 `.lua`, 0 `.conf`, `19_stylized2.5D.lua` present. `find assets -name '*.conf'` → 0. `git show b89fb73 --stat` deletes exactly 14 borders + 18 animations `.conf`. `19_stylized2.5D.lua` was not touched by any of the 4 commits (`git log ffe3b4f..b89fb73 -- …` empty). | none |
| **R10** Scripts resolve Lua only | ✅ PASS | `utils.sh:37-56` `hve_resolve_preset` returns only `$dir/$name.lua` (explicit-extension passthrough otherwise), no `HVE_FORMAT`, no conf fallback. `assemble.sh:9` `FINAL_FILE=…/overlay.lua`. `scan.sh:34` globs `-name "*.lua" -o -name "*.frag"`, never `*.conf`. `detect_format.sh`/`format_test.sh` absent. Contract tests `resolver_is_lua_only`, `assembler_and_scanner_are_lua_only`, `core_scripts_have_no_format_detection`, `color_helpers_drop_conf_paths`, `init_enable_refuses_conf_only_system`, `install_sh_has_preflight_before_build` all green. | none |
| **R11** Tests carry no `.conf` fixtures | ✅ PASS | `cargo test` 669/0. `rg '# >>>\|bind = \|exec-once =' src/settings.rs src/providers/hyprland_settings.rs` → zero; provider/settings fixtures rewritten to Lua (`hyprland_settings.rs:232-239,272-303`). The only `.conf` string in a test file is `scripts_contract.rs:168` (`fragment.replace(".lua", ".conf")`), an assertion needle, plus the env injection at `:102`; neither is a fixture. | none |

**Compliance summary**: 11/11 requirements met; 14/14 scenarios' expected behavior confirmed. 3 of those confirmations rest on source inspection + deterministic shell assertions rather than a dedicated automated test (flagged as WARNING).

## Untestable As Specified

**R8 scenario "preset apply refused / window still opens"** cannot be exercised as specified without a runtime harness that (a) creates a conf-only home, (b) drives the Slint `on_gallery_card_clicked` callback, and (c) asserts the window maps. No such harness exists in this repo. It was validated by two independent proxies instead:

1. **Structural proof of window-open**: `src/main.rs` evaluates the guard at line 902 but has no `return`/`process::exit` on any guard decision; `MainWindow::new()?` at 926 and `window.show()?` at 2798 (non-tray) run unconditionally. The status text is set from `guard_block_message` at 941-945.
2. **Headless render**: `cargo test slice_focus_flow_renders` → 1 passed. PNGs `/tmp/opencode/slice_settled.png` and `/tmp/opencode/slice_midflight.png` were read with the vision tool: the status-bearing window renders correctly — top bar with brand "Hyprland Visual Editor" (top-left) and status "Ready" (top-right), bottom bar "Esc hides" (bottom-left), gallery parallelogram carousel centred (settled Theme 5/6/7; midflight Theme 6/7/8). This proves the window surface and status bar open/render, but it renders the **default** status, not the bilingual guard message.

The `init.sh enable` refusal half of R8 **is** testable and is covered by `scripts_contract::init_enable_refuses_conf_only_system`.

## Findings

### CRITICAL
None. No requirement is unmet; no silent-breakage path was found.

### WARNING
1. **R2 has zero automated coverage.** Neither scenario has a test or shell assertion, contradicting the spec preamble ("each is verifiable by `cargo test` or a deterministic shell assertion"). Behavior is correct by inspection (`config.rs:238-257`, `settings.rs:14-31`), but a future refactor could reintroduce format branching without a red test. *Suggested fix*: a `TempEnv` test writing `hve-settings.conf` then asserting the created file is `hve-settings.lua` and contains none of the `.conf` bytes.
2. **R3 autostart emission is untested.** `set_autostart(true)` is never invoked by any test; only the disable path is. The Lua autostart block is correct by inspection (`settings.rs:343-353`) and its syntax appears in a provider fixture (`hyprland_settings.rs:299`), but the positive emission path (and the spec's `hl.on("hyprland.start"` assertion) has no covering test. *Suggested fix*: assert the emitted block contains `hl.on("hyprland.start"` and never `exec-once =`.
3. **R8 guard-status is not rendered guard-specific.** The only visual regression net renders the default "Ready" status; a regression that cleared `guard_block_message` or early-returned on guard would not fail the render test. *Suggested fix*: a sibling render test that sets `shell_status_text` to the bilingual message (or evaluates the guard) and reads the PNG.

### SUGGESTION
1. **Stale docs.** `README.md` (lines 205-210, 451, 506, 550) and `WIKI.md` (lines 205-210, 479, 534, 578) still document `detect_format.sh` and the `~/.cache/hve/hve_format` cache as live behavior. R1 requires both to cease existing; they do in code, but the user-facing docs now describe a deleted system. Not a spec requirement to update docs, but a consistency gap.
2. **Stale install.sh prompts.** `install.sh:64` (ES) and `:108` (EN) still say "exec-once en hve-settings" / "Uses exec-once in hve-settings", while PR3 corrected the code comment (line 72) to `hl.on("hyprland.start", ...)`. Cosmetic user-facing string drift.
3. **Dead `preset_geometry_for` key** — `src/callbacks.rs:854` still maps `"01_cascade.conf"`. It is `#[allow(dead_code)]`, reads no file, and is never called (unit tests use `.ron` names). Not a resolver path. **Explicitly out of scope**, carried as a dead-code sweep follow-up.

## Residual Risks & Follow-ups

- **Pre-existing noctalia env-lock flake** (`src/providers/noctalia.rs::delegate_noop_when_socket_absent` mutates `XDG_RUNTIME_DIR` at lines 1245/1274 without `test_utils::ENV_LOCK`). It did not reproduce in two full runs this session. **Out of scope** for drop-conf-support; pre-existing.
- **R2/R3 test gaps** above are the only material residual risk in this change. They are coverage gaps, not behavior gaps.
- **Docs drift** (README/WIKI/install.sh strings) is cosmetic but should be swept before a user-facing release.
- **Runtime guard path** was not executed against a real conf-only Hyprland session; confidence rests on the pure predicate (13 tests), the shell mirror (contract test), and main.rs structural inspection.

## Scope & Engine Verification

- Read-only verification: no source file was modified. The only write is this report.
- Change is limited to the negotiated sealed-engine seams (config/settings/provider/markers/guard, i18n, scripts, assets). `src/engine.rs`, `src/theme_manager.rs`, `src/app_state.rs`, `src/watcher.rs` untouched by the 4 commits.
- No `.slint` visual change in this change; the R8 render is a status-bearing window check, not a new visual surface.

## Artifacts

| Artifact | Path |
|---|---|
| Spec (authoritative counts) | `openspec/changes/drop-conf-support/specs/lua-only-config/spec.md` (11 req / 14 scenarios) |
| Design | `openspec/changes/drop-conf-support/design.md` |
| Tasks | `openspec/changes/drop-conf-support/tasks.md` (13/13) |
| Apply progress | `openspec/changes/drop-conf-support/apply-progress.md` (4/4 closed) |
| Verify report | `openspec/changes/drop-conf-support/verify-report.md` (this file) |
| Engram topic | `sdd/drop-conf-support/verify-report` (project `hve`, capture_prompt false, type architecture) |
| Test log | `/tmp/opencode/hve_verify_test.log` |
| Build log | `/tmp/opencode/hve_verify_build.log` |
| Render PNGs | `/tmp/opencode/slice_settled.png`, `/tmp/opencode/slice_midflight.png`, `/tmp/opencode/slice_seam.png` |

## Next Recommended

**ready-for-archive with warnings.** The change is functionally complete and independently verified. The orchestrator may archive now and carry the R2/R3 test gaps + docs drift as follow-ups, or (preferred if bandwidth allows) add the two small tests from WARNINGs 1–2 first. No critical blocker exists either way.

## Skill Resolution

| Skill | Status | Notes |
|---|---|---|
| `hve2` | ✅ Loaded | Visual-only, engine sealed (config/settings/provider seams only), single mutating window preserved |
| `sdd-verify` | ✅ Loaded | Independent re-run, authoritative counts (11/14), envelope validated before persistence, read-only |
