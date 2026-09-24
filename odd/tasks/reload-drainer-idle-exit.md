# Feature: the reload drainer stops outliving its usefulness (without ever losing a reload)

**Locator**: `odd/tasks/reload-drainer-idle-exit.md`
**Engram mirror**: topic `odd/reload-drainer-idle-exit/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches, do NOT rebase, do NOT push)
**Checkpoint before this work (save point)**: `d66f395` — clean tree; roll back here and this patch leaves no trace
**Status**: W7 CLOSED 2026-09-24 — implemented (deepseek-v4-flash), cross-model verified (glm-5.3-flash) with both MINOR findings fixed, and **live-confirmed by the keeper (13 PASS / 0 FAIL)**.
**TDD**: strict — runner `cargo test`
**Delivery strategy**: `ask-on-risk` (no remote configured: delivery is local commits, no PRs)
**Delivery budget forecast**: ~200-350 authored changed lines (script + Rust tests + this document)

## Objective

The reload drainer must **not** survive HVE forever doing nothing, and it must never
buy that with the one thing that is forbidden: losing a reload.

The unit adds a bounded idle exit, guarded by a request handshake that makes
"exiting at the same instant a request arrives" impossible to lose, plus pidfile
hygiene so a dead drainer never leaves a pid behind that a future consumer could
mistake for a live one.

## Problem, with the measured evidence

`assets/scripts/reload_coalescer.sh` runs in two modes: `hve_reload_queue` (sourced
by `assemble.sh`) writes `reload.requested` and spawns the drainer; `drain` is the
single-owner loop. The loop is **designed to run forever** ("then loops forever",
line 13) and sleeps `HVE_RELOAD_DRAIN_MS` (default 600 ms) per tick.

Measured on 2026-09-24 before any change:

- `bash assets/scripts/reload_coalescer.sh drain`, **PPID 1, alive 13 h 42 min**,
  while HVE was not running and while `~/.cache/hve/` held **no**
  `reload.requested` and **no** `reload.claimed`. Nothing was owed; it was waking
  ~1.7 times per second, forever, doing nothing.
- The same class as W6: a child that outlives HVE. Here it is not a leak of a lock
  descriptor; it is a process that simply never ends.
- `~/.cache/hve/reload.drain.pid` was left behind pointing at that dead pid after
  it was killed. Any consumer that trusts the file would `kill -9` a **recycled
  pid** — a real hazard, not cosmetic.

## The trap: why the naive fix LOSES a reload

The obvious change — "exit after N idle ticks" — breaks the protocol that makes a
losing spawn safe. Today a spawn that loses the drain `flock` exits 0 **believing
the winning drainer will pick up the marker** (comment at lines 62-66). With an
idle exit that belief becomes false:

1. The old drainer reaches its idle bound: no marker, decides to exit.
2. In that same instant `hve_reload_queue` writes `reload.requested` and spawns a
   new drainer.
3. The new drainer's `flock -n` fails (the old one still holds it, milliseconds
   from exiting) → it exits 0, trusting the winner.
4. The old drainer exits anyway, never having seen the marker.
5. The marker sits unclaimed until some later request. **A reload is lost.**

The invariant of this unit's sibling (`odd/tasks/coalesce-config-reloads.md`) is
absolute: **an extra reload is always safe, a lost one never is.** So the release
of the lock and the decision to exit must be ordered against marker writes.

## Design

### 1. Idle exit that can never give up on owed work

- Track consecutive ticks in which **neither** `reload.requested` **nor**
  `reload.claimed` exists. Reset the counter to 0 whenever either exists.
- Exit only from the idle path, when the counter reaches
  `HVE_RELOAD_IDLE_TICKS` (default 20; ≈12 s at the default 600 ms window). A
  fixed tick count, independent of the drain window, so tests can set it small.
- **Never exit while a claim is pending.** While Hyprland is down the claim must
  keep being retried, exactly as today (lines 95-107). A claim is owed work: the
  drainer stays alive until it is fired and consumed, however long that takes.

### 2. The request handshake (the correctness core)

One new lock file: `reload.request.lock`.

- **The queue** takes it (blocking) around "write the marker" + "spawn the
  drainer". Held for microseconds; the spawn is `nohup … &` and returns at once.
- **The drainer** takes it (blocking) only when it has decided to exit, re-checks
  the marker **under the lock**, then either continues draining (marker present) or
  exits while holding it (the process exit releases it).

Why every interleaving is safe:

- *Queue first*: it holds the lock, writes the marker, spawns. The new drainer's
  `flock -n` fails because the old drainer still holds the drain lock — but the old
  drainer is blocked on the request lock and therefore **cannot exit**; when the
  queue releases, the old drainer re-checks, sees the marker and drains it.
- *Drainer first*: it re-checks under the lock, finds nothing, and exits holding the
  lock. The queue, blocked, then proceeds: writes the marker and spawns a fresh
  drainer that wins the now-free drain lock.
- *Neither exiting nor racing*: unchanged from today — the spawn loses, the old
  drainer's next tick sees the marker (the existing protocol).
- No deadlock: the drainer holds the **drain** lock while waiting for the request
  lock; the queue holds the request lock while spawning (a non-blocking attempt on
  the drain lock). The queue never waits for the drain lock while holding the
  request lock.

### 3. Pidfile hygiene

- `trap 'rm -f "$HVE_SAFE_DIR/reload.drain.pid"' EXIT` so a normal idle exit (and
  any trapped signal) removes the pidfile. SIGKILL still cannot run a trap —
  document that clearly.
- Because a SIGKILLed drainer always left a stale pidfile, the Rust consumers must
  **verify liveness before signalling**: `Sandbox::drop` (src/reload_coalescer.rs)
  and `kill_drainer` should check `/proc/<pid>` before `kill -9`, so a recycled pid
  can never be signalled. This is a real hazard fix, not a test nit.

## Non-goals

- Redesigning the coalescing protocol, the drain window default, or the
  claim-before-fire discipline.
- Changing the Hyprland-down semantics: a pending claim keeps retrying (today's
  behaviour, deliberately preserved).
- The disable switch and the missing-`flock` fail-open path must keep behaving
  exactly as they do.
- Anything outside `assets/scripts/reload_coalescer.sh` and its Rust tests/harness.

## Acceptance criteria

1. **Idle exit**: a drainer with no markers exits within the idle bound, and
   `reload.drain.pid` is removed (test with a small `HVE_RELOAD_IDLE_TICKS`).
2. **Claim pending → never exits**: with a pending `reload.claimed` and Hyprland
   down, the drainer stays alive past the idle bound; when Hyprland comes up it
   fires exactly once, consumes the claim, and only then exits after the idle bound.
3. **Request pending → never exits**: with a pending `reload.requested` the drainer
   stays alive and fires.
4. **No loss under stress**: repeated bursts with a tiny idle bound (so exits race
   the requests) never lose a reload — `reload_count == bursts` — and no marker is
   left behind.
5. **W5 semantics intact**: one reload per burst; the killed-drainer test; the
   claim-inside-the-window tests; the Hyprland-down test; the disable switch; the
   missing-`flock` fail-open. All existing tests pass unchanged (or with only
   harness changes justified in the diff).
6. **No process leaks**: the full suite leaves no drainer or stub behind, and the
   harness never signals a pid without confirming it is alive.
7. **Live confirmation (keeper)**: after a real theme apply, the reload fires once
   and, once the burst has settled, no `reload_coalescer.sh drain` process is left
   behind when HVE is not running.

## Checks (exact commands)

- `cargo test` — strict TDD, RED observed first, whole suite green at the end.
- `cargo test --bin hve reload_coalescer` — the unit's own tests.
- `bash -n assets/scripts/reload_coalescer.sh` — script syntax.
- `cargo build --release` (or `cargo check`) — no warnings.
- Stray audit after the suite: no leftover `reload_coalescer`/`sleep`/stub processes.

## Risks / residuals (to watch)

- **The handshake is the safety core.** A wrong implementation loses reloads
  silently. Every interleaving above must be covered by a test that would fail
  without it.
- **Idle counting must never include ticks with a marker/claim**: counting a claim
  as idle would let a Hyprland-down claim be abandoned (a lost reload).
- **SIGKILL leaves a stale pidfile** — inherent; mitigated by liveness checks in
  every consumer.
- **A fresh drainer per burst after idle** (~ms of bash + flock) — accepted; the
  drain window dominates.
- Tests are timing-based: use generous deadlines and RAII cleanup, per the house
  pattern.

## Verification record

| What | Who | Evidence |
| --- | --- | --- |
| Suite baseline (before any change) | writer | `cargo test` → **972 passed / 0 failed** (~46 s), 0 build warnings |
| RED-1 (strict TDD: the 4 new W7 tests against the UNMODIFIED script) | writer | `cargo test --bin hve reload_coalescer` → **11 passed / 4 failed** — all four new tests fail because the drainer never exits: `sandboxed_idle_drainer_exits_and_removes_pidfile` (pidfile never removed), `sandboxed_claim_pending_never_exits_and_fires_when_back` (final exit wait times out), `sandboxed_request_pending_keeps_drainer_alive_and_fires` (same), `sandboxed_no_loss_under_idle_exit_stress` (fires never settle). 11 pre-existing tests pass untouched |
| RED-2 (staged NAIVE fix: exit after N passes counting claim ticks as idle, no handshake, no trap) | writer | `cargo test --bin hve reload_coalescer` → **11 passed / 4 failed** — each new test pins one forbidden failure: claim test abandons the Hyprland-down claim deterministically (`a pending claim ... must keep the drainer alive past the idle bound`), request test exits with owed work (`must be the one serving burst 2`), idle test catches the missing pidfile trap, stress test LOSES reloads (`timed out waiting for every burst to fire`). The `kill:` ESRCH noise in that run also demonstrated why consumers must liveness-check before signalling |
| Unit suite (after implementing) | writer | `cargo test --bin hve reload_coalescer` → **15 passed / 0 failed** (6.17 s) — run TWICE, identical (timing-based tests are stable; the second run used an over-broad filter catching the whole module) |
| Full suite | writer | `cargo test` → **976 passed / 0 failed** (49.93 s: 972 baseline + 4 new), 0 build warnings |
| Script syntax | writer | `bash -n assets/scripts/reload_coalescer.sh` → OK |
| Build warnings | writer | `touch src/main.rs && cargo check` → exit 0, **0 warning/error lines** in the forced full recompile |
| Stray audit (after the full suite) | writer | No drainer/stub of this unit left standing (every live pid confirmed before any kill, per the new discipline). One pre-existing LIVE-STACK drainer observed mid-session (PID 809166: repo path, cwd = repo, `HVE_CACHE_DIR` unset → the REAL `~/.cache/hve`): spawned by the keeper's running HVE stack (the documented repo-cwd live stack) executing the pre-fix immortal script — deliberately left untouched for the keeper's live check. My own diagnostic stray (sandboxed `HVE_CACHE_DIR`, original immortal script) was removed with a liveness-confirmed SIGKILL and its temp tree deleted |
| Interleaving coverage | writer | Queue-first: exercised by `sandboxed_request_pending_keeps_drainer_alive_and_fires` (marker written through the queue while the drainer is idle; the same drainer re-checks under the lock and serves it) and statistically by the stress test. Drainer-first: `sandboxed_idle_drainer_exits_and_removes_pidfile` continuation (a request AFTER the exit spawns a fresh drainer that wins the now-free drain lock). No-race: every pre-existing test. No-deadlock: the full suite completes without a hang (a deadlock would stall the blocking `flock 8` and every queue-dependent test would time out loudly). The blind window of a race is microseconds — the stress test is the statistical net, the two deterministic tests pin the counter rules that make the exit safe |
| Cross-model independent verification | verifier (glm-5.3-flash — a DIFFERENT model than the writer, per AGENTS.md model roles) | **PASS WITH FINDINGS.** Every claim re-run: `bash -n` OK; unit suite **15/0 ×4** identical; full suite **976/0** (and one 975/1 flake, finding 1 below); `cargo check` 0 warnings; stray audit clean; the keeper's live pre-fix drainer left untouched. The verifier FALSIFIED the core by experiment, not by reading: queue-first (blocked queue + 1-tick drainer → the drainer stays, re-checks under the lock, serves the marker, fires once, exits); drainer-first (4 blocked-queue rounds → exactly 1 fire each, nothing left); the fd-inheritance claim confirmed with its own probe (plain `nohup` child keeps the lock; the `exec 8>&-` wrapper drops it); the idle counter confirmed never to count a marker/claim tick (2.0 s alive with a pending claim, then one fire, then exit); no deadlock or stall found. Mutants: no-handshake → the marker stranded with **0 fires** (the exact predator interleaving) while the real code served the same race in all 14 rounds; naive claim counting → abandons a pending claim deterministically. Unreproducible from the repo state (not contradicted, event history predates the single commit): the writer's baseline 972, RED-1/RED-2 discrimination, and the stress-test spacing correction. |
| Correction 2026-09-24 (post-verification) | orchestrator | The verifier's two MINOR findings were fixed and the suite re-run: full suite **976/0 twice** (50.68 s / 49.77 s), `cargo check` 0 warnings/errors. See the findings below. |
| Live confirmation | the keeper, on his own desktop (2026-09-24 20:38) | **PASS 13 / FAIL 0** via `odd/tools/hve-live-verify.sh` (dev binary `4fe4b7a9`, repo tree). W7-specific: a reload burst spawned a drainer; the owed reload was **consumed** (no marker left behind); the drainer then **exited on its own** after the idle bound — no immortal child. The same run first removed a stale pre-fix drainer (0 left, nothing owed) and confirmed W6's `kill -9` case. |

## Findings from the cross-model verification (2026-09-24)

- **MINOR — RESOLVED** (`src/reload_coalescer.rs`, the claim test):
  `sandboxed_claim_pending_never_exits_and_fires_when_back` asserted the claim's
  absence IMMEDIATELY after the reload count reached 1. The stub's `rm` follows
  its `hyprctl reload` return by a window the 40 ms poller can straddle — it
  flaked once in three full-suite runs (975/1). Fixed by polling for the claim's
  and the marker's absence (bounded 3 s) instead of asserting immediately.
- **MINOR — RESOLVED** (`Sandbox::drop`, `kill_drainer`): the liveness guard was
  a TOCTOU (`/proc/<pid>` existence, then `kill -9`), so it only *reduced* the
  recycled-pid window. Fixed with an identity check (`is_live_drainer`: the
  process must be alive AND its `/proc/<pid>/cmdline` must read
  `reload_coalescer.sh … drain`), so a stale pidfile can never signal a stranger.
- **NIT — accepted** : the stress test asserts `reload_count == bursts`, a
  count-level invariant, not per-burst identity. A fold+duplicate cancellation
  could in theory mask a loss, but duplicate fires are structurally impossible
  without kills, so the count invariant is sufficient (verified reasoning).
- **NIT — accepted**: the writer's phrasing "spacing greater than twice the drain
  window" is slightly generous (actual 2.2-2.8×); the numeric claim holds.
- **Unreproducible from the repo state** (not contradicted): baseline 972, RED-1,
  RED-2 and the stress-test spacing correction — the event history predates the
  single commit. The shipped tests' discriminating power was nonetheless proven
  by the verifier's own mutants.



## Implementation notes (writer, 2026-09-24)

- **The spawn inherits the handshake lock** (empirically confirmed: a `bash -c`
  child spawned while the parent holds `flock` keeps the lock alive through
  its inherited descriptor — the queue would deadlock every later request).
  The spawn wrapper (`bash -c 'exec 8>&-; exec bash "$1" drain'`) drops the
  inherited descriptor before exec'ing the drainer; the drainer also closes
  fd 8 defensively at start (verified silent no-op on a closed fd).
- **Strict-TDD honesty**: the stress test as first drafted spaced bursts
  INSIDE the drain window (80-160 ms vs 80 ms), which legitimately FOLDS
  bursts — `reload_count < bursts` is coalescing, not loss (the immortal
  script produced 7 fires for 12 requests with zero stray markers). The test
  was corrected to independent bursts (spacing ≥ 2 × drain window) so
  `reload_count == bursts` is the right invariant; no assertion was weakened.
- **SIGKILL/kill noise**: before the liveness fix the harness printed
  `kill: no existe el proceso` when dropping dead drainers; after the fix the
  suite is silent — the liveness check is verified by the absence of that
  noise and by inspection of `Sandbox::drop`/`kill_drainer`.

## Progress

- [x] `HVE_RELOAD_IDLE_TICKS` idle counter that never counts owed work
- [x] `reload.request.lock` handshake in the queue and in the drainer's exit path
- [x] pidfile removed on normal exit + liveness check before signalling in the harness
- [x] Tests: idle exit, claim-pending no-exit, request-pending no-exit, no-loss stress
- [x] Whole suite green, committed as one work unit — `5eb9afc` (code + tests) and the doc commit on `hve2-visual-rewrite` 2026-09-24
- [x] Cross-model verification (glm-5.3-flash) — 2026-09-24: PASS WITH FINDINGS, both MINOR findings fixed, suite 976/0 twice
- [x] Live confirmation by the keeper — 2026-09-24 20:38: 13 PASS / 0 FAIL; the burst's drainer consumed the reload and exited on its own (the pre-fix immortal child is gone)
