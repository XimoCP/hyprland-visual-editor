# ODD — The theme owns the palette: mute the engine, step aside when someone else changes the background

Feature name: `palette-authority-mute-and-yield`
Branch: `hve2-visual-rewrite`
Status: IN PROGRESS — W1 done (see Progress / evidence); W2..W3 not started; W4 (cross-model verification + the keeper's live test) pending by definition.
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
- [ ] **W2 — Step aside when the background change was not HVE's.**
      Detection decision to settle first (see Open questions): the engine's own program showing up in
      Hyprland's event stream (HVE's IPC listener already reads that socket, so presence is free) and/or
      the engine's state read (`outputs --json` / `current`), which HVE's adapter already performs.
      On a background change HVE did not ask for: release the claim (the descriptor is cleared, so the
      theme stops owning the colours and the watcher stops asking) and stay aside until a theme apply
      re-claims and re-mutes. Tests: pure decision function over (program open/closed, background changed
      or not, HVE acting or not), stub binary for the engine read, and the release/re-claim lifecycle.
- [ ] **W3 — Make every decision visible.**
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

Evidence collected while planning (07-08-oct-2026, this machine):

- The engine publishes on every apply: `noctalia palette bridge: published scheme and
  /home/ximo/.config/noctalia/palettes/skwd-wall.json`, verified today at 02:38:33Z, 02:38:56Z, 02:39:03Z.
- The mute works and is observable: at 02:39:55Z the apply logged `theme apply: backend=off` with no
  publish line, and `palettes/JokerTheme.json` was rewritten at 04:39:55.887 local.
- The 07-oct wake: `output set changed (hotplug), reconciling per-output wallpapers` at 17:05:04Z,
  17:05:26Z, 17:05:28Z, each followed by a publish.
- The 07-oct miss on HVE's side: the watcher asked at 19:05:04 and 19:05:26 only; the engine's second and
  third publishes fell inside the silent 5 s cooldown (`color_watcher.sh:151-157`).
