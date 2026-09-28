# Feature: the applied theme owns the palette

**Locator**: `odd/tasks/theme-owns-the-palette.md`
**Engram mirror**: topic `odd/theme-owns-the-palette/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Reported**: 2026-09-28 by the keeper: "he vuelto y he abierto después de una inactividad
y me has puesto una paleta de colores diferente".
**Status**: DONE — implemented in `9f93082` (descriptor, central scripts, provider re-assert) and
then re-homed by Phase 1 of `odd/tasks/hve-capability-routing.md`, where the backend took over the
action and the scripts lost every integration detail. Verification: PASS with six findings, all
closed (`84b1e53`).
**TDD**: strict — runner `cargo test`; the script behaviour is tested through the hermetic
sandbox harnesses that already exist (`src/scripts_contract.rs`, `src/watcher.rs`).
**Route**: one bounded unit at a time, free-model writers; verification by a different model.

## The keeper's rule (verbatim intent)

"Si se aplica un tema, el tema es el que manda por encima de todo. Si luego el usuario quiere
manualmente cambiar el fondo con skwd o la paleta, que le deje HVE — pero automáticamente no:
que sea una decisión del usuario, no automática, ni un secuestro de otra aplicación."

## Evidence of the hijack (measured 2026-09-28)

- 17:51:55 the machine resumed from suspend (`PM: suspend exit`).
- 17:52:05.6 the wallpaper engine (`skwd-wall-vk`, playing the animated
  `ellen-joe-...mp4`) rewrote `~/.config/noctalia/palettes/skwd-wall.json` with an amber set
  (`mPrimary #e4aa67`, `mSecondary #d8dd55`, `mSurface #291f14`).
- The live Noctalia scheme became `custom skwd-wall` and `~/.config/hypr/noctalia.lua` rendered
  `primary = rgb(e4aa67)`.
- 17:52:06 `assets/scripts/color_watcher.sh` detected the change and regenerated HVE's overlay
  from the **live** palette: `~/.cache/hve/colors.json` → `primary #e4aa67`, and the UI painted
  amber.
- The applied theme `Animation` had saved, on 27-sep 08:11, `custom ...mp4-vib` with
  `mPrimary #67abe4` (blue/steel). Its snapshot was intact and ignored.
- The same sequence repeated at 18:02:07 — 10 minutes later, with no resume in between, so it can
  happen unattended at any time. (The wallpaper-delegation spec already de-scopes Noctalia's
  "600s automation".)

## Root cause

The colour pipeline has no theme authority. `assets/scripts/colors.sh::hve_load_colors()` resolves
colours only from **live** state (Noctalia's `color-scheme-get` + palette file, then the rendered
`noctalia.lua`, pywal, matugen, manual); no path in the scripts ever reads a theme's saved
`palette.json`. `color_watcher.sh` watches the palette files and `settings.toml`, so **any**
external write repaints HVE. The apply-time re-assert in Rust
(`src/providers/noctalia.rs::reassert_custom_scheme_blocking`) is bounded to ~10 s and stops, and
the watcher is a follower with no ownership signal to consult.

## Units

- [ ] **U1 — the theme snapshot is the colour source** (`assets/scripts/colors.sh`). Factor the
  palette extraction/validation out of `_hve_try_noctalia_palette` into a reusable helper, add a new
  first-priority `_hve_try_theme_palette`: when `config.json#last_applied_theme` names a theme that
  carries `providers/noctalia-v5/palette.json` whose `source.txt` first token is `custom`, resolve
  the colours from that snapshot. Nothing else in the chain changes; with no theme applied, or with
  a `wallpaper`-source theme, behaviour is exactly today's.
- [ ] **U2 — the watcher re-asserts the theme instead of following the hijack**
  (`assets/scripts/color_watcher.sh` + the shared helper). On an external palette/settings change
  while the theme owns colours: copy the snapshot over the live palette name, `color-scheme-set
  custom <name>`, `templates-apply`, regenerate the overlay — once per event, with a cooldown, and
  re-hashing whatever it wrote so its own write cannot re-trigger the watcher. Same guard on watcher
  start, so a drift that happened while HVE was off is repaired when it starts.
- [ ] **U3 — the rule is recorded** (this document + `AGENTS.md` if the rule belongs there) and the
  known limitation written honestly: while a theme is applied, a deliberate external palette change
  is countered; the sanctioned way to change colours is to apply a theme or a palette from HVE.

## Out of scope

- Rewriting the engine's colour pipeline: this is a gate plus one priority path, not a redesign.
- `src/providers/noctalia.rs` apply-time behaviour and the skwd policy yield: untouched. The gate
  must never fire while HVE has yielded colour authority to the engine (`theme.policy == wallpaper`
  / `skwd_wall_owns_color_scheme`), and it does not need to fire for a `wallpaper`-source theme,
  because such a theme has no palette snapshot.

## Acceptance criteria

1. With `Animation` (a `custom` snapshot) applied, `get_colors.sh` returns the snapshot's colours
   even when the live scheme is `custom skwd-wall` with different values.
2. With no theme applied, and with a `wallpaper`-source theme applied, `get_colors.sh` returns
   exactly what it returns today (regression pinned).
3. An external rewrite of a watched palette file while the theme owns colours does not leave HVE
   painting the external palette, and produces exactly one re-assert (no loop, no ping-pong).
4. A theme name coming from `config.json` can never be interpolated into a path unchecked
   (the existing security posture of this script is preserved: name validated, no eval, 0600 temp
   files, values re-validated as `#rrggbb`).
5. `cargo test` green, 0 warnings; no existing test weakened.

## Checks

- U1 RED: sandbox in `src/scripts_contract.rs` — plant `config.json` + a theme snapshot, stub
  `noctalia color-scheme-get` to report a DIFFERENT live palette, assert `get_colors.sh` prints the
  snapshot. Today it prints the live ones → RED.
- U2 RED: sandbox in `src/watcher.rs` — theme applied, arm the watcher, rewrite the watched palette,
  assert the external values never reach the overlay and that the stub `noctalia` recorded exactly
  one `color-scheme-set custom <theme palette>`; plus a no-theme regression case.
- Cross-model verification after the units land, then the keeper's live test (the only valid check
  for what the desktop shows).
