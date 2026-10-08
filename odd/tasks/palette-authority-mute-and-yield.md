# ODD — The theme owns the palette: mute the engine, step aside when someone else changes the background

Feature name: `palette-authority-mute-and-yield`
Branch: `hve2-visual-rewrite`
Status: IN PROGRESS — W1 done (see Progress / evidence); W2 done (see Progress / evidence); W3 done (see Progress / evidence); W4 (cross-model verification + the keeper's live test) pending by definition.
Supersedes: `odd/tasks/palette-defence-covers-suspend-resume.md` (the temporal permission: monitor one-shot + 60 s resume
grace) and the background-identity study it was heading towards. Both are demoted to safety nets.

## Objective

While an HVE theme owns the colours, nothing else may take the palette away: the wallpaper engine
(`skwd-walld` / `skwd-wall-v2`) keeps painting and rotating whatever it wants, but publishes no palette.
When the keeper changes the background from the engine's own program, that means the keeper wants the
background and not the theme: HVE steps aside and touches nothing until a theme is applied again.

## Problem (measured, 07 and 08-oct-2026)

The engine republishes its own palette on EVERY apply it performs, so it steals Noctalia's custom
palette (`custom JokerTheme`) on any background apply - internal or requested. The keeper reproduced it
by hand within minutes:

```
02:38:33Z  apply Cars/car7.jpg        -> theme apply: backend=noctalia + "noctalia palette bridge: published"  (steal)
02:38:56Z  apply itachi-uchiha video  -> published again                                                      (steal)
02:39:03Z  apply Cars/car9.jpg        -> published again                                                      (steal)
02:39:55Z  apply themes/JoKer/media/poster.png -> theme apply: backend=off and NO publish                     (HVE's mute)
```

The 07-oct wake theft is the same mechanism through another door: the engine reads the monitors coming
back as a hotplug (`output set changed (hotplug), reconciling per-output wallpapers`), re-applies its
remembered per-output background (it even re-applied `joker1.png`, the theme's `wallpaper.txt`, before
correcting to the poster) and publishes three times in 24 s.

So the thief is the engine's PUBLISHING rule (`theme.policy = "wallpaper"` = derive the palette from
whatever I paint), not the background. And HVE already holds the switch: the same knob at `off` makes
the engine paint without publishing, which is what it does today - but only for the duration of an apply.

## Decisions (keeper's rule, confirmed 08-oct-2026)

- **D1 — The theme owns the palette while it is applied.** The engine stays mute: it paints, reconciles
  and rotates; it never publishes. Nothing can steal the colours: not the wake, not a monitor hotplug,
  not a rotation.
- **D2 — Whoever changes the background wins.** If the keeper changes the background (from the engine's
  own program, or through any route HVE did not ask for), that means the keeper does not want the theme's
  colours now: HVE steps aside - it stops defending the palette and touches nothing until a theme is
  applied again. Applying a theme takes the rule back: re-claim the colours and mute the engine again.
- **D3 — Rotation stops being a problem.** With the engine mute, the palette does not depend on the
  background, so a theme may rotate several backgrounds while the colours hold. A theme-driven rotation
  is requested from HVE, so it is not "someone changed the background".
- **D4 — None of this lives in the engine.** The engine is not modified and never learns a rule exists.
  Everything lives in HVE's adapters that already talk to skwd: the `theme.policy` mute, HVE's "I am
  acting" marker, the authority descriptor and the engine state read.
- **D5 — The 07-oct machinery becomes a safety net, not the mechanism.** The ask path
  (`color_watcher.sh` → `assert-color-authority`) keeps working, but its temporal permits
  (`MONITOR_PERMIT`, the 60 s resume grace) and the silent 5 s cooldown stop being what protects the
  palette. Review in W2: keep, tame or retire. The 5 s cooldown's silent drop and the stamp written
  before the helper check (`assets/scripts/color_watcher.sh:151-161`) are known defects either way.

## Scope

**In:** the mute lifetime (W1), the step-aside exception and its detection (W2), observability (W3),
verification (W4). HVE's adapter/provider layer only.

**Out:** the engine's own code, its docs and its upstream behaviour; Noctalia's own settings beyond what
HVE already writes; monitors/HDR (hyprmod); any `.slint` change unless W2's panel state requires one.

## Tasks

- [x] **W1 — Hold the mute while a theme rules.**
      Today `skwd_policy` flips `theme.policy` to `off` around an apply and restores it after. Extend the
      hold: while a valid colour-authority descriptor exists (a theme owns the palette) the engine stays
      `off`; when no theme owns it, the engine's own value is restored. Keep the existing crash safety
      (marker `~/.cache/hve/skwd-policy-yield.json` + the startup repair sweep) and the byte-preserving
      single-token edit. Tests: flip vs no-flip cases, marker lifecycle, restore on release, the existing
      `skwd_policy` suite untouched and green.
- [x] **W2 — Step aside when the background change was not HVE's.**
      Detection decision to settle first (see Open questions): the engine's own program showing up in
      Hyprland's event stream (HVE's IPC listener already reads that socket, so presence is free) and/or
      the engine's state read (`outputs --json` / `current`), which HVE's adapter already performs.
      On a background change HVE did not ask for: release the claim (the descriptor is cleared, so the
      theme stops owning the colours and the watcher stops asking) and stay aside until a theme apply
      re-claims and re-mutes. Tests: pure decision function over (program open/closed, background changed
      or not, HVE acting or not), stub binary for the engine read, and the release/re-claim lifecycle.
- [x] **W3 — Make every decision visible.**
      The guard's own `tracing` goes to `/dev/null` in production (the app is started by the session/tray),
      and the watcher's cooldown drop leaves no line at all - which is why the 07-oct failure took three
      rounds of questions to read. Add a decision log under HVE's cache dir (mute held/released, step-aside,
      re-claim, engine absent) and log the watcher's suppression instead of dropping it silently; fix the
      stamp being written before the helper check.
- [ ] **W4 — Cross-model verification and the keeper's live test.** Verified by a model distinct from the
      writer (HVE rule: writing and verification never share a model), then live: (1) with a theme applied,
      change the background from the engine's program -> HVE does not fight and stays aside; (2) apply the
      theme again -> the theme's palette returns and the engine is mute again; (3) suspend/resume with a
      theme applied -> the palette does not move; (4) change the background outside HVE -> HVE stays out;
      (5) the engine absent/renamed -> HVE degrades honestly and does not guess.
      Status (08-oct): cross-model verification done for W1 and W2 (each found real defects, all fixed);
      W3 is orchestrator-verified (Tier 2, observability only). Keeper's live verdict: the general behaviour
      passes and he likes it; **only case (3), suspend/resume, is still pending**.

## Acceptance criteria

- With a theme applied, no engine publish can change the live palette - including after a wake, a
  monitor hotplug and a background rotation.
- A background change HVE did not ask for wins: the palette stays as the new background set it, and HVE
  touches nothing until a theme is applied again.
- Applying a theme re-claims the colours and mutes the engine again.
- The engine is never modified; all of it lives in HVE's adapters.
- Every state change (mute held, aside, re-claimed) is visible in a log the keeper can read.
- No loop: HVE acting -> engine muted -> nothing to correct.

## Checks (resolved)

- TDD: **strict** (project rule + the hve2 skill). Failing test first. Runner: `cargo test`.
- Evidence: full `cargo test` (report counts), `cargo build` warning-free.
- Never `cargo fmt`; never `git checkout -- .` / `reset` / `stash` / `clean`; no branch switch.
- No `.slint` expected. If W2 needs a visible panel state, the headless render run plus reading the PNGs
  is mandatory (`HVE_RENDER_DIR=/tmp/opencode/render-verify-<unique> cargo test slice_focus_flow_renders`).
- Commit as reviewable work units, Conventional Commits, no AI attribution.

## Delivery

- Forecast: ~200-350 authored lines (Rust + tests), one PR, default `ask-on-risk`, under the review
  budget. Route: W1-W3 to one bounded writer per unit; W4 to an independent delegated verification on a
  different model, then the keeper's live test.
- Architecture check: `src/watcher.rs` tests pin that bash never rewrites backend files nor calls
  `color-scheme-set`; this change must not break that contract.

## Open questions (settle before the unit that needs them)

1. **How HVE notices "the background change was not mine" (W2).** Candidates: (a) the engine's window
   appearing in Hyprland's event stream (free, already listened to) as the gate, plus the engine state
   read to tell whether a background actually changed; (b) the engine state read alone, sampled when
   something happens; (c) HVE's own action marker as the ledger. Cost, lag and honesty differ; (b) alone
   cannot tell a keeper change from an engine reconcile, which is why (a) is the working favourite.
2. **The mute's lifetime limits (W1).** The mute lives in the engine's config, so it must not survive
   HVE's disappearance: the marker plus the startup repair cover a crash or a restart; an uninstall with
   HVE not running cannot be repaired by HVE. Document the limit; do not pretend it is covered.
3. **`theme.authority` (undecoded).** The engine's config carries `theme.authority = "noctalia"` and
   per-background profiles (`theme.wallpaperProfiles`, key shape `static:Cars/car7.jpg`, whose settings
   carry their own `theme.policy`). One pass over the engine's own docs/config UI before W1: if a finer
   switch already exists (e.g. "publish only on explicit apply", or a per-background pin), W2 may become
   a setting instead of a mechanism.
4. **What happens to the ask path (W2/D5).** With the mute held, the watcher's asserts should rarely
   fire. Decide whether the temporal permits are kept as a net or retired once the mute proves itself in
   the live test.

## Relevant files

- `src/providers/skwd_policy.rs` - the `theme.policy` flip, marker, startup repair, byte-preserving edit.
- `src/providers/background.rs` - the apply-background router that owns hold/release of colour authority.
- `src/providers/noctalia.rs` - `apply` vs `reassert_colours`, `palette_needs_reassert`,
  `neutralize_noctalia_wallpaper_rotation` (commit `f8a1180`), the live palette write.
- `src/color_authority.rs` - the descriptor: presence means a theme owns the colours; `clear_descriptor`.
- `src/hypr_ipc.rs` - the Hyprland socket listener (the free presence signal) and today's temporal permits.
- `src/ipc.rs` - `assert-color-authority` / `repair-color-authority` and the guard's decision log.
- `assets/scripts/color_watcher.sh` - the detector, the ask, the 5 s cooldown, the stamp order.
- `~/.config/skwd-wall-v2/config.json` - the engine's knobs (`theme.policy`, `theme.authority`,
  `theme.engine`, `theme.wallpaperProfiles`); its log `~/.cache/skwd-wall-v2/skwd-walld.log`.
- `~/.local/state/noctalia/settings.toml` - the live scheme and `[wallpaper]` pins, `[idle]` timers.

## Progress / evidence

### W1 — done (commit `aac5988`)

Implemented the extended mute lifetime in `src/providers/skwd_policy.rs`:

- `restore_color_authority` consults the colour-authority descriptor before putting the engine's value
  back. With a VALID descriptor it leaves `theme.policy` at `off`, writes NOTHING (the document stays
  byte-identical) and KEEPS the crash marker — which records the previous value a later genuine release
  needs AND is the startup-repair net. With no valid descriptor it restores the engine's own value and
  clears the marker, exactly as before.
- `yield_color_authority` learned the "already held" case: when the config already reads `off` and our own
  pending marker records the value to put back, it hands that value to the caller's release WITHOUT
  flipping or rewriting anything. This closes the lifecycle so a later apply that claims no palette
  (descriptor cleared) restores the engine instead of leaving the mute stuck on. A config that is off with
  no marker of ours still reports "nothing to hold", as before.
- No engine code touched. The byte-preserving single-token edit, the marker + startup sweep, and the
  honest degradation (missing or unparsable config → no-op) are intact.

Files touched:

- `src/providers/skwd_policy.rs` — production logic plus 7 new tests.
- `src/providers/noctalia.rs` — 4 existing apply tests updated to the new mute lifetime (they pinned the old
  "restore after apply" behaviour) plus 1 new end-to-end lifecycle test. Test-only: no production code
  changed in this provider.

Tests (all in temporary sandboxes — the keeper's live config is never read, written or probed):

- `release_keeps_the_mute_while_a_theme_owns_the_palette` (a)
- `release_restores_when_no_theme_owns_the_palette` (b)
- `held_mute_still_recovers_on_the_next_start_so_a_crash_never_sticks` (c)
- `missing_or_unreadable_engine_config_never_invents_a_hold` (d)
- `held_mute_touches_no_byte_beyond_the_single_value_token` (e)
- `an_armed_guard_drop_holds_the_mute_when_a_theme_owns_the_palette`
- `a_second_apply_while_held_restores_when_the_owner_lets_go`
- `v5_apply_non_custom_theme_restores_an_already_held_engine` (noctalia)

Evidence (commands run from `/home/ximo/Proyectos/hve`):

- Baseline `cargo test`: `1457 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`.
- After: `cargo test`: `1465 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` — see the
  correction below: this run was green but the suite failed intermittently; the honest post-fix
  figure is `1466 passed; 0 failed`.
- `cargo build`: clean, no warnings.
- Non-vacuous checks (throwaway breaks, reverted; `sha256sum` confirmed the files returned to their
  pre-break bytes `b35e9772…` / `a678e813…`):
  - disabling the descriptor check in `restore_color_authority` makes
    `release_keeps_the_mute_while_a_theme_owns_the_palette` fail.
  - making the already-held `yield_color_authority` return `Ok(None)` makes
    `a_second_apply_while_held_restores_when_the_owner_lets_go` fail.

Limits (honest, not covered by W1):

- A descriptor CLEARED outside an apply (the W2 step-aside) still needs W2 to invoke the release; W1
  provides the primitive (`restore_color_authority` restores when no descriptor exists) but does not itself
  detect the step-aside event.
- The mute does not survive HVE's disappearance: the startup sweep replays the marker and un-mutes the
  engine (the deliberate crash-safety trade documented in the module). An uninstall with HVE not running
  cannot be repaired by HVE.

### W1 — correction: the full suite was not green (fix commit on this branch)

The writer reported `1465 passed; 0 failed`, but on this machine the full suite failed intermittently, and
the failure is W1's, not pre-existing:

- `providers::noctalia::tests::v5_apply_no_wallpaper_assets_never_yields_even_when_owner` —
  `assert_eq!(stub.sets(), 4)` saw `left: 5` ("gave up at the cap: no war").
- `providers::noctalia::tests::v5_apply_static_yields_before_the_static_hand_off` — the engine config did
  not read `"policy": "off"` after the 500 ms settle.

Root cause (demonstrated, not guessed): W1 replaced the three production-timing apply tests' adaptive
wait — a deadline loop that stayed alive until the re-assert worker's terminal restore — with
`poll("color-scheme-set", 2)` plus a fixed 1500 ms sleep. The re-assert worker is DETACHED and, under the
production timing profile, lives ~4 s (settle 2 s + delay 2 s + the bounded cap), so the test now returned
while the worker was still mid-delay. The worker reads PROCESS-GLOBAL state — `PATH` for the `noctalia`
stub, `SKWD_WALL_V2_CONFIG` / `HOME` for the engine config and the descriptor — so the next `#[serial]`
test installed its own sandbox and the leftover worker ran against it:

- its post-delay `color-scheme-get` hit the NEXT test's stub (scheme reset to `custom skwd-wall`), so it
  re-issued `color-scheme-set` into that test's log (the observed `left: 5`); each further test reset the
  scheme again, so the worker leaked up to its 3-set cap across two or three following tests;
- its `release_color_authority` wrote the previous value back into whatever config read `off` at that
  moment, which could clobber a victim's held mute (the `"policy": "off"` failure).

Evidence (thread-named `noctalia` calls with `date +%s.%N` timestamps): the worker
`reassert[...v5_apply_yields_before_handoff_and_holds_the_mute_while_the_theme_rules]` ran
`color-scheme-set custom JokerTheme` at `…000.07`, `…002.14` and `…004.22` after its own test had ended;
the failing `v5_apply_non_custom_palette_ignores_owner` ran at `…004.09`–`…004.14` and counted the
`…004.22` set. A deterministic subset reproduction (`cargo test -- --test-threads=24 v5_apply skwd_policy`)
failed 1 of 2 runs before the fix and 0 of 4 after.

Fix (test isolation; no assert weakened, no blind sleep added):

- `spawn_custom_scheme_reassert` keeps a live count of detached workers (`REASSERT_WORKERS_ACTIVE`,
  decremented by an RAII guard) and exposes `#[cfg(test)] reassert_workers_idle()`.
- `ColorStub::drop` waits — bounded to 20 s — for every worker to finish BEFORE restoring `PATH` /
  `SKWD_WALL_V2_CONFIG` / `HOME`, so the sandbox a worker depends on is still installed when it runs. The
  `#[serial]` guard is held for the whole test function and locals drop before it, so the next serial test
  cannot start until the worker is done.
- New regression test `a_detached_reassert_worker_never_outlives_its_stub_sandbox` pins the handshake
  (non-vacuous: with the wait disabled it fails deterministically right after the worker's first
  re-assert).

Evidence after the fix: `cargo test` twice — `1466 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`
(`145.16 s` / `144.88 s`); `cargo build` clean, no warnings. The 1466 count is W1's 1465 plus the new
regression test.

Status: unchanged for the feature (W1 done). The `1465 passed; 0 failed` line in the W1 evidence above was
a run that got lucky; this section is the corrected evidence.

### W1 — second correction: the two repairs the cross-model review found (same day)

The cross-model review of the fix above returned `PASS with caveats` with two real holes in the new code,
both closed in the same file:

- **A deadline that expires must be LOUD.** The bounded wait in `ColorStub::drop` used to fall through in
  silence: a worker surviving its 20 s deadline would keep reading the sandbox's process-global state and
  quietly pollute the next test — the exact failure the wait exists to prevent, made undetectable. It now
  prints and panics (guarded by `std::thread::panicking()`, so a test that was already failing is never
  turned into an abort).
- **A failed spawn must not leak the count.** `fetch_add` happened before `std::thread::spawn`, which
  PANICS on a spawn failure with the closure never running — leaving the count at +1 for the rest of the
  process, so every later sandbox would pay the full 20 s deadline. The guard is now built in the parent,
  moved into the worker, and the spawn goes through `Builder::spawn` (a `Result`): a failed spawn drops the
  closure in the parent, which decrements the count by itself; the failure is logged instead of swallowed.

Evidence after both repairs: `cargo test` — `1466 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`
(`144.74 s`), `cargo build` clean, no warnings, and the affected subset `cargo test v5_apply` →
`37 passed; 0 failed`. Verified by the orchestrator (parent spot check), not by the writer's word.

### W2 — done

Implements the step-aside half of the keeper's rule: while a theme owns the palette, a background the
applied theme does not declare is the keeper's own change; HVE releases the claim and stays aside until a
theme is applied again.

Detection source (settled): the engine's OWN state, `skwd-helm current --json` (with `skwd-wall-v2
current --json` as fallback), never its private log. With the mute on the engine publishes no palette, so
the old bridge-file detector is blind; its per-output `current` is the live background.

- `src/providers/wallpaper_authority.rs` — `query_live_backgrounds()`: reads the engine's `current --json`
  through the existing bounded runner and JSON parser, returns the CONNECTED outputs' paths, deduplicated,
  or `None` on any read failure (never an empty/foreign guess). This is the adapter that already performed
  the engine state read (`outputs --json`).
- `src/theme_media.rs` — `declared_backgrounds(theme_dir, provider_dir)`: the background paths the applied
  theme declares as its own, as absolute paths — the packaged poster, the video (packaged first, then the
  legacy absolute `video.txt` record) and the saved `wallpaper.txt`. Empty when nothing is declared.
- `src/providers/background.rs` — the pure decision `decide_palette_authority(live, declared, hve_acting,
  descriptor_present)` plus the impure `step_aside_if_foreign()`:
  - `Hold` unless the descriptor exists, HVE is not acting, the engine was read, the live set is non-empty
    and at least one live path is outside the declared set. Unreadable engine / empty live / no declared
    set / no descriptor / HVE acting all `Hold` (conservative).
  - `StepAside`: clear the descriptor FIRST, then release the mute through `release_color_authority` with
    the pending marker's value (order matters: with the descriptor still present the release would keep the
    mute). No re-mute, no palette re-assert.
  - `hve_is_acting()`: a process-global flag set while a `ColourAuthorityHold` is alive (i.e. the apply's
    hand-off phase) and cleared when its guard drops. This makes the re-apply window safe: a new apply
    holds the mute with the OLD descriptor still on disk, and the flag keeps the detector quiet.
  - `start_foreign_change_watch()`: a process-lifetime poll (5 s) that only samples the engine while a
    descriptor exists (an idle HVE never runs the engine CLI). Idempotent; a failed spawn is logged and
    leaves the watch unstarted.
- `src/providers/skwd_policy.rs` — `held_previous_value()`: the pending marker's `previous_value`, the
  value a step-aside must restore. W1's flip/restore/marker logic untouched.
- `src/providers/mod.rs` — re-export `start_foreign_change_watch` through the registration router;
  `src/main.rs` starts it next to the crash repair. `noctalia.rs` was NOT touched (W1 and its corrections
  stand: the live-worker counter, the loud `ColorStub::drop` deadline and `Builder::spawn` are unchanged).

Files touched:

- `src/providers/background.rs` — pure decision + step-aside shell + acting flag + watch, and 10 tests.
- `src/providers/wallpaper_authority.rs` — `query_live_backgrounds` + 2 tests.
- `src/theme_media.rs` — `declared_backgrounds` + 3 tests.
- `src/providers/skwd_policy.rs` — `held_previous_value` accessor (no behaviour change).
- `src/providers/mod.rs`, `src/main.rs` — start the watch (composition root).

Tests (all in temporary sandboxes — the keeper's live engine, config and descriptor are never read,
written or probed; engine reads use stub binaries on PATH):

- `a_foreign_live_background_steps_aside_and_a_declared_one_holds` (pure table)
- `an_unknown_background_is_never_evidence_of_a_foreign_change` (pure: `None` / empty / undeclared)
- `hve_acting_and_no_descriptor_both_hold` (pure)
- `a_foreign_background_clears_the_descriptor_and_releases_the_mute` (lifecycle)
- `a_declared_live_background_keeps_the_claim_and_the_mute`
- `an_unreadable_engine_never_steps_aside`
- `hve_acting_never_steps_aside`
- `once_aside_further_background_changes_are_a_noop`
- `applying_a_theme_reclaims_and_remutes`
- `declared_backgrounds_are_resolved_from_the_descriptor`
- `live_backgrounds_reads_the_engine_current_json` / `live_backgrounds_is_none_when_the_engine_is_unavailable`
- `declared_backgrounds_collects_poster_video_and_wallpaper` / `..._is_empty_without_any_record` /
  `..._prefers_the_packaged_video`

Evidence (commands run from `/home/ximo/Proyectos/hve`):

- `cargo test`: `1481 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` (`145.28 s`) — W1's 1466
  plus 15 new tests.
- `cargo build`: clean, no warnings.
- Non-vacuous checks (throwaway breaks, reverted; `sha256sum` confirmed the files returned to their
  pre-break bytes `947203d4…` / `465d1a14…`):
  - forcing `step_aside_if_foreign` to always return `Held` makes
    `a_foreign_background_clears_the_descriptor_and_releases_the_mute` fail (`left: Held, right:
    SteppedAside`).
  - making `theme_media::declared_backgrounds` return `Vec::new()` makes
    `declared_backgrounds_collects_poster_video_and_wallpaper` fail.

Limits (honest, not covered without the keeper live):

- The end-to-end live test (keeper changes the background from the engine's own program → HVE steps aside;
  re-apply → palette returns and the engine is mute) is W4 and needs the keeper.
- The watch samples every 5 s, so the step-aside lags a keeper change by up to one interval; the live test
  should confirm the lag is acceptable. The interval is a single constant
  (`FOREIGN_CHANGE_POLL_INTERVAL`) if it needs tuning.
- The declared set is resolved from the descriptor's `palette_file` layout
  (`{theme}/providers/{id}/palette.json`, what the provider's apply writes). A descriptor whose path does
  not have that shape declares nothing and the detector holds — conservative, never a wrong step-aside.
- `hve_is_acting` is in-process: if HVE restarts mid-apply the flag is gone, but a restart also drops the
  descriptor's owner, so the detector has nothing to defend.
- The engine read is one subprocess every 5 s while a theme owns the palette. It is not sampled while
  nothing claims the palette.

### W2 — correction: the cross-model review findings (08-oct-2026)

The cross-model review of `1285524` returned one critical and four warnings; all are closed in
`src/providers/background.rs` (no engine code, no new dependency, no design change):

- **Critical — a partial step-aside could mute the engine forever.** The old code cleared the
  descriptor and THEN released the mute; if the release failed (or there was no pending value), the
  engine stayed `off` with no descriptor and no retry — the next watch tick read `descriptor ==
  None` and returned "nothing claimed", so the silence persisted until an HVE restart, and the log
  still claimed the mute was released. The release is now RECOVERABLE: when the descriptor is
  absent, `step_aside_if_foreign` runs `complete_pending_release`, which releases the pending mute
  whenever one is recorded and HVE is not acting (a theme that re-claimed the palette between ticks
  is protected by `restore_color_authority` itself, which keeps the mute while a descriptor exists).
  A failed release leaves the mute PENDING and is retried on the next tick; the log now tells the
  truth in all three cases — released / already back / failed-and-pending — and never says
  "released" when nothing was restored. Idempotent: the release only ever rewrites the exact `off`
  token we wrote.
- **Warning — false step-aside on the engine's own reconcile.** The decision only knew the declared
  set, so a hotplug or rotation re-applying a background the engine remembered (not declared by the
  theme) looked foreign and HVE abandoned the defence without the keeper touching anything (the
  07-oct wake). The step-aside now also requires that the KEEPER is acting, and its signal is that
  the engine's own picker program is running: a cheap `/proc` scan for a numeric pid whose `comm` is
  `skwd-wall-v2` (the daemon is `skwd-walld`, a different name, so the two never collide). Without
  that program open a background change is attributed to the engine (reconcile/rotation) → Hold; a
  signal that cannot be determined (`/proc` unreadable) degrades conservatively to Hold too.
- **Warning — a theme that declares no background locked the keeper out.** With an empty declared
  set the table answered Hold forever, so the engine stayed mute and its palette was never published
  again. The rule is now the opposite: an empty declared set cannot match any live path, so a live
  background then belongs to someone else → StepAside. Both sides are pinned by tests.
- **Warning — `HVE_APPLY_ACTIVE` was a process boolean.** Two overlapping applies could clear the
  flag for each other, and it was raised AFTER the ~250 ms observe wait (an uncovered window). It is
  now a saturating DEPTH COUNTER raised by `ApplyActivityGuard` at the very top of
  `hold_color_authority` (before the flip and the wait) and lowered on every exit path, including an
  error; `hve_is_acting()` reads `depth > 0`.
- **Suggestion — path comparison.** Both the live and the declared paths are canonicalized (when the
  path exists on disk; a missing path is compared as-is, never dropped) before the exact comparison,
  so the same file spelled two ways — a symlink, a relative-vs-absolute record — is not mistaken for
  a foreign change.

Files touched (this correction): `src/providers/background.rs` only (production logic plus 11 tests;
no engine, config or `.slint` change).

New tests (all hermetic — stubbed engine binaries on `PATH`, private temp sandboxes, a forced
keeper-program signal so no test depends on what runs on the box):

- `a_failed_release_is_retried_and_completes_on_the_next_check` (critical: the release is pending and
  the next check completes it; a third call is a no-op)
- `a_step_aside_never_logs_a_release_it_did_not_do` / `a_step_aside_without_a_pending_mute_does_not_claim_a_release`
  (the log reflects reality)
- `a_foreign_background_without_the_keepers_program_holds` (pure) /
  `a_foreign_background_holds_while_the_keepers_program_is_not_running` (lifecycle)
- `the_keeper_program_signal_reads_the_process_table` (the picker is recognised, the daemon's name is
  not, an empty table is "not running", an unreadable table is "unknown")
- `a_theme_that_declares_no_background_yields_a_live_one` (pure) /
  `a_theme_that_declares_no_background_steps_aside` (lifecycle)
- `a_symlinked_live_background_matches_the_declared_one` (canonicalization)
- `overlapping_holds_keep_hve_acting_until_the_last_one_ends` / `a_failed_hold_never_leaves_hve_acting`

Evidence (commands run from `/home/ximo/Proyectos/hve`):

- `cargo test`: `1492 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` (`145.11 s`) — W2's
  1481 plus 11 new tests.
- `cargo build`: clean, no warnings; `cargo test --no-run`: no warnings.
- Non-vacuous checks (throwaway breaks, reverted; `sha256sum` confirmed `src/providers/background.rs`
  returned to its pre-break bytes `ff5c4763a48059f0f958a4aa7286777707ef0c14ac9873981ba13d48d71f893a`):
  - removing the `complete_pending_release()` retry from the no-descriptor branch makes
    `a_failed_release_is_retried_and_completes_on_the_next_check` fail (`left: "policy": "off"`,
    `right: "policy": "wallpaper"`).
  - making `ApplyActivityGuard::drop` `store(0)` (the old boolean behaviour) makes
    `overlapping_holds_keep_hve_acting_until_the_last_one_ends` fail (`left: 0, right: 1`).

Limits (honest, not covered without the keeper live):

- The keeper gate is the picker program's presence: a background changed from the engine's CLI or a
  shortcut WITHOUT opening its window is not detected as the keeper's, and HVE holds. That is a real
  limit of the chosen signal, not a guess.
- The picker process's lifetime (does `skwd-wall-v2` really stay alive while the keeper drives it,
  and does its `comm` really read exactly that) must be confirmed in the W4 live test.

### W3 — done

Makes every guard decision readable without `tracing` (which goes to `/dev/null` in production, where
the app is started by the session/tray).

New `src/decision_log.rs`: one append-only line per decision under HVE's cache dir
(`$XDG_CACHE_HOME/hve/color-authority.log`, the same `hve_cache_dir()` the rest of the code uses).
Format mirrors the watcher's own log: `[HVE Authority] <UTC timestamp> <DECISION> <detail>`.
Best-effort: a failed write is reported through `tracing` and never aborts the decision it records.
Bounded: the file is capped at `MAX_BYTES` (256 KiB) and rotated to `color-authority.log.1` before an
append that would pass the cap, so disk use never exceeds `2 * MAX_BYTES` and the most recent history
always survives.

Decisions recorded (one line each):

- `MUTE SET` — the flip, with the previous `theme.policy` value.
- `MUTE HELD` — the engine was already held off (pending marker reused), or the release kept the mute
  while a colour-authority descriptor exists.
- `MUTE RELEASED` — a genuine release (apply, crash repair, step-aside, completed pending release).
- `MUTE RELEASE PENDING` / `MUTE RELEASE FAILED` — a failed/pending release and its retry.
- `STEPPED ASIDE` — the W2 step-aside, with its reason (a foreign background while the keeper's program
  is running).
- `CLAIMED` — a theme claims the palette (the descriptor write), i.e. the re-claim on applying a theme.
- `ENGINE ABSENT` / `ENGINE UNREADABLE` — the engine config or its live background could not be read.
- `ASSERT GRANTED` / `ASSERT REFUSED` — the `assert-color-authority` verb, with its source
  (monitor|resume) or its refusal reason.

Watcher (`assets/scripts/color_watcher.sh`): the 5 s cooldown now logs the suppression it used to
swallow, and the `assert-color-authority.last` stamp is written only after the `hve-ipc` helper is
proven present — an absent helper no longer poisons the next 5 s. Bash still only asks: the
capability-routing contract tests (no backend CLI, no backend-file rewrite) stay green.

Files touched:

- `src/decision_log.rs` (new) — the bounded, best-effort log plus 3 tests.
- `src/main.rs` — `mod decision_log;`.
- `src/providers/skwd_policy.rs` — the mute set/held/released/pending/failed and engine
  absent/unreadable decisions, plus 4 tests.
- `src/providers/background.rs` — the step-aside and its release decisions, plus 2 tests.
- `src/color_authority.rs` — the `CLAIMED` decision on a descriptor write, plus 1 test.
- `src/ipc.rs` — the pure `reassert_decision` mapping + the decision-log call, plus 1 test.
- `assets/scripts/color_watcher.sh` — the suppression log line and the stamp ordering.
- `src/watcher.rs` — 1 static contract test pinning both script changes.

Evidence (commands run from `/home/ximo/Proyectos/hve`):

- `cargo test`: `1504 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` (`140.50 s`) — W2's
  1492 plus 12 new tests.
- `cargo build`: clean, no warnings.
- Non-vacuous checks (throwaway breaks, reverted; `sha256sum` confirmed the files returned to their
  pre-break bytes `4f2896a1…` / `35b0f1e0…` / `37417a67…`):
  - making `decision_log::rotate_if_needed` a no-op makes
    `a_full_log_rotates_to_one_previous_generation` fail.
  - moving the stamp write back before the helper check makes
    `the_watcher_logs_suppressed_asks_and_stamps_only_a_real_ask` fail.
  - renaming the yield's `MUTE SET` decision makes
    `the_yield_and_release_are_recorded_in_the_decision_log` fail.

Limits (honest, not covered without the keeper live):

- The log records decisions, not proofs: it says what HVE decided, and the W4 live test is what
  confirms the decisions match the screen.
- Rotation keeps ONE previous generation; a very long incident can push older lines into `.1` and then
  out. The cap is a single constant (`MAX_BYTES`) if the keeper needs more.
- Concurrent writers append with `O_APPEND`; a rotation racing an append can drop at most one line
  (best-effort by design, never an abort).

### Keeper's live verdict (08-oct-2026)

In his words: "a falta de probar lo de suspender... a mi por ahora me parece perfecto y me gusta". So the
general behaviour is validated live by the keeper (theme rules, mute holds, HVE steps aside on a foreign
background change) and the **suspend/resume case is still pending**. W4 stays open until that last case is
run; nothing else in W4 is outstanding.

Delivery state at this point: the new binary and the changed watcher script are installed
(`~/.local/bin/hve`, `~/.local/bin/assets/scripts/color_watcher.sh`, both md5-verified against the repo),
with the previous binary kept at `~/.local/bin/hve.pre-palette-mute-2026-10-08` and the previous script at
`/tmp/opencode/color_watcher.sh.pre-palette-mute`; the restore tag is
`restore-point/pre-palette-mute-2026-10-08`. The branch is pushed. **Still the keeper's call: the PR**,
because the accumulated branch is well past the 400-line review budget and has to be sliced (chained PRs)
or carry a maintainer-approved `size:exception`.

Evidence collected while planning (07-08-oct-2026, this machine):
- The engine publishes on every apply: `noctalia palette bridge: published scheme and
  /home/ximo/.config/noctalia/palettes/skwd-wall.json`, verified today at 02:38:33Z, 02:38:56Z, 02:39:03Z.
- The mute works and is observable: at 02:39:55Z the apply logged `theme apply: backend=off` with no
  publish line, and `palettes/JokerTheme.json` was rewritten at 04:39:55.887 local.
- The 07-oct wake: `output set changed (hotplug), reconciling per-output wallpapers` at 17:05:04Z,
  17:05:26Z, 17:05:28Z, each followed by a publish.
- The 07-oct miss on HVE's side: the watcher asked at 19:05:04 and 19:05:26 only; the engine's second and
  third publishes fell inside the silent 5 s cooldown (`color_watcher.sh:151-157`).
