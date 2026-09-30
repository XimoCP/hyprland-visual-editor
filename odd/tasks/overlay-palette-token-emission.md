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

- [ ] **F1 — RED: a test that fails today.** In `src/scripts_contract.rs` (or the
      existing sandbox harness style), run the **real** `assemble.sh` over every file in
      `assets/borders/` and assert that every bare palette token each preset references
      is defined in the emitted `[SYSTEM: COLORS]` block. It must fail on the current
      code for the right reason. Generate the fragment directory inside the sandbox —
      `assets/fragments/` is gitignored, so the test must not depend on it.
- [ ] **F2 — GREEN: make the emission data-driven.** Emit every token the presets and
      the specs promise (`primary`, `secondary`, `tertiary`, `error`, `surface`,
      `surface_lowest`, `accent`), driven from one declared list rather than repeated
      literal lines. Give `HVE_ERROR` the same lifecycle treatment as the others in
      `colors.sh` (reset, failed-step cleanup, fallback, export) so "forgot to reset or
      export one" cannot recur. No value may be hardcoded in `assemble.sh`.
- [ ] **F3 — Verify by regenerating.** Confirm the emitted overlay defines every
      referenced token, and hand the keeper the exact command to regenerate the live
      overlay so the Hyprland error disappears. The keeper's own reload is the only
      valid proof that Hyprland now accepts the file.

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
  investigation, documented here. No source file touched yet.
