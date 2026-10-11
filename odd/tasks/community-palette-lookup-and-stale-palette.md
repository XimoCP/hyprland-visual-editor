# Feature: community palette lookup + no stale palette in a saved theme

**Locator**: `odd/tasks/community-palette-lookup-and-stale-palette.md`
**Engram mirror**: topic `odd/community-palette-lookup-and-stale-palette/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: DONE pending the keeper's live check and re-save (reported 2026-10-11: a theme
declaring the Catppuccin palette paints the JokerTheme palette; authorized to fix)
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~120-180 authored changed lines → single PR, no chaining.
**Route**: delegated writer (one non-trivial file, `src/providers/noctalia.rs`), parent self-check.

## Objective

A theme's saved palette must always match the palette its own record declares — and a
theme must never carry a palette it does not own.

## Problem / proven evidence (2026-10-11)

The keeper's theme declares `community Catppuccin Mocha Sapphire`
(`themes/<name>/providers/noctalia-v5/source.txt`) and the desktop paints a palette
byte-identical to `~/.config/noctalia/palettes/JokerTheme.json`.

Chain, each step verified:

1. Noctalia stores community palettes with URL-ENCODED file names:
   `~/.local/state/noctalia/community-palettes/Catppuccin%20Mocha%20Sapphire.json`
   (spaces as `%20`), whose real content is the Catppuccin sapphire palette —
   `dark.mPrimary = #74c7ec`, `dark.mSurface = #1e1e2e`.
2. `find_palette_path("community", name)` (`src/providers/noctalia.rs:494-512`) builds
   `.../community-palettes/Catppuccin Mocha Sapphire` — the RAW name, unencoded. That
   path does not exist (verified twice: `ls` fails, and no such file exists on the box).
3. `save()` copies the resolved file when it exists and only warns when it does not
   (`noctalia.rs:1627-1660`) — it never removes a palette saved by an EARLIER save.
4. So the theme kept a stale `palette.json` whose md5 equals
   `~/.config/noctalia/palettes/JokerTheme.json` (`98403941f5cacd2ed15f1f300c24685d`,
   `dark.mPrimary = #2ec436`).
5. `apply()` copies that stored palette to
   `~/.config/noctalia/palettes/Catppuccin Mocha Sapphire.json` and sets
   `custom Catppuccin Mocha Sapphire` (`noctalia.rs:2415-2430`), so the live scheme
   carries JokerTheme's colours under the Catppuccin name. `noctalia msg
   color-scheme-get` answers `custom Catppuccin Mocha Sapphire` while the rendered
   `~/.config/hypr/noctalia.lua` paints `#2ec436`.

Consequence: any community palette whose name contains a space can never be captured,
and the failed capture silently keeps the previous palette. Nearly every community
palette has spaces in its name, so this is the normal case, not an edge case.

## Scope (authorized)

- **U1 — resolve the community name the way Noctalia stores it.** In
  `find_palette_path`, try the ENCODED form first (spaces → `%20`, mirroring Noctalia's
  own file naming) and fall back to the raw form, so any existing convention keeps
  working. Put the encoding in a small PURE helper (e.g.
  `community_palette_file_name(name: &str) -> String`) with unit tests: a name with
  spaces, a name without spaces, an empty name. Do NOT percent-encode `%` itself: a
  name that already carries `%20` must not become `%2520`, and the raw fallback covers
  anything the encoded form misses.
- **U2 — a theme must never carry a palette it does not own.** On save, when the
  palette cannot be resolved (custom file missing, community file missing, empty name)
  OR the declared source implies no palette at all (`wallpaper <mode>`, `builtin`),
  REMOVE the theme's saved `palette.json` and log a warning naming the theme and the
  declared source+name. This mirrors the discipline the poster and video records
  already follow (`remove_stale_artifact`). The invariant to hold:
  `providers/<id>/palette.json` exists if and only if the theme's declared source
  resolves to a palette.
  Extract the decision into a PURE function so it is testable without IPC or Noctalia,
  e.g. `palette_capture_action(source: &str, name: &str, resolved: Option<&Path>) ->
  PaletteCaptureAction { Copy, RemoveStale }`.
  Accepted trade-off, state it in the commit: a theme whose community palette is
  genuinely absent on this machine loses its stored palette content on re-save, because
  a save is a CAPTURE of the live truth — keeping a foreign palette is the defect being
  fixed.
- **U3 — tests.** Unit tests for both pure helpers, including the exact live case: a
  name with spaces resolves to the `%20` file name; an unresolvable palette yields
  `RemoveStale`; a resolvable one yields `Copy`. A save-level test that writes a stale
  `palette.json` and a source that cannot resolve, and asserts the file is gone after
  the decision is applied, is welcome IF it needs no IPC; otherwise pin the pure
  decision only and say so.
- **U4 — parent verification + the keeper's live check**: `cargo test`,
  `cargo check --all-targets`, then the keeper re-saves the affected theme with the
  correct palette live and confirms the captured `palette.json` is the Catppuccin one
  (`dark.mPrimary = #74c7ec`), not `#2ec436`.

## Out of scope

- No change to the colour-authority machinery, the yield/re-assert path, the rendered
  templates, the apply order, or the gallery.
- No change to `wallpapers.json`, the poster/video records, or any other provider.
- No touching the keeper's data: the fix changes what a FUTURE save writes; the
  already-saved stale palettes are corrected by re-saving, which the keeper does.

## Acceptance criteria

1. `cargo test` green, `cargo check --all-targets` clean.
2. A community palette whose name has spaces is resolved and copied on save.
3. A theme whose declared palette cannot be resolved does NOT keep a stale
   `palette.json`; the removal is logged with the theme and the declared source.
4. No behaviour change for a custom palette that resolves today.
5. The live check proves the captured palette matches the declared community palette.

## Checks

- `cargo test`
- `cargo check --all-targets`
- Keeper live check: re-save the theme and compare the captured palette against the
  community file.

## Work units

- [x] **U1** — encoded-then-raw community lookup + the pure name helper. — `c27c94c`
- [x] **U2** — the capture decision (Copy / RemoveStale) + its wiring in `save()`. — `5100f55`
- [x] **U3** — unit tests for both helpers. — `ded1da2`
- [x] **U4** — parent verification done; **the keeper's live check and re-save are open**.

## Verification result

- The writer's end-to-end run: `cargo test` 1514 passed, 0 failed; `cargo check --all-targets`
  clean; `Cargo.toml` / `Cargo.lock` untouched.
- The parent re-ran `cargo test providers::noctalia`: 137 passed, 0 failed.
- Mutation check: making the helper stop encoding spaces makes the end-to-end test fail
  (`NotFound`), so the test detects the original defect rather than the fix.
- `community_palette_file_name(name)`: `""` → `""`; otherwise `format!("{}.json",
  name.replace(' ', "%20"))`. `%` is NOT re-encoded, so `%20` never becomes `%2520`.
- `palette_capture_action(source, name, resolved)`: an empty name or nothing resolved →
  `RemoveStale`; `custom`/`community` → `Copy`; any other source → `RemoveStale`.
- **Second defect found on the way:** the community lookup also omitted the `.json`
  extension, so even a name without spaces could never be captured. Both are fixed.
- **What the keeper must do:** re-save the affected theme(s) with the correct palette live.
  The already-saved stale `palette.json` cannot be reconstructed from the theme — only a
  fresh capture writes the right content.

## Progress log

- 2026-10-11: chain proven from the live box (encoded community file, failed raw lookup,
  stale palette identical to `JokerTheme.json`); contract written.
