# Feature: the transition masks actually reach the compositor (W1)

**Locator**: `odd/tasks/theme-mask-eval-fix.md`
**Engram mirror**: topic `odd/theme-mask-eval-fix/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches, do NOT rebase)
**Checkpoint before this work**: `79ca9df` (clean tree; roll back here and this patch leaves no trace)
**Status**: IN PROGRESS — implementation
**TDD**: strict — runner `cargo test`
**Delivery forecast**: well under the 400-line heuristic (one production function + one pure builder + a spec table + tests). Single PR, `single-pr` preferred; no chain needed.

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

## Tasks

- [ ] **T1 — pure Lua builder.** `lua_config_payload(&[(&str, String)]) -> Option<String>`
      builds a single merged `hl.config({ a = { b = { c = V } } })` with keys in
      sorted order; `lua_literal(kind, raw)` normalises a captured original into
      a Lua literal (`"1"`/`"true"` -> `true`, numeric -> as-is, anything else ->
      `None`). Tests FIRST.
- [ ] **T2 — mask spec table + originals store.** `THEME_MASKS` (3 entries:
      blur/border/shadow, with `key`, `option`, `lua_path`, `kind`); one
      `THEME_MASK_ORIGINALS: Mutex<BTreeMap<&'static str, String>>` replacing the
      four `THEME_ORIG_*` statics.
- [ ] **T3 — honest I/O.** `hypr_eval(lua) -> Result<(), String>`: nonzero exit ->
      Err; stdout/stderr not exactly `ok` -> Err (a zero exit alone proves
      nothing, as the `keyword` bug proved). A mask writer that reports failures
      through an injected runner so the decision is testable without spawning
      processes.
- [ ] **T4 — rewire.** `restore_theme_masks`, `reapply_theme_masks` and the apply
      block use the spec table; `animations` disappears from the mask path end to
      end; failures are logged at `warn` with the option name.
- [ ] **T5 — cross-model verification** of the diff by a DIFFERENT model
      (verification is never the writing model).
- [ ] **T6 — live acceptance (keeper).** Run the listener and apply a theme: the
      section that reported `blur=0: 0/443` must now report the masked keys
      observed as 0, and the owner judges the look.

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
