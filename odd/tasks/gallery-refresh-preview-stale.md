# Gallery refresh leaves a stale card preview

- **Branch**: `hve2-visual-rewrite`
- **Opened**: 2026-10-04
- **Status**: code complete and verified; pending the keeper's live test

## Objective

Make the Save panel's refresh action (the "store the current look into the active
theme" button) redraw the theme's gallery card when that store changes the theme's
background, and find out why theme `joke2` ended up carrying another theme's
background.

## Problem

The keeper changed `joke2`'s background, stored it back into the theme, and the
desktop applied `joker1.png` correctly — but the theme's card in the Slider still
painted the animated wallpaper frame of the `Animation` theme.

## Root cause (measured, not guessed)

1. `sync_cards` preserves the already-baked images of a card by theme name (the S5
   flicker fix), and the thumb scheduler skips rows that already carry images.
   `invalidate_card_bakes` is therefore the ONLY path that forces a card to
   re-resolve and re-bake.
2. `sync_save_gallery_ui` accepts an invalidation set, but only the overwrite
   handler passes a non-empty one (`src/main.rs` ~4713-4723). The refresh handler
   (~4660-4681) passes `HashSet::new()` and never runs `schedule_thumbs`.
3. So a background change made through the refresh path rewrites the theme's files
   on disk (and applies correctly) while its card keeps the previously baked
   pixels, until the app restarts.

Evidence: category-level — `~/.cache/hve/thumbs/` holds both bakes
(`5709538-644c8b5ed09923dd-*` = joker1.png, `4933544-84af591102890158-*` = the ZZZ
video frame, both read as images); `joke2/providers/noctalia-v5/wallpaper.txt` =
joker1.png with no `mpvpaper-assignments.json`; `apply` paints joker1.png while the
running app still shows the video frame.

## Second question: why did `joke2` get the other theme's background?

`joke2`'s provider dir was rewritten twice by the refresh button:

| Time | `wallpaper.txt` | `mpvpaper-assignments.json` | Meaning |
|------|-----------------|-----------------------------|---------|
| 21:03:48 | joker1.png | present (the Animation video) | authority snapshot unusable → legacy "capture everything" arm |
| 21:21:37 | joker1.png | removed | authority said static → exact arm, stale manifest dropped |

The preview plan ranks the live background assignment (Role 1) ABOVE the static
wallpaper text record (Role 3). A theme that carries a captured mpvpaper manifest
therefore previews and re-applies that video, even when its own static wallpaper is
what the keeper wants. Task G2 investigates why the snapshot was unusable at
21:03:48.

## Scope (authorized)

- `src/main.rs`: the refresh handler passes its theme as the invalidation set and
  reuses the existing thumb scheduler, mirroring the overwrite handler.
- `src/shell/ui_tests.rs`: one failing-first regression test for that wiring, plus
  the model-level invalidation expectations the wiring depends on.
- Documentation/comments touched by the change.
- Read-only investigation for G2 (no behaviour change unless a real defect is
  found; a finding is reported, not silently patched).

## Constraints

- Strict TDD: the new test must fail before the fix (`cargo test`).
- Engine and providers stay untouched; this is a UI-state fix.
- No mass `cargo fmt`; keep the surrounding formatting hand-consistent.
- No push; work-unit commits on `hve2-visual-rewrite` only.
- Validation budget: ~400 authored changed lines per task (advisory).

## Tasks

- [x] **G1 — refresh redraws its card** (delegated writer) — commit `80fd917`
      1. Add the failing test first: after a refresh-path store that changed the
         theme's background, the card's four image fields are blank and its source
         still resolves; untouched cards keep their bakes.
      2. Make the refresh handler invalidate the theme it re-saved (its
         `last_applied`) and call the existing `schedule_thumbs`.
      3. Update the stale comment on `sync_save_gallery_ui` that claims the refresh
         path changes no artwork.
      - Checks: `cargo test shell::ui_tests` (new test RED → GREEN), `cargo test`
        full suite, `cargo build --release` warning-free.
      - Evidence: test name, RED/GREEN output, commit hash. RED: `refresh_saved_theme_rebakes_its_card_by_construction` failed on the pre-fix code; GREEN after.
        Full suite `1342 passed; 0 failed`; `cargo build --release` with no warnings.
        The orchestrator re-ran the filtered test (green). Test is a
        construction check that reads `src/main.rs` (house convention for this
        seam) — it asserts both the invalidation set and `schedule_thumbs`, so
        removing either re-breaks it.
- [x] **G3 — the mpvpaper record can no longer hijack a theme** (delegated writer,
      authorized by the keeper 2026-10-04) — commit `db477a1`. `save()` now copies
      the live mpvpaper manifest only when the plugin can actually paint
      (`should_capture_video_manifest` = `wants_manifest && plugin != Some(false)`);
      `Some(true)` and `None` keep the legacy capture, and `Some(false)` also drops
      the stale file through the existing else arm.
      - Checks: unit test RED → GREEN, `cargo test` (`1343 passed / 0 failed`),
        `cargo build --release` warning-free.
      - One test-isolation fix came with it: the new snapshot call exposed a
        pre-existing PATH race between two tests, so the non-serial smoke test
        `test_noctalia_v5_save_creates_provider_dir` is now `#[serial]`.
- [x] **G5 — the refresh invalidation guard stops passing on comments** — commit
      `9cf66c1`, from the cross-model review's warning that two asserted substrings
      also occurred in comments. The decision is now the pure helper
      `refresh_invalidation_set` (`src/main.rs`) with its own unit test, and the
      construction test asserts code-only tokens.
- [x] **G6 — an unreadable plugin state is not a disabled plugin** — commit
      `51f5334`, from the second review's warning: a readable-but-unparseable
      `settings.toml` used to read as "provably disabled", which would have
      silently stopped capturing the video record if the plugin were enabled and
      the format drifted. `mpvpaper_state_in_settings` is now tri-state (explicit
      `[plugins]` entry only) and the capture-specific snapshot falls back to the
      IPC probe; the apply path keeps the old parser untouched.
- [x] **G4 — verification and the binary the keeper will test** — three cross-model
      verifications (read-only, `glm-5.3-flash`), all claims confirmed, no blocking
      findings. Remaining non-blocking notes: the refresh path can re-bake on every
      store (cheap, warm content-keyed cache), a whitespace-only `last_applied` is
      not trimmed (unreachable through the validated save path), and the line-based
      `settings.toml` parser still assumes the `enabled = [...]` shape (pre-existing,
      now with the probe as fallback). Release binary rebuilt and installed
      (`~/.local/bin/hve`, byte-identical to the HEAD build).
- [ ] **G2 — why the other theme's background got captured** (delegated explorer,
      read-only): the 21:03:48 vs 21:21:37 fingerprints, what makes the authority
      snapshot unusable, and whether the "capture everything" fallback can be made
      honest without losing data.
      - Evidence: named code paths, any log lines found, and a clear statement of
        what could NOT be proven from the available artifacts.

## Acceptance criteria

- Pressing refresh after changing a theme's background re-draws that card without
  a restart; unrelated cards do not flicker or re-bake.
- Rename, delete and new-theme save paths keep their current behaviour.
- The suite is green and the build is warning-free.

## Progress and evidence

- 2026-10-04: diagnosis recorded (see above); feature document created.
- 2026-10-04: **G1 done** — commit `80fd917`. Test RED then GREEN, full suite
  `1342 passed / 0 failed`, release build warning-free, orchestrator re-ran the
  filtered test. Cross-model verification launched (read-only, glm).
- 2026-10-04: **G3 authorized by the keeper** ("si esa es solución yo te diría que
  lo apliques ya"). Root-cause hardening for the capture arm that put the foreign
  video inside `joke2`.
- 2026-10-04: **G3 done** — commit `db477a1`; suite `1343 passed / 0 failed`.
- 2026-10-04: **G5 + G6 done** — commits `9cf66c1` and `51f5334`, closing the two
  non-blocking warnings from the cross-model reviews. Suite `1346 passed / 0 failed`,
  release build warning-free.
- 2026-10-04: **G4 done** — cross-model verdicts: G1 all 9 claims confirmed; G3 all 9
  confirmed; G5/G6 all 9 confirmed. No blocking findings in any round. Binary rebuilt
  and installed.
- Pending: the keeper's live test in the morning (the running app must be reopened to
  load the new binary).

## Next step

Hand the morning test to the keeper: reopen HVE, change a theme's background, press the
refresh/store button, and confirm the card repaints; then re-save a theme while the
mpvpaper plugin is off and confirm no `mpvpaper-assignments.json` lands inside it.
