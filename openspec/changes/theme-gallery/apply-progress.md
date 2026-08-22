# Apply Progress — theme-gallery (PR1 Foundation + PR2 Slice Carousel)

## Slice
PR1 Foundation — Phase 1 (7 tareas) — ThemeCard model + tokens — done
PR2 Slice Carousel — Phase 2 (6 tareas) — parallelogram 108↔768, flip, video, preheat — done

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
- Full suite: `cargo test` → 270 passed; 0 failed; 1 ignored
- Type check: `cargo check` → pass (Slint compile validated via forced SliceDelegate import)
- Engine sealed: no modifications to src/engine.rs, config.rs, settings.rs, theme_manager.rs, app_state.rs, watcher.rs, utils.rs, providers/*
- Window mutation preserved: Shell, SizePolicy, NavState intact; HVE 2 single window mutates only

## Commits (stacked-to-main, work-unit)
1. 3509699 — scaffold gallery module
2. 17bed84 — ThemeCard types + mapping
3. d07abca — ThemeGalleryModel + focus clamp S13
4. 7a8758d — ThemeCardDelegate + tokens verification
5. <pending> — feat(gallery): add SliceView carousel 108↔768 OutCubic handle_key flip video preheat + SliceDelegate skew28 shadow glow
   - Verification: `cargo test slice_*` 14 passed, `cargo check` pass

## Scope Guard
- No Hex/Mosaic (PR3) — hexagon.rs, mosaic.rs not touched
- No Slot/Apply (PR4) — slot.rs, main.rs registry not touched
- No engine/providers changes — sealed 8 paths untouched
- Visual styles 1:1 skwd-wall MIT, no GPL hyprmod code

## Next
PR3 Hex + Mosaic + Switch — HexagonView honeycomb r140, HexDelegate, parallax/video, Voronoi Lloyd3, MosaicView cache, kinetic 0.90 cloud 0.55/1.05, Chrome switch 200ms S6.
Budget: PR2 is 752 lines (501 slice.rs + 236 SliceDelegate + 15 mod.rs) exceeds 400 — size:exception recommended (tests included) or split SliceDelegate into separate commit within same stacked PR if reviewer prefers. Stacked-to-main: PR1 → PR2 → PR3 → PR4.

## Artifacts
- src/shell/gallery/mod.rs (re-export SliceView/GalleryView)
- src/shell/gallery/model.rs (PR1 unchanged)
- src/shell/gallery/views/mod.rs (GalleryView trait + Slice export)
- src/shell/gallery/views/slice.rs (PR2 core: width, handle_key, flip, hit-test, video, preheat constants)
- ui/gallery/ThemeCardDelegate.slint (PR1)
- ui/gallery/SliceDelegate.slint (PR2: parallelogram skew28, shadow glow, width anim 350 OutCubic, flip 400 InOutQuad, hit-test mask, video Timer 200ms, preheat 120ms sourceSize 400x720)
- ui/tokens.slint (verified, no change)
- openspec/changes/theme-gallery/tasks.md (PR2 2.1-2.6 checked)
