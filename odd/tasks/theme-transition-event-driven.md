# Feature: the theme transition waits for the compositor, not the clock (W5)

**Locator**: `odd/tasks/theme-transition-event-driven.md`
**Engram mirror**: topic `odd/theme-transition-event-driven/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches, do NOT rebase)
**Checkpoint before this work**: `9405ca2` (clean tree)
**Status**: CLOSED — event-driven finale shipped; the owner's live test passed ("ha quedado perfecto")
**TDD**: strict — runner `cargo test`
**Verification**: cross-model (a different model from the writer), as agreed

## What shipped, after three designs

The unit went through three shapes, and only the third matched reality. Recording all three so nobody repeats them:

1. **W5a — wait for the `fullscreen` event.** FALSIFIED live: during a normal transition HVE is ALREADY immersive, so the compositor has no state change to announce and the event never fires. The signal was wired but could never win. Kept in the code, harmless, documented as dead.
2. **W5b — wait for the reload burst to settle.** Correct in principle (the reload arrives in bursts of 5-6 in one second, plus late ones), but it made the finale ride the reload's timing: it fired at ~455ms, BEFORE the 560ms stage, so the empty desktop became a flicker that exposed the real workspace. The owner caught it immediately: "un parpadeo ... se ve una ventana abierta".
3. **W5c — SHIPPED.** The finale no longer rides the reload's timing. The order is:
   - detect the settled reload for real (no guessing) — this is the robustness;
   - stage the empty desktop (default 0ms delay: the owner picked immediate);
   - HOLD it for the display time (default 940ms) — this is the part he wanted to keep;
   - finale.

That split is the whole lesson: **state changes are decided by reality; the waiting inside a state is decided by taste.** A slow machine delays the state and the display time is still honoured; a fast machine shows the theme for the same time.

## Measured

- Before: the finale fired at a fixed 1500ms, or (W5b) at ~455ms which broke the stage order.
- Now: reload settles at ~455ms, stage immediately, 940ms display, then finale.
- The owner's live verdict: "A quedado perfecto... esto va por estados... y le añadimos segundos de espera propios".

## Tunables (no rebuild needed)

- `HVE_INTERLUDE_STAGE_MS` (default **0**) — delay before staging the empty desktop.
- `HVE_INTERLUDE_DISPLAY_MS` (default **940**) — how long the empty desktop is shown.
- `HVE_RELOAD_SETTLE_MS` (default **450**) — the silence that means "the reload burst ended".


## Objective

Stop the theme transition from guessing WHEN the compositor is ready. Every step
must wait for the compositor's own signal, with the existing timer kept only as a
bounded fallback. The failure this fixes: a slower machine (the owner's laptop) or
a slow reload makes a timer fire before the compositor finished, HVE does not end
up immersive, and the shell bar stays ABOVE the window.

## Evidence this is built on

- The transition is a cascade of timers: T=0 mask, T=560 stage, T=1500 finale,
  T=1900 fullscreen, T=2470 restore, T=3500 watchdog (`src/main.rs`).
- The sentinel's own recording (326 events, real Hyprland) shows:
  - `fullscreen>>0` / `fullscreen>>1` arrive **in pairs**, one per fullscreen
    cycle — this is the signal that says HVE became immersive;
  - `configreloaded` arrives in bursts whose span ranges from **2 s to 12 s**.
    A fixed 1900 ms timer cannot know that, which is exactly why the failure is
    machine-dependent and intermittent;
  - the other events (`activewindow`, `workspace`, `focusedmon`) are focus noise,
    not phase signals.

## Design

The timers stay, but each one becomes **cancelable by its signal**. Whichever
arrives first wins:

| Step | Signal that advances it | Fallback timer (unchanged) |
| --- | --- | --- |
| stage (view -> empty ws) | none — it is a UI step | 560 ms |
| fullscreen cycle | `fullscreen` event for HVE | 1900 ms |
| return view + restore masks | a settled `configreloaded` | 2470 ms |
| whole transition | — | 3500 ms watchdog (unchanged) |

Why this shape and not a rewrite: the existing behaviour is the safety net. The
signal only ever makes a step happen EARLIER than the timer, so a signal that
never arrives degrades to exactly today's behaviour. That makes this change
impossible to hang the desktop with.

## Tasks

- [ ] T1 — signal plumbing: the IPC listener recognises `fullscreen` and a
      settled `configreloaded`, and forwards them to the transition.
- [ ] T2 — cancelable steps: the fullscreen step and the restore step run on
      signal or fallback, whichever first, exactly once (generation-guarded).
- [ ] T3 — tests: pure decision (signal vs fallback), generation guard, and the
      "no signal ever arrives -> fallback still runs" path.
- [ ] T4 — cross-model verification round on the fix.
- [ ] T5 — live acceptance (owner): apply themes on the SLOW laptop, where the
      timers used to fail.

## Acceptance criteria

1. `cargo test` green, no warnings.
2. A step never runs twice (generation guard) and never hangs (fallback intact).
3. Live: on the slow machine, the bar does NOT stay above HVE.
4. The sentinel shows no `SUSPECT` entries across the live test.

## Checks

- `cargo test` (full suite).
- Sentinel report: `python3 ~/.local/bin/assets/scripts/hve-sentinel.py --report`.
- Live acceptance on the slow laptop (owner).

## Route

Delegated-writer candidate: 1 production file (`src/main.rs`) plus its tests, but
the design spans several existing subsystems (IPC listener, transition flags,
timers), so it is written INLINE by the orchestrator and verified cross-model, the
same route W1 took.
