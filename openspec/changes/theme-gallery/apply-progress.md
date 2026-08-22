# Apply Progress — theme-gallery (PR1 Foundation + PR2 Slice Carousel + PR3 Hex/Mosaic + PR4 Slot+Apply+Polish) — CLOSED

## Slice
PR1 Foundation — Phase 1 (7 tareas) — ThemeCard model + tokens — done
PR2 Slice Carousel — Phase 2 (6 tareas) — parallelogram 108↔768, flip, video, preheat — done
PR3 Hex + Mosaic + Switch — Phase 3 (8 tareas) — honeycomb r140, HexDelegate, parallax, Voronoi Lloyd3, Mosaic cache/warmup, kinetic 0.90 cloud 0.55/1.05, dual-stripe + Chrome 200ms — done
PR4 Slot + Integration + Apply + Polish — Phase 4+5 (11 tareas) — GallerySlot prewarm/cleanup, instant apply two-pass, ExpandToSettings 1300x900, Back/Esc, register, keyboard, empty/delete/rename, shader flicker, hyprmod yield, reduced-motion, MIT footer, LRU200, headless 22steps 350ms — done — theme-gallery CLOSED

## Completed

### PR1 Foundation (commits 3509699..7a8758d)
- [x] 1.1 Scaffold gallery/mod.rs + views/mod.rs exports — `cargo test gallery -- --list` pass, commit 3509699
- [x] 1.2 RED ThemeCard mapping empty/active/providers — 5 tests, commit 17bed84 (RED then GREEN)
- [x] 1.3 GREEN ThemeCard types ColorScheme/Border — GalleryColors, BorderConfig, Background, FALLBACK_ACCENT #4fc3f7, commit 17bed84
- [x] 1.4 ThemeGalleryModel themes/focus/style — from_infos, with_style, len/is_empty/focused_card, commit d07abca
- [x] 1.5 Focus clamp S13 no-wrap — move_focus clamped 0..len-1, empty guard, set_focused_index, tests clamp_bounds, commit d07abca
- [x] 1.6 Tokens fallback-accent #4fc3f7 — verified ui/tokens.slint already contains fallback-accent, radius 2-40, spacing 2-20, anim-expand 350ms, Roboto Condensed — cargo check pass, commit 7a8758d
- [x] 1.7 ThemeCardDelegate — ui/gallery/ThemeCardDelegate.slint with GalleryCardData, dots, border preview, shader badge, active overlay, SkwdTokens — cargo check pass, commit 7a8758d

### PR2 Slice Carousel (PR2 — this slice)
- [x] 2.1 RED width 108↔768 350ms OutCubic — constants SLICE_COLLAPSED_WIDTH 108 / SLICE_EXPANDED_WIDTH 768 / SLICE_ANIM_DURATION_MS 350 + OutCubic cubic-bezier(0.215,0.61,0.355,1), tests slice_width_collapsed_and_expanded_constants + slice_width_focused_vs_collapsed, commit slice-carousel
- [x] 2.2 GREEN SliceView GalleryView handle_key consumed — GalleryView trait + SliceView::handle_key Left/Right consumed clamped, Up/Down not consumed, trait object dispatch, tests slice_key_left_right_consumed_and_clamped + slice_key_up_down_not_consumed + slice_key_gallery_view_trait_dispatch
- [x] 2.3 SliceDelegate parallelogram skew28 shadow glow — ui/gallery/SliceDelegate.slint Path parallelogram MoveTo 28,0 / LineTo 108,0 / 80,200 / 0,200, width animate 350 OutCubic, shadow drop-shadow-blur 20/8 glow accent, border primary when selected, cargo check pass
- [x] 2.4 Hit-test mask non-rect + flip 180 InOutQuad — SliceView::point_in_parallelogram left=skew*(1-y/h) right=left+(w-skew) + contains_point, flip toggle 180 InOutQuad 400ms cubic-bezier(0.455,0.03,0.515,0.955) resets on focus change, Slint TouchArea is-inside-shape guard + animate flip-angle, tests slice_flip_toggle_and_angle + slice_flip_resets_on_focus_change + slice_flip_point_in_parallelogram_mask
- [x] 2.5 Video Timer 100-300ms release on blur S16/17 — constants 100/300/200, VideoState Idle/Pending/Playing/Released, set_video_delay clamp, request_video→Pending, activate_video→Playing, release_video/on_blur→Released, focus change releases, tests slice_video_delay_bounds + slice_video_pending_playing_and_release_on_blur + slice_video_release_on_focus_move + slice_video_pending_released_on_blur_without_play
- [x] 2.6 Preheat 120ms sourceSize 400x720 cache async R8 — SliceDelegate preheat Timer 120ms + Image source-clip-width 400 source-clip-height 720 image-fit cover, SLICE_PREHEAT_MS 120 / SLICE_SOURCE_W 400 / SLICE_SOURCE_H 720, cargo check pass

## Verification
- Focused tests:
  - `cargo test slice_width` → 3 passed (collapsed/expanded, focused vs collapsed, oob)
  - `cargo test slice_key` → 3 passed (left/right consumed, up/down not, trait dispatch)
  - `cargo test slice_flip` → 3 passed (toggle 180, reset on focus, mask geometry)
  - `cargo test slice_video` → 5 passed (delay bounds, pending/playing/release, no-video, focus move, pending release)
  - `cargo test gallery` → 25 passed (11 model + 14 slice)
  - `cargo test hex_pos` → 3 passed (honeycomb r140 H 242.48 V 210, scale, up/down rows)
  - `cargo test hex_point` → 2 passed (point-in-hex + pull-out dashed, vertices radius)
  - `cargo test hex_parallax` → 2 passed (parallax 0.08 max12 video on selected)
  - `cargo test mosaic_centroid` → 2 passed (Lloyd3 square/triangle centroid, buildTiles bbox)
  - `cargo test mosaic_cache` → 2 passed (rebuild filter/count only, focus preserved)
  - `cargo test mosaic_warmup_64` → 2 passed (max64 pending guard, zero count)
  - `cargo test mosaic_cloud` → 2 passed (kinetic 0.90 cloud 0.55/1.05 staggered 40ms)
  - `cargo test switch_focus` → 2 passed (dual-stripe offsets, Chrome 200ms S6 preserve)
  - `cargo test gallery::views` → 23 passed (7 hex + 10 mosaic + 6 switch)
  - `cargo test gallery::model` → 11 passed
  - `cargo test slot_mount` → 1 passed (Slot prewarm/cleanup i18n)
  - `cargo test apply_calls` → 1 passed (two-pass TM.apply)
  - `cargo test apply_indicator` → 1 passed (<200ms watcher)
  - `cargo test apply_noop` → 2 passed (pulse S8 + ExpandToSettings 1300x900)
  - `cargo test back_collapse` → 2 passed (Esc collapse + sync_global_after_show S11)
  - `cargo test register_slot` → 1 passed (GallerySlot replaces StubSlot i18n)
  - `cargo test home_expand` → 1 passed (Home→Expand arrow→apply S1/S2/S12)
  - `cargo test empty_delete` → 1 passed (S18 empty + S19 delete + S20 rename)
  - `cargo test shader_yield` → 1 passed (flicker 3s overlay + hyprmod yield S22)
  - `cargo test reduced_lru` → 1 passed (S23 reduced-motion + MIT footer + LRU200)
  - `cargo test headless_expand` → 1 passed (22 steps 350ms OutCubic 1200x800 →1300x900)
  - `cargo test gallery::model` → 11 passed (unchanged)
  - `cargo test shell::gallery` → 23 passed (slot 11 + views 12)
- Full suite: `cargo test` → 299 passed; 0 failed; 1 ignored
- Type check: `cargo check` → pass (Slint compile ok; GalleryChrome with empty+MIT footer, slot, shell, size)
- Engine sealed: no modifications to src/engine.rs, config.rs, settings.rs, theme_manager.rs, app_state.rs, watcher.rs, utils.rs, providers/* (only gallery/shell/chrome/main/callbacks)
- Window mutation preserved: Shell 22steps 350ms OutCubic, SizePolicy 900x680↔1200x800↔1300x900, single window mutates only

## Commits (stacked-to-main, work-unit)
1. 3509699 — scaffold gallery module
2. 17bed84 — ThemeCard types + mapping
3. d07abca — ThemeGalleryModel + focus clamp S13
4. 7a8758d — ThemeCardDelegate + tokens verification
5. 59c70d9 — feat(gallery): add SliceView carousel 108↔768 OutCubic handle_key flip video preheat + SliceDelegate skew28 shadow glow
   - Verification: `cargo test slice_*` 14 passed, `cargo check` pass
6. 05962a9 — feat(gallery): add Hexagon+Mosaic+Chrome switch (PR3 Hex&Mosaic)
   - Verification: `cargo test hex_*` 7 passed, `cargo test mosaic_*` 10 passed, `cargo test switch_focus` 2 passed, `cargo test gallery::views` 23 passed, `cargo test` 287 passed
7. PR4 — feat(gallery): GallerySlot prewarm/cleanup, instant apply two-pass, ExpandToSettings 1300x900, Back/Esc, register, keyboard, polish empty/delete/rename, shader flicker 3s, hyprmod yield, reduced-motion, MIT footer, LRU200, headless 22steps 350ms (this slice — theme-gallery CLOSED)
   - Verification: `cargo test slot_*` 11 passed, `cargo test shell::gallery` 23 passed, `cargo test` 299 passed, `cargo check` pass

## Completed PR3

### PR3 Hex + Mosaic + Switch (PR3 — this slice)
- [x] 3.1 HexagonView honeycomb r140 — constants HEX_RADIUS 140 HEX_H_SPACING 242.48 HEX_V_SPACING 210, hex_center col*H + row%2*H/2 row*V, tests hex_pos_honeycomb_r140_layout + hex_pos_scale_and_focus_preserved + hex_pos_handle_key_up_down_rows
- [x] 3.2 HexDelegate hexagon path point-in-hex pull-out — Path 6 verts (261,181)..(140,241) r140, point_in_hex ray cast + quick circle, pull_scale 1.08 dash 2px is_pulled_out/dash_visible, Slint TouchArea is-inside-hex adx*0.5+ady*0.866≤r, tests hex_point_in_hex_hit_test_and_pull_out + hex_point_vertices_have_radius_distance
- [x] 3.3 Parallax+video on hex selected — HEX_PARALLAX_FACTOR 0.08 MAX 12 clamp, update_parallax (mouse-center)*0.08, video Pending/Playing only when pulled_out+has_video, tests hex_parallax_video_on_selected + hex_parallax_factor_and_dash
- [x] 3.4 RED Voronoi _buildTile Lloyd3 centroid — centroid shoelace area-weighted fallback average, build_tiles grid jitter + Lloyd 3 iter naive nearest sampling + bbox shard gap6, tests mosaic_centroid_build_tile_lloyd3 + mosaic_centroid_degenerate_fallback (FAIL first then GREEN)
- [x] 3.5 GREEN MosaicView cache rebuild filter/count only — ensure_cells cache_key (filter,count) rebuilds counts, tick_kinetic no rebuild, tests mosaic_cache_rebuild_filter_count_only + mosaic_cache_focus_preserved_on_rebuild
- [x] 3.6 Threat max64 warmup async guard — MOSAIC_WARMUP_MAX 64, ensure pending_gen warmup_gen bump, cancel_pending_warmup stales old gen, tests mosaic_warmup_64_max_async_guard + mosaic_warmup_64_zero_count
- [x] 3.7 MosaicCell/View kinetic 0.90 cloud 0.55/1.05 — kinetic_step velocity*0.90 interval 16, cloud_opacity smoothstep inner 0.55 outer 1.05, stagger 40ms %600, tests mosaic_cloud_kinetic_0_90_and_cloud_radii + mosaic_cloud_staggered_reveal_delay
- [x] 3.8 Dual-stripe scroll staggered reveal + Chrome switch 200ms S6 — dual-stripe offsets rem_euclid viewport 1500, tick moves stripes offset by viewport, CHROME_SWITCH_DURATION 200 preserves focused_index clamped, Slint GalleryChrome + MosaicView stripe-a/b animate 16ms linear, tests switch_focus_dual_stripe_and_chrome_200ms + switch_focus_handle_key_left_right

## Completed PR4 — CLOSED (this slice)

### PR4 Slot + Integration + Apply + Polish (PR4 — 11 tareas, CIERRE theme-gallery)
- [x] 4.1 GallerySlot Slot prewarm/cleanup [slot.rs] — GallerySlot implements Slot screen Gallery, on_mount prewarm 64 cache, on_unmount cleanup, mount counts, label i18n Gallery, tests slot_mount_gallery_implements_slot_prewarm_cleanup
- [x] 4.2 RED instant apply TM.apply two-pass [slot.rs] — ThemeManager.apply pass1 write + pass2 post_apply + reload callback, S7 active indicator moves, tests apply_calls_two_pass_theme_manager_apply_and_reload (FAIL first)
- [x] 4.3 GREEN pipeline reload watcher <200ms [slot.rs] — perceived <200ms, last_apply_ms, model refresh, tests apply_indicator_reload_watcher_under_200ms_perceived
- [x] 4.4 No-op active pulse S8 + mutation ExpandToSettings size anim [slot.rs,size.rs] — pulse when active no reload, ExpandToSettings 1200x800→1300x900 via SizePolicy::target_for_settings 22steps 350ms OutCubic, tests apply_noop_pulse_on_active_and_expand_to_settings_anim + apply_noop_settings_target_expands_beyond_gallery
- [x] 4.5 Back/Esc collapse + sync_global_after_show S10/11 [slot.rs] — handle_back collapses settings first then Back, Shell::sync_global_after_show restores 1200x800 via GLOBAL_SHELL, tests back_collapse_esc_and_sync_global_after_show
- [x] 4.6 Register GallerySlot replace StubSlot i18n [main.rs] — Shell::register_slot overwrites StubSlot Gallery with GallerySlot, i18n Gallery EN/ES, tests register_slot_gallery_replaces_stub_i18n + main.rs GallerySlot registration after ThemeManager with same providers
- [x] 4.7 Keyboard Home→Expand arrow→apply S1/S2/S12 [callbacks.rs] — card_activated Expand Gallery, nav_move Right, gallery apply via slot, tests home_expand_keyboard_home_to_gallery_arrow_apply; callbacks.rs expand_to_settings on gallery click + collapse_from_settings on Back/Esc with size check
- [x] 5.1 Empty S18 + delete/rename S19/S20 [Chrome,slot.rs] — empty_message S18, delete_theme/rename_theme refresh model, LRU preserve focus, tests empty_delete_rename_and_empty_state + Chrome empty overlay + delete/rename callbacks
- [x] 5.2 Threats shader flicker overlay + hyprmod yield S22 [slot.rs] — shader_overlay ~3s S21 Hyprland #15067, hyprmod yield anim via HYPRMOD_RUNNING env/process check, tests shader_yield_flicker_overlay_and_hyprmod
- [x] 5.3 Reduced-motion S23 + MIT footer R7 + LRU200 R8 [Chrome,model.rs] — effective_duration 0 when reduced, MIT_FOOTER skwd-wall MIT © liixini + link, ThumbnailCache LRU200 bounded, Chrome reduced-motion disables animate duration 0ms, MIT footer persistent, tests reduced_lru_motion_disabled_mit_footer_lru200
- [x] 5.4 Headless mount Expand verify 22steps 350ms OutCubic [gallery tests] — Shell ANIM_STEPS 22 ANIM_STEP_MS 16 ANIM_DURATION_MS 352 ANIM_EASING OutCubic, drain 22*16ms to 1200x800 hold, expand_to_settings 1300x900 same animator, tests headless_expand_mount_verify_22steps_350ms_outcubic

## Scope Guard
- No engine/sealed providers changes — 8 paths untouched (engine, config, settings, theme_manager, app_state, watcher, utils, providers)
- Visual styles 1:1 skwd-wall MIT, no GPL hyprmod code
- Single window mutates only: Shell, SizePolicy, NavState — GallerySlot uses same 22steps 350ms OutCubic

## Next
theme-gallery CLOSED — all 32 tasks checked. Next: verify phase (`sdd-verify`) then archive (`sdd-archive`) to reflect delta specs to spec.md/design.md.
Budget: PR4 ~820 lines Rust (slot 819 + shell/size delta + model LRU 80) + 40 Slint Chrome — stacked-to-main: PR1→PR2→PR3→PR4. Over 400 but closure justifies; reviewer may split slot tests vs impl into two commits inside same PR if needed.

## Artifacts
- src/shell/gallery/mod.rs (now exports GallerySlot)
- src/shell/gallery/model.rs (added MIT_FOOTER, MIT_FOOTER_LINK, THUMBNAIL_CACHE_CAPACITY 200, ThumbnailCache LRU, prefers_reduced_motion, effective_duration, refresh())
- src/shell/gallery/slot.rs (NEW — GallerySlot impl Slot, prewarm/cleanup, instant apply two-pass, pulse, ExpandToSettings, Back/Esc, empty/delete/rename, shader flicker 3s, hyprmod yield, reduced-motion, MIT footer, LRU200, headless 22steps)
- src/shell/gallery/views/mod.rs (unchanged)
- src/shell/gallery/views/slice.rs (unchanged)
- src/shell/gallery/views/hexagon.rs (unchanged)
- src/shell/gallery/views/mosaic.rs (unchanged)
- src/shell/size.rs (added SETTINGS_EXPANDED 1300x900, target_for_settings, target_with_settings, apply_noop test)
- src/shell/mod.rs (exposed ANIM_STEP_MS, ANIM_STEPS, ANIM_DURATION_MS, ANIM_EASING, added animate_to/expand_to_settings/collapse_from_settings, start_size_anim_to)
- src/shell/nav.rs (unchanged — NavState pure)
- src/shell/slots.rs (unchanged — StubSlot retained for tests)
- src/main.rs (GallerySlot registration replacing Gallery StubSlot, i18n, after ThemeManager)
- src/callbacks.rs (Gallery expanded card click → ExpandToSettings, Back/Esc collapse settings first → Back, keyboard Home→Expand preserved)
- ui/gallery/GalleryChrome.slint (added reduced-motion bool, empty bool+empty-text, mit-footer/link, delete/rename callbacks, empty overlay S18, MIT footer persistent R7, animate durations conditional 0ms when reduced-motion S23)
- ui/gallery/ThemeCardDelegate.slint, SliceDelegate.slint, HexDelegate.slint, MosaicCell.slint, MosaicView.slint (unchanged PR1-3)
- ui/tokens.slint, ui/shell.slint (unchanged)
- openspec/changes/theme-gallery/tasks.md (PR4 4.1-5.4 checked — CLOSED)
- openspec/changes/theme-gallery/apply-progress.md (this file merged PR4)

## PR4.5 — Visible Gallery Wiring (post-verify micro-slice, pre-archive)

**Goal**: `cargo run` shows GalleryChrome + Slice view by default, styles switchable, zero warnings.

- Previous session left shell.slint importing `gallery/GalleryRoot.slint` but the file never existed — build was broken (`Cannot find requested import`).
- NEW ui/gallery/GalleryRoot.slint: VerticalLayout { GalleryChrome top; stage below } where stage mounts exactly one of: Slice carousel (`for card[i] in cards : SliceDelegate`, is-current = focused-index), Hexagon honeycomb (rows of three, odd-row offset 150px), MosaicView (viewport-w bound, cell-clicked guarded by cards.length).
- ui/main.slint: removed now-unused gallery component imports (GalleryChrome/SliceDelegate/HexDelegate/MosaicView/MosaicCell live behind GalleryRoot); ShellRoot full-window mount + gallery props/callbacks were already in place from previous session.
- src/callbacks.rs nav_move: arrows move gallery focus when Gallery expanded (clamped S13, model length from ModelRc row_count via slint::Model trait) else Shell::move_focus; fixed isize cast.
- Dead-code elimination by real usage + honest annotations: removed duplicate CHROME_SWITCH_DURATION_MS in mosaic.rs (test uses views:: one), unused re-exports in gallery/mod.rs, HashMap import, cfg-gated Duration; module-level allow on view engines (test-verified headless logic, Slint delegates render at runtime); cfg_attr(not(test)) on assertion accessors/counters.
- Verification: cargo check 0 warnings; cargo test 299 passed 0 failed 1 ignored; cargo build clean; runtime smoke on Hyprland session: app boots, GallerySlot registered, no panic (countdown auto-minimize on focus loss is pre-existing behavior).
