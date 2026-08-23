# Apply Progress — gallery-immersive-redesign

- Slice status
- PR1 Tokens & Fullscreen — tasks 1.1–1.8 done · PR1.1 + PR1.2 hotfixes done · 1.9 [V] visual check pending for the user (RE-RUN after PR1.2).
- PR2 Overlap Geometry, Snap, Wheel — tasks 2.1–2.10 done (10 work-unit commits) · PR2.HF hotfix done (borders + paint order) · 2.11 [V] visual check pending for the user (RE-RUN after PR2.HF).
- PR4 FilterBar & Thumbnails — tasks 4.1–4.8 done (9 work-unit commits) · 4.9 [V] visual check pending for the user.

## Completed

### Slice PR4 — FilterBar & Thumbnails (4.1–4.8 done · 4.9 [V] pending)

Commits: 50a491a → c48e0bf (see per-task list below).

- [x] 4.1 [WIRE] ui/gallery/FilterBar.slint NEW: invisible 24px top TouchArea strip reveals bar 250ms slide; pointer-exit arms 250ms one-shot auto-hide Timer; entering bar body cancels it; margin-top 30, maxWidth parent−20. Mounted as overlay in GalleryRoot one task early so build.rs compiles the new file (orphan .slint is invisible to cargo check). Commit 50a491a
- [x] 4.2 Skewed pills: Path parallelogram skew 10px, h24, cascade step 14px (−10px vertical overlap, manual placement — layouts can't overlap), active filled primary, 200ms fill/stroke/text cross-fade (legacy S6). Slice + Mosaic only (#632), legacy numbering 0/2 kept. Commit 51a17b6
- [x] 4.3 [WIRE] Stage cross-fade: views key off UI-side `shown-style` mirror lagging Rust style by one 200ms fade-out then commit+fade-in via Timer; mirror converges to Rust truth (no state desync); focused card preserved by existing clamp in on_gallery_style_selected. Commit dc5f7d6
- [x] 4.4 [WIRE] GalleryChrome → transparent overlay (empty banner + bottom-right MIT pill, 9px outline@0.6); toolbar + footer deleted; mit-opened chain → Rust xdg-open constant argv; unreachable Hex branch removed from GalleryRoot (#632; HexDelegate.slint file untouched). Stage declared first so overlays paint above it. Commit c21ff5e
- [x] 4.5 [RED] thumbs.rs cache-key + LRU prune contracts — stable per input, differ on path/mtime, 16-hex format; LRU bounded at 200 evicting beyond cutoff. RED via unimplemented!() stubs. Commit d1f969f
- [x] 4.6 [GREEN] cache_key (SipHash DefaultHasher over path+mtime), cover_geometry pure math (center crop to 400:720, integer-exact, never upscales: 1920×1080→crop 600×1080→out 400×720; 100×100 stays 55×100), generate() decode+resize(Triangle)+PNG write to $XDG_CACHE_HOME/hve/thumbs/<key>.png with disk-cache short-circuit. New dep image = "0.25". Commit 2a80848
- [x] 4.7 [WIRE] find_source_image (deterministic sorted extension scan of theme dir incl. provider subdirs), plan_jobs, preheat (one std::thread per job → invoke_from_event_loop marshals only the PNG PATH because slint::Image is NOT Send; UI side loads through Weak<MainWindow> upgrade). Stale-write guard re-checks row name before set_row_data. schedule_thumbs() runs on initial build + apply-refresh, skipping rows whose thumb.size().width > 0. GalleryCardData gains `thumb: image`. Sources discovered under config_dir/hve/themes/<name>/ (engine sealed — layout read-only). Commit 099b065
- [x] 4.8a SliceDelegate shimmer/fade: gate = preheat-done && thumb.width > 0; pulsing veil (Timer phase alternation, stops when ready) cross-fades to thumb 200ms OutCubic using persistent elements (no remount flash); 120ms PR2.HF.c preheat gate kept. SliceCarousel feeds card.thumb into all 7 paint passes. Commit 5a95111
- [x] 4.8b MosaicCell same shimmer/fade multiplied by staggered reveal alpha; MosaicView takes cards model and maps thumbs onto placeholder cells — out-of-range index yields DEFAULT empty image (verified Slint semantics) so cardless cells keep shimmering until Voronoi lands in 5.7. GalleryRoot passes cards. Commit c48e0bf

## Verification (PR4)

- `cargo test thumbs`: RED 0 passed / 5 failed (4.5) → 9 → **12 passed** (final)
- Full suite: `cargo test` → **337 passed, 0 failed** (+12 vs PR3 baseline 325)
- `cargo check` → clean except ONE PRE-EXISTING warning (`unused_imports` in src/shell/gallery/mod.rs re-export trim, present before this batch)

## Deviations / Notes for review (PR4)

- TouchArea has NO entered/exited callbacks in Slint 1.17 → hover-reveal uses `changed has-hover` (SliceDelegate precedent).
- GOTCHA (compiler panic): `animate opacity` directly on a ROOT-LEVEL conditional element panics i-slint-compiler 1.17.0 remove_unused.rs:99. The old Chrome's banner animated fine inside a VerticalLayout but panicked as a direct conditional child. Workaround: animation dropped (stage cross-fade covers transitions); bisected and documented.
- D10 pill text is a SHORT label ("skwd-wall (MIT, © liixini)") rather than the full mit-footer string; gallery-mit-footer prop now stops at ShellRoot (Rust still sets it upstream). Pill placement still flagged for user approval at PR4 review per design open question.
- Cross-fade implemented as fade-out(200ms)→commit→fade-in(200ms) via shown-style mirror; true simultaneous double-mount crossfade would need UI-owned style state (rejected per no-desync convention).
- Source discovery is a generic theme-dir scan, NOT per-provider wallpapers.json parsing (engine sealed; duplicating provider parsing out of scope). Themes without image files keep shimmer placeholders — pipeline degrades gracefully.
- Empty-banner 150ms opacity animate removed (panic workaround above).
- Unrelated pre-existing worktree changes untouched: assets/fragments/border.lua, .atl/*, untracked openspec proposal/design/specs files, stash@{0} hex experiments.

### PR1.1 Hotfix — fullscreen did not fire at startup (<80 lines)
- [x] PR1.1.a [RED] `initial_gallery_expand_mounts_gallery_screen` (main.rs tests, headless `init_no_event_loop`) — named-helper contract: dispatching the initial Expand(Gallery) mounts Gallery (`mounted_screen == 1`, expanded). RED verified: E0425 before helper existed
- [x] PR1.1.b [GREEN+WIRE] Extracted `dispatch_initial_gallery_expand(&Rc<RefCell<Shell>>)` in main.rs; REMOVED the setup-time dispatch (old line ~474) that ran BEFORE `composer::init_global` → `enter_gallery_session` was a silent no-op and the window stayed floating. The dispatch now fires from the 200ms post-show timer AFTER `ctrl.composer().focus()`: show → focus → expand(fullscreen session), so Hyprland has HVE mapped+focused when the fullscreen dispatch arrives
- [x] PR1.1.c Tiling-mode ordering decision (documented per orchestrator request): when `startup_tiling` is ON, `toggle_float()` runs FIRST and the expand fires in the SAME 600ms tick after it (float → fullscreen). Rationale: `exit_gallery_session` restores floating geometry, so sessions are designed to start from a floating window; toggling float AFTER fullscreen risks dropping the fullscreen flag in Hyprland. Non-tiling path: expand right at 200ms after focus+rules
- [x] PR1.1.d [WIRE] Tray-mode branch keeps an immediate initial expand so the first reveal still lands on Gallery (regression guard): composer dispatch is a harmless no-op while hidden (no mapped client → session stays inactive). Fullscreen-on-reveal for tray mode remains out of PR1.1 scope (noted risk below)
- [x] PR1.1.e [WIRE] FIX 3 — ui/main.slint window background now conditional: `mounted-screen == 1 ? transparent : HveColors.bg-dark`. Real per-pixel window transparency via `background: transparent` (same keyword pattern as shell.slint slot-area from 1.8); Hyprland/Wayland composites ARGB so the desktop shows through behind GalleryRoot's alpha-0 backdrop. If [V] shows a black/opaque window instead, fallback plan is an elegant semi-transparent dark scrim (documented limit)

### PR1.2 Hotfix — dead keyboard after gallery mount + shell paints over the live desktop (<80 lines)
- [x] PR1.2.a FIX B — ui/shell.slint ShellRoot background now conditional: `mounted-screen == 1 ? transparent : SkwdTokens.surface` (same proven ternary pattern as slot-area 1.8 and main.slint window bg from PR1.1). Root cause: the unconditional surface fill covers the WHOLE window (a Rectangle fills its bounds, including behind children), hiding MainWindow's transparent backdrop and GalleryRoot's alpha-0 scrim → user still saw a fullscreen opaque window
- [x] PR1.2.b FIX A — keyboard focus re-seed: `init => { shell-kbd.focus(); }` + `changed mounted-screen => { shell-kbd.focus(); }` on ShellRoot. Root cause: `forward-focus` only seeds focus when the window is CREATED; when Gallery mounts and the compositor mutates to fullscreen the FocusScope loses internal focus → keys dead until a mouse click. Syntax verified against Slint docs: `changed` callbacks fire queued on the next event-loop tick and only when the value actually changed — i.e. after the newly mounted subtree settled. Rust-side `focus_window()` reinforcement NOT needed: window-level focus is already handled by the composer dispatch chain (PR1.1); the defect was internal widget focus

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
- PR1.2 hotfix (slint-only): full suite `cargo test` → **308 passed, 0 failed** (no new Rust logic ⇒ no new tests) · `cargo build` + `cargo check` → clean, 0 warnings (build.rs compiles the .slint sources)

## Deviations / Notes for review
- Design D1's V5 spelling was an open question and is now RESOLVED and corrected (see 1.3): `hl.dsp.window.fullscreen` nested under `window.`, string modes, set/unset actions. V4 branch kept exactly per design fallback contract.
- Task 1.6 file list mentioned slot.rs but sessions live in Shell::process_queue hook pair (design D10 migration: "one hook pair in Shell::process_queue"); slot.rs untouched.
- Task 1.8 required one extra line outside its declared file: shell.slint slot-area background conditional — without it the opaque parent would hide the live desktop entirely.
- True desktop see-through ultimately depends on window-level translucency (MainWindow paints bg-dark). Deferred to 1.9 [V]: if the desktop is NOT visible at check time, next micro-step is making MainWindow background conditional when gallery is mounted.
- Unrelated pre-existing worktree changes left untouched/uncommitted: assets/fragments/border.lua, .atl/*, src/shell/gallery/mod.rs re-export trim.

# Slice PR2 — Overlap Geometry, Snap, Wheel (2.1–2.10 done · 2.11 [V] pending)

## Completed

### PR2.HF Hotfix — card borders lost + collapsed neighbors swallowed (<100 lines)
- [x] PR2.HF.a REGRESSION 1 (border lost): bound `stroke`/`stroke-width` on the SliceDelegate face Path (PR2.6/2.7 left it fill-only, `stroke: transparent`). skwd-wall contract restored: focused = `SkwdTokens.primary` width 3px; hovered = primary @40% (`transparentize(0.6)`) width 1px; idle = black @60% (`transparentize(0.4)`) width 1px. Stroke is safe from content overdraw: mask boxes are transparent and their opaque children sit ≥8px inside (VerticalLayout padding), so even the 1.5px inner half of the focused stroke stays visible
- [x] PR2.HF.b REGRESSION 2 (neighbors swallowed): SliceCarousel now renders TWO visible-gated passes over the same model — neighbors first (`visible: i != focused-index`), focused card LAST (`visible: i == focused-index`) so it overlaps each neighbor by exactly gap (−30px) instead of losing its expanded edges to right-of-focus siblings (child order = paint order in Slint; no z-index exists). Geometry audit: all cards already shared unconditional height 520 and y=0 on the centered row baseline — the defect was paint order only
- [x] PR2.HF.c Supporting tweak: delegate preheat Timer no longer gated on `root.visible` (`running: !preheat-done`) so the hidden overlay copy warms its thumb at startup — without it every focus change flashed the placeholder for 120ms
- [x] PR2.HF REGRESSION 3 (user screenshot, border cut along card edges): Slint clips Path paint to the Path's own geometry box; the face box was exactly the parallelogram bounding (viewbox 0,0,wn+sn,hn), so the stroke's outer half fell outside and was clipped. Fix gives the face a uniform stroke-room pad of 4px per side (> focused w3 half-width 1.5px): element origin at −pad,−pad, box +2·pad wide/tall, viewbox shifted to −p with size +2p — vertex scale stays 1:1 and every vertex maps back to its exact original position, so visual geometry is byte-identical; only the stroke gains room. Mask containers (clip:true safe-zone boxes) untouched — they clip content, not the stroke, and remain consistent automatically

## Pending
- [x] 2.1 [RED] `cum_offset` exact-px tests for focused=0/mid/last + geometry constants corrected to skwd values in slice.rs (collapsed 108→135, expanded 768→924, skew 28→35, added SLICE_HEIGHT 520 + SLICE_SPACING_PX −30); old constant assertions updated to the corrected contract. RED via 0.0 stub, commit 1946c58
- [x] 2.2 [GREEN] Pure `cum_offset(i, focused) = Σ_{j<i} width(j) + i·(−30)` + `card_width_at`, commit 6fb5388
- [x] 2.3 [RED] `snap_x` exact-px tests (center=500, focused 0..4) + pixel-centering invariant swept over 7 indices, commit f25c66c
- [x] 2.4 [GREEN] `snap_x = viewport_center − 462 − cum_offset(focused, focused)`, commit b3baf84
- [x] 2.5 [WIRE] New ui/gallery/SliceCarousel.slint: clipped stage, per-card absolute x from a closed-form cum-offset mirror, row container x bound to snap-x mirror with 350ms OutCubic animate, row vertically centered (520px). HorizontalLayout path dropped from GalleryRoot, commit 13b7d4d
- [x] 2.6 SliceDelegate real parallelogram: dynamic viewbox (width+skew)×520, vertices (35,0)(35+w,0)(w,520)(0,520), slant corners rounded with QuadraticTo and r clamped to min(token radius, w/2−1, slant/2−1); rect border/drop-shadow removed (PR3 replaces with Path shadow/dim/glow), commit 6f58757
- [x] 2.7 Face mask containers: clip:true boxes inset to the parallelogram safe zone [skew..width] over the same Path fill surface; true bitmap masking deferred to PR4 (thumbs.rs bakes parallelogram alpha into cached PNGs per design D8), commit 09cd055
- [x] 2.8 [RED] Wheel gesture machine tests: ±1 max per gesture (multi-notch bursts collapse to sign(dir)), clamped no-wrap both ends, dir=0 noop, empty model safe; free fn wheel_target contract test, commit e1629a6
- [x] 2.9 [GREEN] `wheel_target` signum+clamp implementation shared by SliceView::wheel_step and callbacks, commit c1df9f1
- [x] 2.10 [WIRE] gallery-wheel-step callback chain SliceCarousel → GalleryRoot → ShellRoot → MainWindow → callbacks.rs; scroll events only arm the gesture (last direction wins), 400ms debounce Timer commits one ±1 step after idle via tested wheel_target; wheel TouchArea sits UNDER the cards so clicks keep working (card touch areas reject scrolls which bubble down); container x animation from 2.5, commit 94177c3

## Pending
- [ ] 2.11 [V] USER VISUAL CHECK (RE-RUN after PR2.HF/HF2): `cargo run` → slices overlap −30px, all cards 520px tall vertically centered, 35px shear, radius 0; focused card expands to 924px and lands dead-center after the wheel idles; motion glides 350ms OutCubic; borders visible AND UNCUT on every card on all four edges (idle hairline / focused primary glow w3); collapsed neighbors peek on BOTH sides of the expanded card.

## Verification
- Focused runs per task recorded in each commit message (`cargo test slice`: 14→17→19→22→26 passed at each GREEN gate)
- Full suite: `cargo test` → **318 passed, 0 failed** · `cargo check` → clean, 0 warnings (after 2.10)
- PR2.HF hotfix (slint-only): full suite `cargo test` → **318 passed, 0 failed** (no new Rust logic ⇒ no new tests) · `cargo check` → clean (build.rs compiles the .slint sources)
- PR2.HF2 hotfix (slint-only): full suite `cargo test` → **318 passed, 0 failed** · `cargo check` → clean, 0 warnings. Chosen variant: grow the Path element box with origin offset + shifted viewbox (6 binding lines, zero vertex edits) instead of a second stroke-only overlay Path — one painted shape, no duplicate geometry to keep in sync

## Deviations / Notes for review
- "Per-card x fed from Rust positions" (task 2.5) implemented as token-driven pure Slint bindings mirroring the headless-tested slice.rs fns — same parity convention as hex/mosaic tasks 5.6/5.7. Rationale: feeding a positions array from Rust adds model-sync desync risk on every focused/model change; bindings cannot desync. Numbers flow from SkwdTokens only.
- Design D3 says "QuadTo"; the Slint element is spelled `QuadraticTo` (compiler-directed fix).
- Stale slice.rs constants (108/768/28) were replaced (not duplicated) as part of 2.1 — design Component Inventory assigns slice.rs the "135/924/520 constants"; existing tests asserting the old values were updated in the same commit.
- Wheel delta convention: scroll down = next card (winit LineDelta y<0). If the [V] check finds it inverted, flip one ternary in SliceCarousel.wheel-zone.
- Debounce semantics (D5 "400ms Timer commits the snap after idle"): events arm, idle commits ONE step — this is what makes "±1 max per gesture" true for trackpads/floods. Container x itself animates immediately on commit (350ms OutCubic retarget).
- ~~Right-of-focus cards paint OVER the focused card's trailing edge during PR2~~ RESOLVED in PR2.HF.b (two-pass visible-gated rendering, focused paints last). PR3's D4 runtime z-rank remains the proper long-term layering for shadows/dim.
- Unrelated pre-existing worktree changes still untouched: assets/fragments/border.lua, .atl/*, src/shell/gallery/mod.rs re-export trim, untracked openspec proposal/design/specs files.

# Slice PR3 — Depth Cues (3.1–3.6 done · 3.7 [V] pending)

## Completed

- [x] 3.1 [RED] Depth-cue formula tests in slice.rs: edge_fade_full_zone (min(0.6, 672/halfView) with degenerate guard), edge_norm_dist, fade_opacity (flat inside zone, linear →0 at normDist 1.2), card_center_x, dim_level (0/0.15/0.4), z_layer (2 current > 1 hovered > 0 idle). RED verified: E0425 before fns existed. Commit a2c7186
- [x] 3.2 [GREEN][WIRE] Pure fns implemented + SliceCarousel declarative mirrors (edge-full-zone / edge-norm-dist / fade-opacity / card-fade-opacity; SkwdTokens only); SliceDelegate binds root opacity to fade-opacity with 200ms animate — dissolves image+shadow+dim together at stage edges. Commit 27d0fdd
- [x] 3.3 [WIRE] Three visible-gated passes in declaration order (design D4): idle (`i != focused && i != hovered`) → hovered → current; exactly one live delegate per card; instances persist across focus/hover swaps (no remount). Hover routing: delegate `changed has-hover` → `hovered(bool)` callback → carousel hovered-index (−1 none). Entry-animation gating deferred to 5.5 (no entry animation exists yet — D4 risk note not applicable). Commit e9861d8
- [x] 3.4 Projected shadow: parallelogram command list extracted into local `SliceShape` Path component reused by shadow + face (single source of geometry per PR2.HF2 lesson); shadow declared FIRST (paints below), current x4/y10 α0.5 vs rest x2/y5 α0.3, black fill, 200ms on opacity/x/y. Commit f09d569
- [x] 3.5 Dim overlay: third SliceShape painted ABOVE front/back content containers, black alpha 0 current / 0.15 hover / 0.4 idle, 200ms; mirrors slice.rs dim_level ∘ card_state. Commit 2c85ef4
- [x] 3.6 Glow stroke cadence: stroke contract already live from PR2.HF/HF2 (current primary w3 / hover primary@0.4 w1 / idle black@0.6 w1); added 200ms animate on stroke + stroke-width (skwd ColorAnimation parity). Layer copies bind by state flags → no flash on visibility swaps. Commit 6107f76

## Pending

- [ ] 3.7 [V] USER VISUAL CHECK: `cargo run` → edge cards dissolve progressively toward the borders (flat inside fullZone, gone at normDist 1.2); current stacks above hovered above idle; shadow under every card (deeper on current); neighbors dimmed black 0.4, hovered 0.15, current clean; glow primary w3 on current.

## Verification

- `cargo test slice`: 24 → 31 passed (+7 depth-cue tests)
- Full suite: `cargo test` → **325 passed, 0 failed** · `cargo check` → **0 warnings** (after 3.6)

## Deviations / Notes for review

- Norm-dist computed on TARGET layout (focused index + closed-form cum-offset), NOT the animated row x — skwd reads live ListView contentX; ours would re-evaluate the curve every frame of the slide and the 200ms opacity animate already smooths transitions. Visual delta limited to the 350ms glide.
- 3.2 file list spillover: SliceDelegate.slint gained the `fade-opacity` property + root opacity binding because the binding target lives there.
- 3.3 file list spillover (inverse direction): the hover callback plumbing required a small SliceDelegate addition (changed handler + callback).
- fullZone numerator uses tokens (expW/2 + 2·(cardW+gap) = 462+210 = 672) — matches task's literal "(462+2·105)".
- Float midpoints asserted with 1e-5 epsilon; exact boundaries (zone edge =1.0, normDist 1.2 =0.0) hit early-return paths and stay exact-equality.
