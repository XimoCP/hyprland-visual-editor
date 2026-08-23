# Apply Progress — gallery-immersive-redesign (Slice PR1 Tokens & Fullscreen)

## Slice
PR1 Tokens & Fullscreen — tasks 1.1–1.8 done · PR1.1 hotfix done · 1.9 [V] visual check pending for the user (RE-RUN after PR1.1).

## Completed

### PR1.1 Hotfix — fullscreen did not fire at startup (<80 lines)
- [x] PR1.1.a [RED] `initial_gallery_expand_mounts_gallery_screen` (main.rs tests, headless `init_no_event_loop`) — named-helper contract: dispatching the initial Expand(Gallery) mounts Gallery (`mounted_screen == 1`, expanded). RED verified: E0425 before helper existed
- [x] PR1.1.b [GREEN+WIRE] Extracted `dispatch_initial_gallery_expand(&Rc<RefCell<Shell>>)` in main.rs; REMOVED the setup-time dispatch (old line ~474) that ran BEFORE `composer::init_global` → `enter_gallery_session` was a silent no-op and the window stayed floating. The dispatch now fires from the 200ms post-show timer AFTER `ctrl.composer().focus()`: show → focus → expand(fullscreen session), so Hyprland has HVE mapped+focused when the fullscreen dispatch arrives
- [x] PR1.1.c Tiling-mode ordering decision (documented per orchestrator request): when `startup_tiling` is ON, `toggle_float()` runs FIRST and the expand fires in the SAME 600ms tick after it (float → fullscreen). Rationale: `exit_gallery_session` restores floating geometry, so sessions are designed to start from a floating window; toggling float AFTER fullscreen risks dropping the fullscreen flag in Hyprland. Non-tiling path: expand right at 200ms after focus+rules
- [x] PR1.1.d [WIRE] Tray-mode branch keeps an immediate initial expand so the first reveal still lands on Gallery (regression guard): composer dispatch is a harmless no-op while hidden (no mapped client → session stays inactive). Fullscreen-on-reveal for tray mode remains out of PR1.1 scope (noted risk below)
- [x] PR1.1.e [WIRE] FIX 3 — ui/main.slint window background now conditional: `mounted-screen == 1 ? transparent : HveColors.bg-dark`. Real per-pixel window transparency via `background: transparent` (same keyword pattern as shell.slint slot-area from 1.8); Hyprland/Wayland composites ARGB so the desktop shows through behind GalleryRoot's alpha-0 backdrop. If [V] shows a black/opaque window instead, fallback plan is an elegant semi-transparent dark scrim (documented limit)

## Pending

### PR1 Tokens & Fullscreen
- [x] 1.1 [RED] FakeComposer session contract tests — `test_set_fullscreen_session_records_true_then_false` (exact sequence `set_fullscreen(true)`, `set_fullscreen(false)`) + `test_set_fullscreen_failure_returns_false_continues_windowed` (fallback-on-false), commit 046fb5e (RED verified: compile failure before trait method existed)
- [x] 1.2 [GREEN] `Composer::set_fullscreen(&self, on: bool) -> bool` + FakeComposer records calls, `with_fullscreen_result(ok)` ctor; false = continue windowed, commit 046fb5e
- [x] 1.3 [GREEN] Hyprland impl: focus HVE by title first, then constant dispatches. V5 spelling CORRECTED against installed 0.56.2 source (`src/config/lua/bindings/LuaBindingsDispatchers.cpp@v0.56.2`): real dispatcher is `hl.dsp.window.fullscreen({ mode = "fullscreen", action = "set" | "unset", window = "title:..." })` — NOT `hl.dsp.fullscreen({mode=1|0})`; numeric `mode=1` means MAXIMIZED in the Lua API. V4 fallback contract unchanged: `focuswindow` + `fullscreen 1|0`. Any failure → false. Live probe: `hyprctl dispatch 'hl.dsp.no_op()'` → ok (V5 confirmed on this machine). Commit 046fb5e
- [x] 1.4 [RED] Startup-sanity tests — 4 tests in `hyprland.rs`: stuck fullscreen yields `["fullscreen_off"]`, non-HVE fullscreen clients ignored, tiled adds `refloat` after off, healthy/absent/garbage JSON needs nothing. RED verified (missing fns). Commit b70e2a3
- [x] 1.5 [GREEN] `parse_stuck_state`/`repair_plan` pure core + `HyprlandComposer::startup_sanity()` executor; shared query path via extracted `clients_json()` used by `hve_in_special` too; wired as `composer::startup_fullscreen_sanity()` in main.rs BEFORE first show / tray branch. JSON shapes taken from live 0.56.2 output (`fullscreen`/`fullscreenClient` ints 0|1|2, `floating` bool). Commit b70e2a3
- [x] 1.6 [WIRE] `Controller::enter_gallery_session`/`exit_gallery_session` (+ `gallery_session_active`, failed enter leaves inactive); hooks fired in `Shell::process_queue` on Gallery mount/unmount through global controller; exit guarded by active-session (spec GIVEN); graceful no-op headless. Tests: `test_gallery_session_enter_exit_records_exact_sequence`, `test_gallery_session_failed_enter_stays_windowed`. Commits a4e49a5, fa4fb44 (guard refinement)
- [x] 1.7 tokens.slint swap fixed: h 924→520px, expanded-w 520→924px (`gallery-slice-expanded-w`), angle props replaced by `gallery-slice-spacing:-30px` + `gallery-slice-skew-offset:35px`, added `gallery-slice-radius:0px`. Existing token test asserted the OLD swapped values — updated to the corrected contract (acts as the fix guard). Commit 57a3ab2
- [x] 1.8 [WIRE] GalleryRoot root transparent + black backdrop scrim fading 0→`backdrop-alpha` (in-property, default 0 = skwd no-dim default) over anim-300 on mount (`init => opened=true`); VerticalLayout now explicit 100%×100% (sibling breaks lone-layout auto-fill); shell.slint slot-area paints transparent only under gallery (`mounted-screen == 1`). Commit b7fd438

## Pending
- [ ] 1.9 [V] USER VISUAL CHECK (re-run after PR1.1): `cargo run` → at startup Gallery mounts AND the window goes undecorated fullscreen over the live desktop (desktop visible through the transparent window); Esc/close restores prior float+geometry; simulate stuck start (fullscreen an HVE window, kill, relaunch) repairs to floating before first show.

## Verification
- `cargo test composer` → 21 passed (13 prior + 2 session contract + 2 controller session + 4 sanity)
- `cargo test ui_tests` → 7 passed (tokens corrected contract included)
- `cargo test shell::` → 136 passed (hook path exercised with global=None skip)
- Full suite: `cargo test` → **307 passed, 0 failed** · `cargo check` → **0 warnings** (PR1 baseline)
- PR1.1 hotfix: `cargo test initial_gallery_expand` → RED (E0425) then GREEN; full suite `cargo test` → **308 passed, 0 failed** (+1 helper test) · `cargo check` → **0 warnings**

## Deviations / Notes for review
- Design D1's V5 spelling was an open question and is now RESOLVED and corrected (see 1.3): `hl.dsp.window.fullscreen` nested under `window.`, string modes, set/unset actions. V4 branch kept exactly per design fallback contract.
- Task 1.6 file list mentioned slot.rs but sessions live in Shell::process_queue hook pair (design D10 migration: "one hook pair in Shell::process_queue"); slot.rs untouched.
- Task 1.8 required one extra line outside its declared file: shell.slint slot-area background conditional — without it the opaque parent would hide the live desktop entirely.
- True desktop see-through ultimately depends on window-level translucency (MainWindow paints bg-dark). Deferred to 1.9 [V]: if the desktop is NOT visible at check time, next micro-step is making MainWindow background conditional when gallery is mounted.
- Unrelated pre-existing worktree changes left untouched/uncommitted: assets/fragments/border.lua, .atl/*, src/shell/gallery/mod.rs re-export trim.
