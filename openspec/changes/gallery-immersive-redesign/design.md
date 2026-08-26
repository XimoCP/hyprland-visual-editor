# Design: Immersive Gallery Redesign

> **Changelog 2026-08-23:** Hexagon removed per user decision #632 — architecture now covers 2 styles (Slice + Mosaic) only. HexDelegate / arc / honeycomb geometry deleted.

Change: `gallery-immersive-redesign` · Branch target: stacked-to-main · Budget: <400 lines/PR

## Architecture Overview

```
Slint UI (single window, mutates)                 Rust
┌────────────────────────────────────┐   ┌──────────────────────────────────┐
│ main.slint / shell.slint           │   │ NavState ──▶ Shell::process_queue│
│  └ GalleryRoot                     │   │    │ mount/unmount hooks         │
│     ├ Backdrop (α0, fade 300ms)    │   │    ▼                             │
│     ├ Stage                        │◄──┤ SlotRegistry: GallerySlot        │
│     │  ├ SliceCarousel (3 layers)  │   │  ├ model.rs  ThemeGalleryModel   │
│     │  └ MosaicView                │   │  ├ thumbs.rs  NEW pipeline       │
│     ├ FilterBar (hover strip 24px) │   │  │  └ views/slice.rs geometry fns│
│     └ MIT pill (bottom-right)      │   │ Controller ──▶ Box<dyn Composer> │
└────────────────────────────────────┘   │  └ set_fullscreen(bool) NEW      │
                                         │       ▼                          │
                                         │   HyprlandComposer (hyprctl)     │
                                         └──────────────────────────────────┘
```
*Single mutating window — engine intact (`engine.rs`, `config.rs`, `settings.rs`, `theme_manager.rs`, `app_state.rs`, `watcher.rs`, `utils.rs`, `providers/*` reused, not rewritten). Styles 1:1 with skwd-wall (Slice + Mosaic only after 2026-08-23).*

## Component Inventory

**Create**: `ui/gallery/FilterBar.slint`, `ui/gallery/SliceCarousel.slint` (manual-x carousel host), `src/shell/gallery/thumbs.rs` (decode/scale/disk-cache).
**Modify**: `src/composer/mod.rs` (trait + Controller), `src/composer/hyprland.rs` (dispatches + startup sanity), `src/shell/mod.rs` (fullscreen session hooks on Expand(Gallery)/Back), `src/shell/gallery/model.rs`, `slot.rs`, `views/slice.rs` (135/924/520 constants + pure geometry), `ui/tokens.slint`, `GalleryRoot.slint`, `SliceDelegate.slint`, `GalleryChrome.slint` (dismantle toolbar/footer → pill), `shell.slint`/`main.slint` (new properties/callbacks), `callbacks.rs`.
**Untouched**: engine, providers, config, settings, theme_manager, watcher, nav state machine.

## Decisions

### D1 — Fullscreen lifecycle via Composer
- **Context**: skwd launches as layer-shell Overlay; Slint can't do wlr-layer-shell. Spec adds a `composer` delta requirement.
- **Decision**: `Composer` trait gains `fn set_fullscreen(&self, on: bool) -> bool`. `Controller` gains `enter_gallery_session()`/`exit_gallery_session()`. Hyprland impl reuses the proven pattern *focus HVE by title first* (dispatches act on focused window): V5 `hl.dsp.focus(...)` then `hl.dsp.fullscreen({mode=1|0})`; V4 `focuswindow` + `fullscreen 1|0`. All arguments are **compile-time constants** — no interpolation, no new injection surface. Startup sanity check parses existing `hyprctl clients -j` (same code path as `hve_in_special`) and repairs stuck fullscreen before first show (`fullscreen 0` + float restore). If dispatch fails → return false and continue floating (gallery stays fully functional windowed); UI never blocks on it.
- **Alternatives rejected**: Slint window decorations-fullscreen API (no compositor coordination with special-workspace hide/show flow); wlr-layer-shell (out of scope).
- **Risk**: crash while fullscreen → mitigated by startup sanity (spec scenario).

### D2 — Overlap −30 via manual x placement
- **Context**: `HorizontalLayout` cannot express negative spacing.
- **Decision**: absolute placement inside a clipped stage. Pure function in `views/slice.rs`:
  `cum_offset(i, focused) = Σ_{j<i} width(j) + i·spacing`, `width(j)=924 if j==focused else 135`, `spacing=−30`. Card x = `cum_offset(i, focused)`; container x = `viewport_center − 924/2 − cum_offset(focused, focused)` (see D5). Unit tests assert exact px for focused=0/mid/last.
- **Alternative rejected**: ListView (no negative spacing either; snap semantics differ).
- **Risk**: none beyond arithmetic — fully headless-testable.

### D3 — Rounded parallelogram without Shape/MultiEffect
- **Decision**: one `Path` per face, viewbox `(0..924+35, 0..520)` with vertices `(35,0),(959,0),(924,520),(0,520)`; slant corners rounded with `QuadTo` control at each vertex, radius r clamped to `min(517, ...)`. Image masked by placing the `Image` inside a `clip:true` container whose shape is the same Path (fill surface). Shadow = second identical Path declared first (paints below), translated x4/y10 α0.5 (current) vs x2/y5 α0.3 (rest).
- **Alternative rejected**: pre-baked SVG masks (breaks dynamic width animation).
- **Risk**: Path fill cannot clip a child Image directly → clip container + matching Path background achieves the mask; golden screenshot test guards fidelity.

### D4 — Edge fade + z-rank (runtime z impossible)
- **Context**: verified against Slint docs: the common `z` property **must be a compile-time constant** — runtime z-rank per card is NOT possible. This supersedes the orchestrator's "z-index property por card" assumption.
- **Decision**: three conditional layers inside SliceCarousel, painted in declaration order: (1) idle row `if !current && !hovered`, (2) hovered card, (3) current card. Exactly one live delegate instance per card; layers share the same pure-position functions from D2. Opacity: full inside `fullZone=min(0.6,(462+2·105)/halfView)`, linear →0 at normDist 1.2. Dim overlay 0/.15/.4, glow stroke primary-w3 / primary@.4-w1 / black@.6-w1, all 200ms.
- **Alternative rejected**: single-layer paint-order tricks (right-of-focus cards would always overlap the current card).
- **Risk**: hover crossing layers remounts a delegate → entry animation suppressed for non-idle layers (opacity animation gated to first mount only).

### D5 — Snap-centering
- **Decision**: container.x animated 350ms OutCubic toward `viewportCenter − 924/2 − cum_offset(focused)`. Wheel: pure Rust `SliceView::wheel_step(dir)` advances index ±1 max per gesture; a 400ms debounce Timer (Slint `Timer`) commits the snap after idle. Hover-select after >2px travel mirrors focus through `gallery-focused`. Arrows stay clamped/no-wrap (existing S13 path).
- **Alternative rejected**: flick physics port (deceleration timers) — deferred; wheel±1 covers the spec scenarios exactly.

### D6 — Y-flip approximation
- **Decision**: two-phase width-squeeze: faces wrapper animates `width` full→0 (200ms InOutQuad, clip:true, centered so both edges move inward), a 200ms `Timer` swaps front/back visibility at the midpoint, then 0→full (another 200ms). Total 400ms InOutQuad ≈ Y-rotation projection. Back face = metadata sheet (name 13px tertiary, ext·dims·size 10px, favourite parallelogram toggle 48×24/knob 22×18 150ms OutCubic, tag pills h26/11px, VIEW/RETAG/DELETE h30 — no STEAM).
- **Alternative rejected**: rotation-angle hacks (element rotation is 2D around z, not Y-axis).
- **Risk**: squeeze reads slightly different from true 3D; accepted by proposal.

### D7 — FilterBar hover-reveal
- **Decision**: `FilterBar.slint`: invisible top TouchArea strip h=24 always active; `entered` sets revealed=true (bar slides down 250ms); 250ms auto-hide `Timer` armed on exit. Pills: skewed rectangles skew 10px, h24, −10px vertical overlap, active filled primary. Style switch cross-fade 200ms preserving focused card (existing behavior moved out of dismantled GalleryChrome toolbar).
- **Risk**: strip intercepts clicks meant for nothing else (top 24px is empty backdrop) — safe.

### D8 — Thumbnails: Rust-side `<image>`, not string paths
- **Context**: verified against Slint docs: `@image-url` addresses **must be compile-time string literals**, and `Image.source` takes type `image` — a dynamic string path property does NOT work in 1.17. The orchestrator's "property image-path string" assumption is corrected here.
- **Decision**: `thumbs.rs` decodes the wallpaper (new dep `image = "0.25"`), cover-crops+scales to ≤400×720, writes PNG to `$XDG_CACHE_HOME/hve/thumbs/<hash>.png` keyed by source path+mtime (LRU 200 prune = existing ThumbnailCache semantics, now over real files), then `slint::Image::load_from_path` → `GalleryCardData.thumb` (existing `<image>` field used by delegates). Skeleton shimmer shows until `thumb.width > 0`; fade-in 200ms OutCubic. Generation happens off-thread (`std::thread`) with results marshaled to the UI thread via `slint::invoke_from_event_loop`.
- **Alternative rejected**: loading original files directly (`load_from_path(full_res)`) — unbounded memory with many/large wallpapers.

### D9 — Hexagon geometry (arc/offset, pull-out bigR=r·3, honeycomb) — REMOVED (Hex dropped 2026-08-23)
- **Status:** Removed per decision #632 (2026-08-23). No honeycomb centering, arc `yOffset = −normDist²·r·1.2`, or pull-out `bigR = r·3` in scope. Retained only as historical note.

### D10 — MIT credit lives in a bottom-right pill
- **Decision**: discrete pill anchored bottom-right over the backdrop: text 9px outline@0.6, click opens `mit-link` (reuses existing `mit-footer`/`mit-link` properties; footer Rectangle deleted). Keeps R7 attribution visible without stealing stage space. Flagged for user approval at review.

### D11 — Slice plan
PR1 tokens+fullscreen → PR2 geometría+overlap → PR3 depth+snap → PR4 FilterBar+thumbs+Mosaic → PR5 Y-flip+micro (Hex removed — PR5 no longer contains Hex parity). Strict dependency chain PR1→PR5, each independently revertable. 5 slices, 46 active tasks + 1 removed (Hex) preserved for traceability.

## Data Flow

```
Rust model                    Slint properties                back to Rust
ThemeManager.list() ─▶ ThemeGalleryModel ─▶ gallery_cards:[GalleryCardData] (thumb:image)
focused_index ──────────▶ gallery-focused ─▶ carousel positions (D2/D5)
wheel/hover (Slint) ─── gallery-wheel-step(i) / gallery-hover(i) ─▶ SliceView state
style pills ─────────── gallery-style-selected ─▶ GalleryStyle mirror
Expand(Gallery)/Back ── Shell hooks ─▶ Controller.enter/exit_gallery_session ─▶ Composer.set_fullscreen
theme apply/delete ──── gallery-card-clicked/right-clicked ─▶ GallerySlot (unchanged apply paths)
```
New MainWindow surface: `gallery-backdrop-alpha`, `gallery-flipped:i32` (or per-card), `gallery-wheel-step(int)`, `gallery-hover-changed(int)`, plus FilterBar style callback reuse.

## Testing Strategy

| Layer | What | How |
|---|---|---|
| Unit | cum_offset/snap math, fade falloff, wheel debounce state machine, thumb cache key/LRU, sanity-check JSON parse | headless `cargo test`, FakeComposer call recording |
| Integration | fullscreen enter/exit sequences via FakeComposer; mount/unmount ordering | existing composer test harness pattern |
| Visual | parallelogram+shadow, FilterBar reveal, flip midpoint swap | `slint-viewer --screenshot` goldens |

## Threat Matrix

Applicability review (design changes subprocess usage: new hyprctl dispatches):
Documentation-like paths — N/A (none touched). Git repository selection — N/A. Commit state — N/A. Push state — N/A. PR commands — N/A. Subprocess note (not a matrix row): all new `hyprctl` invocations use constant argv with the established focus-by-title pattern; `safe_workspace_target` allowlist unchanged and reused; RED test = FakeComposer asserts exact dispatch sequence and fallback-on-false.

## Migration / Rollout

No data migration. Each PR reverts independently; PR1 call site is one hook pair in `Shell::process_queue`.

## Open Questions

- [ ] D10 pill placement — confirm at PR4 review.
- [ ] Exact V5 Lua spelling for fullscreen mode (verify against installed Hyprland during PR1; V4 branch is the fallback contract).
