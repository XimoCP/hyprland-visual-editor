# Tasks: Theme Gallery (Module 2)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1450 |
| 400-line budget risk | High |
| Chained PRs recommended | Yes |
| Suggested split | PR1 Foundation → PR2 Slice → PR3 Hex+Mosaic → PR4 Slot+Apply (4 PRs) |
| Delivery strategy | auto-chain |
| Chain strategy | stacked-to-main |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: High

### Suggested Work Units

| Unit | Goal | Likely PR | Focused test command | Runtime harness | Rollback boundary |
|------|------|-----------|----------------------|-----------------|-------------------|
| 1 | Foundation ThemeCard+tokens | PR1 | `cargo test gallery::model` | N/A pure data | `gallery/`+`tokens.slint` |
| 2 | Slice carousel | PR2 | `cargo test gallery::slice` | `cargo run` Home Enter→nav | `slice.rs`+`SliceDelegate` |
| 3 | Hex+Mosaic+switch | PR3 | `cargo test gallery::views` | `cargo run` switch styles | `hexagon.rs`+`mosaic.rs`+ui |
| 4 | Slot+Apply+Polish | PR4 | `cargo test shell::gallery` | `cargo run` click→Esc→hide | `slot.rs`+`main.rs`+`Chrome` |

Stacked to `hve2-visual-rewrite`, merge in order.

---

## Phase 1 — Foundation (PR1 ~280)

- [x] 1.1 Scaffold gallery/mod.rs views/mod.rs exports [f: gallery/mod.rs] [t: `cargo test gallery -- --list`] [dep: -] [S]
- [x] 1.2 RED ThemeCard mapping empty/active/providers [f: model.rs] [t: `cargo test gallery_maps_empty` FAIL] [dep:1.1][S]
- [x] 1.3 GREEN ThemeCard types ColorScheme/Border [f: model.rs] [t: `cargo test gallery_active_flag`] [dep:1.2][S]
- [x] 1.4 ThemeGalleryModel Arc<Mutex<TM>> themes/focus/style [f: model.rs] [t: `cargo test gallery_focus_clamped`] [dep:1.3][S]
- [x] 1.5 Focus clamp Right/Left no-wrap S13 [f: model.rs] [t: `cargo test clamp_bounds`] [dep:1.4][S]
- [x] 1.6 Extend tokens fallback-accent #4fc3f7 [f: tokens.slint] [t: `cargo check`] [dep:-][S]
- [x] 1.7 ThemeCardDelegate image/dots/border/shader [f: ThemeCardDelegate.slint] [t: `cargo check`] [dep:1.1,1.6][S]

## Phase 2 — Slice Carousel (PR2 ~340)

- [x] 2.1 RED width 108↔768 350ms OutCubic [f: slice.rs] [t: `cargo test slice_width` FAIL] [dep:P1][S]
- [x] 2.2 GREEN SliceView GalleryView handle_key consumed [f: slice.rs] [t: `cargo test slice_key`] [dep:2.1][S]
- [x] 2.3 SliceDelegate parallelogram skew28 shadow glow [f: SliceDelegate.slint] [t: `cargo check`] [dep:2.2][M]
- [x] 2.4 Hit-test mask non-rect + flip 180 InOutQuad [f: slice.rs,SliceDelegate] [t: `cargo test slice_flip`] [dep:2.3][S]
- [x] 2.5 Video Timer 100-300ms release on blur S16/17 [f: slice.rs] [t: `cargo test slice_video`] [dep:2.4][S]
- [x] 2.6 Preheat 120ms sourceSize 400x720 cache async R8 [f: SliceDelegate] [t: `cargo check`] [dep:2.3][S]

## Phase 3 — Hex + Mosaic + Switch (PR3 ~380)

- [x] 3.1 HexagonView honeycomb r140 [f: hexagon.rs] [t: `cargo test hex_pos`] [dep:P2][S]
- [x] 3.2 HexDelegate hexagon path point-in-hex pull-out [f: HexDelegate.slint] [t: `cargo test hex_point`] [dep:3.1][M]
- [x] 3.3 Parallax+video on hex selected [f: hexagon.rs] [t: `cargo test hex_parallax`] [dep:3.2][S]
- [x] 3.4 RED Voronoi _buildTile Lloyd3 centroid [f: mosaic.rs] [t: `cargo test mosaic_centroid` FAIL] [dep:P2][M]
- [x] 3.5 GREEN MosaicView cache rebuild filter/count only [f: mosaic.rs] [t: `cargo test mosaic_cache`] [dep:3.4][S]
- [x] 3.6 Threat max64 warmup async guard [f: mosaic.rs] [t: `cargo test mosaic_warmup_64`] [dep:3.5][S]
- [x] 3.7 MosaicCell/View kinetic 0.90 cloud 0.55/1.05 [f: MosaicCell/View] [t: `cargo test mosaic_cloud`] [dep:3.5][M]
- [x] 3.8 Dual-stripe scroll staggered reveal + Chrome switch 200ms S6 [f: MosaicView,Chrome] [t: `cargo test switch_focus`] [dep:3.7][M]

## Phase 4 — Slot + Integration + Apply (PR4 ~360)

- [x] 4.1 GallerySlot Slot prewarm/cleanup [f: slot.rs] [t: `cargo test slot_mount`] [dep:P3][S]
- [x] 4.2 RED instant apply TM.apply two-pass [f: slot.rs] [t: `cargo test apply_calls` FAIL] [dep:4.1][S]
- [x] 4.3 GREEN pipeline reload watcher <200ms [f: slot.rs] [t: `cargo test apply_indicator`] [dep:4.2][S]
- [x] 4.4 No-op active pulse S8 + mutation ExpandToSettings size anim [f: slot.rs,size.rs] [t: `cargo test apply_noop`] [dep:4.3][S]
- [x] 4.5 Back/Esc collapse + sync_global_after_show S10/11 [f: slot.rs] [t: `cargo test back_collapse`] [dep:4.4][S]
- [x] 4.6 Register GallerySlot replace StubSlot i18n [f: main.rs] [t: `cargo test register_slot`] [dep:4.1][S]
- [x] 4.7 Keyboard Home→Expand arrow→apply S1/S2/S12 [f: callbacks.rs] [t: `cargo test home_expand`] [dep:4.6][S]

## Phase 5 — Polish & Verify (PR4 tail)

- [x] 5.1 Empty S18 + delete/rename S19/S20 [f: Chrome,slot.rs] [t: `cargo test empty_delete`] [dep:4.1][S]
- [x] 5.2 Threats shader flicker overlay + hyprmod yield S22 [f: slot.rs] [t: `cargo test shader_yield`] [dep:4.3][S]
- [x] 5.3 Reduced-motion S23 + MIT footer R7 + LRU200 R8 [f: Chrome,model.rs] [t: `cargo test reduced_lru`] [dep:P3][S]
- [x] 5.4 Headless mount Expand verify 22steps 350ms OutCubic [f: gallery tests] [t: `cargo test headless_expand`] [dep:4.6][S]

## Post-Verify Micro-Slice — PR4.5 Visible Gallery Wiring (pre-archive)

- [x] PV1 GalleryRoot.slint (Chrome + Slice/Hex/Mosaic stage) + main.slint import cleanup + nav_move gallery arrows (S13 clamp) + dead-code warnings to zero [f: ui/gallery/GalleryRoot.slint, ui/main.slint, ui/shell.slint, src/callbacks.rs, src/main.rs, src/shell/gallery/*] [t: `cargo check` 0 warnings, `cargo test` 299 pass, `cargo run` smoke OK] [dep:5.4][M]

Total: 32 tasks, 5 phases, 4 stacked PRs, <400/slice, TDD `cargo test`. (+1 post-verify PV1 wiring task)
