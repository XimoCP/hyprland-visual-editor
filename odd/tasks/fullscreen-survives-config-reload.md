# Feature: immersive fullscreen survives a config reload

**Locator**: `odd/tasks/fullscreen-survives-config-reload.md`
**Engram mirror**: topic `odd/fullscreen-survives-config-reload/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches)
**Checkpoint before this work**: `1b150b0` (clean tree; roll back here and the patch leaves no trace)
**Status**: W1 IMPLEMENTED 2026-09-23 — cross-model verification and live confirmation pending
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~250-400 authored changed lines (source + tests +
this document). NOTE: the forecast was MISSED — the shipped unit is ~700 authored
lines, mostly tests (7 test functions across two files) plus the document; the
production code alone is ~180 lines. Recorded here so the size is visible
before the orchestrator's review, following the delivery-note precedent of
`skwd-policy-toggle-around-apply.md`.

## Objective

A config reload must not leave HVE windowed below the shell bar. When a reload
has wiped the immersive fullscreen (its only lever against layer-shell
surfaces) and HVE is supposed to be showing, put it back — with a single
owner, debounced, and never inside the theme-transition window (the finale
owns the cycle there).

## Problem, with the measured evidence

Every `hyprctl reload` resets the runtime keyword overrides (animations and
blur come back ON) **and re-floats HVE**, dropping its immersive fullscreen.
HVE sits above the Noctalia bar only while it is in immersive fullscreen mode
2; its own code documents that as its only lever against layer-shell surfaces
(`src/composer/hyprland.rs:369-443`, `set_fullscreen` — V5 `mode="fullscreen"`,
V4 fallback `fullscreen 2`). `src/main.rs:854-858` says so in its own comment
("re-floats HVE visibly").

Today's reaction to a reload is incomplete:

- `reapply_theme_masks()` (`src/main.rs:859-868`) re-applies the four masks
  and **never restores the fullscreen**; it is also gated on
  `is_theme_transitioning_flag()`, so a reload that lands *after* the
  transition ends is not even re-masked — which is why the desktop blur
  stays on.
- The `configreloaded` path in `src/hypr_ipc.rs:175-182` states explicitly
  that it deliberately stopped moving HVE and running the fullscreen cycle,
  because that caused a 4-actor race (event return vs step1/step2 timers).
- `reassert_gallery_fullscreen()` (`src/main.rs:890+`) is the only unset→set
  cycle and it returns early unless `Shell::is_gallery_expanded()`.

Live consequence, reported by the keeper on 2026-09-23: once a reload takes
the fullscreen away, his HVE window keeps its border and the desktop blur
stays on, with the bar drawn above it, **until he hides the app and launches
it again**. Measured in one minute of his testing: 19 "masks re-applied after
reload wipe" lines — i.e. ~19 reloads.

## Intent

When a config reload has taken HVE out of its immersive fullscreen and HVE is
supposed to be showing, run the SAME unset→set cycle the gallery reassert
uses — debounced with the same shared 1.5s window, decision-gated, and never
while a theme transition is in flight.

The unconditional event-path cycle was the documented 4-actor race and is NOT
being reintroduced: this restore is (a) owned by one decision function, (b)
debounced, (c) excluded from the transition window, and (d) re-checked at the
cycle's step 2 so a hide/transition mid-cycle cancels the set.

## Verified premises (read from source, 2026-09-23)

1. **`slint::Timer` is thread-local.** `Timer::single_shot` writes a
   THREAD-LOCAL timer list (`i-slint-core 1.17.0` `timers.rs:111-121,428`),
   silently swallowing the error when the thread has no timer infrastructure.
   A step-2 timer created on the IPC listener thread NEVER fires. The restore
   must hop to the UI thread via `slint::invoke_from_event_loop` (which
   returns `Err(EventLoopError::NoEventLoopProvider)` before the loop runs —
   ignored, best-effort; the listener's `on_config_reload` already uses this
   pattern).
2. **The shared debounce exists.** `LAST_REASSERT` + `reassert_debounce_ok()`
   (`src/main.rs:736,870-878`) already bound visible cycles to one per 1.5s.
   The restore reuses it, so gallery cycles and reload restore cycles share
   the same window and cannot stack.
3. **"Is HVE supposed to be showing" has one existing source of truth**: the
   `ShowStateMachine` (`src/show_state.rs`). Hidden = parked in the
   scratchpad (tray); Entering = a show in flight whose settle owns
   fullscreen; Visible = settled and showing. Only Visible may restore.
4. **The cycle mechanics exist**: `reassert_gallery_fullscreen`'s unset→set
   with a 150ms settle gap. Its gallery gate (`Shell::is_gallery_expanded`)
   is why it never runs for this case; the decision of *whether* to cycle
   must not be confused with gallery expansion, so the mechanics are factored
   into a shared `run_fullscreen_cycle(gate)` with the gate injected per
   caller.
5. **"HVE should hold immersive fullscreen" is tracked, not invented.**
   The controller's `gallery_session` flag mirrors the immersive session
   (enter on Gallery mount, exit on Gallery unmount — `shell/mod.rs:592-611`)
   and survives reloads (it is process state). The settings panel float is a
   DELIBERATE windowed presentation: `schedule_float` leaves fullscreen and
   floats the window — the window IS the live border preview — without
   closing the session (`shell/mod.rs:342-347`). So the immersive gate is
   `gallery_session_active() && !is_panel_floating_global()` — the same gate
   `Controller::run_settle` uses before its fullscreen dispatch. Without it,
   a reload burst during border tuning would force fullscreen over the
   preview (a regression the brief did not settle; see Risks).
6. **The fullscreen state is queryable headlessly**: `parse_hve_seen` /
   `query_hve_seen` (`src/composer/hyprland.rs:592-640`) parse
   `hyprctl clients -j` into mapped/fullscreen, never panicking on malformed
   or absent data.

## Scope

In scope (HVE only):

- Pure decision `reload_fs_action` + thin impure shell, wired at the
  `configreloaded` line path (`hypr_ipc.rs`, before the 3s throttle).
- Shared cycle factory `run_fullscreen_cycle`, the reload restore reusing the
  gallery reassert's exact mechanics; the gallery gate stays in
  `reassert_gallery_fullscreen` only.
- UI-thread hop for the restore (thread-local timers — see premise 1).
- Honest logging: refusals at debug, cycle outcomes at info. Never panic.
- No new visibility/heartbeat flag: `ShowStateMachine` and the existing
  statics are the only sources of truth.

Out of scope (recorded, not done here):

- Anything about reload frequency (`assets/scripts/assemble.sh` is
  untouched); reducing reloads is a SEPARATE, later decision the keeper has
  not taken yet.
- The theme-transition finale ownership (unchanged — see risk 4).
- Live compositor confirmation (keeper's check).

## Design constraints (house pattern)

- Pure decisions plus a thin impure shell, the same shape as `hide_plan`,
  `blur_decision` and `startup_fs_action`.
- Pure: given (theme transition in flight?, HVE supposed to be visible?,
  immersive fullscreen expected?, HVE currently immersive fullscreen?, time
  since the last cycle?) → the action (nothing, or the unset→set cycle).
  Every branch testable without a compositor.
- Impure: query the real state, claim the shared debounce, call the shared
  cycle, log honestly.
- Never panic: every query is best-effort (`Option`), every failure is logged
  and degrades to today's behaviour.

## Work units

**W1 — restore the immersive fullscreen after a config reload.**
Pure decision (`reload_fs_action`), factored shared cycle
(`run_fullscreen_cycle`), reload-path shell (`reassert_fullscreen_after_reload`
+ `apply_reload_fs_action`), wiring on every `configreloaded` line with the
UI-thread hop, shared debounce claim, step-2 gate re-check, and the strict-TDD
test set below.

## Acceptance criteria

1. A `configreloaded` line while HVE is Visible, inside the immersive
   presentation (gallery session active, panel not floating), not fullscreen
   (dropped by the reload), no transition in flight, no cycle in the last
   1.5s → exactly one unset→set cycle, nothing else.
2. The same while a theme transition is in flight → nothing (the finale owns
   the cycle; the single-fire claim in `reassert_gallery_fullscreen` is
   untouched).
3. The same while HVE is parked/hidden (ShowStateMachine not Visible) →
   nothing, no dispatch at all.
4. The same while HVE is outside the immersive presentation (collapsed Home
   — the session is closed on Gallery unmount — or the settings panel
   floats) → nothing: those presentations are deliberately windowed and a
   fullscreen force would destroy them.
5. The same while HVE is already immersive fullscreen → nothing (there is
   nothing to restore).
6. A second reload inside the 1.5s debounce window → no second cycle.
7. The cycle is the SAME mechanics as `reassert_gallery_fullscreen`'s (shared
   `run_fullscreen_cycle`); the reload restore never re-implements a second
   cycle and never consults the gallery expansion.
8. Mid-cycle (between unset and set) a hide, a transition start, or leaving
   the immersive presentation cancels the set; the unset alone is the state
   the reload already left.
9. No panics; refusals are logged at debug and cycle outcomes at info.

## Checks

- `cargo test` — strict TDD, RED first, whole suite green at the end.
- Suite baseline before W1: **934 passed / 0 failed** (2026-09-23, ~35 s).
- Live confirmation belongs to the keeper: reload while HVE is fullscreen and
  the border/blur/bar state must survive.

## Risks / residuals (to watch)

1. **Reload bursts mean one visible cycle per 1.5s.** At the keeper's
   measured cadence (~19 reloads/min ≈ one per 3.1s) a burst can visibly
   cycle repeatedly. Every cycle is a real fullscreen drop+restore — the
   SAME visible event the gallery reassert already produces, and a far
   better default than the stuck windowed state. The root cause (reload
   frequency) is a separate, later decision.
2. **The shared 1.5s debounce couples the two reasserts.** A gallery expand
   within 1.5s of a reload restore is skipped (and vice versa) — that is the
   documented one-visible-cycle contract, accepted.
3. **The immersive gate is a design decision the brief did not settle.**
   The brief's pure-input list has four items; the implementation adds
   `immersive_expected` (gallery session active && panel not floating).
   Evidence: without it, a reload burst while the settings panel floats
   would force fullscreen over the live border preview (the floating panel
   keeps the session flag active — `shell/mod.rs:342-347` never calls
   `exit_gallery_session`) and a reload on the collapsed Home would
   fullscreen a windowed screen. The keeper's hard constraints (never while
   hidden, never during a transition) are unaffected.
4. **`query_hve_seen` treats maximized as fullscreen** (Hyprland `fullscreen`
   field: 1 fullscreen, 2 maximized — `hyprland.rs:601-606,619`). A maximized
   window therefore never cycles: conservative, consistent with the startup
   settle.
5. **The finale ownership must not drift.** The reload restore refuses
   entirely inside the transition window. If the finale's single-fire
   behaviour ever changes, risk 1 changes too — the transition-window tests
   pin this.
6. **The pre-loop hop is lost.** A `configreloaded` line arriving before
   `run_event_loop` (e.g. the startup rules rewrite) queues nothing: the hop
   errors and is ignored. Masks still re-apply directly; the restore waits
   for the next reload line. Bounded and best-effort by design.
7. **A manual fullscreen exit is not observed.** If the keeper exits
   fullscreen manually (SUPER+F) while the gallery session is still active,
   the next restore re-asserts within the debounce window (the session flag
   is the app's contract; the app does not track manual exits — tracking
   them would be a new flag, which the brief forbids). The keeper's escape
   hatches are the gallery unmount and the tray hide.

## Verification record

| Check | Who | Result |
|---|---|---|
| Suite baseline | writer, before any change | `cargo test` → **934 passed / 0 failed** (~35 s), 0 build warnings. |
| RED first (strict TDD) | writer, observed before implementing | 7 new tests failed against `todo!()` stubs; the refreshed decision signature (added the `immersive_expected` gate, see Risks 3) failed to compile before its implementation — both REDs captured. |
| Full suite (W1) | writer, after implementing | `cargo test` → **941 passed / 0 failed** (~34 s), 0 build warnings. |
| Cross-model independent verification (read-only, adversarial) | a DIFFERENT model than the writer (per AGENTS.md model roles) | **PENDING** — Tier 3 (state machine + timers + compositor subsystem). |
| Live behaviour | **NOT RUN** | Needs the keeper: reload while HVE is fullscreen and watch the border/blur/bar survive. |

## Progress

- [x] W1 — decision + shared cycle + reload-path wiring (this unit). Shipped
  as a work-unit commit on 2026-09-23 (hash reported by the orchestrator, not
  self-referential on purpose). Primitives live in `src/main.rs`:
  pure `reload_fs_action` (five inputs — including the `immersive_expected`
  gate the brief did not settle, see Risks 3; `ReloadFsAction` /
  `ReloadFsSkip` including `NotImmersive`), shared
  `run_fullscreen_cycle(gate)` factored out of `reassert_gallery_fullscreen`,
  impure shell `reassert_fullscreen_after_reload()` +
  `apply_reload_fs_action`, `hve_supposed_visible()` backed by
  `ShowStateMachine`, `immersive_expected()` backed by
  `gallery_session_active()` + `is_panel_floating_global()`,
  `REASSERT_DEBOUNCE_MS` shared with `reassert_debounce_ok`. Wiring in
  `src/hypr_ipc.rs`: every `configreloaded` line runs masks + the restore
  (UI-thread hop — thread-local timers) before the 3s throttle, via the
  headless-testable `handle_configreloaded_line`.
- [ ] Cross-model verification.
- [ ] Live confirmation (keeper).