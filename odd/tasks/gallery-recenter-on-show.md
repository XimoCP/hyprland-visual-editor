# Gallery recenters on the active theme every time the window comes back from hidden

## Objective

Every time the window transitions from hidden to shown (SUPER + H toggle), the
gallery slider must recenter on the theme that is currently applied, no matter
where the keeper left the slider before hiding.

## Problem

`Controller::show_window` (`src/composer/mod.rs`, l. 357) restores the window and
drives the show state machine, but never touches the gallery. The window is the
same Slint window across hide/show, so `gallery-focused` and the slice ring keep
the position the keeper left them at. Startup does recenter
(`src/main.rs` l. 3731: `set_gallery_focused(gallery_initial_focus(..))` +
`refresh_slice_ring()`); the return-from-hidden path does not. The same symptom
was already reported once for restarts (see the backfill note at `src/main.rs`
l. 707-713, "always the same theme centered").

## Why

The keeper hides the window, moves the slider, hides again, and expects the
carousel to open on what is actually applied. Today it opens wherever the last
interaction left it, which reads as the app forgetting the active theme.

## Scope

In scope:

- Recenter the gallery slider on the active theme on every hidden -> shown
  transition.
- Keep today's fallback when no theme is active or the list is empty: first card
  (`gallery_initial_focus` already returns 0 there).

Out of scope:

- The Mosaic presentation's cursor ring: unchanged.
- Startup behaviour: already recenters; must not change.
- A plain raise of an already-visible window (`show_window` on a visible window):
  unchanged, must NOT recenter.
- Any engine file. `engine.rs`, `config.rs`, `settings.rs`, `theme_manager.rs`,
  `app_state.rs`, `watcher.rs`, `utils.rs` and `providers/` are sealed.

## Constraints

- Strict TDD: the failing test lands first, then the implementation.
- Test runner: `cargo test`.
- Engineered artifacts in English.
- Visual proof is mandatory for this change: the headless render, read back as an
  image, is the evidence. "Tests green + build clean" is not verification.
- The show state machine must keep its current semantics: the recenter runs once
  per real hidden -> shown transition, never on a re-entrant show.

## Design

Seam chosen: the controller owns the show funnel, so the recenter hangs off it.

1. `Controller` gains one optional hook slot plus a setter. The hook is
   `Arc<dyn Fn() + Send + Sync>`; no window argument is needed.
2. `show_window` invokes the hook only on the real hidden -> shown branch, after
   the hidden flag clears. The already-visible early return and the
   "show already in flight" branch never invoke it.
3. `main` registers the hook right after `composer::init_global(controller)`
   (`src/main.rs` l. 5407-5408); `refresh_slice_ring` is in the same `main`
   scope (created at l. 3145). The hook body snaps `gallery-focused` to
   `gallery_initial_focus(&win)` and then runs `refresh_slice_ring()` — the
   exact pair the startup path uses, so the ring settles there with zero drift.
   The Mosaic cursor is not touched.

## Checklist

- [x] T1 (RED) `src/composer/mod.rs` tests: a hidden window's show runs the
      registered hook exactly once; an already-visible window's `show_window`
      does not run it; a show while Entering does not re-run it. Must fail first.
      RED seen: `error[E0599] no method named set_show_hook` (API absent), then
      `left: 0, right: 1` on the once-per-transition assertion.
- [x] T2 `Controller` hook slot + setter; invoke on the hidden -> shown branch
      only. T1 turns green (`cargo test show_hook`: 3 passed).
- [x] T3 `main` registers the hook via `recenter_gallery_on_active` (snap focus
      to `gallery_initial_focus` + `refresh_slice_ring`), right after
      `composer::init_global`. Mosaic, startup and plain raises unchanged.
- [x] T4 (RED -> GREEN, visual) render test extended with the hide/show cycle.
      RED with the invocation disabled: `left: 2, right: 9`. GREEN: settled ring
      centered on the active card.
- [x] T5 Full `cargo test` and `cargo build` clean; exact commands and observed
      results recorded below.
- [x] T6 Cross-model review findings closed: the hide branch is now pinned, and
      the render test's comment states what is production code and what is a
      hand-copied replica.

## Verification

- `cargo test` (full suite).
- `cargo build`.
- Headless render:
  `HVE_RENDER_DIR=/tmp/opencode/render-verify-gallery-recenter cargo test slice_focus_flow_renders -- --nocapture`,
  then read the PNGs from that per-run directory and confirm the active card is
  the centered one after a hide/show cycle.
- Cross-model check of the diff and the test contract (writer and verifier are
  different models).

## Evidence

- Commits (branch `hve2-visual-rewrite`, not pushed):
  - `a3fdcac` `feat(gallery): the carousel reopens on the active theme after SUPER+H`
    — `src/composer/mod.rs` (+111), `src/main.rs` (+34), `src/shell/ui_tests.rs` (+114).
  - `57063e4` `test(gallery): pin the hide branch and the show-hook registration`
    — test code and comments only, no behaviour change.
- Commands, with observed results:
  - `cargo test show_hook`: 5 passed, 0 failed.
  - `cargo test`: 1524 passed, 0 failed (143.63s). One earlier pass reported
    `reload_coalescer::sandboxed_disable_switch_keeps_immediate_reload` failing
    under load; it passes isolated and the module passes twice, so it is not a
    regression of this change.
  - `cargo build`: clean, no warnings.
  - `HVE_RENDER_DIR=/tmp/opencode/render-orch-gallery-recenter cargo test
    slice_focus_flow_renders -- --nocapture`: 1 passed; the run printed its own
    directory. Re-run by the orchestrator on the committed bytes.
- Visual evidence, read back as images from the per-run directory:
  `slice_stale_before_show.png` shows Theme 2 centered (where the slider was
  left); `slice_recentered_on_active_after_show.png` shows Theme 9, the active
  theme, centered with Theme 8 / Theme 10 as neighbours. Geometry is unchanged
  from the pre-change frames.
- Cross-model review (`glm-5.3-flash`, a different model from the writer):
  `verified_with_findings`, no functional defect. It enumerated every
  show path and confirmed all of them funnel through `show_window` except the
  intentional startup `window.show()`; traced the hook's synchronous reach and
  found no path back into the controller mutex (no deadlock); and mutation-tested
  the three controller tests against a hook moved to the raise path, a hook that
  never fires, and a hook in the re-entrant branch.
- Findings closed by `57063e4`: the `toggle_tray` hide branch had no test
  pinning the hook idle (now it does, with its own RED against a deliberately
  broken copy); the render test's comment overstated what it pins (now it names
  the real seam and the hand-copied replica); and dropping the hook registration
  in `main()` would have left the suite green (now pinned by a source-contract
  assertion, the existing `ui_tests.rs` idiom).
- Review tiers used: Tier 3 for the change (state machine plus visible
  behaviour) and Tier 1 for the follow-up commit (test code and comments only,
  orchestrator readback plus a re-run of the pinned tests).

## Next step

Done. The keeper's own live check is the last step: SUPER + H, move the slider,
SUPER + H again — the carousel must reopen on the applied theme.
