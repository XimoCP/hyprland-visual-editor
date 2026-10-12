# Feature: gallery preview delivery — visibility and index-shift repair

**Locator**: `odd/tasks/gallery-preview-delivery-visibility.md`
**Engram mirror**: topic `odd/gallery-preview-delivery-visibility/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: **CLOSED — keeper sign-off 2026-10-12** ("todo lo revisado tiene mi OK"); the pending
live check is resolved. Authorized 2026-10-11: "vale implementa... si no tiene mucho coste o riesgo, siempre es bueno una red de seguridad".
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~80-140 authored changed lines → single PR, no chaining.
**Route**: delegated writer (2 non-trivial files: `src/main.rs`, `src/shell/gallery/thumbs.rs`), cross-model verification not required (no deletion, no cross-subsystem behaviour: Tier 2 per the project's proportionality rule) — parent self-check plus one live check by the keeper.

## Objective

Make a gallery tile that fails to receive its baked preview impossible to lose
silently, and repair the index-based handshake that caused it.

## Problem / evidence (observed 2026-10-11 ~00:43)

The gallery opened with 6 tiles; `Animation` and `Cars` painted, `Fantasy`, `JoKer`
(focused), `Space` and `Work` stayed solid dark until the keeper interacted.

Verified facts:

- The bake cache `~/.cache/hve/thumbs` held COMPLETE artifact sets for all six
  themes, including the four dark ones → sources, decoding and baking were fine.
- The dark tiles are exactly the later indices of the ring (2..5); the two that
  painted are 0 and 1. `Fantasy` had been re-saved at 00:42, immediately before.
- Delivery (`src/main.rs:3292-3322`): each worker result is marshalled to the UI
  thread and matched with `model.row_data(idx)` plus `row.name == name`
  (`src/main.rs:3316-3317`). On a mismatch the payload is dropped with NO log —
  the only branch that runs is the "stale row still counts toward batch
  completion" one. A missing row is dropped the same way.
- The job's `index` is the model row index captured at planning time
  (`thumbs::plan_jobs`, `src/shell/gallery/thumbs.rs:703-710`,
  `PreheatJob { index, name, spec }`).
- The result of `slint::invoke_from_event_loop` is discarded (`let _ =`,
  `src/shell/gallery/thumbs.rs:825`), so a failed marshal is invisible too.
- `tracing` is initialised with no file writer (`src/main.rs:2714-2716`): when HVE
  runs as a tray process, nothing about this pipeline survives the run.

Mechanism (now concrete, not a guess): the handshake is positional. When the theme
list is rebuilt while a batch is in flight (a save at 00:42 rescans the list), every
index from the insertion point on points at a different row, so those payloads fail
the name check and are dropped. Indices 0 and 1 still matched, which is exactly the
pattern observed.

## Scope (authorized)

- **U1** — persistent log. `src/main.rs:2710-2716`: keep the existing `EnvFilter`
  behaviour and stderr output, and ADD a file layer under the existing cache dir
  (`~/.cache/hve/`), `hve.log`. Bounded growth without a new dependency: at startup,
  if `hve.log` exceeds 2 MiB, rename it to `hve.log.1` (replacing any previous
  `.1`) before opening. If the file cannot be created or opened, fall back to
  stderr-only and say so on stderr — a log failure must never fail startup or panic.
  No new crate, no background thread, no timer.
- **U2** — the delivery handshake. Keep the index fast path exactly as it is, and
  repair the fallback:
  1. On an index/name mismatch, resolve the row by NAME over the model's current
     rows and fill THAT row, instead of dropping the payload.
  2. Drop with a `warn` naming the theme and the index ONLY when the name is no
     longer in the list at all (the theme was deleted).
  3. Log the index shift itself at `debug` (index expected vs index found), so a
     future one-off leaves a trail.
  4. Keep the existing batch-completion/refresh semantics untouched
     (`is_last_completion` + `refresh_clone(false)`), and do not add timers or
     retries: the repair is the name lookup, not a timeout.
  5. Put the lookup in a small PURE helper next to the other planning helpers in
     `src/shell/gallery/thumbs.rs` — e.g.
     `resolve_row_index(rows: &[(usize, String)], index: usize, name: &str) -> Option<usize>`
     — so it can be unit-tested with plain data and no Slint model. `main.rs`
     calls it with the model's rows.
- **U3** — tests: unit tests for the pure helper: exact index match wins; a shifted
  index resolves by name; a name absent from the rows returns `None`. No test may
  build a Slint window.

## Out of scope

- No change to baking, decoding, source resolution, cache keys, or the preview
  source chain (`plan_with` / `declared_sources` stay untouched).
- No new dependency, no async runtime, no timer, no retry loop.
- No change to the "Listo" indicator (a separate truthfulness defect, follow-up).
- Nothing under `~/.config/hve/` except the log file this feature creates in the
  CACHE dir.

## Acceptance criteria

1. `cargo test` green, `cargo check --all-targets` clean.
2. A tile whose payload arrives with a shifted index is filled by name, not dropped.
3. A payload for a theme that no longer exists is dropped WITH a warning naming it.
4. `~/.cache/hve/hve.log` exists after a run, is bounded (2 MiB + one rotated file),
   and a read-only/unwritable cache dir degrades to stderr without failing startup.
5. No behaviour change when every index still matches (the common case).

## Checks

- `cargo test`
- `cargo check --all-targets`
- Live check by the keeper: open the gallery cold and confirm every tile fills
  without touching anything.

## Work units

- [x] **U1** — persistent log with startup rotation and graceful degradation (`src/main.rs`). — `5d01084`
- [x] **U2** — name-based delivery fallback + warnings + the pure helper (`src/main.rs`, `src/shell/gallery/thumbs.rs`). — `3341977`
- [x] **U3** — unit tests for the helper (`src/shell/gallery/thumbs.rs`). — `721d43d`
- [x] **U4** — parent verification: suite, check, no timer/retry/dependency added. — verified by the
  parent (the three new tests pass; `is_last_completion` is still called exactly once per
  payload on every path; `Cargo.toml` untouched). **The keeper's live check is still open.**

## Verification result

- `cargo test` at the end of U3: 1504 passed, 0 failed. `cargo check --all-targets`: clean.
- The repair was checked by mutation: removing the name lookup makes
  `resolve_row_index_resolves_a_shifted_index_by_name` fail (`left: None, right: Some(3)`),
  so the test does pin the repair.
- `resolve_row_index(rows, index, name)`: the exact index+name pair wins; otherwise the row
  with that name; `None` when the name is gone.
- Log: an index shift is `debug` with `theme`, `expected`, `found`; a payload whose theme
  left the list is `warn` with `theme` and `index`.
- **Finding for the keeper (one-line decision):** the existing console layer writes to
  **stdout**, not stderr (`tracing-subscriber 0.3.23`, `fmt_layer.rs:749` sets
  `make_writer: io::stdout`), verified with a redirected probe. It was left as it was, so
  the change adds no behaviour outside its scope; moving the console log to stderr is a
  one-line change if the keeper wants it.

## Progress log

- 2026-10-11: mechanism identified from code plus the observed index pattern; contract written.
