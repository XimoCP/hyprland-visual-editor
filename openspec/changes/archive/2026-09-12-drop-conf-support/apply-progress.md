# Apply Progress: Drop `.conf` Support (Lua-only)

## Work Unit: `pr1-red-fixtures-and-guard-contract` (PR 1 of 4)

- **Status**: complete — committed, all focused and full tests green.
- **Commit**: `ffe3b4f` — `feat(config): add migration guard contract and Lua marker constants`
- **Scope delivered**: the migration-guard contract as a self-contained module
  pair plus the shared marker constants. No production consumer yet; the module
  pair is intentionally unwired until PR 3 so PR 1 stays independently green.

### Files Changed

| File | Action | Notes |
|---|---|---|
| `src/config_markers.rs` | Create | Six Lua settings markers + HVE block pair; single source of truth; 3 tests. |
| `src/config_guard.rs` | Create | `ConfigGuardDecision` + `evaluate_config_guard`; 8 tests covering the full R6/R7 matrix. |
| `src/main.rs` | Modify | Two `mod` lines only (`config_guard`, `config_markers`). |

### Tasks Now Done

- **1.2** — matrix + prompt/ready RED tests in `src/config_guard.rs` (R6, R7).
- **1.3** — shared marker constant tests in `src/config_markers.rs` (R4, R7).
- **2.1** — `ConfigGuardDecision` / `evaluate_config_guard` implemented to green (R6, R7).

Task 1.1 (rewrite conf-locked fixtures in `settings.rs` /
`providers/hyprland_settings.rs`) remains deferred to PR 2 as planned.

### TDD Evidence

RED captured before implementation: `cargo test config_guard` failed to compile
with `E0425` (undefined `LUA_*`/`HVE_BLOCK_*` constants and
`evaluate_config_guard`) and `E0433` (undeclared `ConfigGuardDecision`).

GREEN after implementation:

```
$ cargo test config_guard
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 647 filtered out

$ cargo test config_markers
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 652 filtered out

$ cargo test
test result: ok. 655 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Runtime Harness

N/A — pure Rust predicate and constants, no compositor/UI boundary in this slice.

### Rollback Boundary

Remove `src/config_guard.rs`, `src/config_markers.rs`, and the two `mod` lines
in `src/main.rs`. Nothing else in the tree depends on them yet.

## Work Unit: `pr2-lua-only-rust-core` (PR 2 of 4)

- **Status**: complete — committed, focused and full tests green.
- **Commit**: `120eb91` — `refactor(config): collapse settings core to Lua-only`
- **Scope delivered**: the Lua-only Rust core. `settings.rs` emits one Lua
  template and Lua keybind/autostart blocks from the shared `config_markers`
  constants; `hve_format()` and its cache read are gone; the settings path is
  always `hve-settings.lua`; the provider is format-stateless and reads the
  same constants. The idempotent no-rewrite/no-reload guard is preserved.

### Files Changed

| File | Action | Notes |
|---|---|---|
| `src/config.rs` | Modify | Deleted `hve_format()` + `OnceLock`/cache read; `hve_settings_path()` always `hve-settings.lua`; legacy rename retargeted to `hve-windowrules.lua` → `hve-settings.lua`; stale `.conf` left orphaned. |
| `src/settings.rs` | Modify | Lua-only template/keybinds/autostart; three marker pairs from `config_markers`; `window_rules_block()` dropped the `format`/`_tiling` params; doc comments refreshed. |
| `src/providers/hyprland_settings.rs` | Modify | Removed the `format` field and `markers()`; `new()` is a plain constructor; save/apply build marker tuples from `config_markers`; fixtures rewritten to Lua. |

### Tasks Now Done

- **1.1** — conf-locked fixtures rewritten to Lua in `src/settings.rs` and
  `src/providers/hyprland_settings.rs` (R3, R4, R11).
- **2.2** — `settings.rs` Lua templates, empty rules, keybinds, autostart and
  idempotent writes wired to `config_markers` (R3–R5, R11).
- **2.3** — `hve_format`/cache branching removed from `config.rs`; always
  `.lua`; provider updated (R1, R2, R3, R11).

### TDD Evidence

RED captured after rewriting fixtures, before implementation:

```
$ cargo test settings
test result: FAILED. 13 passed; 2 failed; 0 ignored; 0 measured; 640 filtered out
  - providers::hyprland_settings::tests::test_apply_removes_block_when_state_none
  - providers::hyprland_settings::tests::test_save_with_absent_markers_then_apply_removes_active_block

$ cargo test hyprland_settings
test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 652 filtered out
```

GREEN after implementation:

```
$ cargo test settings
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 640 filtered out

$ cargo test hyprland_settings
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 652 filtered out

$ cargo test
test result: ok. 655 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

One full-suite run first reported a flaky failure in
`providers::noctalia::tests::delegate_noop_when_socket_absent`; it passes in
isolation and the rerun was 655/655 green. Root cause is pre-existing: that
test mutates `XDG_RUNTIME_DIR` without holding `test_utils::ENV_LOCK`, racing
other env-mutating tests. Unrelated to this slice.

Grep check — `hve_format` / `HVE_FORMAT` in `src/`: zero matches. Remaining
`hve_format` references live only in `assets/scripts/*` (PR 3/4) and docs.

### Runtime Harness

N/A — settings backend and provider state only, no compositor/UI boundary in
this slice; behavior is covered by the Rust tests above.

### Rollback Boundary

Revert `src/config.rs`, `src/settings.rs` and
`src/providers/hyprland_settings.rs` (and their tests) to the PR 1 state.
No other file depends on this slice, and `config_markers`/`config_guard` from
PR 1 remain usable without it.

## Work Unit: `pr3-gates-and-scripts` (PR 3 of 4)

- **Status**: complete — committed, full suite and targeted verification green.
- **Commit**: `0c70f57` — `feat(config): gate mutations on Lua guard and make scripts Lua-only`
- **Scope delivered**: the migration guard is wired into production and the
  whole script layer is Lua-only. `config_guard` gains the mutation policy plus
  the startup path adapter and loses its `#![allow(dead_code)]`; `main.rs`
  evaluates the guard before `ensure_settings_file()` and gates every mutating
  call site while still opening the window and surfacing a bilingual status;
  `init.sh` mirrors the guard and refuses a conf-only system.

### Files Changed

| File | Action | Notes |
|---|---|---|
| `src/config_guard.rs` | Modify | `permits_mutation` / `permits_init_enable`, `Clone+Copy`, `evaluate_config_guard_at`, `evaluate_config_guard_default`; `#![allow(dead_code)]` removed; 6 new tests. |
| `src/config_markers.rs` | Modify | Module-level `#![allow(dead_code)]` removed; targeted `#[allow(dead_code)]` on the shell-mirrored `HVE_BLOCK_END`. |
| `src/main.rs` | Modify | Guard evaluated before settings write; gated gallery apply, saved-theme apply, geometry/border/animation/shader callbacks, and startup writes; status text/`guard_block_message` captured per closure. |
| `src/tr.rs` | Modify | EN/ES resolution tests for `shell.lua_migration` / `shell.lua_enable`. |
| `src/scripts_contract.rs` | Create | 7 deterministic shell-contract tests (resolver, static Lua-only invariant, preset fragments, assembler/scanner, color helpers, init refusal, install preflight). |
| `i18n/en.json`, `i18n/es.json` | Modify | Bilingual `shell.lua_migration` and `shell.lua_enable`. |
| `assets/scripts/utils.sh` | Modify | `hve_resolve_preset` resolves only `$dir/$name.lua`; `HVE_FORMAT` read removed. |
| `assets/scripts/init.sh` | Modify | Format detection/conf injection removed; shell guard before `setup_files`; Lua-only setup; `clean_hyprland_conf` kept as one-way hygiene. |
| `assets/scripts/assemble.sh` | Modify | Single `overlay.lua` target; Lua emission + Lua-side validation only; symlink/reload kept. |
| `assets/scripts/scan.sh` | Modify | Detection/dedup removed; globs `*.lua` + `*.frag` (shaders), never `*.conf`. |
| `assets/scripts/border.sh`, `apply_animation.sh`, `shader.sh`, `geometry.sh` | Modify | Hardcoded Lua fragment/resolver/fallback; opposite-format cleanup removed; validation/security preserved. |
| `assets/scripts/colors.sh` | Modify | `_hve_extract_conf_vars` and its call sites removed; Lua arms + format-agnostic sources kept. |
| `assets/scripts/color_watcher.sh` | Modify | `.conf` watches and conf rendered-file entries removed. |
| `install.sh` | Modify | Preflight guard before build + bilingual preflight messages; stale `hve-settings.{lua,conf}` / `exec-once … --tray` comments corrected. |

Not touched (PR 4): `.conf` assets, `detect_format.sh`, `format_test.sh`,
`uninstall.sh`, `clean_hyprland_conf` removal, watchdog conf cleanup.

### Tasks Now Done

- **3.1** — mutation gates + bilingual i18n wired without closing the window (R7, R8).
- **3.2** — Lua-only resolver/setup/assembler/scanner + init refusal (R1, R6, R8, R10).
- **3.3** — preset scripts reduced to Lua fragments (R9, R10).
- **3.4** — colors/watcher conf paths removed; install preflight added (R1, R8, R10).

### TDD Evidence

RED captured before implementation:

```
$ cargo test config_guard
error[E0425]: cannot find function `evaluate_config_guard_at` in this scope (x3)
error[E0599]: no method named `permits_init_enable` ... (x4)
error[E0599]: no method named `permits_mutation` ... (x4)

$ cargo test shell_guard_keys
test result: FAILED. 0 passed; 2 failed   (None vs Some(...))

$ cargo test scripts_contract
test result: FAILED. 0 passed; 7 failed
  - resolver_is_lua_only (resolved=…/preset.conf)
  - init_enable_refuses_conf_only_system (old script injected a conf block)
  - core_scripts_have_no_format_detection
  - prompts_target_lua_fragments_only
  - assembler_and_scanner_are_lua_only
  - color_helpers_drop_conf_paths
  - install_sh_has_preflight_before_build
```

GREEN after implementation:

```
$ cargo test config_guard
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 656 filtered out

$ cargo test shell_guard_keys
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 667 filtered out

$ cargo test scripts_contract
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 662 filtered out

$ cargo test
test result: ok. 669 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 19.74s
```

`cargo check` reports zero warnings from the PR 3 Rust files (the two
unwired-symbol warnings from PR 1 are gone).

### Runtime Harness

R8 visual verification: `cargo test slice_focus_flow_renders` →
`test result: ok. 1 passed`. PNGs read with the vision tool:
`/tmp/opencode/slice_settled.png` and `/tmp/opencode/slice_midflight.png`
show the status-bearing shell window rendering correctly (brand +
"Ready" status top bar, "Esc hides" bottom bar, gallery parallelogram
carousel centred).

Deterministic shell harnesses (run in hermetic temp homes with stub
`pgrep`/`hyprctl`/`noctalia` on `PATH`):

```
$ bash -n assets/scripts/{utils,init,assemble,scan,border,apply_animation,shader,geometry,colors,color_watcher}.sh install.sh
all OK
```

- Resolver: `HVE_FORMAT=conf` sandbox still returns `preset.lua`,
  `legacy.conf` refuses — asserted by `resolver_is_lua_only`.
- `init.sh enable` on a conf-only home refuses with the bilingual message
  and creates no `hyprland.lua` — asserted by `init_enable_refuses_conf_only_system`.
- `init.sh enable` on valid Lua without the HVE block injects the
  `-- >>> HYPRLAND VISUAL EDITOR START <<<` block exactly once and writes
  `overlay.lua` + `hve-settings.lua` (manual sandbox run, no `.conf`).
- `assemble.sh` sandbox emits a Lua `overlay.lua` (shebang, `hl.curve`,
  quoted color vars) and the `overlay.current` symlink; no reload fired.

### Size Note

18 files, **665 insertions / 468 deletions = 1,133 changed lines** — above
the ~700 cap for this unit. The four tasks form one dependency-closed slice
(scripts' contract tests assert both the Rust gates and the Lua-only
scripts), and the diff is deletion-heavy (net −74 lines). Recommend a
`size:exception` for this single PR 3 work unit rather than splitting it;
no cohesive sub-slice leaves both halves independently verifiable.

### Rollback Boundary

Revert `src/main.rs`, `src/config_guard.rs`, `src/config_markers.rs`,
`src/tr.rs`, `src/scripts_contract.rs`, `i18n/{en,es}.json`, `install.sh`,
and the 10 Lua-only scripts under `assets/scripts/` (utils, init, assemble,
scan, border, apply_animation, shader, geometry, colors, color_watcher) to
the PR 2 state. PR 4 (asset deletion, detector removal, verification) is
independent and remains untouched.

## Work Unit: `pr4-assets-and-verification` (PR 4 of 4 — FINAL)

- **Status**: complete — committed, full suite and visual verification green.
- **Commit**: `b89fb73` — `chore(config): drop .conf preset twins and format detector scripts`
- **Scope delivered**: the 32 Lua-twin removals and the two dead detector
  scripts, plus the final repository proof. This is the last work unit; after
  it the change is fully implemented and every task (1.1–4.3) is done.

### Files Changed

| File | Action | Notes |
|---|---|---|
| `assets/borders/*.conf` (14) | Delete | All 14 `.lua` twins retained. |
| `assets/animations/*.conf` (18) | Delete | All 19 `.lua` twins retained, including `19_stylized2.5D.lua` (Lua-only, untouched). |
| `assets/scripts/detect_format.sh` | Delete | Dead runtime format detector; wrote the `hve_format` cache. |
| `assets/scripts/format_test.sh` | Delete | Dead transpiler test; already broken (referenced non-existent `transpile.sh`). |

34 files changed, **1,578 deletions, 0 insertions**.

### Pre-deletion safety check (R9)

Before deleting, confirmed no production path resolves a `.conf` preset:

- `assets/scripts/utils.sh` `hve_resolve_preset` returns only `$dir/$name.lua`
  (bare names) or an exact existing name; with the twins gone, a `.conf` name
  cannot resolve.
- `assets/scripts/scan.sh` globs only `*.lua`/`*.frag`; the engine receives the
  `file` field from `scan.sh`, so it only ever sees `.lua` names.
- No `rg` hits for the asset dirs (`assets/borders`, `assets/animations`) in `src/`.

### Findings reported, not papered over

- `src/callbacks.rs:854` — `preset_geometry_for()` still holds the dead demo
  key `"01_cascade.conf"`. It is `#[allow(dead_code)]`, reads no file, and is
  never called, so it is **not** a resolver path and was left untouched
  (out of scope; prefer minimal edits). Flagging for a future dead-code sweep.
- Remaining `.conf` literals in `src/` are all legitimate: contract-test
  fixtures/needles in `src/scripts_contract.rs`, the external Noctalia artifact
  names in `src/providers/shell.rs` (`noctalia-colors.conf`), generic
  `utils.rs` temp fixtures, and the `hyprland.conf` guard path. None of them
  resolves the deleted assets.

### Verification (R1, R8, R9, R10, R11)

Asset counts after deletion:

```
assets/borders/    : 14 .lua, 0 .conf
assets/animations/ : 19 .lua, 0 .conf  (19_stylized2.5D.lua present: YES)
detect_format.sh   : absent
format_test.sh     : absent
find assets -name '*.conf' : 0 results
```

Full suite:

```
$ cargo test
test result: ok. 669 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 20.21s
```

Visual verification (R8):

```
$ cargo test slice_focus_flow_renders
test shell::ui_tests::slice_focus_flow_renders_settled_and_midflight ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 668 filtered out
```

PNGs read with the vision tool: `/tmp/opencode/slice_settled.png` and
`/tmp/opencode/slice_midflight.png` show the status-bearing shell window
rendering correctly — brand ("Hyprland Visual Editor") + "Ready" status in the
top bar, "Esc hides" bottom bar, gallery parallelogram carousel centred
(settled: Theme 5/6/7; midflight: Theme 6/7/8). The window opens and renders.

Forbidden-leftover scan:

```
HVE_FORMAT / detect_format / hve_format in src/, assets/scripts/, install.sh
  → only src/scripts_contract.rs (the contract test's own search needles +
    a stale-env injection that asserts the resolver ignores HVE_FORMAT). Zero
    production reads.
'*.conf' globs in assets/scripts/          → none
'# >>>' markers in src/                    → none
'bind = SUPER, H, exec,' / 'exec-once =' in src/ → none
'.conf' under assets/borders|animations/   → none
```

`cargo test slice_focus_flow_renders` recompiled with 11 pre-existing
`unused_variables` warnings in the test binary (unchanged from PR 3; not
introduced here).

### Runtime Harness

R8 visual boundary: `cargo test slice_focus_flow_renders` → 1 passed, PNGs
inspected (above). Static-asset slice only; no other runtime boundary.

### Rollback Boundary

Restore the 32 deleted `.conf` files and the two deleted scripts from git
history (`git revert b89fb73`). No source or script logic changed in this
unit, so the revert cannot disturb prior work.

## Final Snapshot — ALL WORK UNITS COMPLETE

- PR 1 `pr1-red-fixtures-and-guard-contract` — `ffe3b4f` ✅
- PR 2 `pr2-lua-only-rust-core` — `120eb91` ✅
- PR 3 `pr3-gates-and-scripts` — `0c70f57` ✅
- PR 4 `pr4-assets-and-verification` — `b89fb73` ✅

Tasks 1.1–4.3 are all checked. `cargo test` = 669 passed / 0 failed. The
`.conf` support is fully dropped: Rust core, guards, scripts, and assets are
Lua-only, and no production path reads `HVE_FORMAT`, resolves a `.conf`, or
emits conf syntax. Next: `sdd-verify`, then archive.
