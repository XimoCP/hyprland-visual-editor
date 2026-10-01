# ODD — Palette defence must cover resume from suspend

Feature name: `palette-defence-covers-suspend-resume`
Branch: `hve2-visual-rewrite` (current; not the default branch)
Status: T1..T3 DONE (writer batch, evidence below); T4 (cross-model verification + the
keeper's live test) PENDING
Follows: `odd/tasks/palette-defence-on-monitor-events.md` (commit `7becc15`), whose accepted
limit "if resume emits no `monitoradded`, that session has no correction path" has now been
observed live and is the bug this change fixes.

## Objective

When the machine returns from suspend and the wallpaper engine re-applies a different palette for
the same image, HVE must correct it back to the applied theme's snapshot — without weakening the
rule that a legitimate manual palette change is left alone.

## Problem (observed live, 2026-10-01 18:29, evidence on disk)

Chain, all verified:

1. Suspend ends at **18:28:55** (`journalctl`: `systemd-logind: Operation 'suspend' finished`).
2. At **18:29:13**, ~18 s later, skwd re-applies the theme and regenerates a DIFFERENT palette for
   the same image, setting the live Noctalia scheme to `custom skwd-wall`.
3. HVE's watcher detects it and asks for a re-assert twice, at **18:29:13** and **18:29:24**
   (`~/.cache/hve/color_watcher.log`).
4. Both requests were answered **`noop`** (refused): permission requires a
   `monitoradded`/`monitorremoved` line inside a 5 s window (`src/hypr_ipc.rs:141-149`, `:238`,
   `:254-256`, `:269-284`; sole consumer `src/ipc.rs:475`), and **resume emits no such event**.
5. Proof of the final state: `~/.config/hve/config.json` has `last_applied_theme: "Animation"` and
   that theme's snapshot is the ONLY one holding `mPrimary #67abe4`
   (`~/.config/hve/themes/Animation/providers/noctalia-v5/palette.json`), while the live desktop has
   `e4aa67` — a colour present in **no** theme snapshot — and `noctalia msg color-scheme-get` answers
   `custom skwd-wall`.

Supporting facts that shape the fix:

- **The colour logic is already correct**: `NoctaliaV5Provider::reassert_colours`
  (`src/providers/noctalia.rs:1740-1802`) copies the saved snapshot deterministically (writes the live
  palette from it at `:1794`, then `color-scheme-set custom <name>` `:1796`, `templates-apply` `:1798`).
  It never re-derives from the wallpaper. Nothing in the colour pipeline needs to change.
- **The preconditions hold**: `source.txt` for `Animation` is `custom <palette_name>` (so the custom
  branch runs; the non-`custom` early return at `:1756-1758` does not apply) and there is no
  `~/.cache/hve/skwd-policy-yield.json` hold marker, so `:1767-1772` does not short-circuit either.
- **The refusal is invisible**: no `tracing` exists on that path, `hve-ipc`'s reply is discarded
  (`assets/scripts/color_watcher.sh:161-174`, stderr-only redirection), and
  `~/.cache/hve/assert-color-authority.last` is written **before** the call and shared with the repair
  verb (`color_watcher.sh:159-160`) — so it proves "asked", never "ran".
- **A 5 s window from the resume would have missed the burst anyway**: resume at `:55`, first skwd write
  at `:13` (18 s later), second burst pass at `:24`. And the permit is one-shot, so even an armed window
  covers only the first pass.

## Decisions

- **D1 — Resume is a second permission source, detected by HVE itself.** No new dependency: no D-Bus
  crate is present and adding one for this is disproportionate. HVE compares wall-clock advance against
  monotonic advance between two samples; a suspend shows up as wall ≫ monotonic. The check is evaluated
  **on demand** when a re-assert is requested, so no polling loop or timer is needed. An NTP/time jump
  can cause one false grant; that is accepted (bounded, one grace window) and documented.
- **D2 — The resume permission is a GRACE WINDOW, not a one-shot.** Detecting a suspend opens a
  `RESUME_GRACE` (60 s) during which re-asserts are permitted. Justification: the measured chain needs
  more than one pass (18 s to the first hijack, plus a second burst pass 11 s later), and the original
  one-shot semantic was measured to fail exactly this way. The grace cannot loop: the provider only
  writes when live differs from the snapshot (`palette_needs_reassert`,
  `src/providers/noctalia.rs:1016-1033`), and the watcher throttles requests to one per 5 s
  (`HVE_THEME_ASSERT_COOLDOWN`, `color_watcher.sh:150-157`).
- **D3 — The monitor path is untouched.** `MONITOR_PERMIT` keeps its one-shot 5 s semantics and its
  tests. The new source is added alongside it; both funnel through one entry point that returns which
  source granted, so the decision is loggable.
- **D4 — Log the decision.** Arming/granting/refusing now emits `tracing` (with the source and the
  reason). The 18:29 diagnosis was possible only by inference; a refusal must be visible next time.
- **D5 — The watcher is NOT changed.** It was proposed to skip its declared refreshes when the assert is
  refused, but `_reassert_theme_authority` runs BEFORE `_run_declared_refreshes`
  (`color_watcher.sh:264-268`), so once the permission works the correction lands first and the refresh
  renders the corrected palette. Changing the skip rule would also flip the meaning of a legitimate
  manual edit, which the design deliberately respects. Recorded as a follow-up, out of scope.

## Scope

**In:** the second permission source (resume), its grace window, the single entry point used by the
assert path, the logging of the decision, and tests for all of it.

**Out:** the colour provider (already correct), the monitor one-shot semantics, the watcher script, the
engine, anything in skwd, monitors/HDR layout (hyprmod's domain).

## Tasks

- [x] **T1 — Tests first (RED).** Headless, no Slint: **12 new tests** (11 in `src/hypr_ipc.rs`, 1
      logging contract in `src/ipc.rs`), all driven by synthesized clock samples — nothing sleeps.
      (a) `a_suspend_is_detected_when_wall_advances_far_more_than_monotonic`,
      `normal_drift_and_a_small_skew_are_not_a_suspend`,
      `the_suspend_tolerance_is_two_seconds`;
      (b) `a_detected_suspend_permits_the_reassert_and_the_second_burst_pass` (baseline grants
      nothing → 18 s hijack granted with source=resume → the 11 s second pass granted by the grace);
      (c) `with_no_monitor_event_and_no_suspend_the_reassert_is_refused` (and again 10 min later);
      (d) `a_request_far_beyond_the_grace_is_refused`, `the_grace_boundary_is_exclusive`;
      (e) the 5 existing monitor tests are untouched and still green, plus
      `a_monitor_event_grants_through_the_same_entry_point_and_is_reported` and
      `a_spent_monitor_window_does_not_block_the_resume_grace`.
      RED proven: `cargo test` failed to compile with 52 errors, every one of them "cannot find"
      (`suspend_gap`, `ResumeState`, `RESUME_GRACE`, `ReassertPermit`, `SystemTime`, …). Every new
      assertion was then falsified with a throwaway probe that was reverted (`sha256sum` verified the
      file back to its pre-probe state): never detecting the gap failed 5 tests; a grace that never
      expires failed 2; a grace consumed like a one-shot failed the second-pass test; a monitor permit
      that never spends failed 5; reporting the wrong source failed 2; and un-logging the gate failed
      the `src/ipc.rs` contract test.
- [x] **T2 — Implement the resume source.** `SUSPEND_GAP_TOLERANCE = 2 s`, `RESUME_GRACE = 60 s`,
      `ReassertPermit { Monitor, Resume, None }`. One entry point, `consume_reassert_permit()`, used by
      `src/ipc.rs:475`; the monitor branch runs first so its one-shot 5 s semantics and its tests are
      literally unchanged. Pure decision functions over plain values (`suspend_gap`, `resume_grace_open`,
      `ResumeState::observe`, `decide_reassert`), with the clock reads (`clock_sample`) only at the
      production edges, so no test needs to sleep.
- [x] **T3 — Log the decision.** Three cheap `tracing::info!` lines, one per decision at most: the
      resume detection (`[colour-defence] resume from suspend detected: re-asserts permitted for 60s`),
      the grant with its source (`re-assert GRANTED (source=monitor|resume)`), and the refusal with its
      reason (`re-assert REFUSED (no monitor event, no resume grace): …`). Pinned by
      `the_reassert_gate_logs_its_decision_with_source_and_reason`.
- [ ] **T4 — Cross-model verification** (independent model, not the writer's) and the keeper's live test:
      suspend the machine, resume, confirm HVE corrects the palette back to the theme's snapshot within
      the grace, and confirm a manual palette change made well after the resume is still left alone.

## Acceptance criteria

- After suspend/resume, a skwd palette hijack is corrected back to the applied theme's snapshot, even
  when the hijack lands ~20 s after the resume and arrives in more than one pass.
- A palette change made with no monitor event and no recent suspend is still never touched.
- No permanent permission: the grace expires and later changes are refused again.
- No ping-pong/loop: a repeat request when the live palette already equals the snapshot performs no
  write.
- The monitor-path behaviour and its tests are unchanged. The watcher script is untouched.
- A refusal is visible in the log with its reason.

## Checks (resolved)

- TDD: **strict** (project rule + hve2 skill). Failing test first. Runner: `cargo test`.
- Evidence: full `cargo test` (expected all green, report counts), `cargo build` warning-free.
- Never `cargo fmt`; never `git checkout -- .`/`reset`/`stash`/`clean`; no branch switch.
- No `.slint` expected; if any is touched, the headless render run plus reading the PNGs is mandatory.
- Commit as one reviewable work unit, Conventional Commit, no AI attribution.

## Delivery

- Forecast: ~120-220 authored lines (Rust + tests) — one PR, default `ask-on-risk`, under the review
  budget. Route: T1-T3 to one bounded writer; T4 to an independent delegated verification on a
  different model.

## Progress / evidence

Writer batch (T1-T3), 2026-10-01, branch `hve2-visual-rewrite`. Files: `src/hypr_ipc.rs` (the whole
resume permission), `src/ipc.rs` (the gate: one entry point + the decision log + its contract test).

- **Design as built.** `ClockSample { mono: Instant, wall: SystemTime }`; a step is a suspend when
  `wall_advanced.saturating_sub(mono_advanced) > 2 s` (a backwards wall step, i.e. an NTP step, is not).
  `ResumeState { last, until }`: `last` is the baseline (first sample only establishes it — it can
  never detect), `until` is the open grace, and a NON-suspend sample never closes it. `decide_reassert`
  observes the sample, then tries the monitor one-shot (consumed exactly as before) and only then the
  grace. `consume_reassert_permit()` is the single production entry point.
- **Sampling points (a decision this document did not specify).** Samples are taken when the listener
  connects, on every socket2 event line, on the listener's OWN existing 30 s read timeout, and on
  demand at the request. The last three mean the baseline is never more than 30 s of RUNNING time
  stale — either a line arrived or the read timed out — which is what keeps a legitimate edit made
  long after a resume from looking like the hijack that followed one. No new timer, no new thread, no
  poll: the reads reuse the wake-ups the listener already had. The DECISION itself is only ever taken
  on demand, as required.
- **T1/T2/T3 evidence.** RED: 52 compile errors. GREEN: `cargo test` → **`ok. 1221 passed; 0 failed;
  0 ignored`** (baseline before this change: 1209 passed, so **+12 = the 12 new tests**), **8 consecutive
  full-suite runs green** afterwards (every one captured as `1221 passed / 0 failed`), plus 25 stress
  runs of the two touched modules (46 tests each, 0 failures). `cargo build` → `Finished dev profile`,
  **warning-free** (`grep -c warning` = 0, both on the full build and on the final revision).
  Throwaway probes falsified every new assertion (see T1). No test sleeps; the only clock reads in
  tests are synthesized values.
- **Honest note.** One full-suite run during the writing session reported `1220 passed; 1 failed`. Its
  name was lost to an output filter and it did **not** reproduce in 4 later full runs nor in 25 stress
  runs of `hypr_ipc::tests::` + `ipc::tests::` (46 tests). Reported rather than hidden; the failing
  test is unidentified.
- **Not changed:** the colour provider, the watcher script, `MONITOR_PERMIT`/`defence_permits_reassert`
  and their tests, the engine, skwd, monitors/HDR. No `.slint` file was touched, so the headless render
  check does not apply.
- **Commit:** `750afc0a1c207a719a0eb31af66230441757177b` — 3 files, +742/−13 (of which this document is
  +201). It carries the code, the 12 tests and this document. Measured Rust-authored lines: **239
  production + 302 tests + 15 deletions = 556** — above the ~400 advisory, see the note below.
- **Why the advisory overage (556 authored Rust lines vs ~400):** the design the task prescribed needs
  four pure pieces (sample, gap test, grace decision, one entry point) each with the house's mandatory
  doc comment, and T1 asks for five distinct sub-cases whose boundary behaviour ("the first sample
  grants nothing", "the second pass is still inside the grace", "there is no permanent permission") is
  only provable with separate tests. Nothing was trimmed to fit: no comment, test or assertion was
  removed, and `cargo fmt` was never run.

## Follow-ups (not this change)

- The watcher's unconditional declared refreshes re-render whatever palette is live; if a future hijack
  is ever refused, that refresh bakes it in. Revisit only if a hijack path without any observable event
  is found (`assets/scripts/color_watcher.sh:264-268`,
  `assets/scripts/color_sources.d/noctalia_lua.sh:106-111`).
- The watcher cannot tell "asked" from "ran": make `hve-ipc`'s reply usable (or stamp on success
  instead), so `assert-color-authority.last` means something.
- Monitors/HDR/display layout stays hyprmod's job.
- **A wall-clock STEP larger than 2 s (a coarse NTP correction, or SIGSTOP/manual `date`) opens one
  false grace window**, exactly as D1 accepted. The sampling bound above keeps the reverse case (a
  legitimate edit long after a resume) out of the grace, but it cannot tell a step from a sleep: both
  look like wall ≫ monotonic. If a false revert is ever observed, the fix is a narrower source of
  truth (e.g. reading a boot-time-based clock) and not a bigger tolerance.

## Relevant files

- `src/hypr_ipc.rs` — permit window and its constants (`:220-284`), listener branches (`:108-149`).
- `src/ipc.rs` — the gate call (`:475`) and the assert/repair entry points (`:474-504`).
- `src/providers/noctalia.rs` — `reassert_colours` (`:1740-1802`), `palette_needs_reassert` (`:1016-1033`).
- `assets/scripts/color_watcher.sh` — the detector and the ask (unchanged).
- `~/.cache/hve/color_watcher.log`, `~/.cache/hve/assert-color-authority.last`,
  `~/.cache/hve/color-authority.json` — runtime evidence from the failure.
