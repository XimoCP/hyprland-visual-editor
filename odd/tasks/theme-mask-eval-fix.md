# Feature: the transition masks actually reach the compositor (W1)

**Locator**: `odd/tasks/theme-mask-eval-fix.md`
**Engram mirror**: topic `odd/theme-mask-eval-fix/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches, do NOT rebase)
**Checkpoint before this work**: `79ca9df` (clean tree; roll back here and this patch leaves no trace)
**Status**: W1 IMPLEMENTED — cross-model verification round 1 found one CRITICAL defect, fixed; round 2 pending; live acceptance pending
**TDD**: strict — runner `cargo test`
**Delivery forecast**: MISSED. Recorded honestly: ~420 authored lines in `src/main.rs` (source + 15 tests + comments) plus this document. The overshoot is the second pass: cross-model review found that making the masks REAL created crash/double-apply failure modes the dead code never had, and those could not be fixed in fewer lines without shipping a new way to break the desktop permanently.

## Verification record

Round 1 (GLM 5.3 Flash, cross-model, falsification brief): verdict FALSIFIED. One CRITICAL (F1) and two MAJOR findings, all now addressed:

| Finding | Severity | Resolution |
| --- | --- | --- |
| F1 a second apply during the transition re-captured the already-masked values, so every later restore wrote the masked values back (`permanent` loss of blur/border/shadow) | CRITICAL | capture is first-writer-wins (`mask_plan`), the same guard the interlude view state already used; pinned by `a_second_apply_does_not_recapture_the_masked_values` |
| F2 a crash between masking and restoring left the compositor masked with no in-process net | MAJOR | crash marker written BEFORE the mask lands, cleared after a successful restore, replayed by `startup_recover_crashed_masks` (same protocol as `skwd_policy`'s yield marker) |
| F3 a late `configreloaded` landing after the finale cleared the store masked with nothing able to restore it | MAJOR | `should_reapply(transitioning, have_originals)` refuses when nothing could ever restore; AND every mask-writing path (apply, finale restore, reapply, startup repair) holds `MASK_WRITE_SERIAL`, so a reapply that passed its guard can no longer land after the finale. NOT covered by a test — a race is not deterministically testable; it rests on the lock-order argument (always serial -> store, one `hyprctl` call inside, no other taker) |
| F4 a semantically failed `eval` could still answer `ok` | MINOR (residual) | not falsifiable without mutating the live compositor; the three paths are pinned against the project's own Lua assets. Accepted residual: a future option rename would answer `ok` and silently no-op. Candidate mitigation (readback after the first mask of a process) recorded, not built |
| F5 the builder accepted paths/literals that render invalid Lua | MINOR | `is_lua_ident` + `is_lua_literal`; pinned by `the_builder_refuses_anything_it_cannot_render_as_valid_lua` |
| F7 stderr noise next to a successful `eval` turned success into a reported failure | MINOR | success is decided by the answer, via the pure `eval_outcome` |

Also: one transient suite failure was observed in one run (991 tests) and NOT reproduced in three consecutive full runs plus 15 individual runs; it was not in this unit's tests and remains unidentified.

## Tasks

- [x] **T1 — pure Lua builder** (+ identifier/literal validation).
- [x] **T2 — mask spec table + one originals store** replacing the four `THEME_ORIG_*` globals.
- [x] **T3 — honest I/O**: `hypr_eval` + pure `eval_outcome`; mask writes report failures through an injected runner.
- [x] **T4 — rewire**: `restore_theme_masks`, `reapply_theme_masks` and the apply block use the table; `animations` gone end to end.
- [x] **T4b — crash safety** (found by review): first-writer-wins capture, mask marker + startup replay, reapply guard.
- [ ] **T5 — cross-model verification round 2** on the fixes.
- [ ] **T6 — live acceptance (keeper)**: run the listener and apply a theme; the probe section that reported `blur=0: 0/443` must now see the masked keys at 0, and the owner judges the look.

## Objective

Make the four compositor options the theme transition masks (`blur`, `border`,
`shadow`; `animations` deliberately excluded) actually change on Hyprland
0.56.2, and make a failure impossible to hide.

## Problem, with the measured evidence

`hypr_set` (`src/main.rs:839`) runs `hyprctl keyword <option> <value>` and
**discards the result** (`let _ = ... .output()`). In this Hyprland (0.56.2, Lua
/ "non-legacy" parser) that command is rejected:

```
$ hyprctl keyword decoration:blur:enabled 0
keyword can't work with non-legacy parsers. Use eval.
exit=0            <-- SUCCESS exit code
```

Measured: after that command `getoption` still reported `true`. The listener
recorded **0 of 443 samples** with `blur=0` or `animations=0` across 10 theme
applies. Therefore the masks have never been applied on this machine, and
`restore_theme_masks` / `reapply_theme_masks` were no-ops too.

Consequence (measured + inferred): the desktop blur, the window border and the
compositor-animated fullscreen flips were never suppressed. This is why the
visual symptoms kept coming back after every timing fix: the visual half of the
design was never functional.

Verified replacement mechanism (measured live, restored afterwards):

```
$ hyprctl eval 'hl.config({ decoration = { blur = { enabled = false } } })'
ok
$ hyprctl getoption decoration:blur:enabled -j
{"bool": false}          <-- now it changes
```

And verified live: `hyprctl reload` DOES wipe runtime `hl.config` changes
(blur/animations reverted to the config values). So a reload-robustness story
(`reapply_theme_masks`) only becomes meaningful once the masks are real — which
is exactly what this unit unlocks.

## Product decision recorded here

`animations` is REMOVED from the mask set. Its declared intent was to make the
reveal an instant cut, but the owner has only ever seen the animated version and
calls it a hit; masking it would degrade the effect he wants. The mask set is an
explicit constant so the A/B (does `border` / `shadow` earn its place?) is a
one-line change.

## Scope

IN: `hypr_set` replacement, a pure Lua payload builder, the mask spec table, one
originals store instead of four globals, honest error reporting, tests.
OUT: the transition timing architecture (W2-W5), the engine modules
(`src/engine.rs`, `config.rs`, `settings.rs`, `theme_manager.rs`, `app_state.rs`,
`watcher.rs`, `utils.rs`, `src/providers/*`), anything visual in `.slint`.

## Planned scope of this unit

The task list and its live status are recorded in the status block at the top of
this document; the original plan is superseded there. Summary: T1 pure Lua
builder, T2 mask spec table + one originals store, T3 honest I/O, T4 rewire
(`animations` out end to end), T4b crash safety (added after cross-model review),
T5 cross-model verification round 2, T6 live acceptance by the owner.

## Acceptance criteria

1. `cargo test` green, no warnings introduced.
2. Unit tests pin the exact Lua string for representative inputs, including the
   real 3-mask set, and the `None` case for an unparseable original.
3. A failing `hyprctl eval` is REPORTED (test with an injected failing runner);
   no silent `let _ =`.
4. `animations` is not captured, not masked and not restored.
5. Live: the probe observes the masked keys at 0 during a transition.
6. The owner's visual judgement of the transition, recorded in this document.

## Checks

- `cargo test` (full suite) before and after.
- Parent spot check: re-run the pinned builder test from a clean invocation.
- Live acceptance: `/tmp/opencode/hve-burst-listener.py` (section 3b).

## Route

One mechanical, already-understood change in ONE production file (`src/main.rs`,
with its tests), so it is done **inline** by the orchestrator rather than
delegated: the writer trigger (2+ non-trivial files) is not met, and the design
is resolved. Verification IS delegated to a different model (T5), as required.
