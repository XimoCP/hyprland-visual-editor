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
  Implemented in `81173cd`; see the verification block below.
- [x] **B5/B6 implementation — commits `c65ce01` (B5) and `81173cd` (B6).** B5 ships both halves:
  a per-tick `Config::save()` in the hot path (measured 0.031 ms per save, so the millisecond
  budget holds) and `flush_geometry_persist`, which stops the idle timer and drains the owed
  persist synchronously right after `run_event_loop_until_quit()` returns. B6 sandboxes
  `test_system_minimize_10s` under `TempEnv` against a temp project, turns the three flaky
  assertions into bounded polls (the flakes were a shared class: asserting an eventual side
  effect immediately after an earlier signal), and routes the three `settings.rs` reloads through
  `reload_hyprland()`, a no-op under `cfg(test)`. Real `hyprctl` calls during a full suite went
  from 14 to 2, both read-only `-j` queries, zero reloads.
- [x] **B5/B6 verification — cross-model (GLM 5.3-flash).** **B6: PASS.** Independently measured
  over 4 full suites: environment byte-identical before and after (`~/.config/hve/config.json`,
  `~/.config/hypr/hyprland.lua`, all of `~/.cache/hve/`, `assets/fragments/*.lua`), 2 read-only
  `hyprctl -j` invocations per suite and zero reloads, and the full diff of `watcher.rs`,
  `reload_coalescer.rs`, `noctalia.rs`, `settings.rs` and `ui_tests.rs` reviewed for weakened
  assertions: none deleted, none `#[ignore]`d, none relaxed. **B5: FAIL on its test, not on its
  code** — see B7. The 400 ms live claim stayed unverified by the reviewer.
- [x] **B5 live acceptance — the keeper, 2026-09-27.** Rebuilt and installed binary (08:07), the
  thickness slider responds instantly, and changing the thickness and closing HVE immediately
  followed by `hyprctl reload` keeps the value. The unverified half of B5 is now closed by the
  strongest available check (the keeper's own live test).
- [x] **B7 — make the shutdown flush test protect the REAL wiring (CRITICAL, candidate-caused).**
  `flush_on_shutdown_writes_the_last_geometry` calls `flush_geometry_persist` directly
  (`src/main.rs:4802`), so it never exercised the call site right after
  `run_event_loop_until_quit()`. Proof it was a false safety net: in a disposable worktree the
  reviewer deleted the production call and ran `cargo clean -p hve`; the test still passed.
  **Done in `84fbde3`.** The replacement is
  `shutdown_flushes_the_deferred_geometry_before_teardown`, which parses `src/main.rs` into a
  Rust AST (`syn`, dev-dependency) and asserts `main` calls the flush **exactly once**, as a
  plain **top-level** statement, **after** `run_event_loop_until_quit()` returns and **before**
  `ipc::cleanup()`. The old drain test is kept (it pins last-value-wins and the no-op-without-owe
  semantics — a distinct property) and its doc comment now says so.
  Design note: an end-to-end binary run (`env!("CARGO_BIN_EXE_hve")` + IPC quit) would need a live
  Wayland/XRandR session and a real compositor, which B6 forbids the suite from touching; the AST
  check is the strongest hermetic option, and the keeper's own live test is the complementary
  real-binary check. **The "RED first" note above was wrong** and is corrected here: the original
  RED only proved the function did not exist, never that the call was wired — which is exactly
  why the B5 test could stay green with the call deleted.
  Self-validating proof (disposable worktree at `84fbde3`, `cargo clean -p hve`, same method as
  the reviewer): removing the production call → test **FAILED** (`main must call
  flush_geometry_persist exactly once; left: 0, right: 1`); nesting it in `if false { ... }` →
  **FAILED** (`must be a plain top-level call, never nested in a branch or loop`); moving it
  before the event loop → **FAILED** (`flush at statement 128, quit at 129`); restored to HEAD →
  **passed**. Worktree removed with `git worktree remove --force`. Full suite 1012 passed /
  0 failed, 0 warnings; B6 fence intact (only 2 read-only `hyprctl -j` calls, 0 reloads) and the
  environment hashes were byte-identical after two full suites.
- [x] **B2 — recover the radius / gap_in / gap_out sliders** at the top of Borders, as a
  "Geometry" block next to size, with the keeper's personal comment. Watch the engaged-slider
  seat numbers left free by U3a. **Done in `d39c585`.**
  - Ranges recovered from history, not invented: the U3a commit `ef68106` shows the removed
    4-slider block as size 1-5, radius 0-100, gap_in 0-30, gap_out 0-30, all step 1
    (`GeometrySlider` rounds its float to `int`). The block now renders under a "Geometry"
    heading with the keeper's note (English, intent not literal translation) and four plain
    descriptions.
  - New keyboard seat map (tune-local = global minus the preset-card count):
    0=size, 1=radius, 2=gap-in, 3=gap-out, 4=angle, 5=inactive, 6=strip (colours only),
    7=add (colours only), 8=remove (colours only), then 9=glow-enabled
    (+range/power/colour/inactive when on), the three anim leaves (enabled +speed/bezier/style
    when on), rule, save-name, save-button. Zero colours drops the three slot stops, so
    glow-enabled is 6 and the strip/add/remove positions do not exist. `borders_tune_stop_count`
    went 10→13 fixed (16 with colours, 13 without; 29 with everything on).
  - The slot-block shift is centralised: the pane derives `idx-add`/`idx-remove`/`idx-glow-en`
    from `slot-stops`, the section exposes `out property idx-strip` and `PanelRoot` reads it
    instead of the old literal `3`, so the ←/→ strip dispatch can never drift from the map again.
  - Plumbing untouched by design: `slider-changed` writes the edited `geom-*` value and restarts
    the SAME 80ms timer, which sends all four through the existing `apply-geometry`; the B1 hot
    apply and the B5 coalesced durable write are unchanged, so a change to one slider cannot wipe
    the other three and durability still holds.
  - Verification: `cargo test` full suite **1014 passed / 0 failed, 0 warnings**; suite sandbox
    intact (SHA-256 of `~/.config/hve/config.json`, `~/.config/hypr/hyprland.lua`,
    `assets/fragments/*.lua` and all of `~/.cache/hve` byte-identical before/after).
  - New tests, RED first: `borders_geometry_block_edits_one_value_and_keeps_the_rest` drives real
    key events into PanelRoot and the real `panel-apply-geometry` callback, then asserts each of
    the four seats moves exactly one value and leaves the other three intact (one apply per edit);
    `borders_geometry_block_renders_each_slider` renders the radius / gap-in / gap-out rows and
    asserts each focused row is a 96px slider carrying its white knob. The tune-count and
    zero-colour ghost-stop tests were re-pinned to the shifted map.
  - Render PNGs read with vision: `borders_geometry_block.png` shows the "Geometry" heading and
    Border Thickness 2px / Corner Radius 10px / Inner Gap 5px / Outer Gap 6px above Gradient
    Angle; `borders_geometry_radius_focus.png`, `borders_geometry_gapin_focus.png` and
    `borders_geometry_gapout_focus.png` each show the corresponding row with the icy focus ring
    and white knob.
- [x] **B3 — simplify the tune pane** to Colors, Angle, Inactive color, Halo/Shadow, Save.
  Delicate: touches the save path. Do it last. **Done in `64d4e09`** — see the B3
  record in Progress below.
- [x] **B4 — i18n** for everything added (preset `@Title`/`@Desc` are English-only today and not
  wired to i18n; `13_the_joker` also has Spanish comments and no `@Color`). **Done in `e348cbb`** —
  see the B4 record in Progress below. **B4-correction:** the preset DESCRIPTIONS never
  reached the picker (the `border-descs` channel was dead end to end) and 13 of the 14 EN
  values were invented fragments; fixed — see the B4-correction record in Progress below.

## Key files

- `ui/panel/sections/BordersSection.slint` — the section and the tune pane (largest file).
- `ui/panel/PanelRoot.slint:773-776` — the four geometry bindings into the section.
- `ui/components.slint` — `GeometrySlider`.
- `src/main.rs:4110` `on_panel_apply_geometry`; `src/main.rs:1410` `hypr_eval`.
- `src/engine.rs:179` `apply_geometry`; `src/app_state.rs:81`; `src/config.rs`.
- `assets/scripts/geometry.sh`, `assets/scripts/assemble.sh` — the slow path.
- `ui/panel/sections/borders_text.slint` — every user-visible string of the Borders panel (B4).
- `ui/panel/sections/SavedPresetCard.slint` — the preset card itself. B4-correction gave it the
  `desc` property it paints under the name; `BordersSection.slint`'s `BordersListPane` forwards the
  translated `border-descs` model into it. The model is the channel, not `BordersText`: it is
  filled by `presets.rs::translate_presets`, which is what the i18n preset keys feed.
- `src/panel_i18n.rs` — fills that global from the embedded i18n map (`apply_borders`).
- `i18n/en.json`, `i18n/es.json` → `borders.*` — the panel copy and the 14 preset name/desc keys.
  The EN preset `desc` values mirror each preset's own `@Desc` (the fallback), so the key and the
  fallback read the same; ES is their neutral translation.

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
  a game) the watcher failed 7/10 full runs at `src/watcher.rs:480` and the reload test 1/10 at
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
- 2026-09-27: **B7 done** (this commit, `84fbde3`). Replaced the false safety net with
  `shutdown_flushes_the_deferred_geometry_before_teardown`, an AST-level test over `main`'s
  shutdown sequence (`syn` dev-dependency; no new runtime dependency). It asserts the flush is
  called exactly once, as a plain top-level statement, after `run_event_loop_until_quit()` and
  before `ipc::cleanup()`, so it fails on all three plausible regressions. The old direct-call
  test is kept and re-documented: it still pins the drain semantics (last value wins, nothing
  owed is a no-op). Verified self-validating in a disposable worktree: call removed -> FAILED,
  nested in a branch -> FAILED, called before the loop -> FAILED, restored -> passed; worktree
  removed. Full suite 1012 passed / 0 failed, 0 warnings; environment hashes byte-identical
  after two suites; B6 fence intact (2 read-only `hyprctl -j`, 0 reloads).
- 2026-09-27: **B2 done** (this commit, `d39c585`). See the B2 checkbox above for the full record:
  ranges recovered from `ef68106`, the new seat map, the `idx-strip` de-duplication, and the two
  new tests (behaviour through the real keyboard path + per-slider render). Full suite 1014
  passed / 0 failed, 0 warnings; environment hashes byte-identical before/after.
- 2026-09-27: **B3 done** (this commit, `64d4e09`). The tune pane is trimmed to Geometry,
  Gradient Angle, Inactive, Active Border Colors, Glow (Halo/Shadow) and Save; the whole
  "Border Animations" block (Border Angle / Border Pulse / Fade Shadow, each with Curve and
  Style) and the "Floating Window Rule" toggle are gone. **`anim-*` / `tune-anim-*` model: KEPT,
  editing surface only removed.** Decision with evidence: `tune_params_from_window`
  (`src/main.rs:303-318`) reads `w.get_tune_rule_enabled()` and `w.get_tune_animations()`, and
  BOTH the live-draft callback (`on_panel_tune_changed`, `src/main.rs:4449`) and the save
  callback (`on_panel_save_border_preset`, `src/main.rs:4337`) build their `BorderParams` through
  it. Dropping the properties would make any edit or save of an animation-bearing preset silently
  strip its `hl.animation` / `windowrulev2` data. `sync_border_tune_pane`, the per-leaf Rust
  setters and the sealed engine are untouched; the pane simply no longer edits the leaves. The
  applies themselves never depended on the model — `border.sh` copies the preset `.lua` verbatim
  — so animation-bearing presets keep animating when applied.
  - New keyboard seat map (tune-local): 0=size, 1=radius, 2=gap-in, 3=gap-out, 4=angle,
    5=inactive, [6=strip, 7=add, 8=remove while colours exist], then 6 with zero colours / 9 with
    colours = glow-enabled (+1 range, +2 power, +3 colour, +4 inactive glow when on), save-name,
    save-button. 9 fixed stops (12 with colours), +4 with glow: 9 / 12 glow-off, 13 / 16 glow-on.
    The three mirrored maps moved together — the pane's `idx-*` (now `base-tail` feeds
    `idx-save-name`/`idx-save-btn`), the section's `idx-glow-en`/`tune-count`, and
    `callbacks::borders_tune_stop_count` (the three anim params were dropped).
  - Tests, RED first on the new count contract: `animation_bearing_preset_round_trips_through_the_tune_model`
    (13_the_joker's three leaves survive load -> `tune_params_from_window` -> `generate_border_lua_full`,
    which re-emits `hl.animation`) and `borders_tune_seat_map_walks_each_remaining_control_once` (a REAL
    Down+Enter walk in both colour cases through PanelRoot's FocusScope: zero colours -> add/remove never
    fire, 13 seats; with colours -> add then remove then glow then Save, 16 seats), plus the render test
    `borders_tune_trimmed_pane_ends_at_save`. Existing count/focus tests were re-pinned to the smaller
    map; no assertion covering a remaining control was weakened. `borders_strip_picker_opens_above_strip`
    now uses a glow-bearing fixture: the R10 top-anchor is content-clamped, and with the animation/rule
    blocks gone the bare pane is shorter than a viewport, so the card lands as high as the content allows
    (still fully visible, directly above the strip) — the assertion was kept, not relaxed.
  - Visual PNGs read with vision: `borders_tune_trimmed_save_focus.png` (pane ends at Save; inventory
    Geometry -> Angle -> Inactive -> Active Border Colors -> Glow -> Save; no Border Animations, no
    Floating Window Rule, no empty container), `borders_tune_trimmed_past_save.png` (no dead tail),
    `borders_tune_focus_middle.png` (glow-enable seat), `borders_tune_focus_last.png` (glow-expanded tail
    ends at Save), `slice_settled.png` (gallery unaffected).
  - Diff: 4 files, +278 / -930 authored lines (1208 total), dominated by the ~600 removed render lines;
    above the ~400 soft budget, reported rather than split (a split would not compile: the map, count and
    removed blocks move together). Full suite **1017 passed / 0 failed, 0 warnings** (was 1014; +3 tests).
- 2026-09-27: **B4 done** (this commit, `e348cbb`). The whole Borders panel reads in the selected
  language. The i18n section existed and was fully translated but DEAD: `borders.header_title` said
  "Visual Styles", the geometry/radius/gap descriptions said "Define line thickness (0-5px)" while the
  panel said something else, and only the preset titles/descs were reachable at all (through
  `scan.sh` -> `borders.presets.<stem>.title`, live at HEAD).
  - **Wiring.** Every user-visible string of `ui/panel/sections/BordersSection.slint` (55 labels, plus
    the ones the pane composes: "Active Border Colors (N slots)", "N of 8", the picker title) is bound
    to a new exported Slint global `BordersText` (`ui/panel/sections/borders_text.slint`, new), filled
    by `src/panel_i18n.rs::apply_borders` (new) from the same embedded map the shell strings use.
    Chosen over `window.set_*` because the tune pane is instantiated TWICE (two-column and stacked) and
    threading ~50 strings through MainWindow -> ShellRoot -> PanelRoot -> section is several hundred
    lines of boilerplate with no behaviour; exported globals are already this codebase's Rust<->Slint
    channel (`SkwdTokens` is read from Rust). The global's defaults ARE today's English copy, so a run
    that never calls the pass reads exactly as before. `main()` calls it once, pinned by
    `main_applies_the_borders_i18n_before_the_event_loop` — the B7 lesson: without the call-site test,
    deleting the call leaves the label tests green.
  - **Stale keys reconciled to the UI, keys unchanged**: `header_title` "Visual Styles" -> "Borders",
    `geometry.desc` -> the panel's real sentence, and so on; the only English that moved is
    `borders.presets.*` (below). `borders.gaps.title` / `gaps.desc` are left UNUSED on purpose: the
    panel has no "Gaps" heading (the four rows live under Geometry), they were unreachable before too,
    and deleting the keeper's translations is a bigger call than leaving them. Reported, not silently
    dropped.
  - **Preset names/descriptions**: the mechanism already existed (`scan.sh` emits
    `borders.presets.<stem>.title|desc`, `PresetInfo` carries it, `translate_presets` falls back to the
    raw `@Title`/`@Desc`), so B4's work was reconciling the values with what each `.lua` does — all 14
    had keys. Deliberate English changes: the `08_neon` name "Neon Breath" -> "Neon Flicker" (the file's
    `glitch` curve overshoots: it flickers, it does not breathe) and 13 of the 14 descriptions rewritten
    to match the file. The keeper's decision stands: the visible name is the i18n one, so `01_cascade`
    shows "Cascade"/"Cascada" while its `@Title` still says "Waterfall"; a test pins that the raw value
    never wins while a key exists, and that the stale values are still there as the fallback.
    **CORRECTED:** the rewrite of those 13 descriptions was a mistake — they were invented
    abstractions, not the presets' real text — and the descriptions were never painted at all.
    See the B4-correction record below.
  - **Final preset table** (file | EN | ES): `01_cascade` Cascade / Cascada; `02_diagonal` Diagonal /
    Diagonal; `03_duo` Duo Contrast / Dúo Contraste; `04_tri` Trident / Tridente; `05_spectrum` Spectrum
    / Espectro; `06_pulse` Pulse / Latido; `07_infinity` Infinity / Infinito; `08_neon` Neon Flicker /
    Parpadeo Neón; `09_glitch` Cyber Glitch / Cyber Glitch; `10_golden` Golden Luxury / Golden Luxury;
    `11_toxic` Toxic Green / Verde Tóxico; `12_neon_cyberpunk` Neon Cyber-Glow (Dual) / Neón Cyber-Glow
    (Dual); `13_the_joker` The Joker / El Joker; `14_looper` Looper / Looper.
  - **`13_the_joker.lua`**: five Spanish comments -> English, and `@Color: #39ff14` added (the only
    preset without one). The same leaked line (`-- fundido a invisible al perder foco`) sat in 12 and
    14; translated too, since the whole rationale is "Spanish comments in an English codebase". The new
    test pins `@Title`/`@Desc`/`@Color` and English-only comments for all 14 files.
  - **Bug found by that translation**: `border_preset::extract_glow` took the FIRST boundary-valid
    `shadow` token, so the English comment "-- Keep the original purple shadow." above the block made
    `13_the_joker` parse as glow-less — three existing tests went red (glow absent, tune-pane sync,
    fixtures) before the cause was known. Fixed at the root: `find_shadow_block` now walks past prose
    occurrences exactly like `find_colors_block` and `find_rule_block` already did, with
    `glow_is_found_when_a_comment_mentions_shadow_first` covering it. The Lua parser is
    comment-sensitive; that is now written down in the code.
  - **Tests** (+8, all RED-first in practice; the same-model mutation checks below were run and reverted):
    `borders_panel_labels_follow_the_language` (the 55-label EN/ES table read from `BordersText`, the
    composed sentences, EN/ES frame pixel diffs, and the ES -> EN round trip restoring the English frame
    byte for byte), `border_picker_title_follows_the_language` (picker open on the glow channel),
    `border_preset_names_and_descriptions_follow_the_language` (REAL `scan.sh` +
    `populate_presets`, both languages, plus the stale `@Title` fallback), `every_borders_label_has_its_own_spanish_text`,
    `the_borders_section_reads_every_label_from_the_text_global`, `main_applies_the_borders_i18n_before_the_event_loop`,
    `every_border_preset_declares_title_desc_and_color`, `glow_is_found_when_a_comment_mentions_shadow_first`.
    Nothing deleted, `#[ignore]`d or weakened; the 33 existing Borders render tests kept their exact
    English assertions and still pass.
  - **Mutation checks (evidence the tests are not decorative)**: hardcoding
    `text: "Geometry"` back left the global assertions GREEN — measured, not assumed — which is why
    `the_borders_section_reads_every_label_from_the_text_global` exists; with it added the same mutation
    FAILS. Making `apply_borders` a no-op FAILS the label test. Dropping `@Color` from 13 FAILS the
    fixture test.
  - **Why the accessible-text route was rejected**: `ElementQuery`/`ElementHandle` need Slint element
    debug info, and building with it ALSO disables the compiler's `optimize_useless_rectangles` pass, so
    the item tree under test stops being the tree the shipped binary renders (the B7 false-safety-net
    pattern). Measured: the 44 Borders PNGs are byte-identical either way, but the flag was reverted
    anyway; the tests read the global the section binds to, check statically that it binds all 55, and
    measure the frames.
  - Visual PNGs read with vision: `borders_i18n_en_top.png` / `borders_i18n_es_top.png` (Geometry rows,
    Angle, Inactive, colour strip: "Geometría" / "Grosor del Borde" / "Espacio Interno" / "Ángulo del
    Degradado" / "Colores del Borde Activo (2 ranuras)", longest description on one line, nothing
    clipped), `borders_i18n_en_save.png` / `borders_i18n_es_save.png` (glow group, save form and the
    keyboard hint: "Brillo (decoración de sombra)" / "Guardar como Preajuste" / "Nombre del preajuste…"
    / "Guardar" / "↑↓ navegar • ←→ cambiar de panel • Enter aplicar/activar • flechas ±1 • Esc volver" —
    the hint is the longest Spanish string in the frame and still ends well inside the pane),
    `borders_i18n_en_picker.png` / `borders_i18n_es_picker.png` ("Color personalizado — el color del
    brillo" with the colour field intact), `borders_i18n_en_presets.png` / `borders_i18n_es_presets.png`
    (the 14 card names: "Cascade" -> "Cascada", "Neon Flicker" -> "Parpadeo Neón", "Toxic Green" ->
    "Verde Tóxico"; "Waterfall" is nowhere), plus `slice_settled.png` from the mandated
    `slice_focus_flow_renders`, unchanged.
  - **B4 surfaced three non-i18n gaps, deliberately NOT started** (in Next step):
    `SavedPresetCard.apply-text` is declared and never painted, and the panel nav strip in
    `PanelRoot.slint:911` is still hardcoded English (shared chrome across all four sections).
    The third gap it named — "`border-descs` has no reader" — was real and is now CLOSED by the
    B4-correction below: that channel was exactly where the preset descriptions were supposed to
    reach the picker, and B4 left it dead.
  - Diff: 12 files, **+1221 / -119 authored lines (1340 total)**, of which 560 are the new test block and
    237 the two new files; above the ~400 soft budget, reported rather than split. Full suite
    **1025 passed / 0 failed, 0 warnings** (was 1017; +8 tests). B6 fence intact — the new tests use
    `TempEnv` and read `assets/`; no state-changing `hyprctl`, no reload.
- 2026-09-27: **B4 correction done** (this commit, `70c1874`). An independent adversarial review
  (GLM) returned FAIL on B4 with one required fix, and it was right: the translated preset
  DESCRIPTIONS never reached the UI. The finding was reproduced before any fix — emptying the
  `border-descs` model changed **0 pixels** of the picker (the first reduction of
  `border_preset_descriptions_are_painted_and_follow_the_language`), so the descriptions could only
  ever be asserted from the model, which is what B4 did.
  - **The dead channel.** `border-descs` travels `presets.rs` -> `main.slint` -> `shell.slint` ->
  `PanelRoot.slint` -> `BordersSection`, which declared the property and read it nowhere; the card
  the list renders, `SavedPresetCard`, had no description slot at all. B4's own record had already
  noticed this ("`border-descs` has no reader") and filed it as a follow-up instead of wiring it.
  - **The fix, one channel.** `SavedPresetCard` gains `in property <string> desc: ""` and paints it
    under the name (`if root.desc != ""`, `overflow: elide`). `BordersListPane` gains
    `builtin-descs` and binds `desc: i < root.builtin-descs.length ? root.builtin-descs[i] : ""`;
    both instantiations (two-column and stacked) forward `builtin-descs: root.border-descs`. No new
    channel was invented: the descriptions ride the same translated model the names already use.
    An empty description paints nothing, so Motion's cards and user presets are unchanged.
  - **Design note.** Routing them through `BordersText` (the static panel-copy global) was the other
    option the review offered. It was rejected: the descriptions are per-preset DATA with a
    `.lua @Desc` fallback, and `presets.rs::translate_presets` already resolves key-or-fallback per
    row. Re-encoding 14 descriptions as 14 static globals would drop that fallback for no gain.
  - **EN values reconciled.** 13 of the 14 EN descriptions B4 shipped were invented abstractions
    ("Soft vertical gradient that blends the primary colour into the surface colour.") instead of the
    presets' real text. They are now each file's own `@Desc`, so the i18n value and the fallback read
    the same; ES is their neutral translation. The one deliberate name change from B4 (`08_neon`
    "Neon Breath" -> "Neon Flicker") and every preset NAME are untouched — only the descriptions
    moved. Final table: `01` "Dynamic border with vertical gradient using the Noctalia palette.";
    `02` "Smooth gradient at a 45° angle using the Noctalia palette."; `03` "High contrast between
    Noctalia's Primary and Secondary color."; `04` "The perfect balance between Primary, Secondary and
    Tertiary."; `05` "The complete Noctalia color cycle (Static)."; `06` "Electrocardiogram effect. A
    pulse of color runs through the window when focused."; `07` "Fluid loop of Noctalia colors.
    Constant and elegant rotation."; `08` "Cyberpunk effect. The edge light flickers, moving back and
    forth like unstable electricity."; `09` "Aggressive digital glitch effect. Alert colors with
    ultra-fast rotation."; `10` "24k gold. An intense white reflection travels over a real gold
    surface."; `11` "Intense radioactive green with toxic flow effect."; `12` "Two-color glow
    simulation using a white/violet light base."; `13` "Joker Aesthetic: Acid green and deep purple
    with electric glow."; `14` "Looper Aesthetic: Noctalia colors with Joker structure and glow."
  - **The test that can fail on the physical claim** (`border_preset_descriptions_are_painted_and_follow_the_language`):
    (1) empty the description model -> the list MUST repaint (0 before the fix, thousands after);
    (2) keep the Spanish names, swap the descriptions to English -> MUST repaint again, so an
    untranslated description cannot hide behind a constant; (3) a source check that the model reaches
    the painted `Text` (`border-descs` -> section -> list -> card -> `text:`). The B4 test keeps its
    model assertions but its doc no longer claims the descriptions land in a component that reads
    them, because that was the false half.
  - **"Cascade" in Spanish — fixture artifact, VERDICT: names localise.** `borders_i18n_es_top.png`
    does show the built-in card as "Cascade" while the panel is Spanish, but that frame comes from
    `borders_tune_pane_fixture`, which hardcodes `border_titles = ["Cascade"]` as a SYNTHETIC card
    and never runs `scan.sh`; that test asserts PANEL copy, not preset names. The real scan path
    renders "Cascada" — read in `borders_i18n_es_presets_descs.png` ("Cascada", "Dúo Contraste",
    "Parpadeo Neón", "Neón Cyber-Glow (Dual)"). The fixture now carries a comment saying exactly that,
    so the frame stops reading as a localisation bug.
  - **Visual verification (PNGs read with vision, both languages):**
    `borders_i18n_es_presets_descs.png` — full Spanish picker: "Cascada / Borde dinámico con
    degradado vertical usando la paleta de Noctalia.", "Dúo Contraste / Alto contraste entre el color
    primario y el secundario de Noctalia.", "Parpadeo Neón / Efecto cyberpunk: la luz del borde
    parpadea de un lado a otro como electricidad inestable." — the longest description, one line,
    ending well inside the card, nothing clipped or overflowing; cards stay 56px.
    `borders_i18n_en_presets.png` — the same list in English, each line matching its `@Desc`
    ("Cascade / Dynamic border with vertical gradient using the Noctalia palette.", "Pulse /
    Electrocardiogram effect. A pulse of color runs through the window when focused.").
    `borders_i18n_es_presets_descs_blank.png` — Spanish names, NO descriptions (the pre-fix frame).
    `borders_i18n_es_presets_en_descs.png` — Spanish names with ENGLISH descriptions, the physical
    proof that the painted text follows the model's language.
  - Diff: 5 files, **+175 / -65 authored lines (240 total)** — i18n 56, tests 165, the two `.slint`
    files 19 — under the ~400 soft budget. Full suite **1026 passed / 0 failed, 0 warnings** (was
    1025; +1 test). B6 fence re-measured: SHA-256 of `~/.config/hve/config.json`,
    `~/.config/hypr/hyprland.lua`, all of `~/.cache/hve` and `assets/fragments/*.lua` byte-identical
    before/after a full suite.

## Next step

B1, B5, B6, B7, B2, B3 and B4 are done and B4's correction is in (`70c1874`); the full suite is
green, the picker PNGs are read in both languages. Two follow-ups B4 surfaced remain, neither of
them i18n wiring and neither started: `SavedPresetCard.apply-text` is declared and never painted,
and the panel nav strip ("Save / Borders / Motion / Filters / System", `ui/panel/PanelRoot.slint:911`)
is still hardcoded English — shared chrome across all sections, so it needs its own decision.
