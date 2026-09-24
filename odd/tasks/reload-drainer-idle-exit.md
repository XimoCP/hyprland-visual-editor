# Feature: the reload drainer stops outliving its usefulness (without ever losing a reload)

**Locator**: `odd/tasks/reload-drainer-idle-exit.md`
**Engram mirror**: topic `odd/reload-drainer-idle-exit/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches, do NOT rebase, do NOT push)
**Checkpoint before this work (save point)**: `d66f395` — clean tree; roll back here and this patch leaves no trace
**Status**: PLANNED 2026-09-24 — nothing implemented yet
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
| (pending) | | |

## Progress

- [ ] `HVE_RELOAD_IDLE_TICKS` idle counter that never counts owed work
- [ ] `reload.request.lock` handshake in the queue and in the drainer's exit path
- [ ] pidfile removed on normal exit + liveness check before signalling in the harness
- [ ] Tests: idle exit, claim-pending no-exit, request-pending no-exit, no-loss stress
- [ ] Whole suite green, committed as one work unit
- [ ] Cross-model verification (glm-5.3-flash)
- [ ] Live confirmation by the keeper
