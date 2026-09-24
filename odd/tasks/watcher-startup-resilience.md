# Feature: the colour watcher always actually runs (startup resilience)

**Locator**: `odd/tasks/watcher-startup-resilience.md`
**Engram mirror**: topic `odd/watcher-startup-resilience/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches, do NOT rebase, do NOT push)
**Checkpoint before this work (save point)**: `1ab296e` — clean tree; roll back here and this patch leaves no trace
**Status**: W6 CLOSED 2026-09-24 — implemented (deepseek-v4-flash), cross-model verified (glm-5.3-flash), findings fixed, and **live-confirmed by the keeper (13 PASS / 0 FAIL)**. Accepted residuals are listed in the findings section.
**TDD**: strict — runner `cargo test`
**Delivery strategy**: `ask-on-risk` (no remote configured: delivery is local commits, no PRs)
**Delivery budget forecast**: ~300-450 authored changed lines (script + Rust + tests +
this document). Tests dominated the previous units' final counts; the same risk is
recorded in advance.
**Actual authored changed lines**: ~764 (code + tests, `git diff --numstat`:
`src/watcher.rs` +696/-33, `color_watcher.sh` +15/-6, `Cargo.toml` +1, `Cargo.lock`
+7/-6); +~55 for this document. Over the advisory budget, as forecast: the three
sandboxed acceptance tests follow the house sandbox pattern ~100 lines each. Not
minified — the work-unit-commits rule forbids shrinking tests/comments/docs to hit a
number.

## Objective

Every HVE session must end up with a **live colour watcher**, and HVE must **know**
whether it has one. Today the watcher can be rejected at startup and HVE logs it as
a success, so the session silently loses its colour sync until the next restart.

Three layers, in one unit:

1. **The singleton lock stops leaking into children** — killing the watcher must
   release the lock immediately (root cause, measured).
2. **The watcher dies with HVE** — parent-death signal, kernel-enforced: if HVE is
   killed ungracefully, the watcher must not survive holding the lock forever.
3. **HVE verifies and retries the start honestly** — never log "started" for a
   child that already exited; retry a bounded number of times; on final failure
   log a real error.

## Problem, with the measured evidence

### Live evidence (23-sep, ~20:02) — the defect that opened this unit

- HVE (PID 275596) started at 20:02:14 and logged
  `INFO hve::watcher: [HVE] Color watcher started (pid: 277918)`.
- The **watcher's own log** at the same moment:
  `Singleton guard: another color watcher holds ~/.cache/hve/color_watcher.lock — exiting without starting`.
- Measured result: **zero** live watcher processes while HVE ran. HVE believed it
  had a watcher; it did not. Colour changes stopped being watched for the rest of
  that session, because HVE launches the watcher only once, at startup.

### Root cause — CONFIRMED by reproduction, not inferred (2026-09-24)

`assets/scripts/color_watcher.sh:105-115` holds the singleton lock with
`exec 9>"$LOCK_FILE"; flock -n 9`, for the whole process lifetime. Two Linux
properties combine:

- A `flock` lives attached to the **open file description**, not to the process
  that requested it.
- `exec 9>` does **not** set close-on-exec, so every child inherits fd 9.
  Reproduced: a child of the lock holder lists fd `9` in `/proc/<pid>/fd`.

The watcher's long-lived child is `inotifywait -t 30` (`color_watcher.sh:170`).
When HVE kills the watcher (`ColorWatcher::terminate` only kills the direct child),
that orphan survives holding fd 9. Reproduced: with the parent killed, the orphan
kept the lock, so an external `flock -n` still failed. Until the orphan dies (up to
30 s, or **forever** when HVE died ungracefully and the watcher loop keeps running)
any new watcher is rejected with `exit 0`, and `spawn_color_watcher`
(`src/watcher.rs:55-59`) logs success the moment `spawn()` returns a pid.

### Reproduced sandbox checks (2026-09-24)

Executed under `/tmp/opencode/locktest/`; the artifacts are kept there.

| Check | Result |
| --- | --- |
| Child inherits fd 9 from the lock holder | **inherited** (fd 9 listed) |
| Killing the lock holder with a long-lived orphan | **lock stays held** (defect reproduced) |
| `sleep 60 9>&-` child, then kill the holder | child fds = `0 1 2`; **lock FREE immediately** (fix works) |
| `true 9>&-` with fd 9 never opened (fail-open path) | rc=0, no stderr → safe no-op |
| `prctl(PR_SET_PDEATHSIG, SIGKILL)` then `execv` bash; kill the parent | **child died** → the signal survives `execve` |

### Live evidence of the same orphaning class, observed 2026-09-24

HVE was not running, but a **stray child survived it**:
`bash assets/scripts/reload_coalescer.sh drain`, PPID 1, alive 21+ minutes.
Same class of "child outlives HVE". Recorded as a follow-up, NOT fixed here.

## Verified premises (read from source, 2026-09-24)

- `src/watcher.rs:30-72` — `spawn_color_watcher(proj)` spawns
  `bash <proj>/assets/scripts/color_watcher.sh`, logs `Color watcher started
  (pid: N)` immediately, and returns an owning `ColorWatcher` guard.
- `src/watcher.rs:17-27` — `terminate()` does `child.kill()` + `child.wait()`;
  it kills the direct child only, never the process group.
- `src/main.rs:3865` — the watcher is spawned **once**, from `main()` (main
  thread), and dropped at `src/main.rs:3969`. Because it runs on the main thread,
  `PR_SET_PDEATHSIG` is safe (it fires when the parent *thread* dies).
- `assets/scripts/color_watcher.sh:95-115` — flock singleton guard, fail-open when
  `flock` is missing (a deliberate W1 property that MUST be preserved).
- `assets/scripts/color_watcher.sh:143,150,156,170,204,221` — the external
  commands a child can outlive the watcher on: `get_colors.sh`, `hve-ipc`,
  `assemble.sh` (initial + loop), `inotifywait`, `noctalia`.
- `Cargo.toml` has no direct `libc`, but `libc 0.2.186` is already in
  `Cargo.lock` (transitive) and cached; `cargo add libc` resolves offline.
- `openspec/config.yaml` — `strict_tdd: true`, runner `cargo test`.
- Model roles (`~/.config/opencode/opencode.json`): writing set is
  `deepseek-v4-flash`, verification set is `glm-5.3-flash` — no overlap.

## Scope

### Layer 1 — the lock stops leaking into children (`assets/scripts/color_watcher.sh`)

Close fd 9 for every external command a child could outlive the watcher on:
`inotifywait` (critical), the two `assemble.sh` calls, `get_colors.sh`,
`noctalia`, `hve-ipc`. Use `9>&-` on the invocation itself.

- `9>&-` must be a no-op when fd 9 was never opened (the fail-open path): verified
  above.
- Do NOT `exec 9>&-` in the script shell: closing fd 9 in the shell releases the
  lock. Only per-command redirections.
- Add a comment next to the guard stating the invariant: *any future long-lived
  child must close fd 9, or a killed watcher leaves the lock held*.

### Layer 2 — the watcher dies with HVE (`src/watcher.rs`)

- Set `PR_SET_PDEATHSIG = SIGKILL` in `Command::pre_exec` (Linux-only, cfg-gated),
  so the kernel kills the watcher the instant HVE dies — including an ungraceful
  `kill -9`, where the `ColorWatcher` guard never runs.
- `terminate()` must also kill the **process group** so the orphaned `inotifywait`
  does not linger: spawn the watcher with `process_group(0)` and, on teardown,
  `libc::kill(-pgid, SIGKILL)` followed by `child.kill()` + `child.wait()`.
  **Hazard:** `kill(-pid)` when the child is NOT a group leader can signal an
  unrelated process group. Group-kill only when the guard was created by
  `spawn_color_watcher` (which sets `process_group(0)`); track that explicitly
  (e.g. an `Option<i32>` pgid field) and keep `child.kill()` as the universal
  fallback. Never call `kill(-pid)` unconditionally.

### Layer 3 — honest start, bounded retry (`src/watcher.rs`)

- Extract the attempt logic into a testable core taking an injectable spawn
  closure, so retry behaviour is tested without touching the keeper's live session.
- After a successful `spawn`, poll with a short grace (~250 ms, ~20 ms steps):
  - child still alive at the end of the grace → real success, log
    `Color watcher started (pid: N)`;
  - child already exited → record the exit status, retry (bounded, ~5 attempts,
    short backoff); exit early on the first observed exit instead of waiting out
    the full grace.
- On exhaustion: `tracing::error!` naming the attempts and the last observed exit
  status, and return `None`. **Never** log "started" for a dead child.
- Do NOT claim more than this: alive-after-grace is the honest, bounded check.
  Verifying that the watcher is *functional* is explicitly out of scope.

## Explicit non-goals

- The stray `reload_coalescer.sh drain` (different script, different unit).
- Anything about monitors/HDR/layout (hyprmod territory).
- Changing the singleton semantics: one watcher per machine stays. A legitimately
  rejected start must be logged as a truthful failure, not removed.
- The zbus / broken-pipe panic seen at shutdown in `~/.cache/hve/live-final.log`.
- Touching the engine modules outside `src/watcher.rs`.

## Work units

**W6 — one work unit, three layers** (single coherent behaviour: the watcher
always runs, or HVE says it did not).

- Route: **delegated direct** — one writer. Trigger evidence: 3+ non-trivial files
  (`assets/scripts/color_watcher.sh`, `src/watcher.rs`, `Cargo.toml`) plus tests;
  the writer trigger applies.
- Tests live with the code, commit with it. Commit message in English,
  Conventional Commits, no AI attribution.

## Acceptance criteria

1. **Layer 1 test**: sandboxed — start watcher instance 1 (real `color_watcher.sh`
   + stubs, house sandbox), wait for the lock; kill instance 1's bash while its
   long-lived stub child is alive; assert instance 2 acquires the lock and logs
   `Starting watcher`. Without the fix this test must fail (RED).
2. **Layer 2 test**: end-to-end parent-death — a helper process (re-invoked test
   binary via `current_exe` + env var, sandboxed `HOME`/`HVE_CACHE_DIR`/`PATH`)
   calls the real `spawn_color_watcher`, prints the child pid, then exits; the test
   asserts that pid disappears without anyone killing it. Fallback only if this
   proves genuinely flaky: a `PR_GET_PDEATHSIG` round-trip unit test, and the
   downgrade must be recorded in this document and reported — never silently.
3. **Layer 3 tests** (deterministic, no env races): an immediately-exiting child is
   retried exactly N times and yields `None`; a child that dies k times then stays
   alive succeeds on attempt k+1; a child alive at the grace end succeeds on the
   first attempt. No success outcome for a dead child.
4. Existing guard tests still pass, updated only where the new pgid field makes the
   struct literal incomplete (documented in the diff).
5. Fail-open property preserved: missing `flock` still logs
   `Singleton guard unavailable` + `UNGUARDED` and keeps working.
6. Whole suite green, no build warnings.
7. **Live confirmation (keeper, mandatory)**: normal start → watcher present;
   `kill -9` HVE → watcher gone within ~1 s and the lock free; start HVE again →
   watcher starts on the first attempt.

## Checks

Exact commands the writer runs in the foreground and reports as
`<command>: <observed result>`:

- `cargo test` — strict TDD, RED observed first, whole suite green at the end.
- `cargo build --release` (or `cargo check`) — no warnings.
- `bash -n assets/scripts/color_watcher.sh` — script syntax.
- Targeted: `cargo test --bin hve watcher::tests`.

Recorded in the verification record: the suite baseline before the change, the
staged RED output, the GREEN output, and the final full-suite count.

## Risks / residuals (to watch)

- **Startup cost**: the failure path can delay startup by a few seconds (bounded).
  The normal path costs one ~250 ms grace. Accepted; document it in the code.
- **`libc` becomes a direct dependency** (already transitive; low risk).
- **PDEATHSIG is Linux-specific**: cfg-gate it; HVE is a Linux/Hyprland binary.
- **`kill(-pgid)` hazard** if ever called on a non-leader — prevented by the
  explicit pgid tracking above.
- **Future children**: a new long-lived child added without `9>&-` reintroduces the
  lock leak; mitigated by the invariant comment and by layer 2's group kill.
- **Sandbox test flakiness**: timing-based, bounded with generous deadlines (house
  pattern uses 30 s deadlines), always with RAII cleanup so a failure cannot leak
  a process.

## Verification record

| What | Who | Evidence |
| --- | --- | --- |
| Baseline suite (before change) | writer (deepseek-v4-flash) | `cargo test`: **966 passed; 0 failed** (~50 s) |
| Layer 1 — RED | writer | `cargo test --bin hve -- watcher::tests::sandboxed_lock_is_released...` FAILED: instance 2 logged `Singleton guard: another color watcher holds … — exiting without starting` while instance 1's orphaned stub was alive (10.06 s deadline) |
| Layer 1 — GREEN | writer | same test **ok in 0.10 s** after `9>&-` on the six external invocations |
| Layer 2 — RED | writer | `guard_termination...` FAILED (5.05 s); `watcher_child_dies_when_hve_dies_end_to_end` FAILED: `watcher 70848 still alive 10 s after its parent died — PR_SET_PDEATHSIG missing?` — genuine e2e RED, no downgrade needed |
| Layer 2 — GREEN | writer | both **ok** (0.10 s / 0.05 s) after PDEATHSIG + `process_group(0)` + pgid-tracked group kill |
| Layer 3 — RED | writer | `core_*` tests: 2 of 3 FAILED against the extracted single-attempt skeleton (exists→None / dead-child-succeeds paths) |
| Layer 3 — GREEN | writer | all three `core_*` tests **ok in 0.50 s** (grace + bounded retry + honest error log) |
| Final suite | writer | `cargo test`: **972 passed; 0 failed** (45.17 s) — 966 + 6 new watcher tests |
| No build warnings | writer | `cargo check`: clean; `cargo build --release`: Finished, 0 warnings |
| Script syntax | writer | `bash -n assets/scripts/color_watcher.sh`: SYNTAX-OK |
| Targeted watcher tests | writer | `cargo test --bin hve watcher::tests`: **12 passed; 0 failed** (0.50 s) |
| Stray-process audit | writer | watcher module ×3 + full suite: `NO-STRAYS`; required one test-infra fix: the long-lived stub must `exec sleep` (without it bash forks `sleep` as a child, the marker pid dies, and the sleep leaks past a by-pid kill) |
| Work-unit commit (code + tests) | writer | commit `b8becbe` (this table is the `docs(odd)` follow-up) |
| Cross-model verification | verifier (glm-5.3-flash, independent) | Full suite **972 passed; 0 failed** (48.72 s) reproduced; `cargo build --release` 0 warnings; `bash -n` SYNTAX-OK; `watcher::tests` **12/0** (0.50 s); stray audit NO-STRAYS — all reproduced. Every new test **falsified by mutants**: prctl stripped → the e2e parent-death test RED with the exact recorded panic; group-kill stripped → `guard_termination` RED at 5.05 s; `MAX_START_ATTEMPTS=1` → 2 of 3 `core_*` RED. Causal A/B (sole variable `prctl`, plain child and the real sandboxed script): without prctl both survive their parent ≥3 s orphaned; with prctl both die. Layer 1 confirmed with an independent external `flock` probe: stripped → HELD (orphan fds `0 1 2 9`); fixed → FREE (fds `0 1 2`). Fail-open re-verified live. Verdict: acceptance criteria 1-6 satisfied; AC 7 pending. |
| Live confirmation (keeper, mandatory) | the keeper, on his own desktop (2026-09-24 20:38) | **PASS 13 / FAIL 0** via `odd/tools/hve-live-verify.sh` (dev binary `4fe4b7a9`, repo tree). W6-specific: exactly ONE watcher alive, **not rejected** by the singleton guard; `kill -9` on HVE (pid 948759) → the watcher **died within ~5 s** and the singleton lock was **FREE** (PDEATHSIG + the fd no longer leaking, working on the real compositor, not just in sandboxes); restart → exactly ONE watcher again with **no failed-start error** in the HVE log. The same run also confirmed W7 (a burst's drainer spawned, consumed the owed reload, and exited on its own). The run first removed a stale pre-fix drainer (0 left, nothing owed). |

## Findings from the verification (open, none blocking)

- **MINOR — RESOLVED (`da94564`)** — `src/watcher.rs` layer-2 e2e test env block: the
  helper resolves the watcher log through `dirs::cache_dir()`, which honours
  `XDG_CACHE_HOME`, but the test pinned only `HOME`. With `XDG_CACHE_HOME` exported,
  a helper-run watcher could append stderr to the real
  `~/.cache/hve/color_watcher.log`. Fixed by pinning `XDG_CACHE_HOME` into the
  sandbox with a comment stating why; `watcher::tests` green (12/0) and the real
  log's mtime unchanged across the run.
- **MINOR (theoretical residual)** — `src/watcher.rs:27-29`: `kill(-pgid)` would hit
  an unrelated group only if the child died, its group fully dissolved, and its pid
  were recycled to a new group leader before `terminate()` runs. The structural
  guarantee holds (`pgid: Some` is set only right after `process_group(0)`); the
  stale-pid window is inherent, unproven as reachable, left as documented residual.
- **NIT** — `core_retries_an_immediately_exiting_child...`: under pathological load a
  `sh -c exit 7` taking >250 ms would end the grace "alive" and flake. Not observed
  in repeated runs.
- **NIT** — the e2e test can leak a respawned `sleep 2` stub for ≤2 s after PDEATHSIG
  kills the watcher bash (group members are not pdeathsig'd). Audit found none.
- **Unverifiable here** — the pre-change baseline `966 passed`: reproducing it needs
  a checkout of `1ab296e`, which the read-only verification contract forbids. Plausible:
  972 − 966 = exactly the six new tests.
- **Pre-existing, out of scope** — the stray `reload_coalescer.sh drain` (PPID 1)
  observed again during verification, alive 88 min. Documented as a separate unit.
- **Delivery-size overage** — ~764 authored lines vs the ~400 advisory budget (tests
  dominate). **`size:exception` GRANTED by the keeper (2026-09-24)**; no artificial
  split, no minification.

## Progress

- [x] W6 layer 1 — close fd 9 on the long-lived children (`assets/scripts/color_watcher.sh`)
- [x] W6 layer 2 — PDEATHSIG + process-group teardown (`src/watcher.rs`)
- [x] W6 layer 3 — grace check, bounded retry, honest failure log (`src/watcher.rs`)
- [x] Tests: the three acceptance tests above, RED first
- [x] Whole suite green, committed as one work unit (commit `b8becbe`)
- [x] Cross-model verification (glm-5.3-flash) — 2026-09-24, verdict: AC 1-6 satisfied, all tests falsified by mutants, no blocking findings
- [x] Live confirmation by the keeper (AC 7) — 2026-09-24 20:38: 13 PASS / 0 FAIL on the real desktop (`odd/tools/hve-live-verify.sh`); the `kill -9` case is the one this unit was opened for
