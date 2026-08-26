# Proposal: Immersive Gallery Redesign

> **Changelog 2026-08-23:** Hexagon removed per user decision #632 — gallery now supports 2 styles (Slice + Mosaic) not 3. Slice is principal; Mosaic is the variation. All Hex scope deleted from proposal/spec/design/tasks.

## Change ID: gallery-immersive-redesign

## Summary

Rebuild Theme Gallery visuals 1:1 with skwd-wall (MIT): fullscreen immersive launch over the live desktop, exact slice/mosaic geometry (2 styles — Slice principal + Mosaic), depth cues, and skwd chrome — reusing engine, `Composer` trait, Shell/NavState/SlotRegistry, the tested GallerySlot backend, and existing views as headless engines.

## Problem

Module 2 works but reads as "a window inside a window": opaque 900×680 frame, permanent toolbar, cards 2.6× shorter with positive gaps, no overlap/snap/depth, fake flip, hardcoded accent. User rejected it aesthetically. Exploration `sdd/explore/skwd-wall-visual-language` extracted skwd-wall's literal values as the numeric contract (135/924/520, −30 overlap, skew 35px, dim .4/.15/0, fade @1.2 normDist).

## Scope (In)

1. Fullscreen undecorated launch via Composer on Gallery open; restore floating on close — layer-shell effect without wlr-layer-shell.
2. Fix swapped tokens (`gallery-slice-h`=924 / `gallery-slice-card-w`=520); geometry 135/924/520, spacing −30 overlap, skew 35px X-shear, radius 0.
3. Depth: edge fade linear→0 at 1.2 normDist; z-rank current 100 / hover 90 / rest 50−dist; parallelogram shadow ×4/y10 α0.5; dim idle .4 / hover .15 / current 0 (200ms).
4. Optical center-snap carousel (StrictlyEnforceRange equivalent), wheel ±1 debounce 400ms.
5. Floating hover-reveal FilterBar (24px strip, auto-hide 250ms) replacing permanent toolbar.
6. Real Y-flip 400ms InOutQuad with back-face metadata sheet (name, ext·dims·size, fav toggle, tag pills, actions).
7. Real wallpaper thumbnails: sourceSize 400×720 cover + skeleton shimmer.
8. Micro-interactions: hover-select >2px, staggered entry 250ms OutCubic, glow border primary w3 / w1 / black@0.6.

## Scope (Out)

Real layer-shell (Slint unsupported), matugen runtime palette (static fallback palette now), video previews/Wallhaven/Steam/Ollama tagging/multi-monitor picker, engine & providers changes, V2 particles.

## Approach

Five stacked PR slices (<400 lines each), ultra-atomic tasks (<80 lines), strict TDD:

- **PR1 Presentation**: Composer fullscreen lifecycle + token fixes + backdrop fade 300ms.
- **PR2 Carousel core**: manual-x overlap layout (−30), skew 35px, center snap + wheel physics.
- **PR3 Depth & light**: edge fade, z-rank, shaped shadows, dim/glow states.
- **PR4 Chrome**: floating FilterBar + real thumbnails + entry choreography.
- **PR5 Flip & polish**: Y-flip, back-face sheet, Mosaic parity pass (Hex removed 2026-08-23).

## Capabilities

> Contract for sdd-spec. Existing specs: `base-window`, `composer`, `nav-shell` (no gallery spec archived yet).

### New Capabilities
- `gallery-immersive`: immersive presentation contract — geometry tokens, overlap carousel physics, depth cues, floating chrome, flip metadata sheet, real thumbnails.

### Modified Capabilities
- `composer`: new requirement — fullscreen enter without decorations plus state restore on close (trait gains fullscreen control; show/hide/focus requirements unchanged).

## Affected Areas

| Area | Impact |
|------|--------|
| composer impl + Controller | New fullscreen methods |
| `ui/tokens.slint` | Fix swapped values, skwd scale |
| `ui/gallery/*` | Geometry, depth, chrome rewrite |
| Gallery slot model | Thumbnail pipeline, snap model |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| No Shape/MultiEffect in Slint → rounded parallelogram mask | High | Path+clip primitives; headless golden tests |
| Overlap needs manual x placement (layouts can't negative-gap) | Med | Pure geometry functions, unit-tested headless |
| 3D flip approximation vs QML Rotation | Med | Scale-X squeeze + face swap at 90° |
| Crash leaves window stuck fullscreen | Low | Restore hook on close + startup sanity check |

## Rollback Plan

Each PR reverts independently via git revert. PR1 call site is one line: reverting restores floating-only launch. Token commit revert restores prior constants. No data migrations involved.

## Dependencies

- Exploration #621 numeric contract (sections 1–6)
- Slint skill installed; GallerySlot's 299 tests stay green throughout
- Composer V4/V5 dispatch branches already proven

## Success Criteria

- [ ] Gallery opens fullscreen undecorated over live desktop; closing restores prior floating state
- [ ] Slice metrics match 135/924/520 / −30 / 35px / radius 0; current card pixel-centered after snap
- [ ] Depth cues present at skwd values (edge fade, z-rank, shaped shadow, dim states)
- [ ] Permanent toolbar gone; FilterBar hover-reveals and auto-hides
- [ ] Flip shows functional back-face metadata; thumbnails are real wallpapers
- [ ] All existing tests green plus new headless tests per atomic task

## Estimated Effort

~5 stacked PRs, ~15–20 atomic tasks, 2–3 focused sessions.
