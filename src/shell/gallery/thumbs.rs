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

/// File extensions eligible as thumbnail sources.
const SOURCE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp"];

/// Try the provider wallpapers.json files first (FIX1): themes store the
/// real wallpaper paths there, not as loose files. Priority:
/// wallpapers[DP-3].dark/light, wallpapers[""].dark/light,
/// wallpapers[FALLBACK].dark/light, defaultWallpaper,
/// usedRandomWallpapers[DP-3][0]. First existing path wins.
fn wallpaper_from_provider_json(theme_dir: &Path) -> Option<PathBuf> {
    for rel in ["providers/noctalia/wallpapers.json", "providers/wallpaper/wallpapers.json"] {
        let p = theme_dir.join(rel);
        if !p.exists() { continue; }
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                let mut cands: Vec<String> = Vec::new();
                if let Some(wp) = v.get("wallpapers").and_then(|x| x.as_object()) {
                    for key in ["DP-3", "", "FALLBACK"] {
                        if let Some(e) = wp.get(key) {
                            if let Some(s) = e.get("dark").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) { cands.push(s.to_string()); }
                            if let Some(s) = e.get("light").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) { cands.push(s.to_string()); }
                        }
                    }
                }
                if let Some(s) = v.get("defaultWallpaper").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) { cands.push(s.to_string()); }
                if let Some(used) = v.get("usedRandomWallpapers").and_then(|x| x.as_object()) {
                    if let Some(arr) = used.get("DP-3").and_then(|x| x.as_array()) {
                        if let Some(s) = arr.first().and_then(|x| x.as_str()).filter(|s| !s.is_empty()) { cands.push(s.to_string()); }
                    }
                }
                for cand in cands {
                    let pb = PathBuf::from(&cand);
                    if pb.exists() { return Some(pb); }
                }
            }
        }
    }
    None
}

/// Find the first usable image file inside a theme directory (task 4.7
/// source discovery). Themes currently persist provider state under their
/// directory; the scan is depth-first over sorted entries so results are
/// deterministic. None = theme has no image source (skeleton stays).
pub fn find_source_image(theme_dir: &Path) -> Option<PathBuf> {
    if let Some(hit) = wallpaper_from_provider_json(theme_dir) {
        return Some(hit);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(theme_dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    files.sort();
    // Images first at this level, then recurse into subdirectories
    // (provider subdirs like wallpaper/), keeping traversal ordered.
    if let Some(hit) = files.iter().find(|p| {
        p.is_file()
            && p.extension()
                .and_then(|e| e.to_str())
                .map(|e| SOURCE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
    }) {
        return Some(hit.clone());
    }
    for dir in files.iter().filter(|p| p.is_dir()) {
        if let Some(hit) = find_source_image(dir) {
            return Some(hit);
        }
    }
    None
}

/// One off-thread thumbnail request (task 4.7).
#[derive(Debug, Clone)]
pub struct PreheatJob {
    /// Card row index in the UI model.
    pub index: usize,
    /// Card name captured for stale-write guards on marshal.
    pub name: String,
    pub source: PathBuf,
}

/// Pure planner: keep only cards that have a source image (task 4.7).
pub fn plan_jobs(sources: Vec<(usize, String, Option<PathBuf>)>) -> Vec<PreheatJob> {
    sources
        .into_iter()
        .filter_map(|(index, name, source)| {
            source.map(|source| PreheatJob { index, name, source })
        })
        .collect()
}

/// Generate thumbnails OFF-THREAD and marshal each finished one to the UI
/// thread via `slint::invoke_from_event_loop`, invoking `ready(index, name,
/// png_path)` there (task 4.7 / design D8). One thread per job — galleries
/// are small; failures log a warning and never block the UI. Only Send data
/// crosses the boundary (the PNG PATH, not a decoded image): `slint::Image`
/// wraps non-Send backend storage, so the UI side loads it from disk.
/// `ready` must be Send+Sync because it travels inside an Arc.
pub fn preheat(
    jobs: Vec<PreheatJob>,
    ready: impl Fn(usize, std::sync::Arc<str>, PathBuf) + Send + Sync + 'static,
) {
    let ready = std::sync::Arc::new(ready);
    for job in jobs {
        let ready = ready.clone();
        std::thread::spawn(move || {
            match generate(&job.source, &cache_dir()) {
                Ok(png) => {
                    let idx = job.index;
                    let name: std::sync::Arc<str> = std::sync::Arc::from(job.name.as_str());
                    let _ = slint::invoke_from_event_loop(move || ready(idx, name, png));
                }
                Err(e) => {
                    tracing::warn!("[thumbs] preheat for '{}' failed: {}", job.source.display(), e);
                }
            }
        });
    }
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

    // ── 4.7 WIRE: source discovery + off-thread planner ──

    #[test]
    fn find_source_scans_theme_dir_deterministically() {
        let dir = tempfile::tempdir().expect("tmp");
        let nested = dir.path().join("wallpaper");
        std::fs::create_dir_all(&nested).expect("mkdir");
        std::fs::write(dir.path().join("meta.json"), b"{}").expect("meta");
        std::fs::write(nested.join("a.txt"), b"nope").expect("txt");
        let img = nested.join("bg.jpg");
        // Discovery is extension-based (decode happens later in generate),
        // so plain bytes are enough here.
        std::fs::write(&img, b"not-a-real-image").expect("jpg");

        let found = find_source_image(dir.path()).expect("source found");
        assert_eq!(found.file_name().unwrap(), "bg.jpg");
    }

    #[test]
    fn find_source_none_without_images() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("meta.json"), b"{}").expect("meta");
        assert!(find_source_image(dir.path()).is_none(), "no images -> None");
        assert!(find_source_image(&dir.path().join("missing")).is_none(), "missing dir safe");
    }

    #[test]
    fn plan_jobs_keeps_only_sourced_cards() {
        let jobs = plan_jobs(vec![
            (0, "A".into(), Some(PathBuf::from("/a.png"))),
            (1, "B".into(), None),
            (2, "C".into(), Some(PathBuf::from("/c.jpg"))),
        ]);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].index, 0);
        assert_eq!(jobs[1].index, 2);
        assert_eq!(jobs[1].name, "C");
    }

    // ── FIX1: wallpapers.json provider discovery ─────────────────────

    #[test]
    fn find_source_prefers_wallpaper_json_over_loose_scan() {
        let dir = tempfile::tempdir().expect("tmp");
        let real = dir.path().join("real.png");
        image::RgbaImage::from_pixel(10, 10, image::Rgba([0u8, 0, 0, 255])).save(&real).unwrap();
        let prov = dir.path().join("providers/noctalia");
        std::fs::create_dir_all(&prov).unwrap();
        let json = serde_json::json!({
            "wallpapers": { "DP-3": { "dark": real.to_string_lossy(), "light": real.to_string_lossy() } },
            "defaultWallpaper": "/nope/missing.png"
        });
        std::fs::write(prov.join("wallpapers.json"), serde_json::to_string(&json).unwrap()).unwrap();
        // loose file that would be found by scan but should be ignored when JSON hits
        std::fs::write(dir.path().join("loose.jpg"), b"dummy").unwrap();
        let found = find_source_image(dir.path()).expect("json source");
        assert_eq!(found, real);
    }

    #[test]
    fn find_source_json_falls_back_to_scan_when_candidates_missing() {
        let dir = tempfile::tempdir().expect("tmp");
        let prov = dir.path().join("providers/wallpaper");
        std::fs::create_dir_all(&prov).unwrap();
        let json = serde_json::json!({
            "wallpapers": { "DP-3": { "dark": "/tmp/missing-a.png", "light": "/tmp/missing-b.png" } },
            "defaultWallpaper": "/tmp/missing-c.png"
        });
        std::fs::write(prov.join("wallpapers.json"), serde_json::to_string(&json).unwrap()).unwrap();
        let loose = dir.path().join("fallback.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([1u8, 2, 3, 255])).save(&loose).unwrap();
        let found = find_source_image(dir.path()).expect("fallback scan");
        assert_eq!(found.file_name().unwrap(), "fallback.png");
    }

}
