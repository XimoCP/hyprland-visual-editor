# Guard preset names against path traversal

## Objective

Close the arbitrary local file read in the preset resolver: a crafted preset
name must never escape the built-in or user preset directories.

## Problem

`_hve_preset_in_dir` (`assets/scripts/utils.sh:48-67`) treats any name that
contains a dot as "already carries an extension" and tests
`[ -f "$dir/$name" ]` with **no containment check**. A name such as
`../../../../etc/passwd` resolves to a file outside both preset directories, and
`border.sh:23` / `apply_animation.sh:23` `cat` it into the assembled Hyprland
fragment (`border.lua` / `animation.lua`). The name arrives unsanitized from the
UI.

## Why

Security follow-up recorded at the close of the 2026-10-01 session (Engram
observation #1225): pre-existing local arbitrary file read whose content is
injected into the compositor config.

## Scope

In:

- `assets/scripts/utils.sh` — reject unsafe names in the resolver.
- `src/preset_store.rs` — mirror the name validation at the Rust boundary
  (`save`, `load`, `delete`, `rename`, `exists`, `read_border_file`).
- `src/scripts_contract.rs` — contract test proving a traversal name takes the
  safe fallback and never reaches the overlay.

Out: the built-in-first resolution order, the safe fallback fragments, UI copy,
and any `.slint` change.

## Constraints

- Strict TDD is active (project `AGENTS.md`). Test runner: `cargo test`.
  Failing test first (RED), then implement (GREEN).
- Preserve legitimate dotted names: built-in `19_stylized2.5D.lua` and any
  user preset stem with an interior dot must still resolve.
- Do not run `cargo fmt`; do not switch branches; one writer at a time.
- Engineered artifacts in English.

## Name rule (both layers)

A preset name is **unsafe** when it is empty, contains `/`, contains `\`,
contains NUL, equals `.` or `..`, or starts with `.`. Interior dots are allowed.

## Tasks

- **T1 — RED**: add the contract test in `src/scripts_contract.rs` (traversal
  name falls back safely; a sentinel token from a file outside the preset dirs
  never reaches the overlay) and Rust unit tests for the name guard. Confirm
  they fail against current code.
- **T2**: implement the shell guard in `utils.sh` (`_hve_preset_name_is_safe` +
  containment check), used by `_hve_preset_in_dir`.
- **T3**: mirror the guard in `src/preset_store.rs`.
- **T4 — GREEN**: full `cargo test`; report pass/fail/ignore counts.
- **T5**: commit the work unit(s).

## Acceptance criteria

- `hve_resolve_preset` rejects any name containing a path separator or a leading
  dot; traversal names fall back safely.
- `PresetStore` rejects the same names with an error and never touches a path
  outside `base`.
- Legitimate names (`01_cascade`, `19_stylized2.5D`, `keeper_saved`) keep
  working.
- Full suite green.

## Checks

- `cargo test` (full).
- Cross-model verification (path handling → Tier 3): written by
  `deepseek-v4-flash`, verified by a `glm-5.3-flash` agent.

## Progress

- [x] T1
- [x] T2
- [x] T3
- [x] T4
- [x] T5

## Evidence

- **RED** (`cargo test`, before implementation): 6 new tests failed —
  `scripts_contract::traversal_border_preset_name_takes_the_safe_fallback`,
  `scripts_contract::traversal_animation_preset_name_takes_the_safe_fallback`,
  `scripts_contract::leading_dot_preset_name_takes_the_safe_fallback`,
  `preset_store::tests::save_rejects_traversal_and_writes_nothing_outside_base`,
  `preset_store::tests::read_border_file_rejects_traversal_outside_base`,
  `preset_store::tests::exists_rejects_traversal_name`.
  The traversal contract test proved the outside file's `TRAVERSAL_SENTINEL_TOKEN`
  and its `angle = 424242` reached the assembled overlay.
- **GREEN** (`cargo test`, full): `1236 passed; 0 failed; 0 ignored; 0 measured`.
- **Guard parity check** (`source assets/scripts/utils.sh`): `_hve_preset_name_is_safe`
  rejects ``, `/`, `\`, `.`, `..`, `.hidden`, `../x`, `a/b`; accepts
  `01_cascade`, `19_stylized2.5D`, `keeper_saved`, `a.b.c`.
- **Files**:
  - `assets/scripts/utils.sh` — `_hve_preset_name_is_safe`,
    `_hve_preset_path_is_contained` (`realpath -m` containment), called from
    `_hve_preset_in_dir`.
  - `src/preset_store.rs` — private `validate_name`, called from `save`,
    `load`, `delete`, `rename` (both names), `exists`, `read_border_file`.
  - `src/scripts_contract.rs` — traversal/leading-dot/interior-dot contract tests.
- **Commit**: `fix(security): reject path traversal in preset names`.

## Residual bypass (2026-10-02, verifier finding)

The name guard closed the traversal name, but the single top-level containment
call in `_hve_preset_in_dir` canonicalized `$dir/$name` only. For a bare STEM
(no dot) the resolver then probes `$dir/$name.lua`, so a symlink planted inside
the preset dir (`plain.lua -> /etc/whatever`) was echoed and `cat` by the caller
— the comment claiming escaping symlinks are refused was false for that branch.

Fixed by checking containment of the EXACT candidate path in each branch
(`$dir/$name` for dotted names, `$dir/$name.lua` for bare stems). Regression
test: `scripts_contract::symlinked_preset_inside_the_dir_takes_the_safe_fallback`.

- **RED**: the symlinked preset's `SYMLINK_SENTINEL_TOKEN` and `angle = 424242`
  reached the overlay.
- **GREEN** (`cargo test`, full): `1237 passed; 0 failed; 0 ignored; 0 measured`.
- **Commit**: `fix(security): contain the resolved preset path in every branch`.

## Cross-model verification (Tier 3 — path handling)

Written by `deepseek-v4-flash`; verified by `glm-5.3-flash` (different model).

- `dd9e494` — **PASS with warnings**. Reproduced the pre-fix leak live; confirmed
  the guard is reached on both apply paths and the Rust mirror covers every
  mutation entry point; full suite 1236/0. Warning: the stem + in-dir symlink
  bypass (closed in `0fefb27`).
- `0fefb27` — **PASS**. Both branches now containment-check the exact candidate
  path; the symlink bypass no longer leaks; `19_stylized2.5D.lua` and normal
  presets still resolve; full suite 1237/0.

## Install

- `cargo build --release` → `target/release/hve`.
- `~/.local/bin/hve` updated (34825248 bytes, 2026-10-02 05:04).
- `~/.local/bin/assets/scripts/` synced from the repo; installed `utils.sh`
  identical to the fixed source.
- `hve --version` → `hve 1.0.0`.

## Follow-ups (not part of this security fix)

- Functional (pre-existing): a dotted user-preset stem applied by bare stem via
  the shell path falls back (the UI passes stems for user cards). Not introduced
  here; tracked separately.
- TOCTOU between the containment check and `cat` needs same-user write access to
  the preset dir; no privilege escalation.

## Next step

Closed. Security hole fixed, cross-model verified, binary and scripts installed.
