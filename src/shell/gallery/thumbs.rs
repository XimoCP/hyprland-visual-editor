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

/// Hero upper bound (HF5): aspect-true CONTAIN fit shown on the EXPANDED
/// (current) card so the wallpaper reads undistorted at landscape size.
pub const HERO_MAX_W: u32 = 1600;
pub const HERO_MAX_H: u32 = 900;

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

/// Hero PNG cache path for a source inside `out_dir` (no IO): dedicated
/// `<cache-key>-hero.png` filename under the SAME cache key + mtime scheme
/// and the SAME directory as the portrait thumb (HF5).
pub fn hero_path(source: &Path, out_dir: &Path) -> PathBuf {
    let mtime = source_mtime(source).unwrap_or(SystemTime::UNIX_EPOCH);
    out_dir.join(format!("{}-hero.png", cache_key(source, mtime)))
}

/// Pure CONTAIN fit math (HF5): scale the WHOLE source to fit ENTIRELY
/// within (max_w × max_h) preserving aspect ratio — integer-exact in the
/// same style as `cover_geometry`, NEVER upscaling. Returns `(out_w, out_h)`.
pub fn hero_geometry(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let (w, h) = (src_w as f64, src_h as f64);
    // Contain scale caps at 1.0 — a small source keeps its pixels.
    let scale = 1.0_f64.min(max_w as f64 / w).min(max_h as f64 / h);
    (
        ((w * scale).round() as u32).max(1),
        ((h * scale).round() as u32).max(1),
    )
}

/// Decode ONCE, write BOTH cached PNGs (HF5): the 400×720 cover thumb AND
/// the ≤1600×900 aspect-true hero. Existing cache files short-circuit the
/// decode only when both are present. Returns `(thumb_path, hero_path)`.
pub fn generate_pair(source: &Path, out_dir: &Path) -> Result<(PathBuf, PathBuf), String> {
    let thumb_out = thumb_path(source, out_dir);
    let hero_out = hero_path(source, out_dir);
    if thumb_out.exists() && hero_out.exists() {
        return Ok((thumb_out, hero_out));
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create thumb dir {}: {e}", out_dir.display()))?;
    let img = image::ImageReader::open(source)
        .map_err(|e| format!("cannot open {}: {e}", source.display()))?
        .decode()
        .map_err(|e| format!("cannot decode {}: {e}", source.display()))?;
    let (cx, cy, cw, ch, ow, oh) =
        cover_geometry(img.width(), img.height(), THUMB_MAX_W, THUMB_MAX_H);
    img.crop_imm(cx, cy, cw, ch)
        .resize_exact(ow, oh, image::imageops::FilterType::Triangle)
        .save(&thumb_out)
        .map_err(|e| format!("cannot write {}: {e}", thumb_out.display()))?;
    let (hw, hh) = hero_geometry(img.width(), img.height(), HERO_MAX_W, HERO_MAX_H);
    img.resize_exact(hw, hh, image::imageops::FilterType::Triangle)
        .save(&hero_out)
        .map_err(|e| format!("cannot write {}: {e}", hero_out.display()))?;
    Ok((thumb_out, hero_out))
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

fn wallpaper_from_txt(theme_dir: &Path) -> Option<PathBuf> {
    for rel in ["providers/noctalia-v5/wallpaper.txt", "providers/noctalia/wallpaper.txt"] {
        if let Ok(text) = std::fs::read_to_string(theme_dir.join(rel)) {
            if let Some(line) = text.lines().map(|l| l.trim()).find(|l| !l.is_empty()) { let p = PathBuf::from(line); if p.exists() { return Some(p); } }
        }
    }
    None
}

/// Video extension allowlist (Piano 3): animated wallpaper sources served
/// by mpvpaper.
const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v"];

/// True when `path` carries a video extension (case-insensitive check).
pub fn is_video_source(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Resolve a theme's mpvpaper video assignment (Piano 3): scan
/// `providers/*/mpvpaper-assignments.json` in sorted, deterministic order,
/// take `assignments["*"].local_path` — falling back to the first entry by
/// sorted key — and return it only when non-empty AND the target file
/// exists; otherwise keep scanning. None = no live video assignment.
pub fn video_assignment(theme_dir: &Path) -> Option<PathBuf> {
    let mut providers: Vec<PathBuf> = std::fs::read_dir(theme_dir.join("providers"))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    providers.sort();
    for dir in providers {
        let Ok(text) =
            std::fs::read_to_string(dir.join("mpvpaper-assignments.json"))
        else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let Some(assignments) = v.get("assignments").and_then(|a| a.as_object())
        else { continue };
        // Wildcard "*" wins; remaining entries keep sorted-key order
        // (stable sort preserves the lexical pass above).
        let mut keys: Vec<&String> = assignments.keys().collect();
        keys.sort();
        keys.sort_by_key(|k| k.as_str() != "*");
        for key in keys {
            let Some(local) = assignments
                .get(key.as_str())
                .and_then(|e| e.get("local_path"))
                .and_then(|l| l.as_str())
                .filter(|s| !s.is_empty())
            else { continue };
            let path = PathBuf::from(local);
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

/// Extract a representative frame from a video source into the thumb cache
/// (Piano 3): `<cache-key>-frame.png` under `cache_dir`, keyed by the same
/// SipHash path+mtime scheme as image thumbs. A cached PNG short-circuits
/// ffmpeg entirely. Seeks 1s in to skip black lead-in frames; all ffmpeg
/// output is suppressed. Returns Some only when the output exists AND
/// decodes via the image crate — any failure (ffmpeg missing, non-zero
/// exit, empty output) yields None and malformed video never panics.
pub fn extract_video_frame(video: &Path, cache_dir: &Path) -> Option<PathBuf> {
    let mtime = source_mtime(video).unwrap_or(SystemTime::UNIX_EPOCH);
    let out = cache_dir.join(format!("{}-frame.png", cache_key(video, mtime)));
    if out.exists() {
        return Some(out);
    }
    std::fs::create_dir_all(cache_dir).ok()?;
    let status = std::process::Command::new("ffmpeg")
        .args(["-ss", "1", "-i"])
        .arg(video)
        .args(["-frames:v", "1", "-y"])
        .arg(&out)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    // Trust the artifact only when it decodes as a real image.
    image::ImageReader::open(&out)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    Some(out)
}

/// Find the first usable image file inside a theme directory (task 4.7
/// source discovery). Themes currently persist provider state under their
/// directory; the scan is depth-first over sorted entries so results are
/// deterministic. None = theme has no image source (skeleton stays).
pub fn find_source_image(theme_dir: &Path) -> Option<PathBuf> {
    if let Some(hit) = wallpaper_from_provider_json(theme_dir) {
        return Some(hit);
    }
    if let Some(hit) = wallpaper_from_txt(theme_dir) {
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

/// Generate thumbnails AND heroes OFF-THREAD and marshal each finished pair
/// to the UI thread via `slint::invoke_from_event_loop`, invoking
/// `ready(index, name, thumb_png_path, hero_png_path)` there (task 4.7 /
/// design D8; HF5 adds the aspect-true hero). One thread per job — galleries
/// are small; failures log a warning and never block the UI. Only Send data
/// crosses the boundary (PNG PATHS, not decoded images): `slint::Image`
/// wraps non-Send backend storage, so the UI side loads them from disk.
/// `ready` must be Send+Sync because it travels inside an Arc.
pub fn preheat(
    jobs: Vec<PreheatJob>,
    ready: impl Fn(usize, std::sync::Arc<str>, PathBuf, PathBuf) + Send + Sync + 'static,
) {
    let ready = std::sync::Arc::new(ready);
    for job in jobs {
        let ready = ready.clone();
        std::thread::spawn(move || {
            match generate_pair(&job.source, &cache_dir()) {
                Ok((png, hero)) => {
                    let idx = job.index;
                    let name: std::sync::Arc<str> = std::sync::Arc::from(job.name.as_str());
                    let _ = slint::invoke_from_event_loop(move || ready(idx, name, png, hero));
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

    // ── HF5: hero contain-fit geometry + dedicated cache filename ──────

    #[test]
    fn hero_geometry_landscape_fits_exact() {
        assert_eq!(hero_geometry(1920, 1080, HERO_MAX_W, HERO_MAX_H), (1600, 900));
    }

    #[test]
    fn hero_geometry_portrait_scales_by_height() {
        assert_eq!(hero_geometry(720, 1600, HERO_MAX_W, HERO_MAX_H), (405, 900));
    }

    #[test]
    fn hero_geometry_never_upscales_tiny_sources() {
        assert_eq!(hero_geometry(100, 100, HERO_MAX_W, HERO_MAX_H), (100, 100));
    }

    #[test]
    fn hero_path_uses_dedicated_hero_filename() {
        let dir = Path::new("/cache/thumbs");
        let src = Path::new("/imgs/wall.png");
        // Same mtime resolution as thumb_path (missing file → UNIX_EPOCH).
        let mtime = source_mtime(src).unwrap_or(SystemTime::UNIX_EPOCH);
        let expected = format!("{}-hero.png", cache_key(src, mtime));
        assert_eq!(
            hero_path(src, dir),
            dir.join(expected),
            "hero shares the cache key + mtime scheme under a -hero suffix"
        );
    }

    #[test]
    fn generate_pair_writes_thumb_and_hero_and_hits_cache() {
        let dir = tempfile::tempdir().expect("tmp");
        let src = dir.path().join("src.png");
        image::RgbaImage::from_pixel(40, 30, image::Rgba([255u8, 0, 0, 255]))
            .save(&src)
            .expect("write source png");
        let out_dir = dir.path().join("thumbs");

        let (thumb, hero) = generate_pair(&src, &out_dir).expect("generate pair");
        assert_eq!(thumb, thumb_path(&src, &out_dir), "thumb keeps its path");
        assert_eq!(hero, hero_path(&src, &out_dir), "hero uses its own path");
        assert!(thumb.exists() && hero.exists(), "both PNGs written");
        // 40×30 is smaller than the hero bound in both axes → no upscale.
        let decoded = image::ImageReader::open(&hero).unwrap().decode().unwrap();
        assert_eq!((decoded.width(), decoded.height()), (40, 30));

        let again = generate_pair(&src, &out_dir).expect("regenerate");
        assert_eq!(again, (thumb, hero), "existing cache files short-circuit");
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

        let (first, _) = generate_pair(&src, &out_dir).expect("generate");
        assert!(first.exists(), "png written");
        let decoded = image::ImageReader::open(&first).unwrap().decode().unwrap();
        // 40×30 → aspect crop 16×30; small source must not upscale.
        assert_eq!((decoded.width(), decoded.height()), (16, 30));

        let (second, _) = generate_pair(&src, &out_dir).expect("regenerate");
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

    #[test]
    fn find_source_discovers_wallpaper_txt_and_json_takes_priority() {
        let dir = tempfile::tempdir().expect("tmp");
        let a = dir.path().join("a.png"); image::RgbaImage::from_pixel(10,10,image::Rgba([0u8,0,0,255])).save(&a).unwrap();
        let b = dir.path().join("b.png"); image::RgbaImage::from_pixel(10,10,image::Rgba([1u8,1,1,255])).save(&b).unwrap();
        let tp = dir.path().join("providers/noctalia-v5/wallpaper.txt");
        std::fs::create_dir_all(tp.parent().unwrap()).unwrap();
        std::fs::write(&tp, format!("{}\n", a.display())).unwrap();
        assert_eq!(find_source_image(dir.path()).unwrap(), a);
        let prov = dir.path().join("providers/noctalia");
        std::fs::create_dir_all(&prov).unwrap();
        let j = serde_json::json!({"wallpapers":{"DP-3":{"dark":b.to_string_lossy()}}});
        std::fs::write(prov.join("wallpapers.json"), serde_json::to_string(&j).unwrap()).unwrap();
        assert_eq!(find_source_image(dir.path()).unwrap(), b, "JSON priority over txt");
    }

    // ── Piano 3 RED: mpvpaper video-assignment discovery contracts ──

    /// Fixture helper: write mpvpaper-assignments.json into `provider_dir`.
    fn write_assignments(provider_dir: &Path, json: serde_json::Value) {
        std::fs::create_dir_all(provider_dir).expect("mkdir providers");
        std::fs::write(
            provider_dir.join("mpvpaper-assignments.json"),
            serde_json::to_string(&json).unwrap(),
        )
        .expect("write assignments");
    }

    #[test]
    fn video_assignment_hits_wildcard_local_path() {
        let dir = tempfile::tempdir().expect("tmp");
        let video = dir.path().join("neon.mp4");
        std::fs::write(&video, b"fake-video").expect("video fixture");
        write_assignments(
            &dir.path().join("providers/noctalia-v5"),
            serde_json::json!({
                "version": 1,
                "assignments": { "*": {
                    "filename": "neon.mp4",
                    "local_path": video.to_string_lossy()
                }}
            }),
        );
        assert_eq!(video_assignment(dir.path()), Some(video));
    }

    #[test]
    fn video_assignment_none_without_assignments_file() {
        let dir = tempfile::tempdir().expect("tmp");
        assert_eq!(video_assignment(dir.path()), None, "no providers dir at all");
        std::fs::create_dir_all(dir.path().join("providers/noctalia-v5")).expect("mkdir");
        assert_eq!(video_assignment(dir.path()), None, "provider without the file");
        assert_eq!(video_assignment(&dir.path().join("missing")), None, "missing theme dir safe");
    }

    #[test]
    fn video_assignment_rejects_empty_local_path() {
        let dir = tempfile::tempdir().expect("tmp");
        write_assignments(
            &dir.path().join("providers/noctalia-v5"),
            serde_json::json!({ "assignments": { "*": { "filename": "x.mp4", "local_path": "" } } }),
        );
        assert_eq!(video_assignment(dir.path()), None);
    }

    #[test]
    fn video_assignment_rejects_missing_target_file() {
        let dir = tempfile::tempdir().expect("tmp");
        write_assignments(
            &dir.path().join("providers/wallpaper"),
            serde_json::json!({ "assignments": { "*": { "local_path": "/nope/gone.mp4" } } }),
        );
        assert_eq!(video_assignment(dir.path()), None);
    }

    #[test]
    fn video_assignment_falls_back_to_sorted_first_key_without_star() {
        let dir = tempfile::tempdir().expect("tmp");
        let chosen = dir.path().join("chosen.mkv");
        std::fs::write(&chosen, b"v").expect("fixture");
        write_assignments(
            &dir.path().join("providers/mpvpaper"),
            serde_json::json!({ "assignments": {
                "HDMI-A-1": { "local_path": "/nope/z.mp4" },
                "DP-1": { "local_path": chosen.to_string_lossy() }
            }}),
        );
        assert_eq!(video_assignment(dir.path()), Some(chosen), "DP-1 sorts before HDMI-A-1");
    }

    #[test]
    fn video_assignment_scans_provider_dirs_sorted_deterministically() {
        let dir = tempfile::tempdir().expect("tmp");
        let early = dir.path().join("a.mp4");
        std::fs::write(&early, b"a").expect("fixture");
        let late = dir.path().join("z.mp4");
        std::fs::write(&late, b"z").expect("fixture");
        write_assignments(
            &dir.path().join("providers/zzz-late"),
            serde_json::json!({ "assignments": { "*": { "local_path": late.to_string_lossy() } } }),
        );
        write_assignments(
            &dir.path().join("providers/aaa-early"),
            serde_json::json!({ "assignments": { "*": { "local_path": early.to_string_lossy() } } }),
        );
        assert_eq!(video_assignment(dir.path()), Some(early), "aaa-early wins by provider sort");
    }

    #[test]
    fn is_video_source_extension_table() {
        let cases = [
            ("clip.mp4", true),
            ("clip.mkv", true),
            ("clip.webm", true),
            ("clip.mov", true),
            ("clip.avi", true),
            ("clip.m4v", true),
            ("CLIP.MP4", true),
            ("Upper.Mkv", true),
            ("clip.png", false),
            ("clip.jpg", false),
            ("clip", false),
            ("clip.tar.gz", false),
        ];
        for (name, want) in cases {
            assert_eq!(is_video_source(Path::new(name)), want, "extension case: {name}");
        }
    }

    // ── Piano 3: ffmpeg frame extraction + cache ─────────────────────

    fn ffmpeg_available() -> bool {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn extract_video_frame_writes_decodable_cached_png() {
        if !ffmpeg_available() {
            println!("ffmpeg not available — skipping video frame extraction test");
            return;
        }
        let dir = tempfile::tempdir().expect("tmp");
        let video = dir.path().join("src.mp4");
        // Fixture duration > 1s: the extractor seeks to t=1s and a shorter
        // clip yields no frame at that seek point (verified with ffmpeg 9).
        let ok = std::process::Command::new("ffmpeg")
            .args(["-f", "lavfi", "-i", "testsrc=duration=5:size=320x240:rate=10", "-y"])
            .arg(&video)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "failed to synthesize testsrc mp4 fixture");

        let out_dir = dir.path().join("thumbs");
        let frame = extract_video_frame(&video, &out_dir).expect("frame extracted");
        assert!(frame.exists(), "cached frame png written");
        let decoded = image::ImageReader::open(&frame).unwrap().decode().unwrap();
        assert_eq!((decoded.width(), decoded.height()), (320, 240), "full source frame");

        // Second call short-circuits to the SAME cached artifact.
        assert_eq!(extract_video_frame(&video, &out_dir), Some(frame));
    }

}
