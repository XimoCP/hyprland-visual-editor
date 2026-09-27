# Feature: render artifacts must be per-run (no stale frames)

**Locator**: `odd/tasks/render-artifacts-per-run.md`
**Engram mirror**: topic `odd/render-artifacts-per-run/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Found**: 2026-09-27, by the independent verifier (GLM) of commit `d00a95a`.
**Status**: OPEN — recorded, not started. The fix touches the harness that verifies every
`.slint` change in this project, so it is done awake and with its own verification round.

## Problem

The render tests write their frames to **fixed, shared paths** under `/tmp/opencode/`
(`save_slice_png(...)`) and they **save the frame before running the assertions**. Two
consequences:

1. A run that fails or is aborted leaves frames from an earlier run at the paths it never
   reached. Those stale frames look exactly like current evidence.
2. Nothing in the file says which run (which commit, which tree state) produced it, so a
   stale frame is indistinguishable from a fresh one by inspection.

## Evidence (measured, not inferred)

- After the verifier's mutation runs, `borders_picker_hint_es.png` and
  `borders_picker_hint_marked.png` were **byte-identical to broken-build frames** (0 px
  difference between the two), and `apply_help_preset.png` equalled its `_before` copy.
- A leftover `apply_help_theme_rename.png` (mtime 21:57, written by no current test)
  was **byte-identical** to that day's `apply_help_theme_refresh.png`. That also proves the
  renderer is deterministic, and that misleading stale files accumulate over time.
- A writer on the same day misread a frame and attributed it to the read tool. The read tool
  was innocent: it was this staleness.

## Why it matters here

The keeper mandates visual verification for every `.slint` change: render headless, **read
the PNGs**, confirm with your own eyes. That procedure is only as good as the frames. A stale
frame can pass a change that is not in the build, which is exactly the class of false
evidence this project has been hunting all day (see the false safety-net test of B5, and the
dead i18n channels of B4).

## Fix (to be implemented)

- **Preferred**: a **per-run output directory**. `save_slice_png` writes into a directory that
  carries the run identity (timestamp, or the short commit plus a nonce), the tests print that
  directory, and the verification procedure reads from the run directory only. Nothing
  canonical can be stale because nothing is shared between runs.
- **Weaker alternative**: delete each expected path *before* the test writes it. It removes
  the "old file at a path the run never reached" case only partially, because a test that
  panics before its save still leaves whatever was there for paths it never touched.

## Interim procedure (use this until the fix lands)

1. Before running the suite, **snapshot** the expected frames: copy them aside and record
   their SHA-256.
2. Run the suite.
3. Read the **snapshot** for pre-run state, and for post-run state compare the hash of the
   current file against the snapshot: if it did not change, the run did not produce it.
4. Never treat a frame as evidence without knowing which run wrote it.

## Acceptance criteria

1. Two consecutive runs of the same test produce frames in two different directories, each
   identifiable by the run that wrote it.
2. A run that fails before saving any frame leaves no frame that can be mistaken for it.
3. The verification procedure (in this doc and in the agent instructions for `.slint` work)
   points at the per-run directory.
4. `cargo test` green, 0 warnings; no test weakened or removed.
