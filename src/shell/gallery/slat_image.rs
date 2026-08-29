// HVE 2 — S4a slat image baking (parallelogram alpha).
//
// Parallelogram geometry derived from `ui/gallery/SliceDelegate.slint`
// `SliceShape` (skwd-wall exact, design D3):
//   - skew = 35px X-shear (token `gallery-slice-skew-offset`), passed as
//     `skew_px` param — no hardcoding inside `bake_parallelogram`.
//   - lean direction: top edge shifted RIGHT by `skew` relative to bottom
//     edge. Vertices with r=0: (skew,0) (skew+wn,0) (wn,hn) (0,hn) where
//     wn = face width, hn = 520. Bounding box width = wn + skew.
//   - outside wedges (slant-mask triangles): top-left triangle
//     (0,0)-(skew,0)-(0,hn) and bottom-right triangle
//     (wn,0)-(wn+skew,0)-(wn,hn) → corners (0,0) and (w-1,h-1) are outside.
//   - bounding box = target_width (wn+skew) × target_height (hn).
//
// MIT credit: visual language translated from skwd-wall (MIT, © liixini).

use image::{RgbaImage, imageops};

/// Bake the parallelogram shape into the image alpha (pure, deterministic).
///
/// 1. Cover-crop `src` to `target_width` × `target_height` (center-crop after
///    scale-to-cover).
/// 2. Apply parallelogram alpha mask: pixels outside the slanted shape get
///    alpha 0, inside keep original alpha (multiply semantics).
/// 3. No I/O, no globals.
pub fn bake_parallelogram(
    src: RgbaImage,
    target_width: u32,
    target_height: u32,
    skew_px: f32,
) -> RgbaImage {
    // Degenerate: zero-size source → zero-size image without panic (contract).
    if src.width() == 0 || src.height() == 0 {
        return RgbaImage::new(0, 0);
    }
    if target_width == 0 || target_height == 0 {
        return RgbaImage::new(target_width, target_height);
    }

    // ── 1. Cover-crop ───────────────────────────────────────────────
    let sw = src.width() as f32;
    let sh = src.height() as f32;
    let tw = target_width as f32;
    let th = target_height as f32;
    let scale = (tw / sw).max(th / sh);
    let scaled_w = ((sw * scale).ceil() as u32).max(target_width);
    let scaled_h = ((sh * scale).ceil() as u32).max(target_height);

    // Avoid filtering when scale is identity (keeps exact pixels for tests).
    let scaled = if scaled_w == src.width() && scaled_h == src.height() {
        src.clone()
    } else {
        imageops::resize(&src, scaled_w, scaled_h, imageops::FilterType::Nearest)
    };

    let x_off = (scaled_w - target_width) / 2;
    let y_off = (scaled_h - target_height) / 2;
    let mut out = imageops::crop_imm(&scaled, x_off, y_off, target_width, target_height).to_image();

    // ── 2. Parallelogram alpha mask ──────────────────────────────────
    // Parallelogram vertices (r=0): (skew,0) (skew+wn,0) (wn,hn) (0,hn)
    // Bounding box width = wn + skew = target_width.
    // Lean: top shifted right by skew relative to bottom.
    // Inside test uses pixel center (x+0.5, y+0.5) vs continuous left/right.
    let skew = skew_px.clamp(0.0, tw);
    let inner_w = (tw - skew).max(0.0);
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let cx = x as f32 + 0.5;
        let cy = y as f32 + 0.5;
        let t = cy / th;
        let left = skew * (1.0 - t);
        let right = left + inner_w;
        let inside = cx >= left && cx < right;
        if !inside {
            pixel[3] = 0;
        }
    }
    out
}

/// Gap between collapsed slats — MUST stay in sync with `SkwdTokens.gallery-slat-gap`
/// (24px in `ui/tokens.slint`). S3 face inset: collapsed face = slot − gap.
const SLAT_GAP_PX: f32 = 24.0;

/// Baked slat bbox — MUST equal drawn bbox (face-width + skew) × height.
/// Collapsed: (SLICE_COLLAPSED_WIDTH − SLAT_GAP_PX) + SLICE_SKEW_PX = 111+35 = 146
/// Expanded:  SLICE_EXPANDED_WIDTH + SLICE_SKEW_PX = 924+35 = 959
/// Height:    SLICE_HEIGHT = 520
/// Token `gallery-slat-gap` (24px) and `SLAT_GAP_PX` must stay in sync.
pub const SLAT_BBOX_COLLAPSED_W: u32 = 146; // (135 − 24 + 35) — keep in sync with SLAT_GAP_PX/token
pub const SLAT_BBOX_EXPANDED_W: u32 = 959; // (924 + 35)
pub const SLAT_BBOX_H: u32 = 520;
// Compile-time guard that the literal matches the formula (gap + slice constants).
const _: () = assert!(
    SLAT_BBOX_COLLAPSED_W == (crate::shell::gallery::views::slice::SLICE_COLLAPSED_WIDTH - SLAT_GAP_PX + crate::shell::gallery::views::slice::SLICE_SKEW_PX) as u32
);
const _: () = assert!(
    SLAT_BBOX_EXPANDED_W == (crate::shell::gallery::views::slice::SLICE_EXPANDED_WIDTH + crate::shell::gallery::views::slice::SLICE_SKEW_PX) as u32
);
const _: () = assert!(SLAT_BBOX_H == crate::shell::gallery::views::slice::SLICE_HEIGHT as u32);

/// Baked slat bbox helpers — collapsed uses face-width (slot − gap) + skew = 146.
fn collapsed_bbox() -> (u32, u32) {
    (SLAT_BBOX_COLLAPSED_W, SLAT_BBOX_H)
}

fn expanded_bbox() -> (u32, u32) {
    (SLAT_BBOX_EXPANDED_W, SLAT_BBOX_H)
}

/// Pure helper: bake a slat Rgba to the collapsed/expanded bbox.
pub fn baked_slat_rgba(src: RgbaImage, expanded: bool) -> RgbaImage {
    use crate::shell::gallery::views::slice::SLICE_SKEW_PX;
    let (tw, th) = if expanded { expanded_bbox() } else { collapsed_bbox() };
    bake_parallelogram(src, tw, th, SLICE_SKEW_PX)
}

/// `baked_slat_image` / `baked_slat_image_from_path` removed in Tramo 7:
/// UI wraps `baked_slat_rgba` buffers itself via `Image::from_rgba8` (no
/// Slint image helper on the baking side). Use `baked_slat_rgba` directly.

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid_rgba(w: u32, h: u32, c: Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(w, h, c)
    }

    // ── Corner leaks: top-left and bottom-right outside (lean right at top) ──

    #[test]
    fn corner_leaks_top_left_and_bottom_right_transparent() {
        let src = solid_rgba(40, 20, Rgba([100, 150, 200, 255]));
        let out = bake_parallelogram(src, 40, 20, 10.0);
        assert_eq!(out.dimensions(), (40, 20));
        // Top-left wedge outside → alpha 0
        assert_eq!(out.get_pixel(0, 0)[3], 0, "top-left (0,0) must be transparent (left wedge)");
        // Bottom-right wedge outside → alpha 0
        assert_eq!(out.get_pixel(39, 19)[3], 0, "bottom-right (w-1,h-1) must be transparent (right wedge)");
    }

    #[test]
    fn both_wedge_regions_outside() {
        let src = solid_rgba(40, 20, Rgba([10, 20, 30, 255]));
        let out = bake_parallelogram(src, 40, 20, 10.0);
        // Top edge near left wedge interior checks
        // Another point strictly inside top-left wedge
        assert_eq!(out.get_pixel(1, 1)[3], 0, "near top-left inside wedge");
        // Bottom-right wedge second sample
        assert_eq!(out.get_pixel(38, 18)[3], 0, "near bottom-right inside wedge");
        // Sanity: opposite corners must stay opaque
        assert_eq!(out.get_pixel(39, 0)[3], 255, "top-right inside");
        assert_eq!(out.get_pixel(0, 19)[3], 255, "bottom-left inside");
    }

    #[test]
    fn interior_opaque_rgb_unchanged() {
        let src = solid_rgba(40, 20, Rgba([11, 22, 33, 200]));
        // Source has uniform color with alpha 200 inside
        let out = bake_parallelogram(src, 40, 20, 10.0);
        let center = out.get_pixel(20, 10);
        assert_eq!(center[3], 200, "center keeps original partial alpha");
        assert_eq!(center[0], 11);
        assert_eq!(center[1], 22);
        assert_eq!(center[2], 33);
        // Fully opaque variant
        let src2 = solid_rgba(40, 20, Rgba([99, 88, 77, 255]));
        let out2 = bake_parallelogram(src2, 40, 20, 10.0);
        let c2 = out2.get_pixel(20, 10);
        assert_eq!(c2[3], 255);
        assert_eq!([c2[0], c2[1], c2[2]], [99, 88, 77]);
    }

    #[test]
    fn just_inside_and_outside_slant_edge_mid_height() {
        let src = solid_rgba(40, 20, Rgba([50, 60, 70, 255]));
        let skew = 10.0;
        let out = bake_parallelogram(src, 40, 20, skew);
        let y = 10; // mid-height
        // left = skew*(1 - (y+0.5)/h) ≈ 10*(1 -10.5/20)=4.75
        // Pixels with center < left → outside, >= left → inside
        // Sample ±2px away from edge to avoid exact boundary.
        // Expected inside at x well right of edge, outside well left.
        let left = skew * (1.0 - (y as f32 + 0.5) / 20.0);
        let x_outside = (left as i32 - 2).max(0) as u32;
        let x_inside = (left as i32 + 2) as u32;
        assert_eq!(out.get_pixel(x_outside, y)[3], 0, "just-outside slant edge at mid-height x={x_outside} left~{left}");
        assert_eq!(out.get_pixel(x_inside, y)[3], 255, "just-inside slant edge at mid-height x={x_inside} left~{left}");
        // Right edge: right = left + (w - skew) = left +30
        let right = left + (40.0 - skew);
        let x_inside_r = (right as i32 - 3).max(0) as u32;
        let x_outside_r = (right as i32 + 2) as u32;
        if x_outside_r < 40 {
            assert_eq!(out.get_pixel(x_outside_r, y)[3], 0, "just-outside right edge x={x_outside_r} right~{right}");
        }
        assert_eq!(out.get_pixel(x_inside_r, y)[3], 255, "just-inside right edge x={x_inside_r} right~{right}");
    }

    #[test]
    fn cover_crop_square_target_center_matches_source_center() {
        // Source 20×10 (2:1), target 10×10 square, skew 0 to avoid masking
        let mut src = RgbaImage::new(20, 10);
        // Fill with distinct gradient and mark center
        for y in 0..10 {
            for x in 0..20 {
                src.put_pixel(x, y, Rgba([x as u8 * 12, y as u8 * 25, 0, 255]));
            }
        }
        // Center of source at (10,5)
        let src_center = *src.get_pixel(10, 5);
        let out = bake_parallelogram(src, 10, 10, 0.0);
        assert_eq!(out.dimensions(), (10, 10), "output dims must equal target dims");
        let out_center = *out.get_pixel(5, 5);
        assert_eq!(out_center, src_center, "center pixel must match source center after cover-crop (scale max + center-crop)");
    }

    #[test]
    fn degenerate_skew_zero_rectangle_full_alpha() {
        let src = solid_rgba(8, 8, Rgba([1, 2, 3, 255]));
        let out = bake_parallelogram(src, 8, 8, 0.0);
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(out.get_pixel(x, y)[3], 255, "skew 0 → full rectangle opaque at {x},{y}");
            }
        }
    }

    #[test]
    fn degenerate_one_by_one_source() {
        let src = solid_rgba(1, 1, Rgba([9, 9, 9, 255]));
        // Use small skew so interior still covers center; 1×1 source cover-crops to target
        let out = bake_parallelogram(src, 4, 4, 1.0);
        assert_eq!(out.dimensions(), (4, 4));
        // With 1×1 source scaled to cover 4×4, valid interior pixels keep alpha
        // At least center must be opaque (parallelogram still covers middle)
        assert_eq!(out.get_pixel(2, 2)[3], 255);
        // Also verify 1×1 with skew 0 full rectangle
        let src2 = solid_rgba(1, 1, Rgba([9, 9, 9, 255]));
        let out2 = bake_parallelogram(src2, 4, 4, 0.0);
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(out2.get_pixel(x, y)[3], 255, "skew 0 with 1×1 source → all opaque at {x},{y}");
            }
        }
    }

    #[test]
    fn zero_size_source_returns_empty_without_panic() {
        let src = RgbaImage::new(0, 0);
        let out = bake_parallelogram(src, 10, 10, 5.0);
        assert_eq!(out.dimensions(), (0, 0));
        // Also empty width source
        let src2 = RgbaImage::new(0, 5);
        let out2 = bake_parallelogram(src2, 10, 10, 5.0);
        assert_eq!(out2.dimensions(), (0, 0));
    }

    #[test]
    fn pure_deterministic_no_io() {
        let src = solid_rgba(10, 10, Rgba([42, 42, 42, 180]));
        let a = bake_parallelogram(src.clone(), 10, 10, 3.0);
        let b = bake_parallelogram(src, 10, 10, 3.0);
        assert_eq!(a.dimensions(), b.dimensions());
        assert_eq!(a.as_raw(), b.as_raw(), "pure deterministic: same input → same output");
    }

    // ── S4b RED: baked slat image feed contracts (collapsed/expanded bbox) ──
    // Invariant: baked bbox width == drawn bbox width (face-width + skew).
    // Collapsed drawn = (135 − 24 gap) + 35 = 146; expanded = 924 + 35 = 959.

    #[test]
    fn slat_bbox_constants_match_drawn_geometry() {
        use crate::shell::gallery::slat_image::{SLAT_BBOX_COLLAPSED_W, SLAT_BBOX_EXPANDED_W, SLAT_BBOX_H};
        use crate::shell::gallery::views::slice::{SLICE_COLLAPSED_WIDTH, SLICE_EXPANDED_WIDTH, SLICE_HEIGHT, SLICE_SKEW_PX};
        let gap = 24.0; // must stay in sync with SkwdTokens.gallery-slat-gap (24px) and SLAT_GAP_PX
        assert_eq!(SLAT_BBOX_COLLAPSED_W, (SLICE_COLLAPSED_WIDTH - gap + SLICE_SKEW_PX) as u32, "collapsed bbox must be (135-24)+35=146 (face+skew)");
        assert_eq!(SLAT_BBOX_EXPANDED_W, (SLICE_EXPANDED_WIDTH + SLICE_SKEW_PX) as u32, "expanded bbox must be 924+35=959");
        assert_eq!(SLAT_BBOX_H, SLICE_HEIGHT as u32, "bbox height must be 520");
        assert_eq!(SLAT_BBOX_COLLAPSED_W, 146);
        assert_eq!(SLAT_BBOX_EXPANDED_W, 959);
        assert_eq!(SLAT_BBOX_H, 520);
    }

    // `baked_slat_image` removed — UI now wraps `baked_slat_rgba` buffers via `Image::from_rgba8`.
    // Bbox contracts below are covered by `baked_slat_rgba_*` tests.

    #[test]
    fn baked_slat_rgba_collapsed_matches_drawn_bbox() {
        let src = solid_rgba(80, 60, Rgba([10, 20, 30, 255]));
        let rgba = crate::shell::gallery::slat_image::baked_slat_rgba(src, false);
        assert_eq!(rgba.width(), 146, "baked_slat_rgba collapsed width must be 146");
        assert_eq!(rgba.height(), 520);
        assert_eq!(rgba.width(), crate::shell::gallery::slat_image::SLAT_BBOX_COLLAPSED_W);
    }

    #[test]
    fn baked_slat_rgba_expanded_matches_drawn_bbox() {
        let src = solid_rgba(80, 60, Rgba([10, 20, 30, 255]));
        let rgba = crate::shell::gallery::slat_image::baked_slat_rgba(src, true);
        assert_eq!(rgba.width(), 959, "baked_slat_rgba expanded width must be 959");
        assert_eq!(rgba.height(), 520);
        assert_eq!(rgba.width(), crate::shell::gallery::slat_image::SLAT_BBOX_EXPANDED_W);
    }

    #[test]
    fn baked_slat_image_idempotent_pure() {
        // Retargeted to `baked_slat_rgba` — UI wrapping is trivial `from_rgba8`;
        // purity contract is about the baked rgba buffers being deterministic.
        let src = solid_rgba(20, 20, Rgba([42, 42, 42, 180]));
        let a = crate::shell::gallery::slat_image::baked_slat_rgba(src.clone(), false);
        let b = crate::shell::gallery::slat_image::baked_slat_rgba(src.clone(), false);
        assert_eq!(a.as_raw(), b.as_raw(), "collapsed pure deterministic");
        let c = crate::shell::gallery::slat_image::baked_slat_rgba(src.clone(), true);
        let d = crate::shell::gallery::slat_image::baked_slat_rgba(src, true);
        assert_eq!(c.as_raw(), d.as_raw(), "expanded pure deterministic");
        // Also verify wrapping via from_rgba8 is lossless (what UI does)
        let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(a.width(), a.height());
        buf.make_mut_bytes().copy_from_slice(a.as_raw());
        let img = slint::Image::from_rgba8(buf);
        assert_eq!(img.size().width, a.width());
        assert_eq!(img.size().height, a.height());
    }
}
