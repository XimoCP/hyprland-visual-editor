# Apply Progress — theme-gallery (PR1 Foundation + PR2 Slice Carousel + PR3 Hex/Mosaic)

## Slice
PR1 Foundation — Phase 1 (7 tareas) — ThemeCard model + tokens — done
PR2 Slice Carousel — Phase 2 (6 tareas) — parallelogram 108↔768, flip, video, preheat — done
PR3 Hex + Mosaic + Switch — Phase 3 (8 tareas) — honeycomb r140, HexDelegate, parallax, Voronoi Lloyd3, Mosaic cache/warmup, kinetic 0.90 cloud 0.55/1.05, dual-stripe + Chrome 200ms — done

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
- Full suite: `cargo test` → 287 passed; 0 failed; 1 ignored
- Type check: `cargo check` → pass (Slint compile ok; Hex/Mosaic slint standalone)
- Engine sealed: no modifications to src/engine.rs, config.rs, settings.rs, theme_manager.rs, app_state.rs, watcher.rs, utils.rs, providers/*
- Window mutation preserved: Shell, SizePolicy, NavState intact; HVE 2 single window mutates only

## Commits (stacked-to-main, work-unit)
1. 3509699 — scaffold gallery module
2. 17bed84 — ThemeCard types + mapping
3. d07abca — ThemeGalleryModel + focus clamp S13
4. 7a8758d — ThemeCardDelegate + tokens verification
5. 59c70d9 — feat(gallery): add SliceView carousel 108↔768 OutCubic handle_key flip video preheat + SliceDelegate skew28 shadow glow
   - Verification: `cargo test slice_*` 14 passed, `cargo check` pass
6. HEAD — feat(gallery): add Hexagon+Mosaic+Chrome switch (PR3 Hex&Mosaic)
   - Verification: `cargo test hex_*` 7 passed, `cargo test mosaic_*` 10 passed, `cargo test switch_focus` 2 passed, `cargo test gallery::views` 23 passed, `cargo test` 287 passed

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

## Scope Guard
- No Slot/Apply (PR4) — slot.rs, main.rs registry not touched
- No engine/providers changes — sealed 8 paths untouched
- Visual styles 1:1 skwd-wall MIT, no GPL hyprmod code

## Next
PR4 Slot + Integration + Apply — GallerySlot prewarm/cleanup, instant apply TM.apply two-pass S7 watcher <200ms, no-op pulse S8 + ExpandToSettings, Back/Esc S10/11, register slot main.rs, Home→Expand S1/S2/S12, polish empty S18 delete S19 shader flicker reduced-motion.
Budget: PR3 ~720 lines Rust (hexagon 280 + mosaic 420) + 280 Slint (HexDelegate+MosaicCell/View+Chrome) exceeds 400 — tests included; stacked-to-main: PR1 → PR2 → PR3 → PR4. Reviewer may split slint vs Rust into two commits inside same PR.

## Artifacts
- src/shell/gallery/mod.rs (re-export unchanged)
- src/shell/gallery/model.rs (PR1 unchanged)
- src/shell/gallery/views/mod.rs (now exports HexagonView/MosaicView + CHROME_SWITCH_DURATION_MS 200)
- src/shell/gallery/views/slice.rs (PR2 unchanged)
- src/shell/gallery/views/hexagon.rs (PR3: honeycomb r140, point-in-hex, pull-out 1.08 dash, parallax 0.08, video)
- src/shell/gallery/views/mosaic.rs (PR3: Voronoi Lloyd3 centroid, cache, warmup 64 guard, kinetic 0.90 cloud 0.55/1.05 stagger dual-stripe Chrome 200ms)
- ui/gallery/ThemeCardDelegate.slint (PR1)
- ui/gallery/SliceDelegate.slint (PR2)
- ui/gallery/HexDelegate.slint (PR3: hexagon Path, pull-out scale animate 200 OutCubic, parallax, video Timer, point-in-hex TouchArea)
- ui/gallery/MosaicCell.slint (PR3: shard Path, cloud-opacity, image-alpha stagger 40ms)
- ui/gallery/MosaicView.slint (PR3: dual-stripe tiling, kinetic 16ms animate, 6-cell placeholder Repeater)
- ui/gallery/GalleryChrome.slint (PR3: style switcher 3 buttons, cross-fade 200ms OutCubic, focused-index preserved S6)
- ui/tokens.slint (verified, no change)
- openspec/changes/theme-gallery/tasks.md (PR3 3.1-3.8 checked)
- openspec/changes/theme-gallery/apply-progress.md (this file merged)
