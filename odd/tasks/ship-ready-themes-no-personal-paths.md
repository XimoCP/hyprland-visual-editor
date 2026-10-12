# Feature: ship-ready themes — a theme package carries no personal path

**Locator**: `odd/tasks/ship-ready-themes-no-personal-paths.md`
**Engram mirror**: topic `odd/ship-ready-themes-no-personal-paths/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: **CLOSED — keeper sign-off 2026-10-12** ("todo lo revisado tiene mi OK"). Authorized
2026-10-11: "empaquetes todos los temas que tenemos ya que van a ser los que se van a quedar...
hay que limpiar de rutas personales y dejar listos para subir que son los temas que va a ver la
gente". The ship-ready set is in `assets/themes/`, verified clean, committed and pushed with the
branch.
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~180-260 authored changed lines → single PR, no chaining.
**Route**: delegated writer (`src/providers/noctalia.rs`, `src/theme_package.rs`), parent self-check.

## Objective

A theme package must never carry a path that belongs to the machine that saved it.
Everything the LOOK needs travels inside the theme; every machine-only value is dropped so
the target machine keeps its own. This is the policy the keeper chose on 2026-10-11:
"lo que define el aspecto va adentro; los valores de mi máquina se quedan fuera".

## Problem / verified evidence

Themes saved today carry the keeper's absolute paths. Real counts in the shipped set
(`assets/themes/*/providers/noctalia-v5/settings.toml`, lines matching `= "/`):
Animation 11, joke2 11, ClasicoV4 10, Cars 6, BordesRedondosCentrados 5. Observed keys:

- `[shell]` hooks: `colors_changed`, `shutdown`, `started` → `/home/ximo/.local/share/sddm/themes/SilentDinamic/sddm-sync.sh`
  (not only a broken path on a foreign machine: it points at SOMEONE ELSE'S SCRIPT).
- `[wallpaper] directory` → the keeper's wallpaper folder.
- `[wallpaper.default].path`, `[wallpaper.last].path`, `[wallpaper.monitors.<name>].path` → the keeper's images.
- screenshot `directory`, `[plugin_settings."noctalia/mpvpaper"] video_directory`, `[widget.*] path`.
- `providers/noctalia-v5/wallpaper.txt` and `video.txt` → absolute background records.

`settings.toml` is copied verbatim from `~/.local/state/noctalia/settings.toml`
(`noctalia.rs:1799`) and restored verbatim on apply (`noctalia.rs:2033`), so a foreign
theme overwrites the target's settings with the keeper's paths.

## Design decisions

- **D1 — no new dependency.** The project already reads `settings.toml` line-based
  (`mpvpaper_enabled_in_settings`), so the sanitizer is line-based too: no `toml` crate.
- **D2 — drop, do not fake.** An absolute value that is NOT inside the theme is dropped
  (the whole `key = "..."` line). Emptying it or rewriting it to a guess would be a lie.
- **D3 — rewrite only what lives inside.** An absolute value that IS inside the theme is
  rewritten to the theme-relative path, so a packaged asset stays reachable.
- **D4 — everything else stays byte-identical.** Bar, dock, widgets, OSD, colours,
  templates, presets, geometry: untouched. No reformatting, no reordering, no comment
  changes.
- **D5 — the absolute background records go.** `wallpaper.txt` / `video.txt` name the
  keeper's files; the packaged `poster.txt` / `video-media.txt` is what travels and wins
  at apply. When a theme has no packaged equivalent, the record still goes and the drop is
  REPORTED, because a foreign absolute path is the defect being removed.
- **D6 — a safety net.** After sanitizing, scan every text file inside the theme for
  remaining absolute-path-looking strings and report them. A future leak must be visible
  at package time, not discovered by a user.

## Scope (authorized)

- **U1 — `sanitize_settings_toml(text: &str, theme_dir: &Path) -> (String, SanitizeReport)`**,
  pure and unit-tested. Line-based: rewrite values inside the theme to theme-relative,
  drop the line for any other absolute value, keep every other byte. `SanitizeReport`
  counts rewritten and dropped entries and names the dropped keys. Tests: a hook line is
  dropped; an absolute path inside the theme is rewritten; a relative value and a
  non-path value are untouched; a comment or a blank line is untouched; a section header
  survives its emptied body.
- **U2 — the records and the wiring.** Drop `wallpaper.txt` / `video.txt` at save and at
  package time, with a report when no packaged equivalent exists. Wire the sanitizer into
  the noctalia save path (the theme is clean from the moment it is saved) and into
  `theme_package::package_theme_dir` (so `hve --package-themes` re-cleans an old theme),
  logging what was dropped.
- **U3 — the safety-net scan** `scan_theme_for_absolute_paths(theme_dir) -> Vec<(PathBuf, String)>`
  over the theme's text files (skip `media/` binaries), reported as a warning naming each
  file and value. Tests: a theme with a planted absolute path reports it; a clean theme
  reports nothing; binary media is not scanned.
- **U4 — parent: produce the ship-ready set.** Copy the KEPT themes to a staging root,
  run the packaging against that root (`XDG_CONFIG_HOME=<staging> cargo run -- --package-themes`),
  verify no `/home/` survives anywhere in the result, then place the cleaned set in
  `assets/themes/`.

## The kept set (keeper's decision, 2026-10-11)

The seven v5 themes in `~/.config/hve/themes/`: `Cars`, `Fantasy`, `JoKer`, `Night`,
`Space`, `Spiderman`, `Work`. The two legacy v4 themes (`jokertheme2`, `MiniJoker`) are
NOT part of it: their backend was removed today and they are invisible.

The five themes already in `assets/themes/` (`Animation`, `BordesRedondosCentrados`,
`Cars`, `ClasicoV4`, `joke2`) are the OLDER shipped set. `Cars` exists in both. This
contract REPLACES the shipped set with the kept one; the old names that no longer exist
in the keeper's folder are removed from `assets/themes/` and the change is stated in the
report so the keeper can object before pushing.

## Out of scope

- No change to the apply path, the colour pipeline, the gallery, or the palette capture.
- No new dependency.
- Nothing under `~/.config/hve/`: the keeper's live themes are NOT rewritten tonight.
  They become clean on their next save, once U2 is wired. The shipped copies are produced
  from a staging root.

## Acceptance criteria

1. `cargo test` green, `cargo check --all-targets` clean.
2. `sanitize_settings_toml` drops every machine-only absolute value and rewrites the ones
   inside the theme, leaving every other byte identical.
3. A packaged theme has no `wallpaper.txt` / `video.txt`.
4. `scan_theme_for_absolute_paths` finds a planted absolute path and reports a clean theme
   as clean.
5. After U4, `grep -rn "/home/"` over the new `assets/themes/` set returns NOTHING.
6. The cleaned themes still carry their media, presets, palette and geometry.

## Checks

- `cargo test`
- `cargo check --all-targets`
- `grep -rn "/home/\|/usr/\|/etc/" assets/themes/` → must be empty
- Keeper review before any push.

## Work units

- [x] **U1** — `sanitize_settings_toml` + `SanitizeReport` + tests. — `cc0f56c`
- [x] **U2** — record drops + wiring into save and package + logging. — `91a5f60`
- [x] **U3** — `scan_theme_for_absolute_paths` + tests. — `8efef08`
- [x] **U4** — parent: staging package run, verification, place the ship-ready set. — `99cacc0`

## U4 result

Staging run: the eleven themes (the seven kept plus the four then-shipped) were copied to
`/tmp/opencode/ship-themes/hve/themes/` and packaged with
`XDG_CONFIG_HOME=/tmp/opencode/ship-themes cargo run -- --package-themes`. All eleven
reported `poster=yes`; `Animation`, `Fantasy` and `Night` also packaged their video.

Verification of the result: `grep -rn "/home/\|/usr/\|/etc/"` over the staged set returns
NOTHING; no `wallpaper.txt` / `video.txt` survives; every theme keeps `media/poster.*` and
`poster.txt`.

`assets/themes/` now holds exactly the seven kept themes plus this README. The previous
shipped set (`Animation`, `BordesRedondosCentrados`, `ClasicoV4`, `joke2`) is removed from
the repo and preserved in git history and in
`~/Proyectos/hve-backups/hve-assets-themes-20261011-041416.tar.gz`.

## Findings (reported, not fixed)

1. **A slugified path is not an absolute path.** `Night`'s palette name was the wallpaper
   engine's slug of the keeper's home path (`home-ximo-pictures-livewallpapers-...-vib`),
   recorded in BOTH `source.txt` and `settings.toml` (`custom_palette`). The sanitizer
   only drops values that start with `/`, so it missed both. Hand-fixed in the shipped
   copy by renaming to `Night-palette` in both places (the palette content is untouched,
   and the two names agree, so apply still restores it). FOLLOW-UP: teach the sanitizer
   this class, or make the save path never record a machine-derived palette name.
2. **Stale preset references in the keeper's themes.** `JoKer` records
   `active_anim_file: "prueba1"` and `Space` records `active_border_file: "Space"`.
   Neither resolves: the user store holds `~/.config/hve/presets/borders/Space.lua` (the
   record lacks the `.lua` extension) and no `prueba1` anywhere. The app clears the marker
   when a name matches no preset, so both themes ship without that preset taking effect.
   FIX (keeper, in the app): open each theme, re-select its border/animation preset and
   re-save — the bundler copies exactly the presets a theme references.
3. **`Work` travelled with its poster only.** Its video had no packaged equivalent and no
   known url, so the drop was reported and the theme keeps its static background.
4. **Budget**: the contract forecast 180-260 authored lines; the three code units landed
   ~1043 insertions, almost all tests, including ten existing tests that had to be
   rewritten because they pinned the old "keep the absolute record" contract.
5. **Consequence of dropping the absolute records, stated by the writer.** `saved_theme_wallpaper`
   and `declared_backgrounds` read `wallpaper.txt`, so a saved theme is no longer recognised
   as "active" through that record (it keeps its packaged poster, and the seam degrades to
   "cannot tell" rather than to a wrong answer). Re-packaging an already-clean theme reports
   `skipped (no background record)`, because the records the resolver used are gone by
   design. Neither changes what the shipped themes look like.

## Progress log

- 2026-10-11 (night): policy confirmed by the keeper; design decided (D1-D6); contract
  written. The palette fix landed first (`c27c94c`, `5100f55`, `ded1da2`) because the
  keeper's live theme was painting a foreign palette.
