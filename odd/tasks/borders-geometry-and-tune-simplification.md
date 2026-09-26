# Feature: borders geometry fast apply + tune simplification (HVE 2)

**Locator**: `odd/tasks/borders-geometry-and-tune-simplification.md`
**Engram mirror**: topic `odd/borders-geometry-and-tune-simplification/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches, do NOT rebase)
**Checkpoint at open**: `9d9e631`, suite 1003 passed / 0 failed, 0 warnings
**TDD mode**: strict (runner `cargo test`)
**Writers**: DeepSeek v4-flash. **Verifiers**: GLM 5.3-flash (cross-model, never the writer).

## Objective

Make the geometry sliders (border size and the three geometry values) feel instant on the
live desktop, keep the value durable across a config reload, and trim the Borders tune pane
down to what the keeper still wants (Colors, Angle, Inactive color, Halo/Shadow, Save).

## Problem

Touching the border thickness today runs `assets/scripts/geometry.sh` + `assemble.sh` on every
debounced tick: **100 ms per adjustment**, ~180 ms with the 80 ms slider debounce (measured
with numbers, 2026-09-26). The same effect is reachable in **5 ms** through
`hyprctl eval 'hl.config({...})'` — the tool the theme transition already uses (W1).

A second front: the U3a rebuild removed the radius / gap_in / gap_out sliders from the Borders
section and left only the thickness. The plumbing is still whole underneath, so recovering them
is wiring, not new machinery. The keeper wants them back **at the top, next to the size**, as a
"Geometry" block, and asked for a short personal comment near those parameters saying he put
them there because he did not know where else they belonged.

The tune pane then grew too much: past Halo/Shadow the keeper wants everything else gone
(animations x3, curves, rules).

## Scope

- In: `on_panel_apply_geometry` and the geometry apply path, `geometry.sh` / `assemble.sh` usage,
  the Borders section geometry block, the tune pane contents, i18n keys for the added strings.
- Out: the engine (`src/engine.rs` internals stay), the presets themselves, monitors/HDR (hyprmod).
- Out: the theme transition and the wallpaper engine (closed work; do not reopen).

## Constraints

- One writer per tree. Verification with a DIFFERENT model (writer DeepSeek, verifier GLM).
- The value MUST survive `hyprctl reload`; a runtime `hyprctl eval` override does not survive it
  (verified 2026-09-25), so persistence cannot be dropped, only deferred.
- No AI attribution in commits. Conventional commits.
- Engineered artifacts in English (code, comments, UI copy, docs). Comments may carry the
  keeper's personal note where he explicitly asked for it.
- Visual verification is mandatory for any `.slint` change: run the headless render test, READ
  the PNGs it writes under `/tmp/opencode/`, and extend/add a render test for any new state.

## Acceptance criteria

1. Dragging the thickness slider reaches the compositor in the millisecond range (hot apply),
   not the ~180 ms path; the persisted write happens without being felt on every tick.
2. After `hyprctl reload`, the applied geometry is still there (durability preserved).
3. Radius / gap_in / gap_out are editable again from the Borders section, at the top, next to
   the size, and a change to any of them does not wipe the other three.
4. The tune pane keeps Colors, Angle, Inactive color, Halo/Shadow and Save; animations, curves
   and rules are gone; presets that carry their own animation in their `.lua` still animate.
5. `cargo test` green, 0 warnings; render PNGs inspected.

## Tasks

- [x] **B1 — fast geometry with a single deferred persist.**
  Route: delegated writer (2+ non-trivial files: `src/main.rs` + `ui/panel/sections/BordersSection.slint`).
  - Establish first, with runtime evidence, what the current path really does: does the tick
    reach the live compositor, does the on-disk fragment change, does the value survive
    `hyprctl reload`. (The keeper explicitly asked to confirm the value-saving mechanism before
    trusting the speed-up.)
  - Apply in hot with `hypr_eval` (`src/main.rs:1410`), sending only the options that actually
    changed, and coalesce the persistent `apply_geometry()` so a drag writes once instead of
    once per tick.
  - Tests: a failing test first for "only changed options are pushed hot", plus coverage for
    the coalesced persist.
- [x] **B1 verification — cross-model (GLM 5.3-flash), commit `c29fd35`.** Verdict: **PASS** on
  AC1 (hot in the millisecond range) and AC2 (durability after reload). All four writer claims
  CONFIRMED with adversarial probes; 0 warnings. Two warnings came out of it (B5, B6 below) plus
  three suggestions: the hot chunk does not clamp negatives while `geometry.sh` does
  (unreachable through the UI today, which clamps 1..5); the hot chunk never sends
  `decoration.rounding_power` and the persisted fragment writes it only when radius > 0 (they
  coincide today only because Hyprland's default is 2); and the coalescer tests assert a value
  that is in practice irrelevant (the drain re-reads the config). The writer's "1010 passed /
  0 failed" holds, but the suite is non-deterministic under parallelism (see B6).
- [x] **B5 — close the 400 ms durability window (candidate-caused).** `arm_geometry_persist`
  (`src/main.rs:1449-1454`) only writes when the timer fires: if HVE exits inside those 400 ms
  (window close, IPC quit, crash) the whole drag never reaches disk and the next `hyprctl reload`
  reverts it. The commit message's "deferred, never dropped" is false in that case. Mitigation:
  save the config per tick (cheap) and defer only the fragment/assemble, or flush on event-loop
  exit.
- [x] **B6 — stop `cargo test` from touching the real environment (pre-existing, found here).**
  `src/shell/ui_tests.rs:3319` (`test_system_minimize_10s`) does not use `TempEnv` and calls
  `state.toggle_system(true)` -> `init.sh enable` against the real HOME: it copies the watchdog
  into `~/.cache/hve/`, rewrites `~/.config/hve/config.json` and fires a `hyprctl reload`. A test
  run also regenerates `overlay.lua`; when `assets/fragments/` is empty (gitignored) that overlay
  loses its modules and the reload arbiter changes. Separately, three pre-existing sandbox tests
  fail at random under parallelism (reproduced on the parent commit `9d9e631`):
  `watcher::tests::sandboxed_lock_is_released_when_the_watcher_is_killed_with_a_live_child`,
  `reload_coalescer::sandboxed_request_pending_keeps_drainer_alive_and_fires`,
  `providers::noctalia::tests::v5_apply_custom_owner_reasserts_off_thread_without_blocking`.
- [ ] **B2 — recover the radius / gap_in / gap_out sliders** at the top of Borders, as a
  "Geometry" block next to size, with the keeper's personal comment. Watch the engaged-slider
  seat numbers left free by U3a.
- [ ] **B3 — simplify the tune pane** to Colors, Angle, Inactive color, Halo/Shadow, Save.
  Delicate: touches the save path. Do it last.
- [ ] **B4 — i18n** for everything added (preset `@Title`/`@Desc` are English-only today and not
  wired to i18n; `13_the_joker` also has Spanish comments and no `@Color`).

## Key files

- `ui/panel/sections/BordersSection.slint` — the section and the tune pane (largest file).
- `ui/panel/PanelRoot.slint:773-776` — the four geometry bindings into the section.
- `ui/components.slint` — `GeometrySlider`.
- `src/main.rs:4110` `on_panel_apply_geometry`; `src/main.rs:1410` `hypr_eval`.
- `src/engine.rs:179` `apply_geometry`; `src/app_state.rs:81`; `src/config.rs`.
- `assets/scripts/geometry.sh`, `assets/scripts/assemble.sh` — the slow path.

## Progress

- 2026-09-26: document opened; B1 delegated. Nothing written to source yet.
- 2026-09-26: **B1 done** (this commit). Step 0 established the current path live on Hyprland
  0.56.2: one `geometry.sh 5 44 8 10` tick changed the fragment, and the live compositor
  followed (`border_size 2→5`, `rounding 32→44`, `gaps 5→8/10`) ~600 ms later through exactly
  ONE coalesced `hyprctl reload` (measured with a logging `hyprctl` wrapper). A 200 ms two-tick
  burst produced ONE reload and the last value; a manual `hyprctl reload` kept the file-backed
  value, while `hyprctl eval` was 5 ms but was wiped by the next reload — the durability
  premise holds. Path cost measured 175 ms per tick. Implementation: hot apply pushes only the
  changed options as one `hl.config({...})` chunk via `hypr_eval`; the persistent
  `apply_geometry()` is coalesced behind a 400 ms single-shot timer (`arm_geometry_persist` +
  `GeometryPersistCoalescer`), so a drag writes once. The four exact chunk shapes were accepted
  live by `hyprctl eval`. Full suite 1010 passed / 0 failed, 0 warnings. Desktop state restored
  and verified (2/32/5/5, fragment + overlay md5 matched baseline). No `.slint` change was
  needed: the timer already forwards the live `root.geom-*` values, so the "radius and gaps
  travel with the size" line at BordersSection.slint:2149 is correct as-is and was left intact.
- 2026-09-27: **B5 done** (this commit). The 400 ms window is closed with two halves that
  together leave no exit path uncovered: the config is saved PER TICK in the hot callback
  (measured 0.031 ms per `Config::save()` over 200 writes — the millisecond budget holds), and
  the coalesced fragment/assemble drain is FLUSHED on the way out when
  `run_event_loop_until_quit()` returns. That return covers window close, IPC quit and the
  restart callback (which quits the event loop after spawning the new instance), so all three
  clean exits write the drag. `drain_geometry_persist` is now the single drain shared by the
  idle timer and the flush, so both paths cannot drift; the flush stops the timer first, so it
  can never fire twice. Test `flush_on_shutdown_writes_the_last_geometry` was RED first (the
  flush did not exist) and is GREEN after: it stages a drag, runs the flush with a stub
  `geometry.sh` that records its four arguments, and asserts the durable path saw the LAST
  value, the persisted config carries it, and a second flush with nothing owed is a no-op. No
  Hyprland and no real `hyprctl` are involved.
- 2026-09-27: **B6 done** (this commit). Root cause established with evidence BEFORE choosing fixes,
  and the task's shared-root-cause hypothesis was TESTED and refuted: the three flakes are ONE
  class of harness race (asserting an eventual side effect immediately after an earlier signal
  instead of polling for it with a bound), and the leak is a separate hermeticity gap. It is not
  global env state: the three tests already pass a temp HOME/PATH to their subprocesses. The
  races only WIN under CPU contention, which is why they looked random: on this host (loaded by
  a game) the watcher failed 6/11 full runs at `src/watcher.rs:480` and the reload test 1/11 at
  `src/reload_coalescer.rs:784`; the noctalia failure was proven by code order (below).
  - `watcher`: `color_watcher.sh` logs "Starting watcher" at line 126 and only THEN runs the
    initial `assemble.sh` at line 165; the test waited on the log line and asserted the assemble
    marker immediately. Now it polls the marker with a 10 s bound.
  - `reload_coalescer`: the claim/marker are `rm`-ed by the drainer AFTER the fire returns; the
    test asserted their absence right after `reload_count()==2`. Now polled with a 3 s bound —
    the sibling test above already documented this exact window.
  - `noctalia`: `poll("color-scheme-set", 2)` returns when the set line lands, but
    `templates-apply` is a SEPARATE subprocess issued right after it, so the direct
    `stub.templates()==2` read raced; now polled, exactly like the single-shot sibling test.
  - leak: `test_system_minimize_10s` now runs inside `TempEnv` against a temp project whose
    `init.sh` only records its call; its assertions were kept and extended (persisted flag +
    engine call), never weakened. `init.sh enable` against the real HOME was copying the
    watchdog into `~/.cache/hve`, rewriting `~/.config/hve/config.json`, editing the real
    `hyprland.lua` and firing a real `hyprctl reload`.
  - the 10 real `hyprctl reload`s that remained came from `settings.rs`
    (`set_keybinds`/`set_autostart`/`set_tiling_window_rules`); they now go through
    `reload_hyprland()`, a no-op under `cfg(test)` — the file write it follows is untouched and
    no test observes the reload.
  - Measurement with a delegating `hyprctl` wrapper prepended to PATH: full suite went from 14
    real calls to 2, both read-only `-j layers` / `-j monitors` JSON queries (`-j` = "Output in
    JSON"; they cannot dispatch or set keywords), and 0 reloads. The keeper's compositor is
    never reloaded by the suite again.
  - Acceptance: 5 consecutive full suites, 1011 passed / 0 failed each (was 1010 before this
    commit: B5 added one test), 0 reloads per run, and SHA-256 of `~/.config/hve/config.json`,
    every file under `~/.cache/hve` (including the generated `overlay.lua`),
    `assets/fragments/*.lua` and `~/.config/hypr/hyprland.lua` identical after every run. The
    three named tests also went 8/8 each in a targeted stress pass.
  - NOTE for the keeper: `~/.config/hve/config.json` was ALREADY the test's output when this
    task started (`Config::default()` + `is_system_active=true` + `minimize_seconds=10`) — the
    B1 verifier's suite run had overwritten it. A stale `config.json.bak-1789510200` from
    2026-09-16 (with `active_border_file: 04_tri.lua`, `last_applied_theme: Cars`) survives next
    to it. This fence stops the damage from recurring; it cannot restore what was already lost,
    so the keeper should re-pick the theme/border if the current config looks wrong.
  - Pre-existing, NOT introduced here: `cargo build` (bin profile) warns
    `method has_pending is never used`; the identical warning appears in a clean HEAD worktree
    build, and `cargo test` builds are warning-free.

## Next step

B1 is done and verified. Decide between (a) the keeper's live test with a rebuilt binary
(then B2) and (b) closing B5's durability window and B6's test isolation first.
B2: recover the radius / gap_in / gap_out sliders at the top of Borders as a "Geometry" block
next to size, with the keeper's personal comment.
