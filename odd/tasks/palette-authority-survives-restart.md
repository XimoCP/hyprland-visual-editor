# ODD — Palette ownership survives a restart

Feature name: `palette-authority-survives-restart`
Branch: `hve2-visual-rewrite` (current; not the default branch)
Test runner: `cargo test`
Delivery: work-unit commits on the branch; push/PR are the keeper's decisions
Status: T1 and T2 DONE, each with RED-then-GREEN evidence; full suite 1829 passed / 9 failed, the 9 being the set that already fails at the base commit (verified by stashing this change); cross-model verification DONE on `971e09a` — S1 pass, S2 pass, S3 pass, no blockers (see L7)

## Objective

The keeper's contract, in his own words: while HVE has a theme applied it must keep the engine's
mouth shut, and when HE takes the background over with the engine's picker HVE must stay quiet.
Today only the first half is attempted, and only around an apply: after a restart the claim comes
back while the mute does not, so the engine re-derives a palette HVE then reads as the keeper's own
hand.

## Specs

S1. While a theme owns the colours (a valid colour-authority descriptor exists), a restart must not
hand the palette back: the startup re-arms the engine mute (`theme.policy` -> `"off"` plus the yield
marker remembering the previous value) and keeps the descriptor. Source (keeper, 2026-10-18):
"si hve esta de dueño no deje que nadie le pise".
Acceptance: with a descriptor and the engine's own `theme.policy` value, a startup re-arm flips
exactly the bare `"policy"` value token to `"off"`, leaves every other byte and the file mode
untouched, records `MUTE RE-ARMED` in the decision log, and is a byte-level no-op on a second run.
With no descriptor, with a switch the engine does not own (`fixed`, `off`, missing, ambiguous), or
with an unreadable config, it writes nothing.

S2. While the keeper owns the colours — he changed the palette by hand, or he chose the background
with the engine's own picker — HVE stays quiet: the startup must neither flip the engine switch nor
rebuild the theme's claim over his choice. Source (keeper, 2026-10-18): "si he cambiado yo el fondo
con Skwd-wall entonces sea HVE quien se quede callado".
Acceptance: (a) the re-arm flips nothing while the keeper marker holds; (b) the startup descriptor
repair leaves the descriptor absent while the keeper marker holds; (c) the background step-aside
writes that marker, so the state survives the restart.

S3. Nothing else moves: a theme apply still claims the colours (and clears the keeper marker), the
step-aside still clears the descriptor and releases the mute, and the existing crash repair still
replays a pending marker when nobody owns the palette.

## Tasks

T1 — re-arm the mute at startup while a theme owns the palette (route: inline)
     `rearm_mute_for_existing_owner` in `src/providers/skwd_policy.rs`, reached
     through the single startup seam `startup_settle_engine_mute` wired in
     `src/main.rs` after the descriptor repair.
     RED: `cargo test startup_rearm` → 6 `cannot find function ... in this scope`
     errors at the new call sites. GREEN: the same command, 7 passed.
T2 — persist the keeper takeover, and keep both startup paths quiet (route: inline)
     `mark_keeper_background` in `src/color_authority.rs`, written by
     `release_claim` in `src/providers/background.rs`, honoured by
     `restore_missing_colour_authority` in `src/theme_manager.rs`.
     RED: with the two implementation lines reverted, `cargo test --
     the_step_aside_remembers restore_missing_colour_authority_leaves` → 2 failed
     ("the keeper's choice must outlive the restart", "the keeper's colours must
     not be claimed again by a restart"). GREEN with them restored: same command,
     both passed.

## Log

L1 (keeper, verbatim): "a ver nosotros implementamos que si hve tenia un tema puesto debía de poner
la mano en la boca a Skwd-wall para que no hiciera nada... no cambiara paleta ni nada... pero si
nosotros poniamos un fondo con skwd-wall entonces le dabamos vía libre ya que hay HVE ya no pinta
nada y se queda aparte... entonces que ha fallado y cual a sido el problema?"

L2 (keeper, verbatim, approving the fix): "vale me estas diciendo que puedes hacer que si hve esta
de dueño no deje que nadie le pise y si he cambiado yo el fondo con Skwd-wall entonces sea HVE quien
se quede callado... eso sería lo correcto."

L3 — diagnosis that produced this feature (read-only, 2026-10-18, evidence on disk):
(a) the mute is written only by `yield_color_authority` (`skwd_policy.rs:667`), reachable only from
the apply path (`noctalia.rs:2546`); (b) the startup repair at `main.rs:3523` replays the marker and
un-mutes, and `restore_missing_colour_authority` (`theme_manager.rs:792`) rebuilds the CLAIM without
touching `theme.policy` — nothing at startup re-arms the mute; (c) the step-aside (`background.rs:641`
`release_claim`) logs and clears, persisting nothing, so the next start rebuilt the claim from
`last_applied_theme`. Measured on 2026-10-18: 09:32 the keeper chose `fall-echo-tries.png` from the
engine's picker (step-aside, correct); 16:10 session start rebuilt the claim but not the mute; 16:13:56
the colour defence REFUSED the re-assert ("presumed legitimate") and released the theme's colours;
16:36:47 the next apply finally set the mute and restored poster + Flexoki.

L4 — design decisions taken while implementing (recorded so the verifier can challenge them):
(D1) the keeper takeover reuses the EXISTING `keeper-palette.json` marker instead of a second file —
one state, one predicate (`keeper_owns_palette`), one clear point (`write_descriptor` on the next
theme claim); (D2) the re-arm never clears the descriptor (unlike `yield_color_authority`, which drops
a stale claim around an apply) and never writes when the switch is not the engine's own
(`plan_yield` refuses anything but `"wallpaper"`), so a hand edit is never clobbered;
(D3) the re-arm is NOT wired into the crash repair: `recover_crashed_yield` keeps its meaning ("the
mute must not outlive HVE"), and the startup re-arm runs after it, once ownership is final.

L5 — the architecture contract caught the first wiring and the reference was RE-HOMED instead of
the pin raised. The first attempt called `skwd_policy::rearm_mute_for_existing_owner()` directly
from `main()` and `architecture_contract::architecture_pins_hold_or_improve` failed: "NEW COUPLING:
src/main.rs mentions `skwd` 2x (pin 1)". The pin table's own rule (`src/architecture_contract.rs:337`)
forbids raising a pin to make a new coupling pass, so the fix was to move the reference, not the
number: `main()` keeps its single reach into the engine's policy module, and that one reach is now
the settle seam (crash repair **or** re-arm, decided by ownership). Consequences: (a) the W2
`startup_recover_crashed_yield()` hop moved from right after the single-instance lock to just after
`theme_manager.restore_missing_colour_authority()`, because ownership is only FINAL once the
descriptor repair has run — its old position was the reason the descriptor came back while the mute
did not; (b) the W2 source test now pins the settle seam and keeps asserting "after the lock"; the
S1 source test pins "after the descriptor repair". No pin value changed.

L6 — evidence (2026-10-18, `cargo test`): full suite 1829 passed / 9 failed, and the 9 are exactly
the ones that already fail at the base commit (stash-verified: `scripts_contract::*` × 8 and
`watcher::tests::watcher_reasserts_the_applied_theme_palette_on_an_external_change`). The
`architecture_contract` suite is green again. New tests: 7 for the re-arm, 3 for the settle seam,
1 for the step-aside memory, 1 for the keeper-aware descriptor repair, 2 source-wiring pins.

L7 — cross-model verification (HVE rule: the verifier is never the writer), read-only, against
`971e09a`, on `opencode-go/mimo-v2.6-flash` (writer: `opencode/deepseek-v4.1-flash-free`).
Verdicts: **S1 pass, S2 pass, S3 pass, no blockers, 7 advisories**. Evidence it produced:
- full suite 1831 passed / 8 failed (all inside the known baseline set); focused differential
  baseline 172 passed → candidate 185 passed, 0 failed, with NO test removed or renamed and the
  shared writers (`plan_yield`, `locate_policy_value_token`, `write_atomic`, `write_marker`,
  `recover_crashed_yield`, `reconstruct_missing_yield_marker`, `replace_token`) byte-identical;
- six mutation experiments in a scratch copy (`/tmp/opencode/palette-verify-9RlD/mutations-out.txt`),
  each caught by a test: delete the repair inside the settle (2 fail), delete the re-arm branch (1),
  delete the keeper-marker write (1), delete the early-return (1), reverse the mark/clear order (1),
  force a same-bytes rewrite while held (1). Deleting the settle call from `main.rs` is caught twice:
  by the source-wiring test and by the pin checker's STALE-PIN direction;
- `architecture_contract` 10/10 on both trees and `src/architecture_contract.rs` byte-identical: no
  pin was raised, and `main.rs` keeps exactly one mention of the engine;
- the relocation window was traced rather than assumed: the foreign-change watch's first tick sleeps
  5 s and the picker-voice worker only reacts to IPC events published after the settle, so "spawned
  early, settled late" is benign in both reachable states (a crashed-mid-yield release is completed
  by the watch tick with different log wording).
Advisories accepted as-is, each pre-existing or by design, none fixed: an unreadable-but-present
engine config still skips the re-arm (the spec's "writes nothing"); a W5 hand palette change still
releases the mute at the next start (D3); the rebuilt marker hardcodes `"wallpaper"` (pre-existing
W6 heuristic); a failed `mark_keeper_background` degrades to the pre-fix behaviour rather than to
silence; the mtime no-op assertion is a proxy (mutation M6 proved it bites). Engine-side behaviour
(a running skwd-wall honouring the written `"off"`) is outside this repository and stays the
keeper's live test.
