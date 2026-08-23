// HVE 2 — Real wallpaper thumbnail pipeline (gallery-immersive-redesign
// PR4, design D8).
//
// Decodes a theme's source image, cover-crops + scales it to ≤400×720 and
// caches the result as a PNG under `$XDG_CACHE_HOME/hve/thumbs/<key>.png`,
// keyed by hash(source path + mtime) with LRU 200 prune semantics (spec:
// Real Wallpaper Thumbnails; model.rs R8 cache capacity reused).
//
// MIT credit: visual language translated from skwd-wall (MIT, © liixini).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Thumbnail upper bound (design D8 / spec: decode ≤400×720 source size).
pub const THUMB_MAX_W: u32 = 400;
pub const THUMB_MAX_H: u32 = 720;

/// Cache-key contract (task 4.5): deterministic hash of the SOURCE PATH +
/// MTIME so a changed wallpaper invalidates its stale thumbnail. Uses
/// std's SipHash via DefaultHasher — no crypto dependency; collisions are
/// irrelevant at LRU 200 scale.
pub fn cache_key(source: &Path, mtime: SystemTime) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.to_string_lossy().hash(&mut h);
    if let Ok(d) = mtime.duration_since(SystemTime::UNIX_EPOCH) {
        d.as_secs().hash(&mut h);
        d.subsec_nanos().hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

/// Modification time of a file, None when it cannot be read.
pub fn source_mtime(source: &Path) -> Option<SystemTime> {
    std::fs::metadata(source).ok()?.modified().ok()
}

/// Pure cover-crop + scale math (design D8): pick the largest centered
/// crop of (src_w × src_h) matching the 400:720 aspect WITHOUT upscaling,
/// then the output dims capped to (max_w × max_h). Returns
/// `(crop_x, crop_y, crop_w, crop_h, out_w, out_h)`.
pub fn cover_geometry(
    src_w: u32,
    src_h: u32,
    max_w: u32,
    max_h: u32,
) -> (u32, u32, u32, u32, u32, u32) {
    let (w, h) = (src_w as u64, src_h as u64);
    // Crop to target aspect: width-limited when the source is wider than
    // max ratio, height-limited otherwise. Integer math stays exact.
    let crop_w = w.min(h * max_w as u64 / max_h as u64);
    let crop_h = h.min(w * max_h as u64 / max_w as u64);
    let crop_x = (w - crop_w) / 2;
    let crop_y = (h - crop_h) / 2;
    // Never upscale: scale caps at 1.0.
    let scale = 1.0_f64.min(max_w as f64 / crop_w as f64).min(max_h as f64 / crop_h as f64);
    let out_w = ((crop_w as f64) * scale).round() as u32;
    let out_h = ((crop_h as f64) * scale).round() as u32;
    (
        crop_x as u32,
        crop_y as u32,
        crop_w as u32,
        crop_h as u32,
        out_w.max(1),
        out_h.max(1),
    )
}

/// Thumbnail PNG cache path for a source inside `out_dir` (no IO).
pub fn thumb_path(source: &Path, out_dir: &Path) -> PathBuf {
    let mtime = source_mtime(source).unwrap_or(SystemTime::UNIX_EPOCH);
    out_dir.join(format!("{}.png", cache_key(source, mtime)))
}

/// Decode `source`, cover-crop + scale to ≤400×720 and write the PNG to
/// `out_dir/<key>.png` (task 4.6). Existing cache files short-circuit the
/// decode ("cache async": callers may probe this cheaply off-thread).
pub fn generate(source: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let out = thumb_path(source, out_dir);
    if out.exists() {
        return Ok(out);
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create thumb dir {}: {e}", out_dir.display()))?;
    let img = image::ImageReader::open(source)
        .map_err(|e| format!("cannot open {}: {e}", source.display()))?
        .decode()
        .map_err(|e| format!("cannot decode {}: {e}", source.display()))?;
    let (cx, cy, cw, ch, ow, oh) =
        cover_geometry(img.width(), img.height(), THUMB_MAX_W, THUMB_MAX_H);
    let thumb = img.crop_imm(cx, cy, cw, ch).resize_exact(ow, oh, image::imageops::FilterType::Triangle);
    thumb.save(&out).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    Ok(out)
}

/// `$XDG_CACHE_HOME/hve/thumbs`, falling back to `~/.cache/hve/thumbs`
/// (design D8 cache location).
pub fn cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| dirs::home_dir().map(|h| h.join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("hve").join("thumbs")
}

// ── 4.5 RED tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn st(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }

    #[test]
    fn cache_key_is_stable_for_same_input() {
        let a = cache_key(Path::new("/imgs/wall.png"), st(100));
        let b = cache_key(Path::new("/imgs/wall.png"), st(100));
        assert_eq!(a, b, "same path+mtime must map to the same cache key");
    }

    #[test]
    fn cache_key_differs_by_path() {
        let a = cache_key(Path::new("/imgs/a.png"), st(100));
        let b = cache_key(Path::new("/imgs/b.png"), st(100));
        assert_ne!(a, b, "different sources must not collide");
    }

    #[test]
    fn cache_key_differs_by_mtime() {
        let a = cache_key(Path::new("/imgs/wall.png"), st(100));
        let b = cache_key(Path::new("/imgs/wall.png"), st(200));
        assert_ne!(a, b, "mtime change must invalidate the cached thumb");
    }

    #[test]
    fn cache_key_is_hex_string() {
        let k = cache_key(Path::new("/imgs/wall.png"), st(100));
        assert_eq!(k.len(), 16, "u64 hash rendered as 16 hex chars");
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()), "hex only, got {k}");
    }

    // ── LRU 200 prune semantics (task 4.5; reuses model.rs R8 capacity) ──

    use super::super::model::{ThumbnailCache, THUMBNAIL_CACHE_CAPACITY};

    fn store() -> ThumbnailCache {
        ThumbnailCache::with_capacity(THUMBNAIL_CACHE_CAPACITY)
    }

    #[test]
    fn lru_prunes_to_200_and_evicts_oldest() {
        let mut s = store();
        for i in 0..250 {
            s.insert(cache_key(Path::new(&format!("/i/{i}.png")), st(i)), PathBuf::from(format!("/{i}.png")));
        }
        assert_eq!(s.len(), 200, "LRU bounded at capacity");
        assert!(!s.contains(&cache_key(Path::new("/i/0.png"), st(0))), "oldest evicted");
        assert!(!s.contains(&cache_key(Path::new("/i/49.png"), st(49))), "pre-eviction cutoff gone");
        assert!(s.contains(&cache_key(Path::new("/i/50.png"), st(50))), "cutoff retained");
        assert!(s.contains(&cache_key(Path::new("/i/249.png"), st(249))), "newest retained");
    }

    // ── 4.6 GREEN: cover-crop geometry + decode/scale/write roundtrip ──

    #[test]
    fn cover_geometry_landscape_crops_sides() {
        let (cx, cy, cw, ch, ow, oh) = cover_geometry(1920, 1080, THUMB_MAX_W, THUMB_MAX_H);
        assert_eq!((cx, cy, cw, ch), (660, 0, 600, 1080), "center crop to 400:720 aspect");
        assert_eq!((ow, oh), (400, 720));
    }

    #[test]
    fn cover_geometry_portrait_crops_top_bottom() {
        let (cx, cy, cw, ch, ow, oh) = cover_geometry(720, 1600, THUMB_MAX_W, THUMB_MAX_H);
        assert_eq!((cx, cy, cw, ch), (0, 152, 720, 1296));
        assert_eq!((ow, oh), (400, 720));
    }

    #[test]
    fn cover_geometry_never_upscales_small_sources() {
        let (_, _, cw, ch, ow, oh) = cover_geometry(100, 100, THUMB_MAX_W, THUMB_MAX_H);
        assert_eq!((cw, ch), (55, 100), "aspect crop only");
        assert_eq!((ow, oh), (55, 100), "output keeps source pixels, no upscale");
    }

    #[test]
    fn generated_thumb_roundtrips_and_hits_cache() {
        let dir = tempfile::tempdir().expect("tmp");
        let src = dir.path().join("src.png");
        image::RgbaImage::from_pixel(40, 30, image::Rgba([255u8, 0, 0, 255]))
            .save(&src)
            .expect("write source png");
        let out_dir = dir.path().join("thumbs");

        let first = super::generate(&src, &out_dir).expect("generate");
        assert!(first.exists(), "png written");
        let decoded = image::ImageReader::open(&first).unwrap().decode().unwrap();
        // 40×30 → aspect crop 16×30; small source must not upscale.
        assert_eq!((decoded.width(), decoded.height()), (16, 30));

        let second = super::generate(&src, &out_dir).expect("regenerate");
        assert_eq!(first, second, "same key short-circuits to the cached png");
    }
}
