# Feature: coalesce config reloads (one reload per change, never lost)

**Locator**: `odd/tasks/coalesce-config-reloads.md`
**Engram mirror**: topic `odd/coalesce-config-reloads/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches)
**Checkpoint before this work (save point)**: `f0d77b485755665e4757790cddb5055795ed24b3` — clean tree; roll back here and this patch leaves no trace
**Status**: W1 IMPLEMENTED 2026-09-23 — W5 correction (cross-model MAJOR finding) SHIPPED — re-verification pending
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~250-400 authored changed lines (source + tests +
this document). NOTE: earlier units have systematically missed their forecasts
(~2x, mostly tests); the same risk applies here and is recorded in advance.

## Objective

A theme apply must reload Hyprland **once per change**, not once per script —
without ever losing a reload. Today a single apply fires up to seven reloads
(measured below): every fragment script reloads, the reload itself triggers an
IPC echo that re-runs the assembler (another reload), and the colour watcher
adds its own. Every reload resets the runtime keyword overrides and re-floats
HVE (see `fullscreen-survives-config-reload.md`), so the reloads themselves
are the amplifier behind the flash and the stuck window.

## The safety criterion (primary acceptance criterion)

**NEVER lose a reload — only coalesce it.** The worst acceptable outcome if
the mechanism fails is that a reload arrives LATER, never that it never
arrives. Encoded as the protocol invariant: **an extra reload is safe, a lost
one never is** — a crash may cost at most ONE redundant reload, never a
missing one. Proven by the kill-window, post-fire-crash and Hyprland-down
tests in `src/reload_coalescer.rs`, not promised in prose.

## Problem, with the measured evidence (Task 1, 2026-09-23)

### Method (reproducible, sandboxed)

Sandbox at `/tmp/opencode/coalesce-measure.sh` (harness script) →
`/tmp/opencode/coalesce-measure-sb/` (generated tree): the REAL scripts from
`assets/scripts/` copied unmodified into a temp tree with stub `bin/` prepended
to `PATH` (`hyprctl`, `pgrep`, `noctalia`, `inotifywait`, `notify-send`,
`hve-ipc`), `HOME` and `HVE_CACHE_DIR` redirected into the temp tree — the
house pattern from `hide-idempotency-and-singleton-watcher.md`. The `hyprctl`
stub timestamps every invocation and appends a `configreloaded>>` line per
`reload` (simulating Hyprland's IPC event); the `pgrep` stub reports a running
Hyprland. The stub hyprctl is the ONLY hyprctl in the loop — nothing real is
ever invoked, and nothing on the keeper's desktop is touched.

Three scenarios, run against the code's own call orders:

1. **One theme apply** — the exact order of
   `src/providers/hve_presets.rs:64-148` (`apply`): `apply_animation.sh test`
   → `border.sh test` → `shader.sh test.frag` → `geometry.sh 4 4 5 5`,
   back to back, each via `bash` exactly as `Engine::run_script` does.
2. **The IPC echo** — every reload produced a `configreloaded>>` line;
   the listener logic of `src/hypr_ipc.rs:104-172` was simulated against
   that event stream (line-level masks first, then the throttled
   re-run of `assemble.sh` at most once per 3 s, exactly lines 200-211).
3. **The colour watcher** — the REAL `color_watcher.sh` started with the
   stub `inotifywait`; a Noctalia v5 `settings.toml` palette change while
   it runs.

### Observed numbers (one theme apply, 2026-09-23)

Timeline (epoch-seconds; `reloads.log` + boundary timestamps from the
harness):

| # | t (s) | origin |
|---|-------|--------|
| 1 | +0.069 | `apply_animation.sh` → `assemble.sh` reload (`assemble.sh:92-94`) |
| 2 | +0.140 | `border.sh` → `assemble.sh` reload |
| 3 | +0.211 | `shader.sh` → `assemble.sh` reload |
| 4 | +0.282 | `geometry.sh` → `assemble.sh` reload |
| 5 | +0.355 | **IPC echo**: `configreloaded` → listener re-runs `assemble.sh` (`hypr_ipc.rs:200-211`) → reload — *a reload caused by a reload* |
| 6 | +0.447 | `color_watcher.sh` startup "Initial overlay refresh" → `assemble.sh` → reload |
| 7 | +2.66 | `color_watcher.sh` palette change → `noctalia msg templates-apply` → `assemble.sh` → reload |

**Total: 7 reloads for one theme apply** (6 when the apply carries no palette
change). The IPC echo chain then stops: the echo reload's `configreloaded`
lands inside the listener's 3 s throttle and is skipped — exactly one echo per
burst. The keeper's measured 19 "masks re-applied after reload wipe" lines per
minute are several applies plus watcher cycles at this per-apply cadence.

### Full inventory of reload sources (read from source, attributed)

| Source | Fires per | Reload mechanism |
|---|---|---|
| `apply_animation.sh` | fragment apply | `assemble.sh:92-94` |
| `border.sh` | fragment apply | `assemble.sh:92-94` |
| `shader.sh` (case 1 removes the fragment; case 2 writes it) | fragment apply | `assemble.sh:92-94` |
| `geometry.sh` | fragment apply | `assemble.sh:92-94` |
| `hypr_ipc.rs:200-211` (IPC echo) | every non-throttled `configreloaded` from any reload | re-runs `assemble.sh` → its reload |
| `main.rs:348` (`apply_live_draft`, border tune) | tune draft apply | `assemble.sh` |
| `main.rs:3660` (tune slider commit) | tune slider flush | `assemble.sh` |
| `color_watcher.sh` | startup + per detected change | `assemble.sh` |
| `init.sh:92-98` + `:146` / `:162` | enable / disable | `assemble.sh` **plus a DIRECT `hyprctl reload`** |
| `settings.rs:154,327,485` (windowrules / keybinds / autostart) | only when that setting changes (not during a theme apply) | direct `hyprctl reload` |
| `src/providers/shell.rs:117` (`reload_command`) | never in production — test-only (verified by grep; only test code calls it) | — |

### Task-1 gate: does coalescing require changing visible desktop behaviour?

No STOP condition triggered. Every fragment script writes its fragment to
`$HVE_FRAGMENTS_DIR/*.lua` BEFORE calling `assemble.sh`, and every `assemble.sh`
writes the whole `overlay.lua` atomically BEFORE the reload. A coalesced reload
fires once the burst is quiet and re-reads `overlay.lua` **at fire time**, so
the single reload carries the complete final state of the burst — identical
desktop state to today's 7th reload, reached in one transition instead of
seven. No fragment ever "stops taking effect until something else happens":
the last quiet period always ends with a reload. The only behavioural delta is
timing (the transition lands ~0.6-1.2 s after the last request instead of
instantly per script), which is the entire point of the feature. Direct reloads
that do NOT funnel through `assemble.sh` (`settings.rs` ×3, `init.sh` direct
calls) are untouched — they are rare, non-bursty, and out of this unit's
scripts-only scope.

Therefore: **no Rust production change is required**. All per-apply reload
paths funnel through `assemble.sh`; the coalescer lives exactly there. The
only Rust touch is test scaffolding (`#[cfg(test)] mod`), the house pattern
for testing the real bash scripts.

## Intent

`assemble.sh` stops reloading inline. Instead it queues a reload through a
small, standalone coalescer (`assets/scripts/reload_coalescer.sh`, one new
file): write an atomic `reload.requested` marker, spawn a single-owner
drainer that fires `hyprctl reload` once the burst has been quiet for the
drain window — and, if the drainer dies before firing, the marker survives
and the next request consumes it. Disable with one env var. Tidy seams:

- **Coalescing, not deleting**: one marker generation = one reload; requests
  inside the drain window fold into one; independent changes (separated by
  more than the window) each get their own reload.
- **Never lost**: the pending evidence is the durable memory of "a reload is
  owed". The drainer CLAIMS it (`mv` → `reload.claimed`, atomic) before the
  drain window and consumes it only AFTER a successful fire — so a drainer
  killed at any point before the fire leaves evidence the next drainer pass
  fires. The drainer loops forever; a marker or claim is fired by the
  drainer that owns the lock (or by the next spawn if that one died). The
  ONLY way a reload never arrives is a kill of the drainer AND no further
  request ever: the claim then sits on disk until something else touches the
  desktop (any later apply, click, colour event, or a fresh drainer spawned
  by the next request). Nothing sweeps it periodically — the colour watcher's
  30 s timer only re-arms `inotifywait`, it does **not** re-run the queue, and
  `assemble.sh` does not scan for a claim when sourced. So the honest bound is
  **delayed, never lost while any future request arrives**, and the
  kill-then-request tests pin exactly that. (W5 correction: the original
  protocol removed the marker BEFORE the drain window, so a kill inside that
  window lost the reload despite the "never lost" claim — see the correction
  record below.)
- **Preserved semantics**: the "is Hyprland running" guard runs at request
  time (exactly where it runs today) AND at fire time (the drainer re-checks,
  because Hyprland may have died during the window). Hyprland down at request
  time → nothing is even queued, exactly like today. Hyprland down at FIRE
  time → no reload now, but the pending claim survives and fires when the
  compositor is back — a delayed reload, never a lost one (W5 correction,
  replacing the old consume-without-firing skip).
- **Fail open / disable**: `flock` missing → reload immediately (the watcher's
  fail-open precedent); `HVE_RELOAD_COALESCE=0` → the queue performs today's
  exact inline reload. One obvious switch.

## Design details (house pattern: pure decisions in bash, thin impure shell)

State under `HVE_SAFE_DIR`:
- `reload.requested` — atomic marker ("a reload is owed"); written by
  `hve_reload_queue`, CLAIMED by the drainer (`mv`, atomic) before the drain
  window.
- `reload.claimed` — the durable in-flight claim: the owed reload's memory
  from the claim until AFTER a successful fire; consumed (`rm`) only then.
  Survives a killed drainer and a down Hyprland (retry loop).
- `reload.drain.lock` — `flock` held for the drainer's whole life; only one
  drainer per machine. Spawned losers exit immediately.
- `reload.drain.pid` — the live drainer's PID (diagnosis + the
  killed-drainer test).

`hve_reload_queue()` (sourced by `assemble.sh`):
1. `HVE_RELOAD_COALESCE=0` → immediate reload (pgrep-guarded, today's block).
2. `flock` missing → warn once to stderr, immediate reload (fail open).
3. `echo timestamp > reload.requested.tmp && mv` → atomic marker.
4. `nohup bash reload_coalescer.sh drain >/dev/null 2>&1 &` — detached with
   its own stdio: the engine captures assemble's stdout/stderr pipes, and the
   drainer must never hold them (or `Command::output()` would block until the
   drainer exits). A loser (`flock -n` fails) exits 0; the winner picks up
   the marker.

The drainer (`reload_coalescer.sh drain`):
```
acquire flock -n on reload.drain.lock  (loser: exit 0)
write $$ to reload.drain.pid
loop:
  if marker OR claim exists:
      if marker exists: mv marker claim   # claim/fold, atomic
      sleep DRAIN                         # graceful folding window
      if marker exists: continue          # a request landed during grace → re-drain
      if pgrep -x Hyprland:               # fire, then consume — and only then
          if hyprctl reload: rm claim
      # Hyprland down or fire failed: claim STAYS, retry next pass
  else:
      sleep DRAIN
```
Invariant: the claim is consumed (`rm`) only AFTER a successful fire, so the
pending evidence survives every failure point (kill inside the window, kill
during the fire, Hyprland down, failed hyprctl). A crash after the fire
leaves a claim the next drainer pass fires — at most ONE redundant reload,
the intended trade. Consumption never precedes a fire.
Fire latency after the last request of a burst: ≤ 2 × `HVE_RELOAD_DRAIN_MS`
(default 600 ms → ≤ 1.2 s). The echo reload observed in the measurement
(nr. 5) lands ~0.7 s after the last fragment script — outside the 600 ms
window — so a theme apply fires **2** reloads (the drained burst + the echo)
instead of **7**; the echo's own `configreloaded` is then eaten by the
listener's 3 s throttle, and the chain stops, exactly as measured today.

## Work units

**W1 — the coalescer + assemble wiring** (this unit):
`assets/scripts/reload_coalescer.sh` (new), `assets/scripts/assemble.sh`
(source line + queue call replacing the inline reload block), tests in a new
`src/reload_coalescer.rs` (`#![cfg(test)]`, wired with one `#[cfg(test)] mod`
line in `src/main.rs` — test scaffolding only, no production Rust change).

## Acceptance criteria

1. A burst of fragment applies (animation + border + shader + geometry) fires
   EXACTLY ONE reload, after the burst, with the final overlay in place.
2. A reload is never lost: a drainer killed between marker write and fire
   loses nothing — the next request's drainer fires the leftover marker
   (proven by test, with the reload arriving LATER than the burst).
3. A stray marker (orphaned by a dead drainer, no further edits) is consumed
   by the next request's drainer and fires.
4. Independent changes (separated by ≥ 2 × drain window) each fire their own
   reload: coalescing never swallows a real change.
5. Hyprland not running → no reload (request-time and fire-time guards),
   same as today.
6. `HVE_RELOAD_COALESCE=0` restores today's exact behaviour (one reload per
   assemble, immediate).
7. `flock` missing → fail open: reloads still happen (immediate), with one
   distinct warning line.
8. The engine's `Command::output()` path never blocks: the drainer holds no
   capture pipes (proved by the sandbox tests returning promptly).

## Checks

- `cargo test` — strict TDD, RED first, whole suite green at the end.
- Suite baseline before W1: **955 passed / 0 failed** (2026-09-23), 0 build
  warnings.
- Tests NEVER call the real `hyprctl` and never touch the keeper's live
  session (sandboxed HOME + HVE_CACHE_DIR + stub PATH, house pattern).
- Live confirmation belongs to the keeper: apply a theme and watch the
  reloads collapse to one transition; the flash and the stuck window are the
  symptoms this unit attacks.
- AGENTS.md VISUAL VERIFICATION does not apply: no `.slint` file changes.

## Risks / residuals (to watch)

1. **The echo reload survives (2 per apply, not 1).** The IPC listener's
   throttled re-run of `assemble.sh` lands after the drain window and queues
   its own reload. Bounded (one per burst — the 3 s throttle eats the next
   echo), measured, and far below 7; folding it would require coupling the
   bash coalescer to the Rust throttle, out of scripts-only scope. Recorded,
   not chased.
2. **Fragment writes between assemble and fire are still picked up.** A
   fragment written by a concurrent path that does NOT call assemble before
   the drainer fires would be missed by that fire — but every fragment
   writer in HVE calls assemble immediately after writing; the drainer fires
   only after quiet, so the last write always lands first. The remaining
   window (a writer mid-fragment at fire time) produces a fragment that the
   next assemble covers — same guarantee as today's per-assemble reload.
3. **The drain window is a tunable, not a proof.** `HVE_RELOAD_DRAIN_MS`
   default 600 ms; bursts longer than the window split into multiple
   reloads (safe: more reloads, never fewer). The kill test pins the
   never-lost property; the exact window is for the keeper's live feel.
4. **A killed drainer with NO further requests leaves the marker until the
   next assemble ever runs** (any apply, click, tune, or watcher cycle). The
   criterion's "arrives LATER, never never" holds, with the bound set by the
   next request; a bare `kill -9` mid-wave on a machine with no further HVE
   activity is the only way a reload could go un-fired, and it is the same
   class of failure as today's "process died before reload".
5. **`nohup` availability**: coreutils, always present where HVE runs.
6. **settings.rs / init.sh direct reloads stay untouched** — they do not
   funnel through the assembler and are out of scope (rare, non-burst).

## Verification record

| Check | Who | Result |
|---|---|---|
| Suite baseline (W1) | writer, before any change | `cargo test` → **955 passed / 0 failed** (~47 s), 0 build warnings |
| RED first (strict TDD) | writer, observed before implementing | **Two staged REDs.** RED-1 (module file absent): `cargo test --bin hve reload_coalescer` → **0 passed / 8 failed** — every test fails staging `reload_coalescer.sh` (`No such file or directory`). RED-2 (behavioural, after wiring `assemble.sh` to a no-op `hve_reload_queue`): **1 passed / 7 failed** — the burst never fires a reload, the killed-drainer test finds no drainer pid, the disable switch fires nothing; the 1 pass is `sandboxed_hyprland_down_fires_nothing`, which pins the Hyprland-down guard and is expected to pass at every stage. Both RED outputs kept. |
| Full suite (W1) | writer, after implementing | `cargo test` → **963 passed / 0 failed** (48.9 s: 955 baseline + 8 new), **0 build warnings** (verified with an explicit warning grep after `touch src/main.rs` + `cargo test --no-run`) |
| End-to-end probe (real repo scripts, stub hyprctl, sandbox HOME) | writer | `/tmp/opencode/coalesce-probe.sh`: 4 fragment scripts → **1 reload** (was 4), marker consumed, independent second burst fires its own (2 total), `HVE_RELOAD_COALESCE=0` → immediate reload |
| Live behaviour (keeper) | **the keeper, on his own desktop** (the only valid check for compositor state) | **PASS** — the keeper ran his own visual verification on 2026-09-24 and reported it as a complete success (his words: *"ha sido todo un exito"*), across the three checks he was asked to run. The reload storm is gone: a theme apply no longer fires the seven-reload burst the measurement attributed. |
| Cross-model independent verification | a DIFFERENT model than the writer (AGENTS.md model roles) | — |

### Unplanned live observation (2026-09-23, must be reported)

The keeper's installed HVE (PID 65715, `/home/ximo/.local/bin/hve --tray`) is
running with **cwd = this repo**, so its singleton colour watcher (PID 68027)
and its IPC listener execute the REPO `assets/scripts/assemble.sh` fresh on
every invocation. Editing the repo's `assemble.sh` mid-session therefore went
**live in the keeper's running stack immediately**: at 19:22:28 one real
`hyprctl reload` fired on the live desktop THROUGH this new coalescer (the
watcher's cycle wrote the overlay at 19:22, queued, the drainer consumed the
marker and fired once). Observed aftermath in the real `~/.cache/hve/`:
overlay rewritten, `reload.drain.lock` + `reload.drain.pid` present, **marker
absent** (consumed). The feature behaved exactly per design on a real desktop:
one fire, nothing lost, no cadence change (the watcher's single request per
event produces one fire, coalescing only folds bursts). NOTHING was run,
started or stopped by this unit; the fire was the keeper's own running stack
executing the edited script. Consequences for the keeper: one extra reload
cycle (the same class of event every watcher event already produces; W3's
masks/fullscreen recovery covers it) happened while he was away, and the
coalescer is now live for his session's next applies — which is the intended
feature, active one apply earlier than planned. Tests never touched the real
hyprctl: every suite test runs sandboxed HOME + stub PATH, and the sandbox
paths were cross-checked against this process's environment.

## W5 correction record (cross-model verification, 2026-09-23)

Commit `f9a5e71` (W1) was reviewed by an independent verifier (different
model, per AGENTS.md model roles) and returned **PASS WITH FINDINGS**, one
**MAJOR**:

> **The drainer consumes the marker BEFORE firing.** `reload_coalescer.sh:75-77`
> (`rm -f "$HVE_SAFE_DIR/reload.requested"` → `sleep "$drain_secs"` → pgrep →
> `hyprctl reload`). If the drainer dies during that ~600 ms window after the
> removal, the marker is gone, the reload never happens, and nothing recovers
> it (the next drainer sees no marker). The test
> `sandboxed_killed_drainer_loses_nothing` kills the drainer as soon as the
> pid appears — before it consumes the marker — so it does **not** cover this
> path. So the unit's absolute guarantee ("never lost, only delayed") is
> contradicted by a reachable path.

**Honest note**: the "NEVER LOST" claim (task doc + commit message) was simply
wrong in that window. The marker survived a drainer killed BEFORE it consumed
it, but not one killed in the ~600 ms between the removal and the fire — the
exact range the acceptance criterion cares about. The verifier also flagged as
MINOR the sibling case: a down Hyprland at fire time consumed the marker
without firing, losing the owed reload. Both are fixed here; nothing else was
touched.

**Corrected protocol** (shipped in this correction, see Design details
above): the drainer CLAIMS the marker (`mv` → `reload.claimed`, atomic)
before the drain window and consumes it (`rm`) only AFTER a successful
`hyprctl reload`. Every failure point keeps the pending evidence: a kill
inside the window, a kill during the fire, a down Hyprland, or a failed
hyprctl — the next drainer pass (or the same drainer's next pass, for the
down/retry case) fires the reload. Consumption never precedes a fire.

**The invariant**, encoded in the script's header and the tests: **an extra
reload is safe, a lost one never is.** The trade: a crash right after a
successful fire leaves a claim that the next drainer pass fires — at most ONE
redundant reload (in the normal fold path, zero extra: the leftover claim
merges into the next request's single fire). Never a missing one.

**New coverage** (RED first, kept):
- `sandboxed_killed_drainer_inside_window_recovers_reload` — kills the
  drainer AFTER the claim, BEFORE the fire (wide 1.5 s window), proves zero
  fires, the surviving claim, and the recovered reload via the next request.
- `sandboxed_post_fire_crash_costs_at_most_redundant_reload` — stub-hyprctl
  fire in flight (log then 3 s block), drainer killed inside the fire: the
  in-flight fire completes, the claim survives the crash, the next pass folds
  it — exactly two fires total, stable; never zero, never runaway.
- `sandboxed_hyprland_down_keeps_pending_and_fires_when_back` — up at
  request, down at fire time: no fire, claim kept; the SAME drainer's next
  pass fires once the (stub, file-flipped) compositor is back.
- `sandboxed_hyprland_down_fires_nothing` strengthened in intent (comment)
  but unchanged in behaviour: the request-time guard still swallows the queue,
  exactly like today.

All pre-existing tests untouched and green; suite total 963 → **966 passed /
0 failed, 0 build warnings**.

## Progress

- [x] Task 1 — measurement (this document, above): 7 reloads per theme apply,
  attributed and reproduced. No STOP condition triggered: coalescing changes
  when the reload fires, never which fragments take effect.
- [x] W1 — coalescer + assemble wiring (this unit). Shipped as a work-unit
  commit on 2026-09-23 (hash reported by the orchestrator, not
  self-referential on purpose). `assets/scripts/reload_coalescer.sh` (new,
  sourced queue + drain mode), `assets/scripts/assemble.sh` (source line +
  `hve_reload_queue` call replacing the inline `hyprctl reload`; the
  request-time pgrep guard is kept verbatim), tests in `src/reload_coalescer.rs`
  (new, `#![cfg(test)]`, wired with one `#[cfg(test)] mod` line in main.rs —
  test scaffolding only; zero production Rust changes).
- [ ] Cross-model independent verification.
- [x] W5 correction — the cross-model MAJOR finding (consume-before-fire) and
  the MINOR sibling (down-Hyprland consume-without-fire) fixed via the
  claim-then-fire protocol; new RED-first coverage; re-verification pending.
- [x] Live confirmation (keeper) — **PASS 2026-09-24** (see the verification record).. Note: the coalescer is ALREADY live on the
  keeper's running stack via his repo-cwd HVE (see the unplanned live
  observation) — his next apply is a live confirmation in itself. His live
  drainer (PID 202160, started 19:22:27, BEFORE this correction) still runs
  the OLD protocol until it exits; the fix takes over for drainers spawned
  after it dies. Nothing in this correction kills or restarts it.