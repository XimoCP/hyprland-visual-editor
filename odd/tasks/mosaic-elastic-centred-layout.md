# Feature: Mosaic elastic centred layout — no duplicates, dynamic from the centre

**Locator**: `odd/tasks/mosaic-elastic-centred-layout.md`
**Engram mirror**: topic `odd/mosaic-elastic-centred-layout/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: DONE (2026-10-11) — the elastic centred layout landed, the parent read the renders,
and the keeper's own live check found one follow-up defect (a stale page capacity when the
stage changes) which is fixed and installed. Authorized 2026-10-11: "vamos a partir siempre de
la grande del medio y a partir de ahí vamos acomodando hasta cumplir con el espacio máximo de 10,
luego pasamos a la siguiente página y se va acomodando de la misma manera que el primero... de
forma dinámica desde el centro. Vamos a probarlo."
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~350-500 authored changed lines (layout rework + tests) → single PR.
**Route**: delegated writer (`src/shell/gallery/views/mosaic.rs`, `src/main.rs`), parent
verification INCLUDING a headless render read by eye (the project's rule for visual changes).

## Objective

The Mosaic fills the band with DISTINCT themes, growing or shrinking the tiles to fit, always
starting from a big tile in the centre of each page. Duplicates disappear: they only ever
appeared to plug holes the fixed tile size could not absorb.

## Problem / measured evidence (2026-10-11)

`justified_hero_layout` (`mosaic.rs`) packs a hero-centred band with FOUR segments, and each
segment already scales its tiles uniformly to fill its region exactly ("cover crop absorbs the
distortion"). The duplicates come from `fill_to` (`mosaic.rs:303-320`), which pads a segment
with cycled reals until its NATURAL width reaches the region, so the scale-up stays small.

`mosaic_page_capacity` (`mosaic.rs:634-664`) is a separate geometric ESTIMATE that returns 9
for every stage size measured (2374x1290, 2560x1440, 1920x1080, 3840x2160), while the layout
actually packs 10 tiles in the band. Measured with the real stage (2374x1290):

| reals | capacity | pages | clones | page 1 tiles | distinct | repeated |
| --- | --- | --- | --- | --- | --- | --- |
| 7 | 9 | 1 | 2 | 10 | 7 | 3 |
| 9 | 9 | 1 | 0 | 10 | 9 | 1 |
| 10 | 9 | 2 | 0 | 10 | 9 | 1 (plus page 2 with 1) |
| 11 | 9 | 2 | 0 | 10 | 9 | 1 (plus page 2 with 2) |

So with the keeper's 7 themes the wall paints 10 tiles of which 3 are repeats, and pagination
starts one theme too early: with 10 themes it makes 2 pages (9 + 1) when 10 fit in one. That is
what pushed two new themes to page 2 while page 1 still repeated one.

Also measured: the layout packs 13 REAL tiles in 2 rows without any repeat, so the band is far
more capable than the estimate assumes. And when the hero band is not viable
(`regions_viable = remaining >= seg_count && region_l >= th * 0.6 && all segments non-empty`,
`mosaic.rs:398-400`) the fallback (`:401-461`) only SHRINKS rows to fit: it never grows, so with
1-3 themes the rows stay left-aligned and a hole remains on the right.

## Design decisions (keeper's spec)

- **D1 — each page starts from a big tile in the CENTRE.** The page's first theme is the hero,
  centred and spanning the band height; the rest flow around it. Every page does this, so page 2
  is arranged like page 1.
- **D2 — no duplicates.** The segments scale to absorb the leftover instead of padding with
  cycled reals. `fill_to`'s clone padding goes.
- **D3 — 10 is a CAP, not a duty.** A page holds up to 10 themes while every tile stays at or
  above a MINIMUM size; if 10 would push tiles below that floor, the page holds what fits.
- **D4 — the page size comes from the layout, not from a separate estimate.** This is what
  removes the 9-vs-10 drift for good.
- **D5 — the hero band must work with 1, 2 and 3 themes too**, centred, with no hole.
- **D6 — keyboard navigation stays spatial.** `on_gallery_mosaic_nav` (`main.rs:3991`) already
  moves over the LAID-OUT rectangles (`mosaic_tile_rects` + `mosaic_neighbor`), so it survives
  mixed sizes. What changes: the cursor must be re-anchored to the THEME, not to the tile
  position, whenever the layout is recomputed (`mosaic_tile_real` already translates
  position → real theme; today a position can be a repeat, tomorrow it is nearly the identity).

## Scope (authorized)

- **U1 — the elastic layout.** Remove the clone padding; let each segment's uniform scale absorb
  the leftover. Keep every region filled edge-to-edge (no holes, no overlap). Add the minimum
  tile size as the floor for the scale-up. Extend the hero band to 1, 2 and 3 tiles: one centred
  big tile; hero + one; hero + two. Keep the fallback path for degenerate stages (non-positive
  dims) only.
- **U2 — the page model.** Page size = up to 10, derived from the layout at the minimum size
  (probe the real layout, do not re-estimate geometrically). `clone_fill_count` becomes 0 by
  construction; keep the method only if a test still needs it, and say so.
- **U3 — the cursor identity.** Store the mosaic cursor as the THEME (name or real index of the
  current page's source list) and re-anchor it after any recompute; if the theme is gone, land
  on the first tile. Page-edge stepping keeps using the FRESH tile count.
- **U4 — tests (pure, no window).** For N = 1..20 at the keeper's stage size: no tile is
  repeated; the tiles of each row cover the usable width exactly (gaps only the gutters); the
  band is vertically filled; the hero is centred; every tile height is within
  [minimum, maximum]; the page holds at most 10; page 2 is arranged like page 1 (its first theme
  centred); and the cursor re-anchors to the same theme after a recompute.
- **U5 — visual verification (parent).** Render the Mosaic headlessly and READ the PNGs: the
  wall must look even, with the big tile centred, no black holes and no repeats. Tests alone do
  not prove a look.

## Out of scope

- The Slice (carousel) view, the pagination UI's look, the info panel, the apply path, the
  thumbnail pipeline.
- No new dependency.
- The keyboard's key bindings themselves (arrows, Enter, Ctrl+[ ]) do not change.

## Acceptance criteria

1. `cargo test` green, `cargo check --all-targets` clean.
2. With 7 themes: 7 tiles, all distinct, bigger than today, hero centred, no hole.
3. With 10 themes: ONE page of 10 distinct tiles, no repeats.
4. With 11 themes: page 1 = 10 distinct (no repeat), page 2 = 1 centred.
5. With 1, 2 and 3 themes: centred, no hole.
6. The keyboard cursor stays on the same THEME when the layout is recomputed.
7. The rendered PNGs show an even wall with no holes and no repeats (parent's eyes).

## Checks

- `cargo test`
- `cargo check --all-targets`
- Headless render + reading the PNGs (project rule for visual changes).

## Work units

- [x] **U1** — the elastic centred layout (no clones, minimum size, hero for 1-3 tiles). — `eaa8420`
- [x] **U2** — the page size derived from the layout, cap 10. — `a4107a9`
- [x] **U3** — the cursor re-anchored to the theme. — `b33ffa7`
- [x] **U4** — the pure tests. — `00e6de2`
- [x] **U5** — the parent's render read + install. — read by eye; installed and relaunched.

## Verification result (U5)

- Suite at the end of U4: **1543 passed, 0 failed**; `cargo check --all-targets` clean; no new
  dependency. The writer rewrote six existing tests that pinned the old clone-fill behaviour and
  deleted none; it also removed `clone_fill_count`, `clone_real_index` and `MOSAIC_PAGE_ROW_COUNT`
  as dead.
- The parent READ both renders with its own eyes (the project's rule for visual changes), same
  fixture before and after: BEFORE 10 tiles of which one was a repeat; AFTER **9 tiles, all
  distinct**, the hero centred and the band even with no holes in both cases. The before render is
  kept at `/tmp/opencode/render-before-mosaic/mosaic_hero.png`.
- Budget: U1 alone rewrote 479 lines of one function, over the 400-line PR heuristic; the writer
  flagged it for the keeper's call on splitting.

## Follow-up found by the keeper's live check and FIXED here

The keeper saw 7 themes paginated into 2 pages. Cause: the capacity is now derived from the real
layout against the 96 px minimum tile, so it DEPENDS on the stage size (measured: 5 at 800x600,
9 at 1600x900, 10 at 2374x1290), and the page model was computed once while the window was still
hidden; the refresh guard only asked whether the theme COUNT had changed, so the small-stage
capacity outlived the window's real size. Before this work the capacity was size-insensitive (9
for every stage), which is why the guard's missing half was invisible.

Fix in `971b465`: `MosaicPages` remembers the stage it computed for and
`needs_recompute(real_count, stage_w, stage_h)` is the single place that decides. Test: a model
built for 800x600 (capacity 5) asks for a recompute at 2374x1290 and then holds all 7 themes on
ONE page. Suite after the fix: 1544 passed, 0 failed.

## Next (separate contract)

`odd/tasks/mosaic-active-theme-is-the-hero.md` — the keeper decided the ACTIVE theme must always be
the hero, and the Mosaic must open on its page. Planned, not started.

## Progress log

- 2026-10-11: capacity-vs-reality drift measured with the real stage; the keeper specified the
  centred, elastic, cap-10 model; contract written.
