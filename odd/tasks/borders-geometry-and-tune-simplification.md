# Feature: borders geometry fast apply + tune simplification (HVE 2)

**Locator**: `odd/tasks/borders-geometry-and-tune-simplification.md`
**Engram mirror**: topic `odd/borders-geometry-and-tune-simplification/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches, do NOT rebase)
**Checkpoint at open**: `9d9e631`, suite 1003 passed / 0 failed, 0 warnings
**TDD mode**: strict (runner `cargo test`)
**Writers**: DeepSeek v4-flash. **Verifiers**: GLM 5.3-flash (cross-model, never the writer).

## Objective

Make the geometry sliders (border size and the three geometry values) feel instant on the
live desktop, keep the value durable across a config reload, and trim the Borders tune pane
down to what the keeper still wants (Colors, Angle, Inactive color, Halo/Shadow, Save).

## Problem

Touching the border thickness today runs `assets/scripts/geometry.sh` + `assemble.sh` on every
debounced tick: **100 ms per adjustment**, ~180 ms with the 80 ms slider debounce (measured
with numbers, 2026-09-26). The same effect is reachable in **5 ms** through
`hyprctl eval 'hl.config({...})'` — the tool the theme transition already uses (W1).

A second front: the U3a rebuild removed the radius / gap_in / gap_out sliders from the Borders
section and left only the thickness. The plumbing is still whole underneath, so recovering them
is wiring, not new machinery. The keeper wants them back **at the top, next to the size**, as a
"Geometry" block, and asked for a short personal comment near those parameters saying he put
them there because he did not know where else they belonged.

The tune pane then grew too much: past Halo/Shadow the keeper wants everything else gone
(animations x3, curves, rules).

## Scope

- In: `on_panel_apply_geometry` and the geometry apply path, `geometry.sh` / `assemble.sh` usage,
  the Borders section geometry block, the tune pane contents, i18n keys for the added strings.
- Out: the engine (`src/engine.rs` internals stay), the presets themselves, monitors/HDR (hyprmod).
- Out: the theme transition and the wallpaper engine (closed work; do not reopen).

## Constraints

- One writer per tree. Verification with a DIFFERENT model (writer DeepSeek, verifier GLM).
- The value MUST survive `hyprctl reload`; a runtime `hyprctl eval` override does not survive it
  (verified 2026-09-25), so persistence cannot be dropped, only deferred.
- No AI attribution in commits. Conventional commits.
- Engineered artifacts in English (code, comments, UI copy, docs). Comments may carry the
  keeper's personal note where he explicitly asked for it.
- Visual verification is mandatory for any `.slint` change: run the headless render test, READ
  the PNGs it writes under `/tmp/opencode/`, and extend/add a render test for any new state.

## Acceptance criteria

1. Dragging the thickness slider reaches the compositor in the millisecond range (hot apply),
   not the ~180 ms path; the persisted write happens without being felt on every tick.
2. After `hyprctl reload`, the applied geometry is still there (durability preserved).
3. Radius / gap_in / gap_out are editable again from the Borders section, at the top, next to
   the size, and a change to any of them does not wipe the other three.
4. The tune pane keeps Colors, Angle, Inactive color, Halo/Shadow and Save; animations, curves
   and rules are gone; presets that carry their own animation in their `.lua` still animate.
5. `cargo test` green, 0 warnings; render PNGs inspected.

## Tasks

- [x] **B1 — fast geometry with a single deferred persist.**
  Route: delegated writer (2+ non-trivial files: `src/main.rs` + `ui/panel/sections/BordersSection.slint`).
  - Establish first, with runtime evidence, what the current path really does: does the tick
    reach the live compositor, does the on-disk fragment change, does the value survive
    `hyprctl reload`. (The keeper explicitly asked to confirm the value-saving mechanism before
    trusting the speed-up.)
  - Apply in hot with `hypr_eval` (`src/main.rs:1410`), sending only the options that actually
    changed, and coalesce the persistent `apply_geometry()` so a drag writes once instead of
    once per tick.
  - Tests: a failing test first for "only changed options are pushed hot", plus coverage for
    the coalesced persist.
- [ ] **B2 — recover the radius / gap_in / gap_out sliders** at the top of Borders, as a
  "Geometry" block next to size, with the keeper's personal comment. Watch the engaged-slider
  seat numbers left free by U3a.
- [ ] **B3 — simplify the tune pane** to Colors, Angle, Inactive color, Halo/Shadow, Save.
  Delicate: touches the save path. Do it last.
- [ ] **B4 — i18n** for everything added (preset `@Title`/`@Desc` are English-only today and not
  wired to i18n; `13_the_joker` also has Spanish comments and no `@Color`).

## Key files

- `ui/panel/sections/BordersSection.slint` — the section and the tune pane (largest file).
- `ui/panel/PanelRoot.slint:773-776` — the four geometry bindings into the section.
- `ui/components.slint` — `GeometrySlider`.
- `src/main.rs:4110` `on_panel_apply_geometry`; `src/main.rs:1410` `hypr_eval`.
- `src/engine.rs:179` `apply_geometry`; `src/app_state.rs:81`; `src/config.rs`.
- `assets/scripts/geometry.sh`, `assets/scripts/assemble.sh` — the slow path.

## Progress

- 2026-09-26: document opened; B1 delegated. Nothing written to source yet.
- 2026-09-26: **B1 done** (this commit). Step 0 established the current path live on Hyprland
  0.56.2: one `geometry.sh 5 44 8 10` tick changed the fragment, and the live compositor
  followed (`border_size 2→5`, `rounding 32→44`, `gaps 5→8/10`) ~600 ms later through exactly
  ONE coalesced `hyprctl reload` (measured with a logging `hyprctl` wrapper). A 200 ms two-tick
  burst produced ONE reload and the last value; a manual `hyprctl reload` kept the file-backed
  value, while `hyprctl eval` was 5 ms but was wiped by the next reload — the durability
  premise holds. Path cost measured 175 ms per tick. Implementation: hot apply pushes only the
  changed options as one `hl.config({...})` chunk via `hypr_eval`; the persistent
  `apply_geometry()` is coalesced behind a 400 ms single-shot timer (`arm_geometry_persist` +
  `GeometryPersistCoalescer`), so a drag writes once. The four exact chunk shapes were accepted
  live by `hyprctl eval`. Full suite 1010 passed / 0 failed, 0 warnings. Desktop state restored
  and verified (2/32/5/5, fragment + overlay md5 matched baseline). No `.slint` change was
  needed: the timer already forwards the live `root.geom-*` values, so the "radius and gaps
  travel with the size" line at BordersSection.slint:2149 is correct as-is and was left intact.

## Next step

B2: recover the radius / gap_in / gap_out sliders at the top of Borders as a "Geometry" block
next to size, with the keeper's personal comment.
