# Feature: render artifacts must be per-run (no stale frames)

**Locator**: `odd/tasks/render-artifacts-per-run.md`
**Engram mirror**: topic `odd/render-artifacts-per-run/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Found**: 2026-09-27, by the independent verifier (GLM) of commit `d00a95a`.
**Status**: DONE (2026-09-28) — implemented, committed and independently verified. Commits `fdd0e4b`
(harness + tests) and `2b6ea62` (procedure docs), on top of `f421c9d`.
**TDD**: strict — runner `cargo test` (project rule in `AGENTS.md`). RED demonstrated by mutation
for both new properties, not only by "tests green".
**Route**: bounded delegated writers on the free model (Muse Spark 1.3) under a thin orchestrator,
which also hardened the containment guard inline; cross-model verification by GLM-5.3-Flash.
**Delivery**: single PR — ~240 changed lines, under the 400-line review budget.

## Work units (approved 2026-09-28)

- [x] **W1 — the harness writes into a per-run directory.** `fdd0e4b`. `src/shell/ui_tests.rs`:
  `render_run_dir_from(env_override, stamp_ms, pid)` (pure) and `render_run_dir()` (`OnceLock`,
  `HVE_RENDER_DIR` override, `create_dir_all`, printed once as
  `render artifacts for this run: <dir>`); `save_slice_png` takes a bare file name and joins it to
  the run directory; all 136 original call sites now pass bare names only. The guard requires
  exactly one `Normal` path component, so `"/tmp/x.png"`, `"a/b.png"`, `"."` and `".."` panic
  loudly instead of escaping the run directory — `Path::join` replaces the base on an absolute
  argument, which is exactly what the guard exists to stop.
- [x] **W2 — the procedure points at the run directory.** `2b6ea62`. The `AGENTS.md`
  visual-verification bullet plus the two older task docs that still taught the flat path.
  Anything flat in `/tmp/opencode/*.png` is declared a pre-fix leftover, never evidence.

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

## Fix (implemented)

- **Shipped**: a **per-run output directory**. `save_slice_png` writes into
  `/tmp/opencode/render-<timestamp_ms>-<pid>`, the run prints that directory, and verification
  reads from the run directory only. Nothing canonical can be stale because nothing is shared
  between runs.
- **Weaker alternative** (not used): delete each expected path *before* the test writes it. It
  removes the "old file at a path the run never reached" case only partially, because a test that
  panics before its save still leaves whatever was there for paths it never touched.

## Procedure (current, since 2026-09-28)

1. Run the render with a directory you control:
   `HVE_RENDER_DIR=/tmp/opencode/render-verify-<unique> cargo test slice_focus_flow_renders -- --nocapture`
   — or read the `render artifacts for this run: <dir>` line the run prints.
2. READ the frames from that run directory, and from nowhere else.
3. Anything flat in `/tmp/opencode/*.png` is a pre-fix leftover: never evidence.
4. Never treat a frame as evidence without knowing which run wrote it.

**Retired** with the fix: the snapshot-and-SHA-256 dance. It was the workaround while the harness
shared paths; the run directory makes it unnecessary.

## Acceptance criteria — met

1. ✅ Two consecutive runs produce frames in two different directories, each carrying the identity
   of the run that wrote it (`render-<ms>-<pid>`; `HVE_RENDER_DIR` overrides deliberately).
2. ✅ A run that fails before saving any frame leaves no frame that can be mistaken for it: nothing
   is shared, so a name the run never wrote is simply absent from its own directory.
3. ✅ The procedure here and in `AGENTS.md` points at the per-run directory; flat files are declared
   untrustworthy.
4. ✅ `cargo test` green (1038 passed / 0 failed) with 0 warnings; no test weakened or removed.

## Verification (2026-09-28, GLM-5.3-Flash, independent of the writers)

- **Suite / warnings**: 1038 passed, 0 failed, 0 warnings (base 1034 + 4 new tests).
- **Containment**: an attack table over every forbidden name shape (`""`, `"."`, `".."`,
  `"/tmp/x.png"`, `"a/b.png"`, `"a/../b.png"`, `"x.png/.."`, `"./x.png"`) found no silent escape;
  the only theoretical route is a same-user symlink race inside the run directory, unreachable from
  the test API.
- **Isolation**: two consecutive runs printed two different directories; the `HVE_RENDER_DIR`
  override was honoured; a full-suite run wrote 138 frames inside its own directory.
- **Documented procedure**: the exact `AGENTS.md` command was run verbatim and put its three frames
  in the named directory with this session's mtimes.
- **Nothing flat**: `find /tmp/opencode -maxdepth 1 -name '*.png' -newermt '-20 minutes'` → empty.
- **No test weakened**: the 136 call sites differ only by the removed `/tmp/opencode/` prefix; the
  remaining changes are the new helper, the new tests and comment rewording.
- **Visual, by two pairs of eyes**: the verifier and the orchestrator each READ
  `slice_settled.png` from a run directory and both see a real gallery frame — three parallelograms,
  "Theme 6" expanded with its focus border, the top bar, and the skwd-wall credit.
- **Accepted, non-blocking**: (1) a *failing* probe test can leave its 34-byte text seed at
  `/tmp/opencode/__render_run_probe.png`, because the cleanup line is skipped on panic — it is not a
  rendered frame and it is obviously a probe; (2) the suite count is 1038, not the 1037 the
  orchestrator first stated (four tests were added, not three).

**Residual**: the pre-fix flat frames are still on disk in `/tmp/opencode/`. Nothing writes them any
more, and both procedures now forbid reading them as evidence.
