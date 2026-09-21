# Feature: gallery video-card preview + theme-delete linkage

**Locator**: `odd/tasks/gallery-video-preview-and-delete-linkage.md`
**Engram mirror**: topic `odd/gallery-video-preview-and-delete-linkage/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: PLANNED (keeper authorized 2026-09-21: "investiga y crea una vinculación para que si se borra de settings se borre también de donde tú dices si es preciso")
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~150-220 authored changed lines (additions + deletions), well under the 400-line review budget → single PR, no chaining.

## Source (verified evidence, 2026-09-21)

### S1 — Animated-theme cards render an EMPTY panel (keeper's screenshot, focused card `ThemasAnimados`)

The keeper reported: "la captura de la imagen desde skwd-wall para nuestro slider no se crea, se queda tal que así" (focused card = dark empty panel; neighbours show images).

Root cause (code + disk evidence):
- The gallery resolves a card's preview source through `thumbs::find_source_image` (`src/shell/gallery/thumbs.rs:463`). Its chain is: `video_assignment()` (legacy `providers/*/mpvpaper-assignments.json`, `:381`) → `wallpaper_from_provider_json` → `wallpaper_from_txt` (`:355`, reads `providers/noctalia-v5/wallpaper.txt` and `providers/noctalia/wallpaper.txt`) → recursive file scan for `SOURCE_EXTS` → `None`.
- The video path already renders a frame via ffmpeg (`extract_video_frame`, `:423`, `ffmpeg -ss 1 -i <video> -frames:v 1`), but it is reachable ONLY through `video_assignment()`, i.e. through the legacy mpvpaper manifest.
- `NO` theme in `~/.config/hve/themes/` has `mpvpaper-assignments.json` (verified with `find`). The current save path writes a different, newer authority record: `providers/noctalia-v5/video.txt` (`PAINTER_VIDEO_FILE`, `src/providers/wallpaper_authority.rs:1114`; written by `src/providers/noctalia.rs:1016-1048`, read for restore at `:1144+`, parser `parse_painter_video_record`, `:1139`).
- `ThemasAnimados` holds only `video.txt` (`path=/home/ximo/Pictures/LiveWallpapers/ellen-joe-...mp4`, `painter=skwd-wall-vk`) and NO image anywhere in its tree → `find_source_image` returns `None` → `plan_jobs` (`:520`) drops the card → no `thumb`/`hero`/`slat`/`slat-exp` → empty card.
- Empirical: the very same video extracts cleanly (`ffmpeg` yields a 3840x2160 PNG), and an ORPHANED cached frame for it already exists at `~/.cache/hve/thumbs/c628e057fad10c72-frame.png` — nothing references it, because the only consumer looks for the wrong file.

### S2 — The "(test N)" theme duplicates are INVISIBLE, not deleted

The keeper deleted duplicates from the panel's Save section and the carousel shows no repeats. The theme directories nevertheless still exist (20 dirs, all with mtime 2026-09-19, verified twice). Explanation, verified in code:
- `ThemeManager::list` (`src/theme_manager.rs:269`) enumerates every directory but then keeps only themes whose provider ids are ALL currently registered (`:303-310`).
- The registered set is shell-detected: `noctalia-v5` + `hve-presets` (`src/providers/mod.rs`, `register_default_providers`). The old id `noctalia` (v4, `src/providers/noctalia.rs:492`) is not registered.
- `jokertheme2*` and `MiniJoker*` (12 dirs) declare `providers: ["noctalia", "hve-presets"]` → filtered OUT of both the gallery and any list built from the same manager. `BordesRedondosCentrados (test 6)` declares `noctalia-v5` → it IS listed but has a real preview, so it is not visibly a "duplicate" in the ring.
- Consequence: nothing was deleted. The keeper is right that the slider shows no repeats; both observations are true at once.

### S3 — Palette linkage on delete

- `providers/noctalia-v5/source.txt` holds `custom <PaletteName>`; on apply, HVE copies the theme's saved `palette.json` to `~/.config/noctalia/palettes/<PaletteName>.json` and points Noctalia at it (`src/providers/noctalia.rs:1255-1270`).
- The palette name is SHARED across themes today: `BordesRedondosCentrados`, `BordesRedondosCentrados (test 6)`, `ClasicoV4` and `new_joker` all carry `custom JokerTheme`; `ThemasAnimados` carries `custom skwd-wall`; `Cars` carries `wallpaper vibrant` (not a custom palette → no palette file).
- Therefore "delete theme → delete its palette" MUST be reference-counted, or deleting one of those four breaks the other three.

## Work units

### U1 — the gallery understands the painter video record (the visible bug)

`find_source_image` must consult the painter video record BEFORE the legacy chain:

1. Read `providers/noctalia-v5/video.txt` (via `wallpaper_authority::PAINTER_VIDEO_FILE`) and parse the path with the existing `parse_painter_video_record`.
2. When the path exists and `is_video_source` holds, return a frame through the existing `extract_video_frame` (cached frame wins).
3. When extraction fails, log a warning naming the video and the theme, then fall through to the legacy chain exactly as today (no behaviour change when there is no record).
4. Keep the existing order guarantee: an assigned IMAGE still wins over a video record (a saved static assignment is what the desktop shows); the video record must be consulted at the same priority position as the legacy video assignment.

Acceptance: a theme directory whose only background evidence is `video.txt` yields a preview source; a `video.txt` pointing at a missing file falls through without panicking and without a bogus source.

### U2 — deleting a theme also removes its derived artifacts

`ThemeManager::delete` (`src/theme_manager.rs:398`) keeps its contract (remove the theme dir; clear `last_applied`) and gains a cleanup step that runs while the theme dir still exists:

1. Cached bake artifacts: compute the content cache key the same way the bake does (`content_cache_key_from_bytes` over the theme's resolved source) and remove `<cache_dir>/<key>-thumb.png`, `-hero.png`, `-slat.png`, `-slat-exp.png`, plus `<key>-frame.png` for a video source. Content-addressed keys make a source shared with another theme a non-event (worst case: one re-bake).
2. Noctalia palette: remove `~/.config/noctalia/palettes/<PaletteName>.json` ONLY when NO remaining theme references `<PaletteName>` (scan the surviving theme dirs' `providers/*/source.txt` for `custom <name>`). When another theme still uses it, leave the file untouched.
3. Every removal is best-effort and logged; a failure to clean a derived artifact must never fail the delete itself (the theme dir removal stays the only hard requirement).
4. Deleting a theme must not touch anything outside the theme dir, the bake cache and the palette file.

Acceptance: with two themes sharing `custom JokerTheme`, deleting one leaves `JokerTheme.json` in place; deleting the last one removes it. Cached PNGs for the deleted theme's source are gone; an unrelated theme's cache is untouched.

### U3 — visual proof for the animated card

Extend the headless render coverage (`cargo test slice_focus_flow_renders` and/or a sibling) so a card whose preview comes from a video source renders a NON-EMPTY expanded panel, then READ the produced PNGs and confirm the geometry/look by inspection before reporting done. A UI change verified only by "tests green + build clean" is not verified in this repo.

## Out of scope (recorded debt — do NOT fix here)

- Deleting the 12 invisible legacy-provider themes from the keeper's disk. Whether they should be purged, re-provider-mapped, or left alone is the keeper's call and a separate change.
- Why the Save section did not list those hidden themes; whether it SHOULD surface them is a product decision, not this fix.
- Pruning the whole bake cache, adding an LRU, or deleting palettes for themes removed outside HVE.
- Any `.slint` redesign: no visual layout change is authorized here, only the data feeding the existing cards.
- Retirement of the legacy `mpvpaper-assignments.json` reader in `video_assignment` (still used by older snapshots).

## Constraints

- Preserve the engine seam: this change stays in `src/shell/gallery/*` and `src/theme_manager.rs`; no provider rewrite.
- Reuse, do not duplicate: `PAINTER_VIDEO_FILE`, `parse_painter_video_record`, `extract_video_frame`, `content_cache_key_from_bytes`, `is_video_source`. No second parser for `video.txt`.
- Engineered artifacts in English; keeper replies in neutral Spanish.
- One module at a time; tests green before reporting.

## Checks (exact commands)

1. `cargo test` — full suite must stay green (baseline 843 passed / 0 failed).
2. `cargo test slice_focus_flow_renders` — then READ the PNGs it writes under `/tmp/opencode/`.
3. `cargo check` — zero new warnings (the suite already carries two pre-existing release-build warnings; do not add more).

## TDD

- Mode: strict (project default, `openspec/config.yaml`).
- Source of the mode: HVE 2 skill hard rule ("write the failing test first") + AGENTS.md.
- Runner: `cargo test`. RED before each unit, GREEN after, then refactor.

## Route per task

| Unit | Route | Evidence |
|------|-------|----------|
| U1 | delegated writer (2 files: `thumbs.rs` + tests) | write trigger: non-trivial edit in the gallery bake path |
| U2 | delegated writer (2 files: `theme_manager.rs` + tests) | write trigger: delete-path behaviour change |
| U3 | delegated writer, same session as U2 | visual verification requirement |

## Work units added after the first verification round

The keeper's live test confirmed U1 on real data (the animated card now shows its frame). Two
independent verification rounds followed and drove the units below.

### W2 — close the defects the first verifier reproduced (commits `98e5879`, `7ea0c01`, `6bbc898`)

- **W2-1 (was MAJOR):** planning and extraction were split so no UI-thread path can spawn a
  process. `plan_preview_source` reads records and probes existence only; the synchronous
  `ffmpeg` moved into the existing `preheat` worker, with `PreviewSpec::Video` carrying the theme
  dir so an undecodable video still falls back to the static chain off-thread.
- **W2-2 (was MINOR, data loss):** delete cleanup could remove a user file outside the bake cache
  through a `<cache>/../user/secret-frame.png` record, because `Path::starts_with` is
  component-wise and ignores `..`. Containment is now canonicalized on both sides and fails closed.
- **W2-3 (was MINOR):** the shared-palette reference count missed tab separators, `_`/`.`-prefixed
  survivors and unreadable `providers` dirs. It now splits on any whitespace, scans every directory
  except the deleted one, and fails OPEN (keeps the palette) when a survivor cannot be read.
- **W2-4 (was MINOR):** deleting the currently applied theme removed the LIVE palette. It is now
  skipped with a log line.
- **W2-5:** both release-build warnings removed (dead `is_video_painter`, unread
  `ActiveBackground::output_name`). `cargo check` and `cargo build --release` print ZERO warnings.

### W4 — custom palette re-asserted when the wallpaper engine owns the scheme (commit `f28b670`)

Root cause of the keeper's live complaint ("mis colores no se aplican, los pisa el fondo"):
HVE hands the theme wallpaper to the skwd daemon (`noctalia.rs:1242`), whose live config
(`~/.config/skwd-wall-v2/config.json`: `theme.policy: wallpaper`, `noctalia.themeMode: follow`,
`authority: noctalia`, `matugen: true`) regenerates a wallpaper-derived palette ASYNCHRONOUSLY
(observed writing `palettes/skwd-wall.json`, `settings.toml` and `wal/colors.json` for ~80 s) and
re-imposes `custom skwd-wall` after HVE already set `custom JokerTheme`. The theme snapshots are
byte-identical to the live `JokerTheme.json` (sha256 `6338ff37…`), so HVE restores the right
artifact: it simply lost the race silently.

W4 makes HVE detect that ownership and re-assert the theme's custom palette once: off the UI
thread, settle 2 s, at most 3 verified attempts, hard deadline 8 s, single-shot per apply, every
failure logged and swallowed, and an honest `warn!` naming the knob
(`theme.policy` / `noctalia.themeMode`) when the engine wins anyway. Non-custom palettes (`Cars`
uses `wallpaper vibrant`) issue zero extra commands.

### W5 — symlink containment on delete (commit `3626ec6`)

The second verifier reproduced a hole in W2-2: an OUTSIDE symlink whose target lives inside the
cache canonicalizes as "inside", so the delete removed the outside link entry. `remove_cached_frame`
now requires a non-symlink (`symlink_metadata`) direct child whose parent canonicalizes to exactly
the cache dir; anything else fails closed. Both directions are covered by a test.

## Verification rounds (independent model, read-only, never the writer)

| Round | Scope | Verdict | Outcome |
|-------|-------|---------|---------|
| 1 | `d695941`, `747b8b3` | **FAIL** | 1 MAJOR (ffmpeg on the UI thread) + 3 MINOR (outside-file deletion via `..`, palette refcount false negatives, live palette deleted) + U3 gap → drove W2 |
| 2 | `98e5879`, `7ea0c01`, `6bbc898`, `f28b670` | **PASS** | 868/0 reproduced, zero warnings, live desktop untouched (canary on PATH logged a single pre-existing read-only call); one MINOR symlink gap → drove W5 |

Both rounds ran with a canary/stub harness and confirmed the keeper's real files
(`settings.toml`, palettes, skwd config) were byte-identical before and after the suite.

## Residual risks (deliberately NOT closed here)

1. **Race window.** The re-assert is bounded to ~10 s by design; the observed engine churn ran
   ~80 s. If the engine overrides the palette after that window, HVE logs "held" and the palette is
   lost anyway. Fixing that is a *policy* decision (who owns colour on this desktop), not more
   retries: `theme.policy: wallpaper` makes the wallpaper the colour owner, and `Cars` depends on
   a wallpaper-derived scheme (`wallpaper vibrant`). The keeper must choose.
2. **Hanging IPC.** `noctalia_msg` uses a blocking `.output()` with no timeout, so the detached
   worker can outlive the cap if the daemon wedges (no extra commands are issued meanwhile, and the
   app never blocks). Pre-existing behaviour, not introduced here.
3. **Pre-existing test hazard.** `test_noctalia_v5_paths_apply_wallpaper_ipc_format`
   (`src/providers/shell.rs`) can reach the real `noctalia` binary when it runs outside a stub PATH
   window; it targets a nonexistent `/tmp/fake.png`. Predates these commits.
4. **U3 (render coverage for a video-sourced card)** is still not committed as a test. The card's
   real-data path is proven by the keeper's live session plus a frame inspection of the actual
   3840×2160 artifact; the change is data-only and touches no `.slint` geometry.

## Evidence log

- 2026-09-21 (early): diagnosis completed (code + disk + live ffmpeg reproduction). No source write yet.
- 2026-09-21 (morning): `d695941`, `747b8b3` landed; keeper's live test confirmed the animated card
  renders its frame; frame cache `c628e057fad10c72-frame.png` reachable again.
- 2026-09-21 (afternoon): verification round 1 returned FAIL and drove W2 (`98e5879`, `7ea0c01`,
  `6bbc898`) — 858 passed, zero warnings.
- 2026-09-21 (evening): W4 (`f28b670`) implemented for the keeper's palette complaint — 868 passed.
- 2026-09-21: verification round 2 returned PASS with one MINOR; W5 (`3626ec6`) closed it — 869
  passed / 0 failed, zero warnings, `~/.local/bin/hve` reinstalled from `3626ec6` (sha
  `99dc25d1…`).
