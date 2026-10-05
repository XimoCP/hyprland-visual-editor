# Proposal: Theme Gallery (Module 2)

## Change ID
`theme-gallery`

## Summary
Build the **Theme Gallery** — the first screen and WOW piece of HVE. Cards show complete looks (colors + border + background + shader) with instant apply. Three presentation styles 1:1 from skwd-wall (MIT): **parallelogram carousel**, **hexagon grid**, **voronoi mosaic**. Click a card → window animates resize → card expands into settings panel inside the same mutating window.

## Problem
HVE 1 has a theme list but no visual gallery. Users can't preview complete looks before applying. The skwd-wall visual language (parallelogram/hexagon/mosaic with fluid animations) is the reference — MIT licensed, translatable to Slint.

## Scope (In)
- **Gallery slot** mounted at `Screen::Gallery` via existing `SlotRegistry`
- **ThemeCard model**: complete look (colors, border, background, shader, metadata)
- **Three presentation styles** (user-switchable):
  1. **Slice/Parallelogram Carousel** — skewed cards, horizontal carousel, current item expands (350ms OutCubic), flip on right-click
  2. **Hexagon Grid** — honeycomb layout, pull-out selection, parallax thumbnails
  3. **Voronoi Mosaic** — organic cells, Lloyd relaxation, kinetic scroll, cloud opacity focus
- **Instant apply**: card click → `ThemeManager.apply()` → watcher refresh → UI updates
- **Window mutation**: `Shell::dispatch(Expand(Gallery))` → size animator 900×680 → 1200×800 (Module 1)
- **Card → Settings expansion**: same window, animated resize, Hyprland animates frame
- **Style tokens**: Roboto Condensed, radius 2-40, spacing 2-20, animExpand=350ms, fallbackAccent #4fc3f7
- **Credit**: MIT attribution to skwd-wall (liixini) in UI footer

## Scope (Out)
- Engine/providers (sealed): `engine.rs`, `config.rs`, `settings.rs`, `theme_manager.rs`, `app_state.rs`, `watcher.rs`, `utils.rs`, `providers/*`
- Monitor/HDR/display layout → hyprmod
- GPL code from hyprmod (bezier editor, undo, pending changes — ideas only)
- Wallpaper Engine / Wallhaven / Steam / Ollama — not in HVE scope
- skwd-wall Rust rewrite (not public) — we translate V1 QML visual ideas

## Approach
1. **Data layer**: `ThemeGalleryModel` wrapping `ThemeManager.list()` + provider capabilities
2. **Presentation layer**: Three `GalleryView` implementations sharing `ThemeCard` delegate
3. **Slot implementation**: `GallerySlot` implementing `Slot` trait, mounted by `Shell`
4. **Slint UI**: Canvas 2D for shapes (parallelogram/hexagon/voronoi), `animate`/`transition` with `cubic-bezier`, `set_size` for window mutation
5. **Integration**: Register real `GallerySlot` in `main.rs` replacing `StubSlot`

## Risks
| Risk | Impact | Mitigation |
|------|--------|------------|
| Hyprland `decoration:screen_shader` runtime flicker ~3s (#15067) | Breaks "instant" goal | Detect shader change path; if flicker detected, show loading state; document limitation |
| HVE + hyprmod both managing animations | Last writer wins | Level 1: HVE launches hyprmod CLI optionally. Level 2: Yield animations when hyprmod open. Level 3 (modify GPL) — avoid |
| Slint Canvas 2D performance with many cards | Jank on large theme collections | Virtualized ListView, image preheating (120ms), sourceSize limits, cache |
| Voronoi relaxation CPU on resize | Frame drops | Cache cells, rebuild only on filter/theme count change, max 64 warmup cells |

## Dependencies
- Module 1 (Trunk): `NavState`, `SlotRegistry`, `SizePolicy`, `Shell`, `Shell::set_global`, `Shell::sync_global_after_show` — **COMPLETE**
- `ThemeManager` with providers registered — **EXISTS**
- Slint 1.17: Canvas 2D, `animate`/`transition`, `cubic-bezier`, `set_size` — **VERIFIED**

## Success Criteria
- Gallery opens as first screen (Home → Expand Gallery)
- 3 presentation styles switchable, all animated 350ms OutCubic
- Click card → instant apply (<200ms perceived) → watcher refreshes UI
- Card expands to settings panel with window resize animation
- 245+ tests pass (unit + integration)
- Zero engine/provider modifications

## Estimated Effort
- Exploration: done (this proposal)
- Spec: ~2h
- Design: ~3h
- Tasks: ~2h
- Apply: ~15-20h (3 views + slot + integration + tests)
- Verify: ~3h
- Archive: ~1h