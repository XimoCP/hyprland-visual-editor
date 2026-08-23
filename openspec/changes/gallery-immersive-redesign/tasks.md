# Tasks: Immersive Gallery Redesign (gallery-immersive-redesign)

Tags: [RED] failing test first · [GREEN] minimal impl · [WIRE] connect UI/Rust · [V] visual check (`cargo run`, eyeball the individual change). Size S<50 / M≤80 changed lines. Dep = prerequisite. Runner: `cargo test`; `.slint`-only tasks gate on `cargo check`.

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1500–1800 across 5 slices |
| 400-line budget risk | High |
| Chained PRs recommended | Yes |
| Suggested split | PR1 tokens+fullscreen → PR2 overlap+snap → PR3 depth → PR4 filterbar+thumbs → PR5 flip+micro |
| Delivery strategy | auto-chain |
| Chain strategy | stacked-to-main |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: High

### Suggested Work Units

| Unit | Goal | Likely PR | Focused test command | Runtime harness | Rollback boundary |
|------|------|-----------|----------------------|-----------------|-------------------|
| 1 | Fullscreen lifecycle + fixed tokens + backdrop | PR1 | `cargo test composer && cargo check` | `cargo run`: opens fullscreen over live desktop; close restores float | revert = floating-only launch (one hook pair + token constants) |
| 2 | Overlap geometry + center snap + wheel | PR2 | `cargo test slice` | `cargo run`: wheel ±1, current snaps pixel-centered | revert → static HorizontalLayout row |
| 3 | Fade, layer stack, shadows, dim, glow | PR3 | `cargo test slice` | `cargo run`: depth cues at skwd values | revert → flat opaque row |
| 4 | FilterBar + real thumbnails + MIT pill | PR4 | `cargo test thumbs && cargo check` | `cargo run`: strip reveals bar; thumbs render | revert → permanent toolbar returns |
| 5 | Y-flip sheet + micro + hex/mosaic parity | PR5 | `cargo test` (full) | `cargo run`: right-click flips; hex arcs | revert → crossfade pseudo-flip |

### Slice PR1 — Tokens & Fullscreen (1.1–1.9)

- [x] 1.1 [RED] [f: src/composer/mod.rs tests] [t: cargo test composer] [dep:-] [S] Failing FakeComposer test: session enter records `set_fullscreen(true)`, exit `(false)`; on dispatch failure returns false and UI continues windowed (threat-matrix case: exact dispatch sequence + fallback).
- [x] 1.2 [GREEN] [f: src/composer/mod.rs] [t: cargo test composer] [dep:1.1] [S] Add `fn set_fullscreen(&self, on: bool) -> bool` to `Composer` trait; FakeComposer records calls.
- [x] 1.3 [GREEN] [f: src/composer/hyprland.rs] [t: cargo test composer] [dep:1.2] [S] Hyprland impl: focus HVE by title first, then constant dispatches — V5 `hl.dsp.fullscreen {mode = 1\|0}`, V4 `focuswindow` + `fullscreen 1\|0`; any failure → false. (Verify exact V5 Lua spelling against installed Hyprland during this task.)
- [x] 1.4 [RED] [f: src/composer/hyprland.rs tests] [t: cargo test composer] [dep:-] [S] Failing startup-sanity test: clients JSON showing HVE stuck fullscreen → expect repair dispatches (`fullscreen 0` + float restore).
- [x] 1.5 [GREEN] [f: src/composer/hyprland.rs] [t: cargo test composer] [dep:1.4] [M] Implement sanity check parsing `hyprctl clients -j` (same code path as `hve_in_special`); runs before first show.
- [x] 1.6 [WIRE] [f: src/shell/mod.rs, src/shell/gallery/slot.rs] [t: cargo test] [dep:1.2] [M] `Controller::enter_gallery_session`/`exit_gallery_session`; mount/unmount hooks fire them on Expand(Gallery)/Back.
- [x] 1.7 [f: ui/tokens.slint] [t: cargo check] [dep:-] [S] Fix swap: gallery-slice-h=520px, gallery-slice-expanded-w=924px; replace angle props with gallery-slice-spacing:-30px, gallery-slice-skew-offset:35px, radius 0.
- [x] 1.8 [WIRE] [f: ui/gallery/GalleryRoot.slint] [t: cargo check] [dep:1.7] [S] Transparent backdrop rect α0 (live desktop visible) with 300ms fade-in on open.
- [ ] 1.9 [V] [t: cargo run] [dep:1.1–1.8] [S] Verify: undecorated fullscreen launch; Esc/close restores prior float; crash-repair works after simulated stuck start.

### Slice PR2 — Overlap Geometry, Snap, Wheel (2.1–2.11)

- [x] 2.1 [RED] [f: src/shell/gallery/views/slice.rs tests] [t: cargo test slice] [dep:1.7] [S] Failing `cum_offset` tests: widths 135/924, spacing −30; exact px assertions for focused=0/mid/last.
- [x] 2.2 [GREEN] [f: src/shell/gallery/views/slice.rs] [t: cargo test slice] [dep:2.1] [S] Implement pure `cum_offset(i, focused)`.
- [x] 2.3 [RED] [f: src/shell/gallery/views/slice.rs tests] [t: cargo test slice] [dep:2.2] [S] Failing snap tests: `snap_x = viewport_center − 924/2 − cum_offset(focused)`; current card pixel-centered for every index.
- [x] 2.4 [GREEN] [f: src/shell/gallery/views/slice.rs] [t: cargo test slice] [dep:2.3] [S] Implement snap_x.
- [x] 2.5 [WIRE] [f: ui/gallery/SliceCarousel.slint (new)] [t: cargo check] [dep:2.4] [M] Carousel host: clipped stage, absolute per-card x fed from Rust positions; drop HorizontalLayout path from GalleryRoot.
- [x] 2.6 [f: ui/gallery/SliceDelegate.slint] [t: cargo check] [dep:2.5] [M] Parallelogram Path, viewbox (0..959 × 0..520), vertices (35,0)(959,0)(924,520)(0,520); `QuadTo`-rounded slant corners with clamped r.
- [x] 2.7 [f: ui/gallery/SliceDelegate.slint] [t: cargo check] [dep:2.6] [S] Mask wallpaper Image via `clip:true` container whose background is the same Path fill.
- [x] 2.8 [RED] [f: src/shell/gallery/views/slice.rs tests] [t: cargo test slice] [dep:-] [S] Failing wheel state machine: `wheel_step(dir)` advances ±1 max per gesture, clamped, no wrap.
- [x] 2.9 [GREEN] [f: src/shell/gallery/views/slice.rs] [t: cargo test slice] [dep:2.8] [S] Implement wheel_step.
- [x] 2.10 [WIRE] [f: ui/gallery/SliceCarousel.slint, src/callbacks.rs] [t: cargo check] [dep:2.9] [S] `gallery-wheel-step` callback + 400ms debounce Timer commits snap; container x animates 350ms OutCubic.
- [ ] 2.11 [V] [t: cargo run] [dep:2.1–2.10] [S] Verify: slices overlap −30, 35px shear, radius 0; current lands dead-center after wheel idle.

### Slice PR3 — Depth Cues (3.1–3.7)

- [ ] 3.1 [RED] [f: src/shell/gallery/views/slice.rs tests] [t: cargo test slice] [dep:2.4] [S] Failing fade tests: fullZone = min(0.6,(462+2·105)/halfView); opacity linear → 0 at normDist 1.2.
- [ ] 3.2 [GREEN][WIRE] [f: src/shell/gallery/views/slice.rs, ui/gallery/SliceCarousel.slint] [t: cargo test slice] [dep:3.1] [S] Implement edge-fade fn; bind per-delegate opacity with 200ms.
- [ ] 3.3 [WIRE] [f: ui/gallery/SliceCarousel.slint] [t: cargo check] [dep:3.2] [M] Three conditional paint layers in declaration order — idle (`!current && !hovered`) → hovered → current; exactly one live delegate per card; entry animation gated to first mount.
- [ ] 3.4 [f: ui/gallery/SliceDelegate.slint] [t: cargo check] [dep:3.3] [S] Shadow: identical Path declared first — current x4/y10 α0.5, rest x2/y5 α0.3, 200ms.
- [ ] 3.5 [f: ui/gallery/SliceDelegate.slint] [t: cargo check] [dep:3.3] [S] Dim overlay black: 0 current / 0.15 hover / 0.4 idle, 200ms.
- [ ] 3.6 [f: ui/gallery/SliceDelegate.slint] [t: cargo check] [dep:3.4] [S] Glow stroke: current primary w3 / hover primary@0.4 w1 / idle black@0.6 w1, 200ms.
- [ ] 3.7 [V] [t: cargo run] [dep:3.1–3.6] [S] Verify: edges dissolve, current stacks above hovered above idle; shadow/dim/glow match skwd numbers.

### Slice PR4 — FilterBar & Thumbnails (4.1–4.9)

- [ ] 4.1 [f: ui/gallery/FilterBar.slint (new)] [t: cargo check] [dep:1.8] [M] Top-center bar (margin 30, maxWidth parent−20): invisible 24px TouchArea strip reveals it 250ms; pointer-exit arms 250ms auto-hide Timer.
- [ ] 4.2 [f: ui/gallery/FilterBar.slint] [t: cargo check] [dep:4.1] [S] Skewed pills: skew 10px, h24, −10px vertical overlap, active filled primary.
- [ ] 4.3 [WIRE] [f: ui/gallery/FilterBar.slint, ui/gallery/GalleryRoot.slint] [t: cargo check] [dep:4.2] [S] Style-switch cross-fade 200ms preserving focused card (selection state moves out of old toolbar).
- [ ] 4.4 [WIRE] [f: ui/gallery/GalleryChrome.slint] [t: cargo check] [dep:4.3] [S] Delete permanent toolbar + MIT footer; add bottom-right MIT pill (9px outline@0.6, opens `mit-link`). (Confirm placement with user at PR4 review.)
- [ ] 4.5 [RED] [f: src/shell/gallery/thumbs.rs tests (new)] [t: cargo test thumbs] [dep:-] [S] Failing: cache key = hash(source path + mtime); LRU 200 prune semantics.
- [ ] 4.6 [GREEN] [f: src/shell/gallery/thumbs.rs] [t: cargo test thumbs] [dep:4.5] [M] Decode (dep `image = "0.25"`), cover-crop + scale ≤400×720, write PNG to `$XDG_CACHE_HOME/hve/thumbs/<hash>.png`.
- [ ] 4.7 [WIRE] [f: src/shell/gallery/thumbs.rs, src/shell/gallery/model.rs] [t: cargo test thumbs] [dep:4.6] [M] Generation off-thread (`std::thread`); results marshaled via `slint::invoke_from_event_loop` into `GalleryCardData.thumb`.
- [ ] 4.8 [f: ui/gallery/SliceDelegate.slint, ui/gallery/HexDelegate.slint, ui/gallery/MosaicCell.slint] [t: cargo check] [dep:4.7] [S] Skeleton shimmer until `thumb.width > 0`; 200ms OutCubic fade-in.
- [ ] 4.9 [V] [t: cargo run] [dep:4.1–4.8] [S] Verify: toolbar gone; hover strip reveal/auto-hide; real thumbnails in all three views.

### Slice PR5 — Flip, Micro-interactions, Parity (5.1–5.9)

- [ ] 5.1 [RED] [f: src/shell/gallery/views/slice.rs tests] [t: cargo test slice] [dep:2.9] [S] Failing flip machine: squeeze width full→0 (200ms InOutQuad), face-swap flag at midpoint, expand 0→full; reduced-motion short-circuit.
- [ ] 5.2 [GREEN][WIRE] [f: ui/gallery/SliceDelegate.slint, src/callbacks.rs] [t: cargo test slice] [dep:5.1] [M] Faces wrapper `clip:true` centered width animation; 200ms Timer swaps front/back visibility at midpoint; right-click triggers.
- [ ] 5.3 [f: ui/gallery/SliceDelegate.slint] [t: cargo check] [dep:5.2] [M] Back-face metadata sheet: name 13px bold tertiary, ext·dims·size 10px, tag pills h26/11px, actions VIEW/RETAG/DELETE h30 uppercase (no STEAM).
- [ ] 5.4 [WIRE] [f: ui/gallery/SliceDelegate.slint, src/shell/gallery/slot.rs] [t: cargo test gallery] [dep:5.3] [S] Favourite parallelogram toggle 48×24, knob 22×18, 150ms OutCubic; state persists via model.
- [ ] 5.5 [WIRE] [f: ui/gallery/SliceCarousel.slint, src/callbacks.rs] [t: cargo check] [dep:2.5] [S] Micro-interactions: hover-select after >2px travel mirrors `gallery-focused`; staggered entry opacity 0→1 + scale 0.85→1, 250ms OutCubic.
- [ ] 5.6 [f: src/shell/gallery/views/hexagon.rs, ui/gallery/HexDelegate.slint] [t: cargo test gallery] [dep:1.7] [S] Hex parity: arc yOffset = −normDist²·140·1.2; pull-out grows to bigR 420 from source cell position, 400ms.
- [ ] 5.7 [f: src/shell/gallery/views/mosaic.rs, ui/gallery/MosaicView.slint] [t: cargo test gallery] [dep:1.7] [S] Mosaic parity: exactly 2 Lloyd iterations per activation; reveal delays 20+(key%7)·14ms, alpha 0→1 over 200ms OutCubic.
- [ ] 5.8 [V] [t: cargo run] [dep:5.1–5.7] [S] Verify: flip swaps faces at 90° with functional sheet; hex columns bow and pull out; mosaic staggers.
- [ ] 5.9 [t: cargo test] [dep:5.1–5.8] [S] Full suite green: 299 GallerySlot tests + all new headless tests, zero regressions.

## Traceability

Fullscreen delta → 1.1–1.6 · Slice geometry/nav → 1.7, 2.1–2.11, 5.5 · Depth cues → 3.1–3.7 · FilterBar → 4.1–4.4 · Thumbnails → 4.5–4.8 · Entry choreography → 1.8, 5.5 · Flip/sheet → 5.1–5.4 · Hex → 5.6 · Mosaic → 5.7 · Preserved (instant apply, settings mutation, empty state) → untouched, guarded by 5.9.
