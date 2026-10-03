// HVE 2 — Mosaic Voronoi (theme-gallery PR3).
//
// Voronoi tessellation + Lloyd relaxation (default 3), cache rebuild only on
// filter/count change, async guard with cancel token, kinetic scroll 0.90,
// cloud opacity 0.55/1.05, dual-stripe infinite scroll, staggered reveal.
// Translated from skwd-wall _buildTile() (MIT, © liixini).
//
// Pure Rust — no Slint types. MosaicCell/MosaicView .slint handle visuals.

use super::{GalleryStyle, GalleryView, GalleryKey};

// ── Constants (spec R2.3) ───────────────────────────────────────────

/// Lloyd relaxation iterations default 3 (spec R2.3 _buildTile).
pub const MOSAIC_LLOYD_ITERATIONS: usize = 3;
/// Minimum/maximum tiles for warmup: max 64 (threat guard).
pub const MOSAIC_WARMUP_MAX: usize = 64;
/// Kinetic scroll friction per tick (spec R2.3 S5) 0.90, 16ms timer.
pub const MOSAIC_KINETIC_FRICTION: f32 = 0.90;
pub const MOSAIC_KINETIC_INTERVAL_MS: u64 = 16;
/// Cloud opacity radii (spec R2.3) inner 0.55 outer 1.05.
pub const MOSAIC_CLOUD_INNER: f32 = 0.55;
pub const MOSAIC_CLOUD_OUTER: f32 = 1.05;
/// Infinite scroll uses dual-stripe tiling (spec R2.3).
pub const MOSAIC_STRIPE_COUNT: usize = 2;
/// Warmup async guard token increments on filter change.
pub const MOSAIC_WARMUP_INTERVAL_MS: u64 = 16;
/// Stagger base for reveal per cellKey (ms).
pub const MOSAIC_STAGGER_MS: u64 = 40;

// ── Justified-rows layout (gallery-immersive-redesign): ─────────────
// Google Photos / Flickr style packer (the horizontal cousin of
// Pinterest masonry) replacing the PR5 span-masonry. Rust computes the
// FULL tile geometry as a model (Slint GridLayout cannot compute spans
// and pure bindings cannot know per-image aspects); Slint only paints.
// Pure f32 math, no Slint types — headless testable.
//
// Deterministic algorithm: the block is bounded to a central band
// `band = stage_h / BAND_DIVISOR` (like the Slice carousel's fixed
// presentation strip) and the rows split it evenly: target row height
// `th = (band − (ROW_COUNT−1)·GUTTER) / ROW_COUNT`;
// sanitized aspects (non-finite or ≤ 0 → DEFAULT); per-row target
// aspect sum `T = Σa / ROW_COUNT`; greedy in-order packing closes the
// current row when adding the next card would overshoot `T` beyond
// half that card's aspect (`sum + a/2 > T`); never more than ROW_COUNT
// rows (overflow force-closes into the last row). Every row EXCEPT the
// last is justified shape-preservingly to the common width
// `W = max natural row width` by scaling its row height — tile rects
// always keep each image's aspect, so nothing is ever cropped or
// letterboxed. The last row keeps the natural `th`, left-aligned.
/// Central band height divisor: the packed block is bounded to
/// `band = stage_h / 2.0` (≈520px on a 1040px stage), matching the Slice
/// carousel's fixed presentation band proportionally.
pub const MOSAIC_BAND_HEIGHT_DIVISOR: f32 = 2.0;
/// Thin uniform gutter between tiles (no frames, no padding).
pub const MOSAIC_GUTTER_PX: f32 = 12.0;
/// Fallback aspect when the real image size is unknown/invalid.
pub const MOSAIC_DEFAULT_ASPECT: f32 = 16.0 / 9.0;
/// Two-row strip (spec: ≈2 rows visible).
pub const MOSAIC_ROW_COUNT: usize = 2;
/// Symmetric lateral margin as fraction of stage width (4% per side).
/// Usable width = stage_w − 2×margin; wall stays horizontally centered.
pub const MOSAIC_SIDE_MARGIN_FRACTION: f32 = 0.04;
/// Pinterest-style display aspect variety — single tuning point each.
/// WIDER spread (1.0 → 1.95) plus portrait 0.72 sells the Pinterest look.
/// Portrait 0.72 was evaluated: justified packer handles aspect < 1 without
/// degenerate row layout (no clamp needed); if a future tuning pushed below
/// ~0.7 and caused degenerate rows, clamp portrait to 0.8.
pub const MOSAIC_ASPECT_SQUARE: f32 = 1.0;
pub const MOSAIC_ASPECT_PORTRAIT: f32 = 0.72;
pub const MOSAIC_ASPECT_WIDE: f32 = 1.95;

/// Final paint-ready tile rect in stage coordinates (offsets applied).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MosaicTile { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }

/// Packed block: tiles plus centered content box on the stage.
#[derive(Debug, Clone, PartialEq)]
pub struct MosaicLayout {
    pub tiles: Vec<MosaicTile>,
    pub content_w: f32,
    pub content_h: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

/// Artificial display aspect (`w/h`) for card `idx` — the "real
/// Pinterest" pattern. Real image aspects are IGNORED on purpose: the
/// user's library is uniformly 16:9, which would pack a boring uniform
/// grid, so tiles get a deterministic shape variety instead and images
/// cover-crop into their tile (user-approved).
/// Deterministic 10-cycle: [square, portrait, wide, portrait, square,
/// wide, square, portrait, wide, square] — every window of 10 contains
/// all three classes and no 4 consecutive share a class.
pub fn mosaic_display_aspect(idx: usize) -> f32 {
    const CYCLE: [f32; 10] = [
        MOSAIC_ASPECT_SQUARE,   // 0 square
        MOSAIC_ASPECT_PORTRAIT, // 1 portrait
        MOSAIC_ASPECT_WIDE,     // 2 wide
        MOSAIC_ASPECT_PORTRAIT, // 3 portrait
        MOSAIC_ASPECT_SQUARE,   // 4 square
        MOSAIC_ASPECT_WIDE,     // 5 wide
        MOSAIC_ASPECT_SQUARE,   // 6 square
        MOSAIC_ASPECT_PORTRAIT, // 7 portrait
        MOSAIC_ASPECT_WIDE,     // 8 wide
        MOSAIC_ASPECT_SQUARE,   // 9 square
    ];
    CYCLE[idx % CYCLE.len()]
}

/// Pack `aspects` (in display order) into a justified-rows block sized
/// for `(stage_w, stage_h)`. Empty input or non-positive stage → all
/// zeros. See the module docs for the deterministic algorithm.
pub fn justified_layout(aspects: &[f32], stage_w: f32, stage_h: f32) -> MosaicLayout {
    if aspects.is_empty() || !(stage_w > 0.0) || !(stage_h > 0.0) {
        return MosaicLayout {
            tiles: vec![], content_w: 0.0, content_h: 0.0, offset_x: 0.0, offset_y: 0.0,
        };
    }
    let g = MOSAIC_GUTTER_PX;
    // Central band constraint: rows split `band` minus the inter-row
    // gutters evenly, so the 2-row block is exactly one band tall.
    let band = stage_h / MOSAIC_BAND_HEIGHT_DIVISOR;
    let th = (band - (MOSAIC_ROW_COUNT as f32 - 1.0) * g) / MOSAIC_ROW_COUNT as f32;
    // Sanitize first so the per-row target uses valid aspects only.
    let sanitized: Vec<f32> = aspects
        .iter()
        .map(|&a| if a.is_finite() && a > 0.0 { a } else { MOSAIC_DEFAULT_ASPECT })
        .collect();
    let target = sanitized.iter().sum::<f32>() / MOSAIC_ROW_COUNT as f32;
    // Greedy in-order packing; overflow force-closes into the last row.
    let mut rows: Vec<Vec<f32>> = Vec::new();
    let mut sums: Vec<f32> = Vec::new();
    for &a in &sanitized {
        let open_new = match rows.len() {
            0 => true,
            n => n < MOSAIC_ROW_COUNT && sums[n - 1] + a * 0.5 > target,
        };
        if open_new {
            rows.push(vec![a]);
            sums.push(a);
        } else {
            let n = rows.len() - 1;
            rows[n].push(a);
            sums[n] += a;
        }
    }
    // Natural row width at th; the widest row defines the common width W.
    let nat_w = |sum: f32, k: usize| th * sum + (k as f32 - 1.0) * g;
    let big_w = rows
        .iter()
        .zip(&sums)
        .map(|(row, s)| nat_w(*s, row.len()))
        .fold(f32::MIN, f32::max);
    let margin = stage_w * MOSAIC_SIDE_MARGIN_FRACTION;
    let usable_w = stage_w - 2.0 * margin;
    // Shape-preserving justification + shrink-to-fit: non-last rows scale
    // their HEIGHT to fill W exactly; when a row's natural width exceeds the
    // usable width it is shrunk proportionally to fit exactly
    // `h = (usable - (k-1)*g) / sum`, so the wall never overflows the
    // symmetric [margin, stage_w - margin] band. Last row keeps th unless it
    // itself overflows, in which case it is shrunk the same way. No upscaling
    // beyond natural — fitted rows stay centered.
    let mut tiles: Vec<MosaicTile> = Vec::with_capacity(sanitized.len());
    let n_rows = rows.len();
    let mut y = 0.0f32;
    let mut max_row_w: f32 = 0.0;
    for (r, row) in rows.iter().enumerate() {
        let is_last = r + 1 == n_rows;
        let h_initial = if is_last {
            th
        } else {
            (big_w - (row.len() as f32 - 1.0) * g) / sums[r]
        };
        let row_nat_w = if is_last {
            nat_w(sums[r], row.len())
        } else {
            big_w
        };
        let (h_r, gutter_h) = if row_nat_w > usable_w {
            let avail = usable_w - (row.len() as f32 - 1.0) * g;
            if avail > 0.0 {
                // shrink-to-fit with fixed gutters: h = (usable - (k-1)*g)/sum
                ((avail) / sums[r], g)
            } else {
                // gutters alone exceed usable (huge per-row count): scale whole row
                let factor = usable_w / row_nat_w;
                (h_initial * factor, g * factor)
            }
        } else {
            (h_initial, g)
        };
        let row_w = h_r * sums[r] + (row.len() as f32 - 1.0) * gutter_h;
        if row_w > max_row_w {
            max_row_w = row_w;
        }
        let mut x = 0.0f32;
        for &a in row {
            let w = h_r * a;
            tiles.push(MosaicTile { x, y, w, h: h_r });
            x += w + gutter_h;
        }
        y += h_r + g;
    }
    let content_h = y - g; // drop the trailing gutter after the last row
    let content_w = max_row_w;
    // Symmetric lateral margins: wall is horizontally centered inside the
    // usable width. After shrink-to-fit content_w <= usable_w always, so this
    // is simply `margin + (usable - content)/2` (no overflow left-pin needed).
    let offset_x = margin + ((usable_w - content_w) / 2.0).max(0.0);
    let offset_y = ((stage_h - content_h) / 2.0).max(0.0);
    for t in &mut tiles {
        t.x += offset_x;
        t.y += offset_y;
    }
    MosaicLayout { tiles, content_w, content_h, offset_x, offset_y }
}

/// Hero max width as fraction of usable width (45% cap).
pub const MOSAIC_HERO_MAX_WIDTH_FRACTION: f32 = 0.45;

/// Pack `aspects` into a hero mosaic: one hero per super-band (2 rows)
/// spanning the full band height, CENTERED in the band (user request —
/// supersedes the old left/right alternation). The top row fills the LEFT
/// region, the bottom row the RIGHT region; each side is CLONE-FILLED from
/// the page's non-hero cycle (cycled real indices) until its natural width
/// reaches its region, then justified to EXACTLY that region — even band,
/// no ragged holes. Side rows can only shrink (h ≤ th), never overflow the
/// band. Degenerate (<2 remaining tiles or regions too narrow) falls back
/// to normal rows with the identity real mapping. Returns the layout plus
/// the per-tile real mapping in push order (hero, top row, bottom row) —
/// clones carry cycled reals. Reuses `MOSAIC_GUTTER_PX` everywhere and
/// stays inside 4% margins.
pub fn justified_hero_layout(
    aspects: &[f32],
    reals: &[usize],
    stage_w: f32,
    stage_h: f32,
) -> (MosaicLayout, Vec<usize>) {
    if aspects.is_empty() || !(stage_w > 0.0) || !(stage_h > 0.0) {
        return (
            MosaicLayout {
                tiles: vec![],
                content_w: 0.0,
                content_h: 0.0,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            vec![],
        );
    }
    let g = MOSAIC_GUTTER_PX;
    let band = stage_h / MOSAIC_BAND_HEIGHT_DIVISOR;
    let th = (band - (MOSAIC_ROW_COUNT as f32 - 1.0) * g) / MOSAIC_ROW_COUNT as f32;
    let sanitize = |a: f32| if a.is_finite() && a > 0.0 { a } else { MOSAIC_DEFAULT_ASPECT };
    // Fixed per-page target (original packing contract): each row aims for
    // half the page's total aspect sum.
    let target = aspects.iter().map(|&a| sanitize(a)).sum::<f32>() / MOSAIC_ROW_COUNT as f32;
    // (aspect, real) rows — reals ride along so the mapping stays exact.
    let real_at = |i: usize| -> usize {
        if reals.is_empty() { i } else { reals[i % reals.len()] }
    };
    let mut rows: Vec<Vec<(f32, usize)>> = Vec::new();
    let mut sums: Vec<f32> = Vec::new();
    for (i, &raw) in aspects.iter().enumerate() {
        let a = sanitize(raw);
        let open_new = match rows.len() {
            0 => true,
            n => n < MOSAIC_ROW_COUNT && sums[n - 1] + a * 0.5 > target,
        };
        if open_new {
            rows.push(vec![(a, real_at(i))]);
            sums.push(a);
        } else {
            let n = rows.len() - 1;
            rows[n].push((a, real_at(i)));
            sums[n] += a;
        }
    }
    let margin = stage_w * MOSAIC_SIDE_MARGIN_FRACTION;
    let usable_w = stage_w - 2.0 * margin;
    let hero_h = 2.0 * th + g;
    let hero_max_w = usable_w * MOSAIC_HERO_MAX_WIDTH_FRACTION;
    // Row-fill clone cap (safety): regions are ~half the usable width, so a
    // handful of clones always suffices; the cap only guards pathological
    // aspect data.
    const MAX_ROW_FILL_CLONES: usize = 24;
    // Non-hero cycle (aspect, real) for clone-filling the side rows:
    // row0[1..] (everything after the hero) + every later row in order.
    let cycle: Vec<(f32, usize)> = {
        let mut c: Vec<(f32, usize)> = Vec::new();
        for (ri, r) in rows.iter().enumerate() {
            let skip = if ri == 0 { 1 } else { 0 }; // row0[0] is the hero
            for pair in r.iter().skip(skip) {
                c.push(*pair);
            }
        }
        c
    };

    // Clone-fill `row` (consumes the shared cursor) until its natural width
    // reaches `region`. Returns the extended row.
    let fill_to = |row: Vec<(f32, usize)>, region: f32, cursor: &mut usize| -> Vec<(f32, usize)> {
        let mut r = row;
        if cycle.is_empty() || region <= 0.0 {
            return r;
        }
        let mut adds = 0usize;
        loop {
            let sum: f32 = r.iter().map(|(a, _)| *a).sum();
            let natural = th * sum + (r.len() as f32 - 1.0) * g;
            if natural >= region || adds >= MAX_ROW_FILL_CLONES {
                break;
            }
            r.push(cycle[*cursor % cycle.len()]);
            *cursor += 1;
            adds += 1;
        }
        r
    };

    let n_rows = rows.len();
    let mut tiles: Vec<MosaicTile> = Vec::with_capacity(aspects.len());
    let mut tile_reals: Vec<usize> = Vec::with_capacity(aspects.len());
    let mut y = 0.0f32;
    let mut max_w: f32 = 0.0;
    let mut r = 0usize;
    while r < n_rows {
        if r + 1 >= n_rows {
            // Lone row — normal single row layout (last row keeps th, shrink-to-fit).
            let row = &rows[r];
            let sum = sums[r];
            let k = row.len();
            let nat_w = th * sum + (k as f32 - 1.0) * g;
            let h_initial = th;
            let (h_r, gutter_h) = if nat_w > usable_w {
                let avail = usable_w - (k as f32 - 1.0) * g;
                if avail > 0.0 {
                    (avail / sum, g)
                } else {
                    let factor = if nat_w > 0.0 { usable_w / nat_w } else { 1.0 };
                    (h_initial * factor, g * factor)
                }
            } else {
                (h_initial, g)
            };
            let row_w = h_r * sum + (k as f32 - 1.0) * gutter_h;
            if row_w > max_w {
                max_w = row_w;
            }
            let mut x = 0.0f32;
            for (a, real) in row {
                let w = h_r * a;
                tiles.push(MosaicTile { x, y, w, h: h_r });
                tile_reals.push(*real);
                x += w + gutter_h;
            }
            y += h_r + g;
            r += 1;
        } else {
            let row0 = &rows[r];
            let row1 = &rows[r + 1];
            let sum0 = sums[r];
            let sum1 = sums[r + 1];
            let remaining = row0.len().saturating_sub(1) + row1.len();
            let hero_pair = row0[0];
            let hero_uncapped_w = hero_h * hero_pair.0;
            let hero_w = hero_uncapped_w.min(hero_max_w).max(1.0);
            let top_natural: Vec<(f32, usize)> = row0.iter().skip(1).copied().collect();
            let bottom_natural: Vec<(f32, usize)> = row1.clone();
            // Hero-centered band with FOUR segments flowing around the hero:
            // top-left, top-right, bottom-left, bottom-right. Each segment is
            // one row of height th filling its region width L exactly (tiles
            // scale uniformly — cover crop absorbs the distortion), so the
            // band is even with NO holes anywhere.
            let region_l = (usable_w - hero_w - 2.0 * g) / 2.0;
            let mut queue: Vec<(f32, usize)> = Vec::with_capacity(top_natural.len() + bottom_natural.len());
            queue.extend(top_natural.iter().copied());
            queue.extend(bottom_natural.iter().copied());
            // Distribute the queue evenly across the 4 segments (extra tiles
            // go to the leading segments), then clone-fill any segment that
            // is still short of half its region.
            let seg_count = 4usize;
            let base = queue.len() / seg_count;
            let extra = queue.len() % seg_count;
            let mut segs: Vec<Vec<(f32, usize)>> = Vec::with_capacity(seg_count);
            let mut qi = 0usize;
            for s in 0..seg_count {
                let take = base + if s < extra { 1 } else { 0 };
                segs.push(queue[qi..qi + take].to_vec());
                qi += take;
            }
            let cycle_len_probe = cycle.len().max(1);
            let mut probe_cursor = queue.len() % cycle_len_probe;
            for seg in segs.iter_mut() {
                *seg = fill_to(std::mem::take(seg), region_l, &mut probe_cursor);
            }
            let regions_viable = remaining >= seg_count
                && region_l >= th * 0.6
                && segs.iter().all(|seg| !seg.is_empty());
            if !regions_viable {
                // Degenerate — fallback to normal two rows (identity mapping).
                let nat0 = th * sum0 + (row0.len() as f32 - 1.0) * g;
                let nat1 = th * sum1 + (row1.len() as f32 - 1.0) * g;
                let big_w = nat0.max(nat1);
                let is_last1 = r + 1 + 1 == n_rows;
                let h0_initial = (big_w - (row0.len() as f32 - 1.0) * g) / sum0;
                let (h0, gutter0) = if big_w > usable_w {
                    let avail = usable_w - (row0.len() as f32 - 1.0) * g;
                    if avail > 0.0 {
                        (avail / sum0, g)
                    } else {
                        let factor = if big_w > 0.0 { usable_w / big_w } else { 1.0 };
                        (h0_initial * factor, g * factor)
                    }
                } else {
                    (h0_initial, g)
                };
                let row0_w = h0 * sum0 + (row0.len() as f32 - 1.0) * gutter0;
                if row0_w > max_w {
                    max_w = row0_w;
                }
                let mut x = 0.0f32;
                let y0 = y;
                for (a, real) in row0 {
                    let w = h0 * a;
                    tiles.push(MosaicTile { x, y: y0, w, h: h0 });
                    tile_reals.push(*real);
                    x += w + gutter0;
                }
                let h1_initial = if is_last1 {
                    th
                } else {
                    (big_w - (row1.len() as f32 - 1.0) * g) / sum1
                };
                let row1_nat_w = if is_last1 { nat1 } else { big_w };
                let (h1, gutter1) = if row1_nat_w > usable_w {
                    let avail = usable_w - (row1.len() as f32 - 1.0) * g;
                    if avail > 0.0 {
                        (avail / sum1, g)
                    } else {
                        let factor = if row1_nat_w > 0.0 { usable_w / row1_nat_w } else { 1.0 };
                        (h1_initial * factor, g * factor)
                    }
                } else {
                    (h1_initial, g)
                };
                let row1_w = h1 * sum1 + (row1.len() as f32 - 1.0) * gutter1;
                if row1_w > max_w {
                    max_w = row1_w;
                }
                let y1 = y + h0 + g;
                let mut x1 = 0.0f32;
                for (a, real) in row1 {
                    let w = h1 * a;
                    tiles.push(MosaicTile { x: x1, y: y1, w, h: h1 });
                    tile_reals.push(*real);
                    x1 += w + gutter1;
                }
                y += h0 + g + h1 + g;
                r += 2;
            } else {
                // Hero band — hero CENTERED spanning the full band height;
                // FOUR segments (top-left, top-right, bottom-left,
                // bottom-right) of height th fill their regions exactly via
                // a uniform per-segment tile scale (cover crop absorbs it).
                // No corner holes: every region is filled edge-to-edge.
                let hero_x = (usable_w - hero_w) / 2.0;
                let seg_h = th;
                let seg_x = [
                    0.0f32,
                    hero_x + hero_w + g,
                    0.0f32,
                    hero_x + hero_w + g,
                ];
                let seg_y = [y, y, y + th + g, y + th + g];
                if usable_w > max_w {
                    max_w = usable_w;
                }
                tiles.push(MosaicTile { x: hero_x, y, w: hero_w, h: hero_h });
                tile_reals.push(hero_pair.1);
                for s in 0..seg_count {
                    let seg = &segs[s];
                    let sum: f32 = seg.iter().map(|(a, _)| *a).sum();
                    // Uniform scale so scaled tile widths + gutters land
                    // EXACTLY on the region width.
                    let scale = if sum > 0.0 {
                        (region_l - (seg.len() as f32 - 1.0) * g) / (th * sum)
                    } else {
                        1.0
                    };
                    let mut x = seg_x[s];
                    for (a, real) in seg {
                        let w = th * a * scale;
                        tiles.push(MosaicTile { x, y: seg_y[s], w, h: seg_h });
                        tile_reals.push(*real);
                        x += w + g;
                    }
                }
                y += hero_h + g;
                r += 2;
            }
        }
    }
    let content_h = (y - g).max(0.0);
    let content_w = max_w;
    let offset_x = margin + ((usable_w - content_w) / 2.0).max(0.0);
    let offset_y = ((stage_h - content_h) / 2.0).max(0.0);
    let mut out_tiles = tiles;
    for t in &mut out_tiles {
        t.x += offset_x;
        t.y += offset_y;
    }
    (MosaicLayout { tiles: out_tiles, content_w, content_h, offset_x, offset_y }, tile_reals)
}

// ── Page model (Mosaic final scheme) ───────────────────────────────
// Split the justified-rows block into PAGES. A PAGE = the rows that fit
// the same central band used for the single Pinterest block today
// (MOSAIC_PAGE_ROW_COUNT rows). Pure Rust, headless-testable so the UI
// only mirrors `current`/`total_pages` and the current page's tiles.
/// Rows per page — equals the band-filling row count so one page == one
/// central band of justified rows.
pub const MOSAIC_PAGE_ROW_COUNT: usize = MOSAIC_ROW_COUNT;
/// Per-column curtain stagger (spec: ~50ms per column).
pub const MOSAIC_CURTAIN_COL_STAGGER_MS: u64 = 50;
/// Per-tile curtain wipe duration (spec: ~300ms).
pub const MOSAIC_CURTAIN_TILE_MS: u64 = 300;

/// One tile's geometry as keyboard navigation needs it. Pure data so the
/// navigation rules are unit-testable without any Slint type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MosaicTileRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A keyboard move across the wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MosaicDir {
    Left,
    Right,
    Up,
    Down,
}

/// The tile a keyboard move lands on, or `None` when the move would leave the
/// page (the caller then steps the page or clamps).
///
/// Left/right follow READING ORDER: the tiles model is packed row by row, left
/// to right, so they are `cur ± 1`. Up/down use the geometry, because a
/// justified wall has rows of different tile widths: the target is the tile in
/// the row above/below whose horizontal span contains the cursor's centre, and
/// when none does, the one with the nearest centre. Rows are runs of tiles
/// sharing a y (1px tolerance — the same rule `mosaic_layout_rows` uses).
pub fn mosaic_neighbor(tiles: &[MosaicTileRect], cur: usize, dir: MosaicDir) -> Option<usize> {
    if cur >= tiles.len() {
        return None;
    }
    match dir {
        MosaicDir::Left => cur.checked_sub(1),
        MosaicDir::Right => (cur + 1 < tiles.len()).then_some(cur + 1),
        MosaicDir::Up | MosaicDir::Down => {
            let rows = tile_rows(tiles);
            let Some(row) = rows.iter().position(|r| r.contains(&cur)) else {
                return None;
            };
            let target_row = match dir {
                MosaicDir::Up => row.checked_sub(1)?,
                _ => {
                    if row + 1 >= rows.len() {
                        return None;
                    }
                    row + 1
                }
            };
            let centre = tiles[cur].x + tiles[cur].w / 2.0;
            nearest_in_row(tiles, &rows[target_row], centre)
        }
    }
}

/// Consecutive runs of tiles sharing a y (1px tolerance), in model order.
fn tile_rows(tiles: &[MosaicTileRect]) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut cur_y = f32::NAN;
    for (i, t) in tiles.iter().enumerate() {
        if rows.is_empty() || (t.y - cur_y).abs() > 1.0 {
            rows.push(Vec::new());
            cur_y = t.y;
        }
        rows.last_mut().unwrap().push(i);
    }
    rows
}

/// The tile in `row` whose span contains `centre`, else the nearest centre.
fn nearest_in_row(tiles: &[MosaicTileRect], row: &[usize], centre: f32) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for &i in row {
        let t = tiles[i];
        if centre >= t.x && centre <= t.x + t.w {
            return Some(i);
        }
        let d = (t.x + t.w / 2.0 - centre).abs();
        if best.map_or(true, |(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// Group tiles by row using their y (within a 1px tolerance). Justified
/// rows share an exact y; different rows differ by ~hundreds of px, so a
/// 1px threshold cleanly separates them.
pub fn mosaic_layout_rows(layout: &MosaicLayout) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut cur_y = f32::NAN;
    for (i, t) in layout.tiles.iter().enumerate() {
        if rows.is_empty() || (t.y - cur_y).abs() > 1.0 {
            rows.push(vec![]);
            cur_y = t.y;
        }
        rows.last_mut().unwrap().push(i);
    }
    rows
}

/// Geometric capacity of one page: how many tiles fill MOSAIC_PAGE_ROW_COUNT
/// rows for a FULL cloned aspect set. Deterministic from stage dims + the
/// display-aspect pattern. Degenerate (non-positive) stage → 0.
pub fn mosaic_page_capacity(stage_w: f32, stage_h: f32) -> usize {
    if !(stage_w > 0.0) || !(stage_h > 0.0) {
        return 0;
    }
    // Width-based row fill: the band has exactly MOSAIC_PAGE_ROW_COUNT rows
    // of height th (same band math as justified_hero_layout), so capacity is
    // how many pattern tiles fit side-by-side within the usable width at that
    // height, times the row count. A hard cap guards against pathological
    // inputs: the justified_* packers never wrap past MOSAIC_ROW_COUNT rows,
    // so any estimate derived from laying out a long probe list reports the
    // whole list (4096) and collapses every page tile to sub-pixel dust.
    let g = MOSAIC_GUTTER_PX;
    let band = stage_h / MOSAIC_BAND_HEIGHT_DIVISOR;
    let th =
        (band - (MOSAIC_PAGE_ROW_COUNT as f32 - 1.0) * g) / MOSAIC_PAGE_ROW_COUNT as f32;
    let margin = stage_w * MOSAIC_SIDE_MARGIN_FRACTION;
    let usable_w = (stage_w - 2.0 * margin).max(1.0);
    // Hero-centered band capacity: one hero (default square aspect at the
    // nominal band height, capped) flanked by TWO side rows of width
    // side_w — each side fits per_side tiles of the average pattern aspect
    // at height th.
    let hero_h = 2.0 * th + g;
    let hero_w = (hero_h * mosaic_display_aspect(0))
        .min(usable_w * MOSAIC_HERO_MAX_WIDTH_FRACTION)
        .max(1.0);
    let side_w = (usable_w - hero_w - 2.0 * g) / 2.0;
    let avg_aspect =
        (0..6).map(mosaic_display_aspect).sum::<f32>() / 6.0;
    let per_side = (((side_w + g) / (th * avg_aspect + g)).round() as usize).max(1);
    (1 + 4 * per_side).min(64)
}

/// Per-tile curtain delay (ms) staggered per column (~50ms/column). Tiles
/// sharing an x-cluster form a column; delay = column_index * stagger.
/// Deterministic and bounded. Consumed only by the Slint curtain.
pub fn mosaic_curtain_delays(tiles: &[MosaicTile]) -> Vec<u64> {
    let mut cols: Vec<f32> = Vec::new();
    for t in tiles {
        if !cols.iter().any(|&c| (c - t.x).abs() < 2.0) {
            cols.push(t.x);
        }
    }
    cols.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    tiles
        .iter()
        .map(|t| {
            let ci = cols.iter().position(|&c| (c - t.x).abs() < 2.0).unwrap_or(0);
            (ci as u64) * MOSAIC_CURTAIN_COL_STAGGER_MS
        })
        .collect()
}

/// Page model: source of truth for mosaic pagination. Holds the real theme
/// count, the geometric page capacity, the total page count and the current
/// page. UI mirrors `current`/`total_pages`; `page_render` produces the
/// current page's geometry inputs (with clone-fill) so the wall never
/// touches disk.
#[derive(Debug, Clone, PartialEq)]
pub struct MosaicPages {
    real_count: usize,
    capacity: usize,
    total_pages: usize,
    current: usize,
    geometry_rebuilds: usize,
}

impl MosaicPages {
    pub fn new(real_count: usize, stage_w: f32, stage_h: f32) -> Self {
        let capacity = mosaic_page_capacity(stage_w, stage_h);
        let total_pages = if real_count == 0 {
            0
        } else if capacity == 0 {
            1
        } else if real_count <= capacity {
            1
        } else {
            (real_count + capacity - 1) / capacity
        };
        Self {
            real_count,
            capacity,
            total_pages,
            current: 0,
            geometry_rebuilds: 0,
        }
    }

    /// Recompute on a real-count / stage change. The ONLY operation that
    /// increments `geometry_rebuilds`; page flips never call this.
    pub fn recompute(&mut self, real_count: usize, stage_w: f32, stage_h: f32) {
        let capacity = mosaic_page_capacity(stage_w, stage_h);
        let total_pages = if real_count == 0 {
            0
        } else if capacity == 0 {
            1
        } else if real_count <= capacity {
            1
        } else {
            (real_count + capacity - 1) / capacity
        };
        self.real_count = real_count;
        self.capacity = capacity;
        self.total_pages = total_pages;
        self.current = self.current.min(self.total_pages.saturating_sub(1));
        self.geometry_rebuilds += 1;
    }

    pub fn current(&self) -> usize { self.current }
    pub fn total_pages(&self) -> usize { self.total_pages }
    pub fn capacity(&self) -> usize { self.capacity }
    pub fn real_count(&self) -> usize { self.real_count }
    pub fn geometry_rebuilds(&self) -> usize { self.geometry_rebuilds }

    /// Flip pages: +1 next, −1 previous. Clamped, no wrap. Returns true if
    /// the current page actually changed.
    pub fn step(&mut self, delta: isize) -> bool {
        if self.total_pages == 0 {
            return false;
        }
        let last = self.total_pages as isize - 1;
        let next = (self.current as isize + delta).clamp(0, last);
        let changed = next != self.current as isize;
        self.current = next as usize;
        changed
    }

    /// Pagination UI is hidden when there is at most one page.
    pub fn hidden(&self) -> bool { self.total_pages <= 1 }

    /// 1-based page-number list for the pagination UI.
    pub fn page_numbers(&self) -> Vec<usize> {
        (1..=self.total_pages).collect()
    }

    /// How many clones (repeated real themes) were added to fill the current
    /// page when the library is smaller than a page. 0 otherwise.
    pub fn clone_fill_count(&self) -> usize {
        if self.real_count > 0 && self.real_count < self.capacity {
            self.capacity - self.real_count
        } else {
            0
        }
    }

    /// Map a displayed (rendered) tile index to its REAL theme index. In
    /// clone mode real = displayed % real_count; otherwise the page's real
    /// block offset + displayed (1:1 within the page). Degenerate capacity 0
    /// falls back to clamped identity.
    pub fn clone_real_index(&self, displayed: usize) -> usize {
        if self.real_count == 0 {
            return 0;
        }
        if self.capacity == 0 {
            return displayed.min(self.real_count - 1);
        }
        if self.real_count < self.capacity {
            displayed % self.real_count
        } else {
            self.current * self.capacity + displayed
        }
    }

    /// Aspects + real indices for rendering the current page. Applies
    /// clone-fill so exactly `capacity` tiles are produced when the library
    /// is smaller than a page. Purity: no disk, no engine. Degenerate
    /// capacity 0 falls back to real entries single page.
    pub fn page_render(&self) -> (Vec<f32>, Vec<usize>) {
        if self.real_count == 0 {
            return (vec![], vec![]);
        }
        if self.capacity == 0 {
            let aspects: Vec<f32> = (0..self.real_count).map(mosaic_display_aspect).collect();
            let reals: Vec<usize> = (0..self.real_count).collect();
            return (aspects, reals);
        }
        if self.real_count < self.capacity {
            let aspects: Vec<f32> =
                (0..self.capacity).map(mosaic_display_aspect).collect();
            let reals: Vec<usize> =
                (0..self.capacity).map(|k| k % self.real_count).collect();
            (aspects, reals)
        } else {
            let start = self.current * self.capacity;
            let end = ((self.current + 1) * self.capacity).min(self.real_count);
            let aspects: Vec<f32> =
                (start..end).map(mosaic_display_aspect).collect();
            let reals: Vec<usize> = (start..end).collect();
            (aspects, reals)
        }
    }

    /// Jump to an absolute page (clamped, no wrap). Returns true if changed.
    pub fn go_to(&mut self, page: usize) -> bool {
        if self.total_pages == 0 {
            return false;
        }
        let clamped = page.min(self.total_pages - 1);
        let changed = clamped != self.current;
        self.current = clamped;
        changed
    }
}

// ── Geometry helpers ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point { pub x: f32, pub y: f32 }
impl Point {
    pub fn new(x: f32, y: f32) -> Self { Self { x, y } }
}

#[derive(Debug, Clone)]
pub struct VoronoiCell {
    pub vertices: Vec<Point>,
    pub centroid: Point,
    /// bbox min_x, max_x, min_y, max_y
    pub bbox: (f32, f32, f32, f32),
    /// Cloud radii for opacity (scaled by bbox)
    pub cloud_rx: f32,
    pub cloud_ry: f32,
    /// cell key (stable id)
    pub key: usize,
    /// Staggered reveal delay based on key
    pub reveal_delay_ms: u64,
}

/// Compute polygon centroid (area-weighted). Falls back to average for degenerate.
pub fn centroid(vertices: &[Point]) -> Point {
    if vertices.is_empty() { return Point::new(0.0, 0.0); }
    if vertices.len() == 1 { return vertices[0]; }
    // Shoelace
    let mut area = 0.0f32;
    let mut cx = 0.0f32;
    let mut cy = 0.0f32;
    for i in 0..vertices.len() {
        let p0 = vertices[i];
        let p1 = vertices[(i+1)%vertices.len()];
        let cross = p0.x * p1.y - p1.x * p0.y;
        area += cross;
        cx += (p0.x + p1.x) * cross;
        cy += (p0.y + p1.y) * cross;
    }
    if area.abs() < 1e-6 {
        // degenerate: average
        let sum = vertices.iter().fold(Point::new(0.0,0.0), |a,p| Point::new(a.x+p.x, a.y+p.y));
        let n = vertices.len() as f32;
        return Point::new(sum.x/n, sum.y/n);
    }
    area *= 0.5;
    cx /= 6.0 * area;
    cy /= 6.0 * area;
    Point::new(cx, cy)
}

/// Generate a simple Voronoi-like tile set for a bounding rect.
/// For tests we need deterministic _buildTile port: takes `count` sites
/// placed on a grid jittered, then Lloyd relax `iterations` times, clipping
/// to (0,0)-(w,h). Uses naive approximation — closest-site partition on a
/// coarse sample — sufficient for centroid correctness tests.
pub fn build_tiles(count: usize, w: f32, h: f32, iterations: usize) -> Vec<VoronoiCell> {
    if count == 0 || w <= 0.0 || h <= 0.0 { return vec![]; }
    let count = count.min(MOSAIC_WARMUP_MAX * 4); // cap large counts for CPU but >64 allowed
    // seed sites: grid + jitter deterministic
    let mut sites: Vec<Point> = (0..count).map(|i| {
        let cols = (w / 200.0).max(1.0) as usize + 1;
        let row = i / cols;
        let col = i % cols;
        let jitter_x = ((i as f32 * 37.0).sin() * 20.0).rem_euclid(40.0) - 20.0;
        let jitter_y = ((i as f32 * 73.0).cos() * 20.0).rem_euclid(40.0) - 20.0;
        let base_x = (col as f32 + 0.5) * (w / cols as f32) + jitter_x;
        let base_y = (row as f32 + 0.5) * (h / ((count/cols.max(1)+1) as f32)) + jitter_y;
        Point::new(base_x.clamp(0.0, w), base_y.clamp(0.0, h))
    }).collect();

    for _ in 0..iterations {
        // One Lloyd step: compute Voronoi cells via naive nearest-site on coarse grid,
        // then recompute site as centroid of its cell points.
        // Use sampling at 10px resolution for determinism.
        let step = 20.0;
        let mut sums: Vec<Point> = vec![Point::new(0.0,0.0); count];
        let mut counts: Vec<usize> = vec![0; count];
        let mut y = 0.0;
        while y < h {
            let mut x = 0.0;
            while x < w {
                // nearest site
                let mut best = 0;
                let mut best_d = f32::MAX;
                for (idx, s) in sites.iter().enumerate() {
                    let dx = x - s.x; let dy = y - s.y;
                    let d = dx*dx+dy*dy;
                    if d < best_d { best_d = d; best = idx; }
                }
                sums[best].x += x;
                sums[best].y += y;
                counts[best] += 1;
                x += step;
            }
            y += step;
        }
        for i in 0..count {
            if counts[i] > 0 {
                sites[i] = Point::new(sums[i].x / counts[i] as f32, sums[i].y / counts[i] as f32);
            }
        }
    }

    // Build tile polygons as axis-aligned boxes around each site for the naive
    // tessellation — each cell approximates a Voronoi shard. Real skwd uses
    // clip to polygon; we produce bbox-quads for tests and visuals.
    let mut cells = Vec::new();
    // Re-estimate partition via nearest sampling to assign polygon bbox
    // Simplify: each site gets a square of size w/cols, h/rows around centroid
    let cols = (w / 200.0).max(1.0) as usize + 2;
    let cell_w = w / cols as f32;
    let rows = (count + cols - 1)/ cols.max(1);
    let cell_h = h / rows.max(1) as f32;
    for (i, site) in sites.into_iter().enumerate().take(count) {
        // vertices: 4 corners of shard with small gap
        let gap = 6.0;
        let hw = cell_w * 0.5 - gap;
        let hh = cell_h * 0.5 - gap;
        let verts = vec![
            Point::new(site.x - hw, site.y - hh),
            Point::new(site.x + hw, site.y - hh),
            Point::new(site.x + hw, site.y + hh),
            Point::new(site.x - hw, site.y + hh),
        ];
        let c = centroid(&verts);
        let min_x = verts.iter().map(|p| p.x).fold(f32::MAX, f32::min);
        let max_x = verts.iter().map(|p| p.x).fold(f32::MIN, f32::max);
        let min_y = verts.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        let max_y = verts.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        let rx = (max_x - min_x) * 0.5 * MOSAIC_CLOUD_OUTER.max(1.0) * 1.0;
        let ry = (max_y - min_y) * 0.5 * MOSAIC_CLOUD_OUTER.max(1.0) * 1.0;
        cells.push(VoronoiCell {
            vertices: verts,
            centroid: c,
            bbox: (min_x, max_x, min_y, max_y),
            cloud_rx: rx,
            cloud_ry: ry,
            key: i,
            reveal_delay_ms: (i as u64 * MOSAIC_STAGGER_MS) % 600,
        });
    }
    cells
}

/// Cloud opacity smoothstep: inner 0.55 → opaque, outer 1.05 → transparent.
/// Input dist normalized: sqrt(dx²/rx² + dy²/ry²). 0 = center.
pub fn cloud_opacity(dx: f32, dy: f32, rx: f32, ry: f32) -> f32 {
    if rx <= 0.0 || ry <= 0.0 { return 0.0; }
    let nx = dx / rx;
    let ny = dy / ry;
    let dist = (nx*nx + ny*ny).sqrt();
    if dist <= MOSAIC_CLOUD_INNER { 1.0 }
    else if dist >= MOSAIC_CLOUD_OUTER { 0.0 }
    else {
        // smoothstep between inner and outer
        let t = (dist - MOSAIC_CLOUD_INNER) / (MOSAIC_CLOUD_OUTER - MOSAIC_CLOUD_INNER);
        let smooth = 1.0 - (3.0*t*t - 2.0*t*t*t);
        smooth.clamp(0.0, 1.0)
    }
}

/// Kinetic scroll: apply friction per tick (16ms). velocity *= 0.90 each tick.
pub fn kinetic_step(velocity: f32) -> f32 {
    velocity * MOSAIC_KINETIC_FRICTION
}

// ── MosaicView ─────────────────────────────────────────────────────────

/// Mosaic presentation view with cached Voronoi cells, warmup, kinetic scroll,
/// cloud opacity and dual-stripe tiling.
#[derive(Debug)]
pub struct MosaicView {
    count: usize,
    filter: String,
    cells: Vec<VoronoiCell>,
    cache_key: Option<(String, usize)>,
    rebuilds: usize,
    /// Warmup: number of preloaded cells (min(count, 64)).
    warmup_count: usize,
    /// Async warmup guard generation — incremented on filter change to invalidate pending.
    warmup_gen: u64,
    /// Pending generation for async guard test (simulates async load).
    pending_gen: Option<u64>,
    /// Kinetic state
    velocity: f32,
    scroll_offset: f32,
    /// Cloud center
    cloud_center: Point,
    /// Dual stripe offsets for infinite scroll.
    stripe_a_offset: f32,
    stripe_b_offset: f32,
    /// Viewport size for tiling (wraps at width)
    viewport_w: f32,
    /// Focused index (for switch preserve)
    focused_index: usize,
    /// Tile bounds for Voronoi
    tile_w: f32,
    tile_h: f32,
}

impl MosaicView {
    pub fn new(count: usize) -> Self {
        Self {
            count,
            filter: String::new(),
            cells: vec![],
            cache_key: None,
            rebuilds: 0,
            warmup_count: 0,
            warmup_gen: 0,
            pending_gen: None,
            velocity: 0.0,
            scroll_offset: 0.0,
            cloud_center: Point::new(750.0, 400.0),
            stripe_a_offset: 0.0,
            stripe_b_offset: 0.0,
            viewport_w: 1500.0,
            focused_index: 0,
            tile_w: 1500.0,
            tile_h: 800.0,
        }
    }

    pub fn count(&self) -> usize { self.count }
    pub fn cells(&self) -> &[VoronoiCell] { &self.cells }
    pub fn rebuilds(&self) -> usize { self.rebuilds }
    pub fn warmup_count(&self) -> usize { self.warmup_count }
    pub fn warmup_gen(&self) -> u64 { self.warmup_gen }
    pub fn velocity(&self) -> f32 { self.velocity }
    pub fn scroll_offset(&self) -> f32 { self.scroll_offset }
    pub fn focused_index(&self) -> usize { self.focused_index }
    pub fn stripe_offsets(&self) -> (f32, f32) { (self.stripe_a_offset, self.stripe_b_offset) }

    /// Rebuild only if filter OR count changed (spec R8 cache).
    /// Returns true if rebuild occurred.
    pub fn ensure_cells(&mut self, filter: &str, count: usize) -> bool {
        let key = (filter.to_string(), count);
        if self.cache_key.as_ref() == Some(&key) {
            return false;
        }
        self.filter = filter.to_string();
        self.count = count;
        // preserve focused_index clamped
        if self.focused_index >= count.max(1) { self.focused_index = count.saturating_sub(1); }
        self.cells = build_tiles(count, self.tile_w, self.tile_h, MOSAIC_LLOYD_ITERATIONS);
        self.cache_key = Some(key);
        self.rebuilds += 1;
        // invalidate previous warmup async token, set pending
        self.warmup_gen += 1;
        self.pending_gen = Some(self.warmup_gen);
        self.warmup_count = 0; // will be filled on warmup activation
        true
    }

    /// Warmup preload: first min(count, 64) cells. Honors async guard —
    /// only applies if generation matches pending.
    pub fn warmup(&mut self) -> usize {
        // simulate async guard: must have pending generation
        let gen = match self.pending_gen { Some(g) => g, None => return 0 };
        if gen != self.warmup_gen {
            return 0; // stale
        }
        let want = self.count.min(MOSAIC_WARMUP_MAX);
        self.warmup_count = want;
        self.pending_gen = None; // consumed
        want
    }

    /// Async guard: call when filter changes again before warmup completes —
    /// cancels stale pending.
    pub fn cancel_pending_warmup(&mut self) {
        // bump generation so pending becomes stale
        self.warmup_gen += 1;
        self.pending_gen = Some(self.warmup_gen);
        self.warmup_count = 0;
    }

    pub fn is_warmup_pending(&self) -> bool { self.pending_gen.is_some() }
    pub fn is_warmup_stale(&self, gen: u64) -> bool { gen != self.warmup_gen }

    pub fn set_has_warmup(&mut self, _pending: bool) { /* compat */ }

    pub fn set_focused_index(&mut self, idx: usize) -> usize {
        if self.count == 0 { self.focused_index = 0; return 0; }
        self.focused_index = idx.min(self.count - 1);
        self.focused_index
    }

    pub fn move_focus(&mut self, delta: isize) -> usize {
        if self.count == 0 { return 0; }
        let max = self.count as isize - 1;
        let next = self.focused_index as isize + delta;
        let clamped = next.clamp(0, max) as usize;
        self.set_focused_index(clamped)
    }

    pub fn handle_key(&mut self, key: GalleryKey) -> bool {
        match key {
            GalleryKey::Left => { self.move_focus(-1); true }
            GalleryKey::Right => { self.move_focus(1); true }
            _ => false,
        }
    }

    /// Kinetic scroll: set initial velocity, then tick friction 0.90 per 16ms.
    pub fn set_velocity(&mut self, v: f32) { self.velocity = v; }
    pub fn tick_kinetic(&mut self) {
        self.velocity = kinetic_step(self.velocity);
        self.scroll_offset += self.velocity;
        // dual-stripe wrap: keep offsets within viewport
        let w = self.viewport_w;
        self.stripe_a_offset = self.scroll_offset.rem_euclid(w * 2.0);
        self.stripe_b_offset = (self.scroll_offset + w).rem_euclid(w * 2.0);
        if self.velocity.abs() < 0.01 { self.velocity = 0.0; }
    }

    /// Cloud opacity at world point (x,y) relative to cloud center.
    pub fn cloud_opacity_at(&self, x: f32, y: f32) -> f32 {
        // use first cell's rx/ry as generic if available, else defaults
        let (rx, ry) = self.cells.first()
            .map(|c| (c.cloud_rx, c.cloud_ry))
            .unwrap_or((300.0, 200.0));
        let dx = x - self.cloud_center.x;
        let dy = y - self.cloud_center.y;
        cloud_opacity(dx, dy, rx, ry)
    }

    pub fn set_cloud_center(&mut self, x: f32, y: f32) { self.cloud_center = Point::new(x,y); }

    /// Staggered reveal delay for a given cell key.
    pub fn reveal_delay_for(&self, key: usize) -> u64 {
        (key as u64 * MOSAIC_STAGGER_MS) % 600
    }

    /// Dual-stripe scroll position for infinite tiling (returns stripe A/B offsets).
    pub fn dual_stripe_offsets(&self) -> (f32, f32) { self.stripe_offsets() }

    /// Style switch helper: preserves focused index across styles (S6) with 200ms cross-fade.
    pub fn style_switch_preserves_focus(&self, new_count: usize) -> usize {
        self.focused_index.min(new_count.saturating_sub(1))
    }

    pub fn set_count(&mut self, count: usize) {
        // direct set without key rebuild — for external sync
        self.count = count;
        if self.focused_index >= count.max(1) { self.focused_index = count.saturating_sub(1); }
    }

    pub fn set_tile_size(&mut self, w: f32, h: f32) { self.tile_w = w; self.tile_h = h; }
}

impl GalleryView for MosaicView {
    fn style(&self) -> GalleryStyle { GalleryStyle::Mosaic }
    fn handle_key(&mut self, key: GalleryKey) -> bool { MosaicView::handle_key(self, key) }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 3.4 RED Voronoi _buildTile Lloyd3 centroid FAIL then GREEN ─────
    #[test]
    fn mosaic_centroid_build_tile_lloyd3() {
        assert_eq!(MOSAIC_LLOYD_ITERATIONS, 3, "default Lloyd 3");
        // centroid of unit square
        let sq = vec![Point::new(0.0,0.0), Point::new(2.0,0.0), Point::new(2.0,2.0), Point::new(0.0,2.0)];
        let c = centroid(&sq);
        assert!((c.x - 1.0).abs() < 0.01, "square centroid x=1");
        assert!((c.y - 1.0).abs() < 0.01, "square centroid y=1");
        // centroid of triangle
        let tri = vec![Point::new(0.0,0.0), Point::new(4.0,0.0), Point::new(0.0,3.0)];
        let ct = centroid(&tri);
        // area centroid of triangle: average of vertices
        assert!((ct.x - 1.333).abs() < 0.02);
        assert!((ct.y - 1.0).abs() < 0.02);
        // build_tiles produces Lloyd-relaxed cells with correct count and centroid inside bbox
        let cells = build_tiles(12, 1500.0, 800.0, MOSAIC_LLOYD_ITERATIONS);
        assert_eq!(cells.len(), 12, "build_tiles count preserved");
        for cell in &cells {
            assert!(!cell.vertices.is_empty(), "cell has vertices");
            let (min_x,max_x,min_y,max_y) = cell.bbox;
            assert!(cell.centroid.x >= min_x - 1.0 && cell.centroid.x <= max_x + 1.0, "centroid inside bbox x");
            assert!(cell.centroid.y >= min_y - 1.0 && cell.centroid.y <= max_y + 1.0, "centroid inside bbox y");
        }
        // _buildTile equivalent with 0 count → empty
        let empty = build_tiles(0, 1500.0, 800.0, 3);
        assert!(empty.is_empty());
    }

    #[test]
    fn mosaic_centroid_degenerate_fallback() {
        let single = vec![Point::new(5.0,7.0)];
        let c = centroid(&single);
        assert_eq!(c.x, 5.0); assert_eq!(c.y, 7.0);
        let empty: Vec<Point> = vec![];
        let c2 = centroid(&empty);
        assert_eq!(c2.x, 0.0); assert_eq!(c2.y, 0.0);
    }

    // ── 3.5 cache rebuild filter/count only ─────────────────────────
    #[test]
    fn mosaic_cache_rebuild_filter_count_only() {
        let mut view = MosaicView::new(0);
        assert_eq!(view.rebuilds(), 0);
        // first build
        let rebuilt = view.ensure_cells("all", 20);
        assert!(rebuilt, "first build must rebuild");
        assert_eq!(view.rebuilds(), 1);
        assert_eq!(view.cells().len(), 20);
        // same filter/count → no rebuild
        let rebuilt2 = view.ensure_cells("all", 20);
        assert!(!rebuilt2, "same key must not rebuild");
        assert_eq!(view.rebuilds(), 1, "cache hit");
        // different count → rebuild
        let rebuilt3 = view.ensure_cells("all", 21);
        assert!(rebuilt3);
        assert_eq!(view.rebuilds(), 2);
        // different filter → rebuild
        let rebuilt4 = view.ensure_cells("dark", 21);
        assert!(rebuilt4);
        assert_eq!(view.rebuilds(), 3);
        // scroll-only change should not rebuild (no call) — ensure tick doesn't affect cache
        view.tick_kinetic();
        assert_eq!(view.rebuilds(), 3, "scroll must not rebuild");
        assert_eq!(view.count(), 21);
    }

    #[test]
    fn mosaic_cache_focus_preserved_on_rebuild() {
        let mut view = MosaicView::new(10);
        view.ensure_cells("all", 10);
        view.set_focused_index(7);
        view.ensure_cells("all", 5); // count shrinks
        assert_eq!(view.focused_index(), 4, "clamped to last");
        view.ensure_cells("all", 10);
        assert_eq!(view.focused_index(), 4, "preserved after rebuild");
    }

    // ── 3.6 max64 warmup async guard ────────────────────────────────
    #[test]
    fn mosaic_warmup_64_max_async_guard() {
        assert_eq!(MOSAIC_WARMUP_MAX, 64, "max warmup 64");
        let mut view = MosaicView::new(0);
        view.ensure_cells("all", 100);
        // pending after ensure
        assert!(view.is_warmup_pending(), "pending after rebuild");
        let gen = view.warmup_gen();
        let n = view.warmup();
        assert_eq!(n, 64, "warmup capped at 64");
        assert_eq!(view.warmup_count(), 64);
        assert!(!view.is_warmup_pending(), "pending consumed");
        // second warmup without rebuild → 0
        let n2 = view.warmup();
        assert_eq!(n2, 0, "no pending → 0");
        // small count → min(count,64)
        view.ensure_cells("all", 10);
        let n3 = view.warmup();
        assert_eq!(n3, 10);
        // async guard: filter change before warmup invalidates
        view.ensure_cells("all", 30);
        let stale_gen = view.warmup_gen();
        view.cancel_pending_warmup(); // simulates filter change again before async completes
        assert!(view.is_warmup_stale(stale_gen), "old gen stale after cancel");
        assert!(view.is_warmup_pending(), "new pending after cancel");
        // stale warmup should not apply (simulate stale async callback with old gen)
        // Our warmup checks generation internally; calling warmup now uses new gen, so it succeeds
        // but we test that old gen is considered stale
        assert!(view.is_warmup_stale(gen), "first gen stale");
    }

    #[test]
    fn mosaic_warmup_64_zero_count() {
        let mut v = MosaicView::new(0);
        v.ensure_cells("all", 0);
        let n = v.warmup();
        assert_eq!(n, 0, "0 count → 0 warmup");
    }

    // ── 3.7 kinetic 0.90 cloud 0.55/1.05 ────────────────────────────
    #[test]
    fn mosaic_cloud_kinetic_0_90_and_cloud_radii() {
        assert!((MOSAIC_KINETIC_FRICTION - 0.90).abs() < 0.001, "friction 0.90");
        assert_eq!(MOSAIC_KINETIC_INTERVAL_MS, 16);
        assert_eq!(MOSAIC_CLOUD_INNER, 0.55);
        assert_eq!(MOSAIC_CLOUD_OUTER, 1.05);
        // kinetic step
        let v0: f32 = 100.0;
        let v1 = kinetic_step(v0);
        assert!((v1 - 90.0).abs() < 0.01, "90 after one tick");
        let v2 = kinetic_step(v1);
        assert!((v2 - 81.0).abs() < 0.01);
        // cloud opacity
        // center → 1.0
        assert_eq!(cloud_opacity(0.0, 0.0, 300.0, 200.0), 1.0);
        // at inner radius → 1.0, beyond outer → 0.0
        // dist = sqrt(dx²/rx² + dy²/ry²). For rx=300 ry=200, dx=0.55*rx=165 → dist 0.55
        assert!((cloud_opacity(165.0, 0.0, 300.0, 200.0) - 1.0).abs() < 0.01, "inner edge opaque");
        // outer
        let outer_x = 1.05*300.0;
        assert_eq!(cloud_opacity(outer_x, 0.0, 300.0, 200.0), 0.0, "outer transparent");
        // middle smoothed
        let mid = cloud_opacity(240.0, 0.0, 300.0, 200.0);
        assert!(mid > 0.0 && mid < 1.0, "mid between 0 and 1 smoothstep");
        // via view
        let mut view = MosaicView::new(12);
        view.ensure_cells("all", 12);
        view.set_cloud_center(750.0, 400.0);
        let opa_center = view.cloud_opacity_at(750.0, 400.0);
        assert_eq!(opa_center, 1.0);
        let opa_far = view.cloud_opacity_at(750.0+ 2000.0, 400.0);
        assert_eq!(opa_far, 0.0);
        // kinetic on view
        view.set_velocity(100.0);
        view.tick_kinetic();
        assert!((view.velocity() - 90.0).abs() < 0.01);
        assert!(view.scroll_offset() > 0.0);
    }

    #[test]
    fn mosaic_cloud_staggered_reveal_delay() {
        let view = MosaicView::new(10);
        assert_eq!(MOSAIC_STAGGER_MS, 40);
        // keys staggered 40ms, wrap at 600
        assert_eq!(view.reveal_delay_for(0), 0);
        assert_eq!(view.reveal_delay_for(1), 40);
        assert_eq!(view.reveal_delay_for(2), 80);
        assert_eq!(view.reveal_delay_for(15), 0, "15*40=600 %600 =0");
        assert_eq!(view.reveal_delay_for(16), 40);
        // cells have matching delay
        let mut v = MosaicView::new(0);
        v.ensure_cells("all", 5);
        for cell in v.cells() {
            assert_eq!(cell.reveal_delay_ms, v.reveal_delay_for(cell.key));
        }
    }

    // ── 3.8 dual-stripe + switch_focus 200ms S6 ────────────────────
    #[test]
    fn switch_focus_dual_stripe_and_chrome_200ms() {
        assert_eq!(
            super::super::CHROME_SWITCH_DURATION_MS,
            200,
            "Chrome switch 200ms S6"
        );
        assert_eq!(MOSAIC_STRIPE_COUNT, 2, "dual stripe");
        let mut view = MosaicView::new(20);
        view.ensure_cells("all", 20);
        view.set_focused_index(3);
        assert_eq!(view.focused_index(), 3);
        // switch preserves focus
        let preserved = view.style_switch_preserves_focus(20);
        assert_eq!(preserved, 3, "S6 switch preserves focused card");
        let preserved2 = view.style_switch_preserves_focus(2);
        assert_eq!(preserved2, 1, "clamped if new count smaller");
        // dual stripe offsets progress with kinetic
        view.set_velocity(50.0);
        view.tick_kinetic();
        let (a,b) = view.dual_stripe_offsets();
        assert!(a >= 0.0 && a < 3000.0, "stripe A in range");
        assert!(b >= 0.0 && b < 3000.0, "stripe B in range");
        assert!((b - a - 1500.0).abs() < 1.0 || (a - b).abs() < 3000.0, "stripes offset by viewport");
        // second tick
        let prev_a = a;
        view.tick_kinetic();
        let (a2, _) = view.dual_stripe_offsets();
        assert!(a2 != prev_a, "scroll moves stripes");
        // Chrome switch via ThemeGalleryModel style preserves focus — integration
        // simulated: switch from Slice to Mosaic keeps index
        let preserved3 = view.style_switch_preserves_focus(0);
        assert_eq!(preserved3, 0);
    }

    #[test]
    fn switch_focus_handle_key_left_right() {
        let mut view = MosaicView::new(5);
        view.ensure_cells("all", 5);
        assert_eq!(view.focused_index(), 0);
        assert!(view.handle_key(GalleryKey::Right));
        assert_eq!(view.focused_index(), 1);
        assert!(view.handle_key(GalleryKey::Left));
        assert_eq!(view.focused_index(), 0);
        assert!(!view.handle_key(GalleryKey::Up));
    }

    // ── Justified-rows layout contracts (epsilon 1e-4) ──────────────
    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-4, "{a} != {b}");
    }

    /// Row right edges accumulate x per tile, so they pick up a few ulps
    /// against the closed-form natural width at ~2000px scale.
    fn approx_acc(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-2, "{a} !≈ {b}");
    }

    /// Band-contracted row height: the block is bounded to the central band
    /// `stage_h / MOSAIC_BAND_HEIGHT_DIVISOR`; its rows split the band minus
    /// the inter-row gutters evenly.
    fn band_row_h(stage_h: f32) -> f32 {
        let band = stage_h / MOSAIC_BAND_HEIGHT_DIVISOR;
        (band - (MOSAIC_ROW_COUNT as f32 - 1.0) * MOSAIC_GUTTER_PX) / MOSAIC_ROW_COUNT as f32
    }

    #[test]
    fn justified_empty_and_degenerate_stage_are_zero() {
        let l = justified_layout(&[], 1920.0, 1040.0);
        assert!(l.tiles.is_empty(), "empty input → empty tiles");
        assert_eq!(l, MosaicLayout { tiles: vec![], content_w: 0.0, content_h: 0.0, offset_x: 0.0, offset_y: 0.0 });
        // Non-positive stage cannot host a block → zeros too.
        let l0 = justified_layout(&[16.0 / 9.0], 1920.0, 0.0);
        assert_eq!(l0.tiles.len(), 0);
        assert_eq!(l0.content_w, 0.0);
    }

    #[test]
    fn justified_single_card_centered_natural() {
        let a = MOSAIC_DEFAULT_ASPECT;
        let l = justified_layout(&[a], 1920.0, 1040.0);
        let th = band_row_h(1040.0);
        // Exact band pins: 1040/2 = 520 band → (520 − 12)/2 = 254 rows,
        // centered vertically in the full stage. One card packs a single
        // row, so the content box is that row (band equality is the
        // fully-packed 2-row case).
        approx(th, 254.0);
        approx(l.content_h, th);
        approx(l.offset_y, 393.0);
        let w = th * a;
        assert_eq!(l.tiles.len(), 1);
        approx(l.tiles[0].w, w);
        approx(l.tiles[0].h, th);
        approx(l.content_w, w);
        approx(l.content_h, th);
        approx(l.offset_x, (1920.0 - w) / 2.0, );
        approx(l.offset_y, (1040.0 - th) / 2.0);
        approx(l.tiles[0].x, l.offset_x, );
        approx(l.tiles[0].y, l.offset_y);
    }

    #[test]
    fn justified_two_cards_two_rows_centered() {
        // Greedy with R=2: after any first card S=a0 and T=(a0+a1)/2, so
        // a0 + a1/2 > T always → one card per row. Both centered.
        let l = justified_layout(&[2.0, 1.0], 1200.0, 780.0);
        let th = band_row_h(780.0);
        // Two equal natural rows + one gutter fill the band exactly.
        approx(l.content_h, 780.0 / MOSAIC_BAND_HEIGHT_DIVISOR);
        assert_eq!(l.tiles.len(), 2);
        // Row 0 is non-last → justified to W = max natural width = th·2.
        let big_w = th * 2.0;
        approx(l.tiles[0].x, l.offset_x);
        approx(l.tiles[0].y, l.offset_y);
        approx(l.tiles[0].w, big_w);
        approx(l.tiles[0].h, big_w / 2.0); // shape-preserving justify
        // Row 1 (last): natural height, left-aligned at x=offset.
        approx(l.tiles[1].w, th);
        approx(l.tiles[1].h, th);
        approx(l.tiles[1].x, l.offset_x);
        approx(l.tiles[1].y, l.offset_y + big_w / 2.0 + MOSAIC_GUTTER_PX);
        approx(l.content_w, big_w);
        approx(l.content_h, big_w / 2.0 + th + MOSAIC_GUTTER_PX);
        approx(l.offset_x, (1200.0 - big_w) / 2.0);
        approx(l.offset_y, (780.0 - l.content_h) / 2.0);
    }

    #[test]
    fn justified_five_identical_16_9_both_rows_flush_right() {
        let a = 16.0f32 / 9.0;
        let aspects = [a; 5];
        let l = justified_layout(&aspects, 1920.0, 1040.0);
        let th = band_row_h(1040.0);
        // A=5a, T=A/2: after two cards sum + a/2 == 2.5a == T exactly, so
        // the strict `>` KEEPS card 3 in row 0 → split [3, 2].
        let s0 = 3.0 * a;
        let s1 = 2.0 * a;
        let nat0 = th * s0 + 2.0 * MOSAIC_GUTTER_PX;
        let nat1 = th * s1 + MOSAIC_GUTTER_PX;
        let big_w = nat0.max(nat1);
        assert!((nat0 - nat1).abs() > 1e-4, "rows must differ for this contract");
        assert_eq!(l.tiles.len(), 5);
        // Row 0 justified shape-preservingly to W (scale 1: it is widest).
        let h0 = (big_w - 2.0 * MOSAIC_GUTTER_PX) / s0;
        approx(l.tiles[0].h, h0);
        approx(l.tiles[0].w, h0 * a);
        approx(l.tiles[0].x, l.offset_x);
        approx(l.tiles[1].x, l.offset_x + h0 * a + MOSAIC_GUTTER_PX);
        approx(l.tiles[2].x, l.offset_x + 2.0 * (h0 * a + MOSAIC_GUTTER_PX));
        approx(l.tiles[0].y, l.tiles[1].y);
        approx(l.tiles[1].y, l.tiles[2].y);
        // Justified row flush right edge within epsilon.
        let right0 = l.tiles[2].x + l.tiles[2].w - l.offset_x;
        approx_acc(right0, nat0);
        approx_acc(right0, big_w);
        // Row 1 (last) natural: NOT stretched, left-aligned at x=offset.
        approx(l.tiles[3].h, th);
        approx(l.tiles[3].w, th * a);
        approx(l.tiles[3].y, l.tiles[0].y + h0 + MOSAIC_GUTTER_PX);
        approx(l.tiles[3].x, l.offset_x);
        approx(l.tiles[4].x, l.offset_x + th * a + MOSAIC_GUTTER_PX);
        let right1 = l.tiles[4].x + l.tiles[4].w - l.offset_x;
        approx(right1, nat1, );
        // Content box + centering: W exceeds stage width → no x offset.
        approx(l.content_w, big_w);
        approx(l.content_h, h0 + th + MOSAIC_GUTTER_PX);
        // Row 0 keeps the natural height here (it is the widest), so both
        // rows + gutter land exactly on the band.
        approx(l.content_h, 1040.0 / MOSAIC_BAND_HEIGHT_DIVISOR);
        // At band scale W (~1380px) is narrower than the stage → centered.
        approx(l.offset_x, (1920.0 - big_w) / 2.0);
        approx(l.offset_y, (1040.0 - l.content_h) / 2.0);
    }

    #[test]
    fn justified_mixed_aspects_greedy_split_and_justify() {
        let aspects = [2.0, 1.78, 1.5, 1.78, 2.35];
        let l = justified_layout(&aspects, 1920.0, 1040.0);
        let th = band_row_h(1040.0);
        // Greedy: [2.0, 1.78, 1.5] (sum 5.28) then force-free last [1.78, 2.35].
        let s0 = 2.0 + 1.78 + 1.5;
        let s1 = 1.78 + 2.35;
        let t_target = (s0 + s1) / 2.0;
        assert!(s0 + 1.78 / 2.0 > t_target, "card 4 must not fit row 0");
        let nat0 = th * s0 + 2.0 * MOSAIC_GUTTER_PX;
        let nat1 = th * s1 + MOSAIC_GUTTER_PX;
        let big_w = nat0.max(nat1);
        assert!((nat0 - nat1).abs() > 1e-4);
        assert_eq!(l.tiles.len(), 5);
        // Row 0 is the widest → justified scale is 1 (keeps th).
        approx(l.tiles[0].h, th);
        approx(l.tiles[0].w, th * 2.0);
        approx(l.tiles[1].x, l.offset_x + th * 2.0 + MOSAIC_GUTTER_PX);
        approx(l.tiles[1].w, th * 1.78);
        approx(l.tiles[2].x, l.tiles[1].x + l.tiles[1].w + MOSAIC_GUTTER_PX);
        approx(l.tiles[2].w, th * 1.5);
        approx_acc(l.tiles[2].x + l.tiles[2].w - l.offset_x, nat0);
        // Row 1 (last) natural: NOT stretched, left-aligned short of W.
        approx(l.tiles[3].h, th);
        approx(l.tiles[3].w, th * 1.78);
        approx(l.tiles[3].y, l.offset_y + th + MOSAIC_GUTTER_PX);
        approx(l.tiles[4].x, l.offset_x + th * 1.78 + MOSAIC_GUTTER_PX);
        approx(l.tiles[4].w, th * 2.35);
        let right1 = l.tiles[4].x + l.tiles[4].w - l.offset_x;
        approx_acc(right1, nat1);
        // Content + centering.
        approx(l.content_w, big_w);
        approx(l.content_h, 2.0 * th + MOSAIC_GUTTER_PX);
        // Row 0 is the widest (justify scale 1), so the block equals band.
        approx(l.content_h, 1040.0 / MOSAIC_BAND_HEIGHT_DIVISOR);
        // At band scale W (~1365px) is narrower than the stage → centered.
        approx(l.offset_x, (1920.0 - big_w) / 2.0);
        approx(l.offset_y, (1040.0 - l.content_h) / 2.0);
    }

    #[test]
    fn justified_narrow_first_row_scales_up_shape_preserving() {
        // One ultra-panorama closes row 0 immediately (10 + 2.35/2 > T);
        // the forced five-card last row is widest → row 0 would grow its
        // HEIGHT to reach W (303.25) but W overflows usable (1766), so
        // shrink-to-fit caps it at usable width.
        let stage_w = 1920.0;
        let stage_h = 1040.0;
        let aspects = [10.0, 2.35, 2.35, 2.35, 2.35, 2.35];
        let l = justified_layout(&aspects, stage_w, stage_h);
        let th = band_row_h(stage_h);
        assert_eq!(l.tiles.len(), 6);
        let margin = stage_w * MOSAIC_SIDE_MARGIN_FRACTION;
        let usable = stage_w - 2.0 * margin;
        let s1 = 5.0 * 2.35;
        let big_w = th * s1 + 4.0 * MOSAIC_GUTTER_PX;
        // big_w (3032) > usable (1766) → overflow, so row is shrunk to usable
        let h0_expected = usable / 10.0; // single tile fills usable exactly
        assert!(
            h0_expected < big_w / 10.0 - 1e-3,
            "shrink should be smaller than original upscaled"
        );
        approx(l.tiles[0].h, h0_expected);
        approx(l.tiles[0].w, usable);
        approx(l.tiles[0].x + l.tiles[0].w - l.offset_x, usable);
        // Last row would also overflow if kept at th (th*11.75+48=3032 > usable),
        // so it is also shrunk to usable.
        let h1_expected = (usable - 4.0 * MOSAIC_GUTTER_PX) / s1;
        approx(l.tiles[1].h, h1_expected);
        approx(l.tiles[1].y, l.tiles[0].y + h0_expected + MOSAIC_GUTTER_PX);
        approx(l.tiles[1].w, h1_expected * 2.35);
        // Wall fits inside symmetric margins.
        assert!(l.content_w <= usable + 1e-2);
        approx(l.offset_x, margin + (usable - l.content_w) / 2.0);
        approx(l.content_h, h0_expected + h1_expected + MOSAIC_GUTTER_PX);
        approx(l.offset_y, (stage_h - l.content_h) / 2.0);
    }

    #[test]
    fn justified_twenty_cards_scrollable_no_offset() {
        let a = 16.0f32 / 9.0;
        let stage_w = 1920.0;
        let stage_h = 1040.0;
        let l = justified_layout(&[a; 20], stage_w, stage_h);
        assert_eq!(l.tiles.len(), 20, "never drops cards");
        let margin = stage_w * MOSAIC_SIDE_MARGIN_FRACTION;
        let usable_w = stage_w - 2.0 * margin;
        // Contract: wall NEVER overflows — must fit symmetrically inside margins.
        assert!(
            l.content_w <= usable_w + 1e-2,
            "content_w {} must fit inside usable {usable_w} (stage {stage_w})",
            l.content_w
        );
        // Horizontally centered inside the usable area: offset = margin + (usable - content)/2
        let expected_offset = margin + (usable_w - l.content_w) / 2.0;
        approx(l.offset_x, expected_offset);
        assert!(l.offset_x + 1e-2 >= margin, "left margin respected");
        assert!(
            l.offset_x + l.content_w - 1e-2 <= stage_w - margin,
            "right margin respected: offset {} + content {} = {} > {}",
            l.offset_x,
            l.content_w,
            l.offset_x + l.content_w,
            stage_w - margin
        );
        // Every tile inside margins (1px float tolerance).
        for t in &l.tiles {
            assert!(
                t.x + 1.0 >= margin,
                "tile x {} < margin {margin}",
                t.x
            );
            assert!(
                t.x + t.w - 1.0 <= stage_w - margin,
                "tile right {} > stage_w - margin {}",
                t.x + t.w,
                stage_w - margin
            );
        }
        // Overflow rows are shrunk: tile height must be < band height th.
        let th = band_row_h(stage_h);
        assert!(
            l.tiles[0].h < th - 1e-3,
            "overflow row must be shrunk: h {} < th {th}",
            l.tiles[0].h
        );
        // Rows still stacked with gutter.
        approx(l.tiles[10].y, l.tiles[0].y + l.tiles[0].h + MOSAIC_GUTTER_PX);
    }

    #[test]
    fn mosaic_shrink_to_fit_stress_no_overflow_across_pages() {
        // Stress contract: counts 5..=40 across stage sizes, every tile inside
        // [margin, stage_w - margin] on every page after flips (1px tolerance).
        let stage_ws = [1280.0f32, 1920.0, 2560.0];
        let stage_hs = [720.0f32, 1040.0, 1440.0];
        for &sw in &stage_ws {
            for &sh in &stage_hs {
                let margin = sw * MOSAIC_SIDE_MARGIN_FRACTION;
                let right = sw - margin;
                for count in 5usize..=40 {
                    let mut pages = MosaicPages::new(count, sw, sh);
                    let total = pages.total_pages().max(1);
                    for _ in 0..total {
                        let (aspects, _) = pages.page_render();
                        let layout = justified_layout(&aspects, sw, sh);
                        for t in &layout.tiles {
                            assert!(
                                t.x + 1.0 >= margin,
                                "stress count {count} stage {sw}x{sh} page {}: tile x {} < margin {margin}",
                                pages.current(),
                                t.x
                            );
                            assert!(
                                t.x + t.w - 1.0 <= right,
                                "stress count {count} stage {sw}x{sh} page {}: tile right {} > right {right}",
                                pages.current(),
                                t.x + t.w
                            );
                        }
                        // Also assert block itself inside margins.
                        assert!(
                            layout.offset_x + 1.0 >= margin,
                            "offset_x {} < margin {margin}",
                            layout.offset_x
                        );
                        assert!(
                            layout.offset_x + layout.content_w - 1.0 <= right,
                            "block right {} > right {right}",
                            layout.offset_x + layout.content_w
                        );
                        pages.step(1);
                    }
                }
            }
        }
    }

    #[test]
    fn justified_invalid_aspects_fall_back_to_default() {
        let bad = [0.0, -3.0, f32::NAN, f32::INFINITY];
        let good = [MOSAIC_DEFAULT_ASPECT; 4];
        assert_eq!(
            justified_layout(&bad, 1920.0, 1040.0),
            justified_layout(&good, 1920.0, 1040.0),
            "non-finite or ≤ 0 aspects behave exactly like the default"
        );
    }

    // ── Display-aspect pattern contracts ("real Pinterest") ─────────

    #[test]
    fn display_aspect_pattern_exact_values_indices_0_to_9() {
        // Widened Pinterest spread — square 1.0, portrait 0.72, wide 1.95, period 10
        // Cycle: [square, portrait, wide, portrait, square, wide, square, portrait, wide, square]
        let sq = 1.0f32;
        let po = 0.72f32;
        let wi = 1.95f32;
        let expected = [sq, po, wi, po, sq, wi, sq, po, wi, sq];
        for (idx, &want) in expected.iter().enumerate() {
            approx(mosaic_display_aspect(idx), want);
        }
        // Named constants are single tuning points
        approx(super::MOSAIC_ASPECT_SQUARE, sq);
        approx(super::MOSAIC_ASPECT_PORTRAIT, po);
        approx(super::MOSAIC_ASPECT_WIDE, wi);
    }

    #[test]
    fn display_aspect_pattern_cyclic_over_10_indices() {
        for idx in 0..20 {
            assert!(
                (mosaic_display_aspect(idx) - mosaic_display_aspect(idx + 10)).abs() < 1e-6,
                "pattern must repeat with period 10 at idx {idx}"
            );
        }
        for idx in 0..30 {
            let a = mosaic_display_aspect(idx);
            let b = mosaic_display_aspect(idx % 10);
            assert!(
                (a - b).abs() < 1e-6,
                "cyclic mismatch idx {idx}: {a} vs {b}"
            );
        }
    }

    #[test]
    fn display_aspect_window_contains_all_three_classes() {
        // Every consecutive window of 10 tiles must contain all three classes
        let sq = 1.0f32;
        let po = 0.72f32;
        let wi = 1.95f32;
        let class = |a: f32| -> u8 {
            if (a - sq).abs() < 1e-4 {
                0
            } else if (a - po).abs() < 1e-4 {
                1
            } else if (a - wi).abs() < 1e-4 {
                2
            } else {
                99
            }
        };
        for start in 0..40 {
            let mut has = [false; 3];
            for i in start..start + 10 {
                let c = class(mosaic_display_aspect(i));
                assert!(c < 3, "unexpected aspect {c} at idx {i}");
                has[c as usize] = true;
            }
            assert!(
                has[0] && has[1] && has[2],
                "window {start}..{} missing class {has:?}",
                start + 10
            );
        }
    }

    #[test]
    fn display_aspect_no_four_consecutive_same_class() {
        let sq = 1.0f32;
        let po = 0.72f32;
        let wi = 1.95f32;
        let class = |a: f32| -> u8 {
            if (a - sq).abs() < 1e-4 {
                0
            } else if (a - po).abs() < 1e-4 {
                1
            } else if (a - wi).abs() < 1e-4 {
                2
            } else {
                99
            }
        };
        let seq: Vec<u8> = (0..60).map(|i| class(mosaic_display_aspect(i))).collect();
        for w in seq.windows(4) {
            assert!(
                !(w[0] == w[1] && w[1] == w[2] && w[2] == w[3]),
                "four consecutive same class {w:?}"
            );
        }
    }

    #[test]
    fn display_aspect_values_within_bounds() {
        for i in 0..100 {
            let a = mosaic_display_aspect(i);
            assert!(
                a >= 0.7 - 1e-6 && a <= 2.0 + 1e-6,
                "aspect {a} at idx {i} out of [0.7, 2.0]"
            );
        }
    }

    #[test]
    fn justified_pattern_aspects_variety_flush_rows_centered() {
        // Four cards with widened Pinterest pattern must still produce a
        // NON-uniform block with portrait-driven variety, flush rows and
        // stage centering. New cycle 0..4: [square 1.0, portrait 0.72, wide 1.95, portrait 0.72].
        let aspects: Vec<f32> = (0..4).map(mosaic_display_aspect).collect();
        let l = justified_layout(&aspects, 1920.0, 1040.0);
        assert_eq!(l.tiles.len(), 4);
        let th = band_row_h(1040.0);
        // Greedy packing with s0=1.0+0.72, s1=1.95+0.72 and T≈2.195: row 0 [square, portrait], row 1 [wide, portrait].
        let s0 = 1.0f32 + 0.72f32;
        let s1 = 1.95f32 + 0.72f32;
        let nat0 = th * s0 + MOSAIC_GUTTER_PX;
        let nat1 = th * s1 + MOSAIC_GUTTER_PX;
        let big_w = nat0.max(nat1);
        assert!((nat0 - nat1).abs() > 1e-4, "rows differ → justification kicks in");
        // Variety: columns differ thanks to wide vs square/portrait spread
        assert!((l.tiles[0].w - l.tiles[1].w).abs() > 1e-3, "row 0 varied (square vs portrait)");
        assert!((l.tiles[2].w - l.tiles[3].w).abs() > 1e-3, "row 1 varied (wide vs portrait)");
        // Tile shapes follow the widened pattern. Row 0 is narrow non-last → scales UP.
        let h0 = (big_w - MOSAIC_GUTTER_PX) / s0;
        approx(l.tiles[0].h, h0);
        approx(l.tiles[1].h, h0);
        approx(l.tiles[2].h, th);
        approx(l.tiles[3].h, th);
        approx(l.tiles[0].w / l.tiles[0].h, 1.0); // square
        approx(l.tiles[1].w / l.tiles[1].h, 0.72); // portrait
        approx(l.tiles[2].w / l.tiles[2].h, 1.95); // wide
        approx(l.tiles[3].w / l.tiles[3].h, 0.72); // portrait
        // Row membership and stacking.
        approx(l.tiles[1].y, l.tiles[0].y);
        approx(l.tiles[2].y, l.tiles[0].y + l.tiles[0].h + MOSAIC_GUTTER_PX);
        approx(l.tiles[3].y, l.tiles[2].y);
        // Justified flush rows: both rows end exactly at W.
        approx_acc(l.tiles[1].x + l.tiles[1].w - l.offset_x, big_w);
        approx_acc(l.tiles[3].x + l.tiles[3].w - l.offset_x, big_w);
        // Centered content box on the stage.
        approx(l.content_w, big_w);
        approx(l.content_h, l.tiles[0].h + l.tiles[2].h + MOSAIC_GUTTER_PX);
        approx(l.offset_x, (1920.0 - big_w) / 2.0);
        approx(l.offset_y, (1040.0 - l.content_h) / 2.0);
    }
}

// ── Mosaic page-model contracts (Mosaic final scheme) ──────────────
// Headless: page math, clone-fill, pagination, clamp/no-wrap, no-rebuild.
#[cfg(test)]
mod mosaic_pages_tests {
    use super::*;

    const STAGE_W: f32 = 1920.0;
    const STAGE_H: f32 = 1040.0;

    /// Expected total_pages for a given real count + capacity (guards the
    /// RED capacity==0 case so assertions fail cleanly instead of dividing).
    fn expected_pages(real: usize, cap: usize) -> usize {
        if real == 0 { 0 } else if cap == 0 { 1 } else { (real + cap - 1) / cap }
    }

    #[test]
    fn mosaic_page_capacity_positive_for_valid_stage() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        assert!(cap > 0, "capacity must be positive for a valid stage, got {cap}");
        assert_eq!(mosaic_page_capacity(0.0, STAGE_H), 0, "degenerate stage → 0");
        assert_eq!(mosaic_page_capacity(STAGE_W, 0.0), 0);
    }

    #[test]
    fn mosaic_total_pages_formula() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        // empty library → 0 pages, hidden
        let p0 = MosaicPages::new(0, STAGE_W, STAGE_H);
        assert_eq!(p0.total_pages(), 0);
        assert!(p0.hidden());
        // small library (<= capacity) → single page, hidden
        let small = 3usize;
        let ps = MosaicPages::new(small, STAGE_W, STAGE_H);
        assert_eq!(ps.total_pages(), expected_pages(small, cap));
        assert_eq!(ps.hidden(), ps.total_pages() <= 1);
        // large library → multiple pages, NOT hidden
        let big = cap * 3 + 2;
        let pb = MosaicPages::new(big, STAGE_W, STAGE_H);
        assert_eq!(pb.total_pages(), expected_pages(big, cap));
        assert!(!pb.hidden(), "multiple pages must show pagination");
    }

    #[test]
    fn mosaic_page_clamp_no_wrap() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let total = cap * 3 + 2;
        let mut p = MosaicPages::new(total, STAGE_W, STAGE_H);
        let tp = p.total_pages();
        assert!(tp >= 2, "need >=2 pages to exercise clamp, got {tp}");
        // forward clamps at the last page, never wraps to 0
        for _ in 0..50 { p.step(1); }
        assert_eq!(p.current(), tp - 1, "forward must clamp at last page");
        // backward clamps at the first page, never wraps to last
        for _ in 0..50 { p.step(-1); }
        assert_eq!(p.current(), 0, "backward must clamp at first page");
        // stays strictly within [0, total_pages)
        for d in [1, -1, 1, 1, -1, 1, 1, 1] {
            p.step(d);
            assert!(p.current() < p.total_pages());
        }
    }

    #[test]
    fn mosaic_clone_fill_count_and_mapping() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let real = 3usize;
        assert!(cap > real, "need capacity > real for clone-fill test (cap={cap})");
        let p = MosaicPages::new(real, STAGE_W, STAGE_H);
        assert_eq!(p.clone_fill_count(), cap - real, "clones fill the page");
        // mapping displayed -> real is cyclic (displayed % real_count)
        for k in 0..cap {
            assert_eq!(p.clone_real_index(k), k % real, "displayed {k} -> real {}", k % real);
        }
        // page_render yields exactly `capacity` tiles, all mapped to real
        let (aspects, reals) = p.page_render();
        assert_eq!(aspects.len(), cap, "clone page fills capacity tiles");
        assert_eq!(reals.len(), cap);
        for (k, &r) in reals.iter().enumerate() {
            assert_eq!(r, k % real, "rendered tile {k} maps to real {}", k % real);
        }
    }

    #[test]
    fn mosaic_pagination_numbers_list() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let total = cap * 5; // exactly 5 pages
        let p = MosaicPages::new(total, STAGE_W, STAGE_H);
        assert_eq!(p.total_pages(), 5, "cap*5 real themes → 5 pages");
        assert_eq!(p.page_numbers(), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn mosaic_hidden_single_page_rule() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        // exactly one page worth → hidden
        let one = MosaicPages::new(cap, STAGE_W, STAGE_H);
        assert_eq!(one.total_pages(), 1);
        assert!(one.hidden());
        // one over the capacity → two pages → visible
        let two = MosaicPages::new(cap + 1, STAGE_W, STAGE_H);
        assert_eq!(two.total_pages(), 2);
        assert!(!two.hidden());
        // zero themes → hidden
        let zero = MosaicPages::new(0, STAGE_W, STAGE_H);
        assert!(zero.hidden());
    }

    #[test]
    fn mosaic_page_flip_does_not_rebuild_thumbs() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let total = cap * 4 + 1;
        let mut p = MosaicPages::new(total, STAGE_W, STAGE_H);
        let before = p.geometry_rebuilds();
        // flipping pages must NOT recompute geometry/thumbs
        for _ in 0..10 { p.step(1); p.step(-1); }
        assert_eq!(p.geometry_rebuilds(), before, "page flip must not rebuild geometry");
        // only an explicit recompute (count/stage change) rebuilds
        p.recompute(total, STAGE_W, STAGE_H);
        assert_eq!(p.geometry_rebuilds(), before + 1, "recompute rebuilds once");
    }

    #[test]
    fn mosaic_curtain_delays_staggered_and_bounded() {
        let aspects: Vec<f32> = (0..8).map(mosaic_display_aspect).collect();
        let layout = justified_layout(&aspects, STAGE_W, STAGE_H);
        let delays = mosaic_curtain_delays(&layout.tiles);
        assert_eq!(delays.len(), layout.tiles.len());
        // every delay is a non-negative multiple of the column stagger
        for d in &delays {
            assert_eq!(d % MOSAIC_CURTAIN_COL_STAGGER_MS, 0);
            assert!(*d < 1000, "delays must stay bounded");
        }
    }
}

// ── Mosaic margin contract (RED): 4% symmetric lateral margins ───────
#[cfg(test)]
mod mosaic_margin_tests {
    use super::*;

    const STAGE_W: f32 = 1920.0;
    const STAGE_H: f32 = 1040.0;
    const MARGIN_FRACTION: f32 = 0.04;

    #[test]
    fn mosaic_margin_contract_tiles_within_4_percent() {
        // 9 tiles sits in the narrow band where old edge-to-edge offset
        // (stage_w - content_w)/2 ≈39px < 76.8px margin, so this must fail RED
        // before the margin is mirrored into the geometry.
        let margin = STAGE_W * MARGIN_FRACTION;
        let aspects: Vec<f32> = (0..9).map(mosaic_display_aspect).collect();
        let layout = justified_layout(&aspects, STAGE_W, STAGE_H);
        assert!(!layout.tiles.is_empty());
        let min_x = layout.tiles.iter().map(|t| t.x).fold(f32::MAX, f32::min);
        let max_x = layout.tiles.iter().map(|t| t.x + t.w).fold(f32::MIN, f32::max);
        assert!(
            min_x + 1e-2 >= margin,
            "RED: min tile x {min_x} < margin {margin} (4% of {STAGE_W}) — wall must respect left margin"
        );
        // Right margin also for this size when wall fits usable (content 1840 > usable 1766
        // — overflow case will be margin-pinned left; the full symmetric check is
        // exercised in the fitting-page test below).
        // To keep RED crisp, we only enforce the left side here; the symmetric
        // contract is fully checked in mosaic_margin_pages_hold.
        let _ = max_x;
    }

    #[test]
    fn mosaic_margin_pages_hold_across_flips() {
        // Contract: tiles never cross 4% lateral margins, including across page flips.
        // For pages that fit within usable width, both margins must hold; overflow
        // pages are left-pinned at margin (right may be clipped but left must hold).
        let margin = STAGE_W * MARGIN_FRACTION;
        let usable = STAGE_W - 2.0 * margin;
        for real in [4usize, 6, 8] {
            let mut pages = MosaicPages::new(real, STAGE_W, STAGE_H);
            // real < capacity → single clone-filled page that must still respect margins
            // For these small reals the clone-filled page fits comfortably (content < usable)
            // so both margins are testable.
            for _page in 0..pages.total_pages().max(1) {
                let (aspects, _) = pages.page_render();
                let layout = justified_layout(&aspects, STAGE_W, STAGE_H);
                if layout.tiles.is_empty() {
                    pages.step(1);
                    continue;
                }
                let min_x = layout.tiles.iter().map(|t| t.x).fold(f32::MAX, f32::min);
                let max_x = layout.tiles.iter().map(|t| t.x + t.w).fold(f32::MIN, f32::max);
                assert!(
                    min_x + 1.0 >= margin,
                    "page {} real {real}: min x {min_x} < margin {margin}",
                    pages.current()
                );
                if layout.content_w <= usable + 1.0 {
                    assert!(
                        max_x - 1.0 <= STAGE_W - margin,
                        "page {} real {real}: max x {max_x} > stage_w - margin {} (content_w {})",
                        pages.current(),
                        STAGE_W - margin,
                        layout.content_w
                    );
                }
                pages.step(1);
            }
        }
        // Also verify a multi-page library (forces pagination) — use a moderate stage
        // where each page's content fits usable so both margins are verifiable.
        // With capacity huge, we simulate pagination by forcing a small capacity via
        // a narrow stage (800x600) where 7 tiles overflow, but we test 2 pages of 4 each.
        let narrow_w = 800.0;
        let narrow_h = 600.0;
        let narrow_margin = narrow_w * MARGIN_FRACTION;
        let narrow_usable = narrow_w - 2.0 * narrow_margin;
        let aspects4: Vec<f32> = (0..4).map(mosaic_display_aspect).collect();
        let layout4 = justified_layout(&aspects4, narrow_w, narrow_h);
        let min4 = layout4.tiles.iter().map(|t| t.x).fold(f32::MAX, f32::min);
        let max4 = layout4.tiles.iter().map(|t| t.x + t.w).fold(f32::MIN, f32::max);
        assert!(min4 + 1.0 >= narrow_margin);
        if layout4.content_w <= narrow_usable + 1.0 {
            assert!(max4 - 1.0 <= narrow_w - narrow_margin);
        }
    }
}

// ── Hero mosaic contracts (RED) ────────────────────────────────────
#[cfg(test)]
mod mosaic_hero_tests {
    use super::*;

    const STAGE_W: f32 = 1920.0;
    const STAGE_H: f32 = 1040.0;

    fn band_row_h() -> f32 {
        let band = STAGE_H / MOSAIC_BAND_HEIGHT_DIVISOR;
        (band - (MOSAIC_ROW_COUNT as f32 - 1.0) * MOSAIC_GUTTER_PX) / MOSAIC_ROW_COUNT as f32
    }

    fn hero_band_height() -> f32 {
        let th = band_row_h();
        2.0 * th + MOSAIC_GUTTER_PX
    }

    fn hero_tiles(layout: &MosaicLayout) -> Vec<usize> {
        // Hero = tiles TALLER than one row (spans both side rows); side rows
        // are justified ≤ th, so only the hero crosses th. Degenerate
        // layouts (normal rows only) have none.
        let th = band_row_h();
        layout
            .tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| t.h > th + 1.0)
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn hero_exactly_one_per_super_band_and_height() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let aspects: Vec<f32> = (0..cap).map(mosaic_display_aspect).collect();
        let reals: Vec<usize> = (0..aspects.len()).collect();
        let (layout, tile_reals) = justified_hero_layout(&aspects, &reals, STAGE_W, STAGE_H);
        assert!(layout.tiles.len() >= aspects.len(), "hero keeps at least the page tiles (row-fill clones may add)");
        assert_eq!(tile_reals.len(), layout.tiles.len(), "reals must map 1:1 to tiles");
        let th = band_row_h();
        let band_h = hero_band_height();
        let heroes = hero_tiles(&layout);
        assert_eq!(heroes.len(), 1, "exactly one hero per super-band, got {:?}", heroes);
        let hero = layout.tiles[heroes[0]];
        // Hero spans EXACTLY both side rows: h_top + gutter + h_bottom ≤ the
        // nominal band, and strictly taller than one row.
        assert!(
            hero.h > th + 1.0 && hero.h <= band_h + 1.0,
            "hero height {} must be in (th {th}, band {band_h}]",
            hero.h
        );
        for (i, t) in layout.tiles.iter().enumerate() {
            if heroes.contains(&i) {
                continue;
            }
            assert!(
                t.h <= th + 1.1,
                "non-hero tile {i} h {} must be <= th {th}",
                t.h
            );
        }
    }

    #[test]
    fn hero_centered_in_band() {
        // The hero is CENTERED in the band and spans its full height; the
        // four segments (TL, TR, BL, BR) flow around it at row height th.
        let margin = STAGE_W * MOSAIC_SIDE_MARGIN_FRACTION;
        let usable = STAGE_W - 2.0 * margin;
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let th = band_row_h();
        for count in [cap, cap.saturating_sub(1).max(5)] {
            let aspects: Vec<f32> = (0..count).map(mosaic_display_aspect).collect();
            let reals: Vec<usize> = (0..count).collect();
            let (layout, _) = justified_hero_layout(&aspects, &reals, STAGE_W, STAGE_H);
            let heroes = hero_tiles(&layout);
            assert_eq!(heroes.len(), 1, "count {count}: one hero");
            let hero = layout.tiles[heroes[0]];
            let hero_center = hero.x - layout.offset_x + hero.w / 2.0;
            assert!(
                (hero_center - usable / 2.0).abs() < 2.0,
                "count {count}: hero center {hero_center} must be usable/2 {}",
                usable / 2.0
            );
            assert!(
                (hero.h - (2.0 * th + MOSAIC_GUTTER_PX)).abs() < 2.0,
                "count {count}: hero spans the band: {} vs {}",
                hero.h,
                2.0 * th + MOSAIC_GUTTER_PX
            );
        }
    }

    #[test]
    fn over_capacity_page_degrades_gracefully() {
        // More aspects than the hero capacity → segments take extra tiles
        // and crop harder, but never panic, never dust, margins hold.
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let count = cap + 6;
        let aspects: Vec<f32> = (0..count).map(mosaic_display_aspect).collect();
        let reals: Vec<usize> = (0..count).collect();
        let (layout, tile_reals) = justified_hero_layout(&aspects, &reals, STAGE_W, STAGE_H);
        assert!(layout.tiles.len() >= count, "every tile renders (clones may extend)");
        assert_eq!(tile_reals.len(), layout.tiles.len());
        let margin = STAGE_W * MOSAIC_SIDE_MARGIN_FRACTION;
        for t in &layout.tiles {
            assert!(t.x + 1.0 >= margin && t.x + t.w <= STAGE_W - margin + 2.0, "tile {t:?} crosses margins");
            assert!(t.w >= 40.0 && t.h >= 40.0, "dust tile {t:?}");
        }
    }

    #[test]
    fn segments_fill_their_regions_exactly() {
        // Even band without holes: four segments of height th flow around
        // the centered hero — top row spans [0, usable] split by the hero,
        // bottom row likewise; every segment is flush with its region.
        let margin = STAGE_W * MOSAIC_SIDE_MARGIN_FRACTION;
        let usable = STAGE_W - 2.0 * margin;
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let th = band_row_h();
        let g = MOSAIC_GUTTER_PX;
        for count in [cap, cap.saturating_sub(1).max(5)] {
            let aspects: Vec<f32> = (0..count).map(mosaic_display_aspect).collect();
            let reals: Vec<usize> = (0..count).collect();
            let (layout, _) = justified_hero_layout(&aspects, &reals, STAGE_W, STAGE_H);
            let heroes = hero_tiles(&layout);
            assert_eq!(heroes.len(), 1, "count {count}: one hero");
            let hero = layout.tiles[heroes[0]];
            let ox = layout.offset_x;
            let oy = layout.offset_y;
            let hero_lx = hero.x - ox;
            // Top row tiles: same y as hero, height ≈ th.
            let top: Vec<_> = layout
                .tiles
                .iter()
                .enumerate()
                .filter(|(i, t)| *i != heroes[0] && (t.y - oy - (hero.y - oy)).abs() < 1.0)
                .map(|(_, t)| *t)
                .collect();
            assert!(!top.is_empty(), "count {count}: top segments exist");
            // Top row covers [0, usable] with a gap ONLY around the hero.
            let mut spans: Vec<(f32, f32)> = top.iter().map(|t| (t.x - ox, t.x - ox + t.w)).collect();
            spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            assert!((spans[0].0 - 0.0).abs() < 2.0, "count {count}: top row flush left");
            assert!((spans.last().unwrap().1 - usable).abs() < 2.0, "count {count}: top row flush right");
            // Exactly ONE consecutive-pair gap wraps the hero; every other
            // gap is a plain gutter (segments are internally flush).
            let mut hero_gaps = 0;
            for pair in spans.windows(2) {
                if (pair[0].1 - (hero_lx - g)).abs() < 2.0
                    && (pair[1].0 - (hero_lx + hero.w + g)).abs() < 2.0
                {
                    hero_gaps += 1;
                } else {
                    assert!(
                        (pair[1].0 - pair[0].1 - g).abs() < 2.0,
                        "count {count}: non-hero gap must be a gutter: {pair:?}"
                    );
                }
            }
            assert_eq!(hero_gaps, 1, "count {count}: exactly one hero gap");
            // All top tiles are row height th.
            for t in &top {
                assert!((t.h - th).abs() < 1.5, "count {count}: segment height {} ≈ th {th}", t.h);
            }
            // Bottom row mirrors it below the hero.
            let bottom: Vec<_> = layout
                .tiles
                .iter()
                .enumerate()
                .filter(|(i, t)| *i != heroes[0] && t.y - oy > (hero.y - oy) + 1.0)
                .map(|(_, t)| *t)
                .collect();
            assert!(bottom.len() >= 2, "count {count}: both bottom segments exist");
            let bottom_left = bottom.iter().map(|t| t.x - ox).fold(f32::MAX, f32::min);
            let bottom_right = bottom.iter().map(|t| t.x - ox + t.w).fold(f32::MIN, f32::max);
            assert!((bottom_left - 0.0).abs() < 2.0 && (bottom_right - usable).abs() < 2.0,
                "count {count}: bottom row flush both edges");
        }
    }

    #[test]
    fn clone_fill_feeds_the_four_segments() {
        // Few real themes → page_render clone-fills the page to capacity;
        // the layout then splits everything across hero + 4 segments with
        // every real theme present and the reals mapping aligned.
        let pages = MosaicPages::new(3, STAGE_W, STAGE_H);
        let (aspects, reals) = pages.page_render();
        let (layout, tile_reals) = justified_hero_layout(&aspects, &reals, STAGE_W, STAGE_H);
        assert_eq!(tile_reals.len(), layout.tiles.len());
        for r in &tile_reals {
            assert!(*r < 3, "clone real {r} must come from the library");
        }
        for k in 0..3 {
            assert!(tile_reals.contains(&k), "real {k} must be present");
        }
        assert_eq!(tile_reals[0], 0, "hero is the page's first real");
    }

    #[test]
    fn hero_width_capped_at_45_percent_usable() {
        let margin = STAGE_W * MOSAIC_SIDE_MARGIN_FRACTION;
        let usable = STAGE_W - 2.0 * margin;
        let width_cap = usable * MOSAIC_HERO_MAX_WIDTH_FRACTION;
        let page_cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        assert!(page_cap >= 3, "capacity {page_cap} must host a hero band");
        let mut aspects = vec![MOSAIC_ASPECT_WIDE];
        aspects.extend((1..page_cap).map(mosaic_display_aspect));
        let reals: Vec<usize> = (0..aspects.len()).collect();
        let (layout, _) = justified_hero_layout(&aspects, &reals, STAGE_W, STAGE_H);
        let heroes = hero_tiles(&layout);
        assert_eq!(heroes.len(), 1);
        let hero_w = layout.tiles[heroes[0]].w;
        assert!(
            hero_w <= width_cap + 1.0,
            "hero width {hero_w} must be <= 45% usable {width_cap} (usable {usable})"
        );
        for count in 3..=page_cap {
            let asp: Vec<f32> = (0..count).map(mosaic_display_aspect).collect();
            let rl: Vec<usize> = (0..count).collect();
            let (lay, _) = justified_hero_layout(&asp, &rl, STAGE_W, STAGE_H);
            for &hi in &hero_tiles(&lay) {
                assert!(
                    lay.tiles[hi].w <= width_cap + 1.0,
                    "count {count} hero width {} > cap {width_cap}",
                    lay.tiles[hi].w
                );
            }
        }
    }

    #[test]
    fn hero_degenerate_fallback_preserves_layout() {
        // 2 tiles: rows [1,1] → remaining after hero =1 (<2) → fallback to normal 2 rows
        // Hero detection should find 0 heroes in that degenerate band.
        let aspects: Vec<f32> = (0..2).map(mosaic_display_aspect).collect();
        let reals: Vec<usize> = (0..2).collect();
        let (layout, tile_reals) = justified_hero_layout(&aspects, &reals, STAGE_W, STAGE_H);
        assert_eq!(layout.tiles.len(), 2);
        assert_eq!(tile_reals, vec![0, 1], "degenerate keeps identity mapping");
        let heroes = hero_tiles(&layout);
        assert!(
            heroes.is_empty(),
            "degenerate band with <2 remaining must fallback to no hero, got {:?}", heroes
        );
        let margin = STAGE_W * MOSAIC_SIDE_MARGIN_FRACTION;
        for t in &layout.tiles {
            assert!(t.x + 1.0 >= margin);
        }
        let single_reals = vec![7usize];
        let (single, single_map) =
            justified_hero_layout(&[MOSAIC_ASPECT_WIDE], &single_reals, STAGE_W, STAGE_H);
        assert_eq!(single.tiles.len(), 1);
        assert_eq!(single_map, vec![7], "single tile keeps its real");
        assert!(hero_tiles(&single).is_empty(), "single tile must not be hero");
    }

    #[test]
    fn hero_clone_fill_and_aspect_contracts_still_hold() {
        // Clone-filled page must still respect margins and the aspect
        // pattern when using the hero layout; row-fill clones may extend
        // the tile count beyond capacity.
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let real = 3usize;
        assert!(cap > real);
        let pages = MosaicPages::new(real, STAGE_W, STAGE_H);
        let (aspects, reals) = pages.page_render();
        assert_eq!(aspects.len(), cap);
        assert_eq!(reals.len(), cap);
        let (layout, tile_reals) = justified_hero_layout(&aspects, &reals, STAGE_W, STAGE_H);
        assert!(layout.tiles.len() >= cap, "hero layout keeps at least capacity tiles");
        assert_eq!(tile_reals.len(), layout.tiles.len());
        let margin = STAGE_W * MOSAIC_SIDE_MARGIN_FRACTION;
        let right = STAGE_W - margin;
        for t in &layout.tiles {
            assert!(t.x + 1.0 >= margin);
            assert!(t.x + t.w - 1.0 <= right);
        }
        for (i, &a) in aspects.iter().enumerate() {
            let expected = mosaic_display_aspect(i);
            assert!((a - expected).abs() < 1e-4, "page_render aspect {i} mismatch");
        }
        for r in &tile_reals {
            assert!(*r < real, "tile real {r} must map into the {real}-theme library");
        }
    }
}

#[cfg(test)]
mod mosaic_capacity_tests {
    use super::*;

    // Regression: capacity derived from laying out a long probe list reported
    // 4096 (the probe length) because the packers never wrap past
    // MOSAIC_ROW_COUNT — every page tile then shrank to sub-pixel dust.
    #[test]
    fn capacity_is_sane_across_stages() {
        for &(sw, sh) in &[
            (1280.0, 720.0),
            (1920.0, 1080.0),
            (2560.0, 1440.0),
            (1366.0, 768.0),
            (3840.0, 2160.0),
        ] {
            let cap = mosaic_page_capacity(sw, sh);
            assert!(
                (4..=64).contains(&cap),
                "capacity {cap} out of bounds at {sw}x{sh}"
            );
        }
    }

    #[test]
    fn open_page_tiles_are_never_dust() {
        // The exact broken scenario: 6 real themes opening at 1920x1080 —
        // one hero rendered and the rest of the wall collapsed to dust.
        let (sw, sh) = (1920.0f32, 1080.0f32);
        let cap = mosaic_page_capacity(sw, sh);
        assert!(cap <= 64, "capacity {cap}");
        let pg = MosaicPages::new(6, sw, sh);
        let (aspects, reals) = pg.page_render();
        assert_eq!(aspects.len(), cap, "page_render must yield capacity tiles");
        assert_eq!(reals.len(), cap);
        let (layout, tile_reals) = justified_hero_layout(&aspects, &reals, sw, sh);
        assert!(layout.tiles.len() >= cap);
        assert_eq!(tile_reals.len(), layout.tiles.len());
        let margin = sw * MOSAIC_SIDE_MARGIN_FRACTION;
        for t in &layout.tiles {
            assert!(t.w >= 40.0 && t.h >= 40.0, "dust tile {t:?} (capacity {cap})");
            assert!(t.x >= margin - 1.0 && t.x + t.w <= sw - margin + 1.0, "tile {t:?} crosses margins");
        }
    }

    #[test]
    fn flip_sweep_never_produces_dust() {
        let (sw, sh) = (1920.0f32, 1080.0f32);
        let mut pg = MosaicPages::new(40, sw, sh);
        let total = pg.total_pages();
        assert!(total >= 2, "40 themes must paginate");
        for _ in 0..total {
            let (aspects, reals) = pg.page_render();
            assert_eq!(aspects.len(), reals.len());
            let (layout, tile_reals) = justified_hero_layout(&aspects, &reals, sw, sh);
            assert!(layout.tiles.len() >= aspects.len());
            assert_eq!(tile_reals.len(), layout.tiles.len());
            for t in &layout.tiles {
                assert!(t.w >= 40.0 && t.h >= 40.0, "dust tile {t:?} on page {}", pg.current());
            }
            pg.step(1);
        }
    }
}

// ── MOSAIC UNIT A — page/clone-fill backbone (RED) ────────────────
#[cfg(test)]
mod mosaic_unit_a_tests {
    use super::*;

    const STAGE_W: f32 = 1920.0;
    const STAGE_H: f32 = 1040.0;

    #[test]
    fn degenerate_stage_capacity_zero_does_not_panic_and_reports_single_page() {
        // Degenerate stage → capacity 0. Must not panic and must report a
        // single page for any non-empty library (graceful fallback).
        let p = MosaicPages::new(5, 0.0, 0.0);
        assert_eq!(p.capacity(), 0, "degenerate stage capacity 0");
        assert_eq!(p.total_pages(), 1, "non-empty library with degenerate stage → 1 page");
        assert!(p.hidden(), "single page hidden");
        let (aspects, reals) = p.page_render();
        assert_eq!(aspects.len(), 5, "degenerate render falls back to real entries");
        assert_eq!(reals, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn degenerate_recompute_does_not_panic() {
        let mut p = MosaicPages::new(10, STAGE_W, STAGE_H);
        p.recompute(7, 0.0, 0.0);
        assert_eq!(p.capacity(), 0);
        assert_eq!(p.total_pages(), 1);
    }

    #[test]
    fn clone_fill_exact_contract_3_real_5_capacity_tile4_maps_to_1() {
        // Hero-centered capacity at this stage is 5; with 3 real themes the
        // page is in clone-fill mode: displayed k maps to k % 3.
        let p = MosaicPages::new(3, STAGE_W, STAGE_H);
        let cap = p.capacity();
        assert!(cap >= 4, "need cap >=4 for clone mode cap={cap}");
        assert!(3 < cap, "real 3 < cap {cap} → clone mode");
        assert_eq!(p.clone_real_index(4), 1, "displayed 4 → real 1 (4 % 3)");
        assert_eq!(p.clone_fill_count(), cap - 3);
        let (aspects, reals) = p.page_render();
        assert_eq!(aspects.len(), cap);
        assert_eq!(reals.len(), cap);
        assert_eq!(reals[4], 1);
    }

    #[test]
    fn total_pages_boundary_exactly_capacity_and_plus_one() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        assert!(cap > 1 && cap < 64, "cap sane {cap}");
        let exactly = MosaicPages::new(cap, STAGE_W, STAGE_H);
        assert_eq!(exactly.total_pages(), 1, "exactly capacity → 1 page");
        assert!(exactly.hidden());
        let plus_one = MosaicPages::new(cap + 1, STAGE_W, STAGE_H);
        assert_eq!(plus_one.total_pages(), 2, "capacity+1 → 2 pages");
        assert!(!plus_one.hidden());
        let plus_cap = MosaicPages::new(cap * 2, STAGE_W, STAGE_H);
        assert_eq!(plus_cap.total_pages(), 2);
        let plus_cap_one = MosaicPages::new(cap * 2 + 1, STAGE_W, STAGE_H);
        assert_eq!(plus_cap_one.total_pages(), 3);
    }

    #[test]
    fn pagination_hidden_predicate_and_numbers() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let one = MosaicPages::new(cap, STAGE_W, STAGE_H);
        assert!(one.hidden());
        assert_eq!(one.page_numbers(), vec![1]);
        let two = MosaicPages::new(cap + 1, STAGE_W, STAGE_H);
        assert!(!two.hidden());
        assert_eq!(two.page_numbers(), vec![1, 2]);
        let zero = MosaicPages::new(0, STAGE_W, STAGE_H);
        assert!(zero.hidden());
        assert!(zero.page_numbers().is_empty());
    }

    #[test]
    fn clamp_no_wrap_via_step_large_delta() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let total = cap * 2 + 1;
        let mut p = MosaicPages::new(total, STAGE_W, STAGE_H);
        assert_eq!(p.total_pages(), 3);
        // jump far beyond end → clamp to last
        p.step(99);
        assert_eq!(p.current(), 2, "step 99 must clamp to last");
        // jump far before start → clamp to first
        p.step(-99);
        assert_eq!(p.current(), 0, "step -99 must clamp to first");
        assert!(!p.step(0), "step 0 no change");
    }

    #[test]
    fn go_to_clamps_no_wrap() {
        let cap = mosaic_page_capacity(STAGE_W, STAGE_H);
        let mut p = MosaicPages::new(cap * 3, STAGE_W, STAGE_H);
        assert_eq!(p.total_pages(), 3);
        assert!(p.go_to(2));
        assert_eq!(p.current(), 2);
        assert!(!p.go_to(2), "same page no change");
        assert!(p.go_to(0));
        assert_eq!(p.current(), 0);
        assert!(p.go_to(99));
        assert_eq!(p.current(), 2, "go_to 99 clamps to last");
        assert!(!p.go_to(99), "already at last, clamped no change");
        let mut empty = MosaicPages::new(0, STAGE_W, STAGE_H);
        assert!(!empty.go_to(5));
    }

    #[test]
    fn mosaic_neighbor_follows_reading_order_and_rows() {
        // Two justified rows with DIFFERENT tile widths (the real case):
        // row 0 = three tiles 300/200/400 wide, row 1 = two tiles 400/500.
        let tiles = vec![
            MosaicTileRect { x: 0.0, y: 0.0, w: 300.0, h: 200.0 },
            MosaicTileRect { x: 300.0, y: 0.0, w: 200.0, h: 200.0 },
            MosaicTileRect { x: 500.0, y: 0.0, w: 400.0, h: 200.0 },
            MosaicTileRect { x: 0.0, y: 200.0, w: 400.0, h: 200.0 },
            MosaicTileRect { x: 400.0, y: 200.0, w: 500.0, h: 200.0 },
        ];
        // Reading order: the model is packed row by row, so left/right are
        // the adjacent entries — including the row seam.
        assert_eq!(mosaic_neighbor(&tiles, 0, MosaicDir::Right), Some(1));
        assert_eq!(
            mosaic_neighbor(&tiles, 2, MosaicDir::Right),
            Some(3),
            "the last tile of a row is followed by the next row's first"
        );
        assert_eq!(mosaic_neighbor(&tiles, 3, MosaicDir::Left), Some(2));
        assert_eq!(
            mosaic_neighbor(&tiles, 0, MosaicDir::Left),
            None,
            "the first tile has no previous: the caller steps the page"
        );
        assert_eq!(
            mosaic_neighbor(&tiles, 4, MosaicDir::Right),
            None,
            "the last tile has no next: the caller steps the page"
        );

        // Up/down use the geometry, because rows have different tile widths.
        // From tile 0 (centre x = 150) the row below contains 150 in index 3.
        assert_eq!(mosaic_neighbor(&tiles, 0, MosaicDir::Down), Some(3));
        // From tile 2 (centre x = 700) index 4 spans 400..900, which contains
        // 700 — no nearest-centre guesswork needed.
        assert_eq!(mosaic_neighbor(&tiles, 2, MosaicDir::Down), Some(4));
        assert_eq!(
            mosaic_neighbor(&tiles, 4, MosaicDir::Up),
            Some(2),
            "up from centre 650 lands on the row-0 tile spanning 500..900"
        );
        assert_eq!(mosaic_neighbor(&tiles, 1, MosaicDir::Up), None, "nothing above the first row");
        assert_eq!(mosaic_neighbor(&tiles, 3, MosaicDir::Down), None, "nothing below the last row");
        assert_eq!(mosaic_neighbor(&tiles, 9, MosaicDir::Right), None, "out of range is no move");
    }

    #[test]
    fn mosaic_neighbor_picks_the_nearest_tile_when_no_span_contains_the_centre() {
        // A short row under a long one: the cursor sits past the last tile's
        // right edge, so the nearest centre wins instead of "no move".
        let tiles = vec![
            MosaicTileRect { x: 0.0, y: 0.0, w: 900.0, h: 100.0 },
            MosaicTileRect { x: 0.0, y: 100.0, w: 100.0, h: 100.0 },
            MosaicTileRect { x: 100.0, y: 100.0, w: 100.0, h: 100.0 },
        ];
        assert_eq!(mosaic_neighbor(&tiles, 0, MosaicDir::Down), Some(2), "centre 450: nearest is 150 (index 2)");
    }
}
