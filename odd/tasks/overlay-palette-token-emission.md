# Overlay palette: emit every token the presets can reference

**Locator**: `odd/tasks/overlay-palette-token-emission.md`
**Repo**: `hve` — branch `hve2-visual-rewrite`
**Opened**: 2026-09-30
**Kind**: live defect fix (highest priority — the desktop is currently rejecting its config)

## The defect

Hyprland shows on the running desktop:

```
Your config has errors:
/home/ximo/.cache/hve/overlay.lua:24: error setting 'general.col.active_border':
gradient "colors" must not be empty
```

The generated overlay defines a palette of five names:

```lua
primary = "#67abe4"
secondary = "#d6915c"
surface = "#11202c"
surface_lowest = "#1d3549"
accent = "#67abe4"
```

and then assembles a border module that uses a sixth:

```lua
active_border = {
    colors = { primary, secondary, tertiary },
    angle = 45
},
```

In Lua an undefined name is `nil`, so the gradient list is invalid and Hyprland rejects
the **whole** overlay — every border and decoration setting in the file is dropped, not
just this one.

## Root cause (verified, read-only investigation)

`assets/scripts/assemble.sh:31-40` writes five palette lines by hand. `tertiary` is
**computed, completed and exported one step earlier on that same path** and simply never
printed:

- `assemble.sh:29` sources `assets/scripts/colors.sh`;
- `colors.sh:568-569` calls `hve_load_colors` unconditionally on source;
- `colors.sh:312-326` `_hve_complete_tertiary()` resolves a real same-scheme tertiary;
- `colors.sh:560` guarantees a final fallback (`#94e2d5`);
- `colors.sh:565` exports it.

So the rescue machinery runs and is effective; the emission block just lacks the line.

`HVE_ERROR` is the same bug one token over, and is worse: it is absent from the reset
list (`colors.sh:520-526`), the failed-step cleanup (`:545-550`), the fallback block
(`:558-563`) and the export (`:565`).

Presets that reference the missing names: `assets/borders/04_tri.lua:14`,
`14_looper.lua:14`, `07_infinity.lua:14`, `05_spectrum.lua:14` (`tertiary` + `error`),
`09_glitch.lua:14` (`error`), and the runtime fragment `assets/fragments/border.lua:14`
(gitignored; written by `assets/scripts/border.sh:22` and `src/preset_store.rs:285-293`).

The UI can reproduce it without touching a preset: `src/main.rs:109-116`
`BORDER_SLOT_CYCLE` defaults new colour slots to `p:tertiary` and `p:error`;
`src/border_preset.rs:44-51` emits bare identifiers; `src/preset_store.rs:129-140`
writes `colors = { … }`; `src/main.rs:4502-4507` writes the draft fragment and re-runs
`assemble.sh`.

Nothing in the test suite could catch it: every tertiary test goes through
`get_colors.sh` (which does print tertiary), never through `assemble.sh`'s palette
block. The one harness that runs the real assembler (`src/reload_coalescer.rs`,
`Sandbox`) feeds a preset referencing only `primary`.

## The agnostic norm this fix must honour

The keeper's standing decree: no hand-wired values, backend names or literal lists. A
script that enumerates five palette names and forgets the sixth is exactly what the norm
forbids, so the fix is data-driven, not "add one more line".

Two ways to break the norm while appearing to fix the defect, both forbidden:

- hardcoding a fallback hex inside `assemble.sh` (pins the palette outside the theme's
  authority);
- passing `HVE_TERTIARY` from Rust's own `ColorScheme` (`src/engine.rs:15`), which
  creates a second palette authority — precisely what the 2026-09-29 descriptor work
  ("the theme owns the palette") exists to remove.

The value must keep coming from `colors.sh` and nowhere else.

## Scope

In: the palette emission block, the token lifecycle around it in `colors.sh`, and the
test that closes the class.

Out: the border presets themselves, the tune-pane UI work (separate document:
`border-tune-pane-sections-and-picker.md`), the compositor abstraction, and the engine.

## Tasks

- [x] **F1 — RED: a test that fails today.** — commit `e678323`
      `src/scripts_contract.rs` gained an `OverlaySandbox` (the house pattern from
      `reload_coalescer.rs`) that runs the REAL `border.sh` → `assemble.sh` over all
      fourteen presets plus a UI-shaped draft fragment, with the fragment directory
      built inside the sandbox because `assets/fragments/` is gitignored.
      RED output named exactly the predicted offenders: `04_tri` tertiary, `05_spectrum`
      tertiary+error, `07_infinity` tertiary+error, `09_glitch` error, `14_looper`
      tertiary.
- [x] **F2 — GREEN: make the emission data-driven.** — commit `e678323`
      `colors.sh` declares ONE roster (`HVE_PALETTE_TOKENS`, `:19`) plus
      `hve_palette_token_var`, and derives the reset, the failed-step cleanup, the
      fallbacks, the export and the extractor's accepted names from it. `HVE_ERROR`
      gained the lifecycle it lacked, with its own fallback `#f38ba8` (distinct from
      every sibling, honouring the file's own rule). `assemble.sh` emits one line per
      declared role through indirection and carries no palette name or value of its own.
- [x] **F3 — Verify by regenerating.** — the live install was the real surprise; see
      below. `hyprctl configerrors` now returns EMPTY, which is the compositor's own
      verdict rather than our assumption.
- [x] **F4 — Audit follow-ups.** — commit `1bb0c13`. The independent audit refuted two
      things, both real: `colors.sh` promised a loud refusal the code did not implement
      (the `||` bound to `printf`, so an unknown token exported an empty value), and the
      test's reference scanner required exactly `colors = ` so `colors={…}` slipped past.
      Both fixed; two tests prove them, each failing when the fix is reverted.

## Acceptance criteria

- The new test fails before F2 and passes after, and it names the token and the preset
  when it fails — not just "something is wrong".
- No palette value is written by hand in `assemble.sh`; every one comes from
  `colors.sh`.
- `cargo test` stays green (1183+ tests) and `cargo build` has zero warnings.
- The regenerated overlay defines every palette name any preset or UI default can
  reference.
- The keeper reloads Hyprland and the error overlay is gone.

## Progress

- 2026-09-30 — Defect found live by the keeper, root-caused by a read-only
  investigation, documented here.
- 2026-09-30 — F1/F2 landed as `e678323`. Full suite 1186 passed, architecture ratchet
  7 passed, build clean. Independent cross-model audit returned PASS with two refuted
  findings, both then fixed in `1bb0c13` (suite 1188).
- 2026-09-30 — **LIVE FIX APPLIED AND PROVEN.** The repo fix alone did not repair the
  desktop: the running watcher assembles the overlay from an INSTALLED copy at
  `~/.local/bin/assets/scripts/`, dated 2026-09-26, which predated the colour-module
  split and had no `color_sources.d/` at all. Copying only the emitter would have made
  the roster guard refuse loudly and leave the old overlay in place.
  What was done: backed the installed tree up to
  `~/.local/bin/assets.bak-20260930-052601-pre-palette-fix`; copied `assemble.sh`,
  `colors.sh` and `color_sources.d/` into the installed tree; deliberately did NOT
  overwrite `color_watcher.sh`, because it is a RUNNING process (PID 4552) and replacing
  a script under a live bash interpreter corrupts it. `utils.sh` was not needed: the new
  `colors.sh` resolves its own cache path (`colors.sh:102,179`). Then ran the installed
  `assemble.sh`, which wrote the overlay and queued the coalesced reload.
  Result: the palette now carries all seven roles with the THEME's real colours
  (`tertiary = "#9566cc"`, `error = "#c16f31"`, not the fallbacks), the log line still
  reads `Colors from: applied theme snapshot`, and `hyprctl configerrors` returns empty.
  The desktop no longer rejects its config.

## Known debt (named, not fixed)

1. **`assets/scripts/get_colors.sh:10` keeps a second hand-written token list** — six
   names, and it omits `error`. It feeds the JSON that Rust deserialises into
   `ColorScheme`, which has no `error` field, so it is not broken; but it is a second
   list that will age at the roster's pace. Deriving it from the roster would be honest
   with the agnostic norm.
2. **`first_palette_literal` only recognises `#rrggbb`.** A value written as `0xff…` or
   `rgba(…)` inside `assemble.sh` would escape the structural guard. The value test would
   still catch a KNOWN name; an unknown one would slip.
3. **No script under `assets/scripts/` actually enables `set -u`.** The safety the fix
   preserves is a written contract, not an active mechanism. Worth deciding whether to
   make it real.
4. **`HVE_OUTLINE` / `HVE_SHADOW`** are set by the descriptor file mappings but sit
   outside the roster, so the reset does not govern them. Cosmetic today (they are never
   emitted); they are surviving step variables.
5. **The whole class lives in a blind spot**: `assemble.sh` and `color_sources.d/` are
   not in `SCANNED_FILES` (`src/architecture_contract.rs`), so the architecture test does
   not watch that path. This is the same item the capability-routing closure record
   already names (`odd/tasks/hve-capability-routing-closure.md`, open item 4).

## Install lesson worth remembering

A defect fixed in `assets/` is NOT fixed on the keeper's machine until the installed copy
under `~/.local/bin/assets/` is updated. The install is a snapshot; the repo is the
source. Any live verification must first confirm WHICH copy the running process reads.
