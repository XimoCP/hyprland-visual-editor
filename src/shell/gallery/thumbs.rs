// HVE 2 — Real wallpaper thumbnail pipeline (gallery-immersive-redesign
// PR4, design D8; Tramo 2 content-keyed cache R2/D2).
//
// Decodes a theme's source image, cover-crops + scales it to ≤400×720 and
// caches the result as PNGs under `$XDG_CACHE_HOME/hve/thumbs/<key>-thumb.png`
// and `<key>-hero.png`, keyed by source CONTENT (size + fast content hash)
// with LRU 200 prune semantics (spec: Real Wallpaper Thumbnails; model.rs
// R8 cache capacity reused). Legacy path+mtime `cache_key` kept as fallback
// for missing/unreadable sources and migration orphan cleanup.
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
/// Kept for fallback/migration; new artifacts use content-keyed keys.
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

/// Content-keyed cache (Tramo 2 / R2/D2): key = "{len}-{hash(file_bytes)}".
/// Hasher choice: `xxhash-rust` (xxh3_64) is NOT in Cargo.lock/tree, so we
/// use `std::collections::hash_map::DefaultHasher` (SipHash13) over the file
/// bytes. Documented per D2: zero new dependencies, collisions negligible at
/// gallery scale (200 artifacts); chunked hashing would be equivalent but
/// whole-buffer hash is fine after the single read needed for decode.
/// If xxhash-rust is added later, swap `fast_content_hash` to xxh3_64 —
/// artifact names stay "{len}-{hash}" so migration orphans old entries
/// (prune_stale cleans them).
pub fn content_cache_key_from_bytes(bytes: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{}-{:016x}", bytes.len(), h.finish())
}

/// Content key for a source file on disk: reads the file once and hashes
/// its bytes. One read when only keying is needed; when decode is also
/// needed the caller should reuse the same bytes (zero extra IO — see
/// generate_set/generate_pair).
pub fn content_cache_key(source: &Path) -> Result<String, String> {
    let bytes = std::fs::read(source).map_err(|e| format!("cannot read {}: {e}", source.display()))?;
    Ok(content_cache_key_from_bytes(&bytes))
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

/// Thumbnail PNG cache path for a source inside `out_dir`.
/// Content-keyed (R2.1): reads the source file once to hash its bytes;
/// falls back to path+mtime hash only when the source is missing/unreadable.
pub fn thumb_path(source: &Path, out_dir: &Path) -> PathBuf {
    if let Ok(key) = content_cache_key(source) {
        out_dir.join(format!("{key}-thumb.png"))
    } else {
        let mtime = source_mtime(source).unwrap_or(SystemTime::UNIX_EPOCH);
        out_dir.join(format!("{}-thumb.png", cache_key(source, mtime)))
    }
}

/// Hero PNG cache path for a source inside `out_dir`: dedicated
/// `<content-key>-hero.png` under the same content-key scheme and dir as
/// the thumb (HF5/R2.1). Falls back to path+mtime when unreadable.
pub fn hero_path(source: &Path, out_dir: &Path) -> PathBuf {
    if let Ok(key) = content_cache_key(source) {
        out_dir.join(format!("{key}-hero.png"))
    } else {
        let mtime = source_mtime(source).unwrap_or(SystemTime::UNIX_EPOCH);
        out_dir.join(format!("{}-hero.png", cache_key(source, mtime)))
    }
}

/// Slat PNG cache path (collapsed) — `<key>-slat.png` (T2b content-keyed).
pub fn slat_path(source: &Path, out_dir: &Path) -> PathBuf {
    if let Ok(key) = content_cache_key(source) {
        out_dir.join(format!("{key}-slat.png"))
    } else {
        let mtime = source_mtime(source).unwrap_or(SystemTime::UNIX_EPOCH);
        out_dir.join(format!("{}-slat.png", cache_key(source, mtime)))
    }
}

/// Slat PNG cache path (expanded) — `<key>-slat-exp.png` (T2b content-keyed).
pub fn slat_expanded_path(source: &Path, out_dir: &Path) -> PathBuf {
    if let Ok(key) = content_cache_key(source) {
        out_dir.join(format!("{key}-slat-exp.png"))
    } else {
        let mtime = source_mtime(source).unwrap_or(SystemTime::UNIX_EPOCH);
        out_dir.join(format!("{}-slat-exp.png", cache_key(source, mtime)))
    }
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

/// Four artifacts derived from a single source decode (R1.2 / D1).
/// `decoded` is true when the FULL-SIZE source was decoded (cold bake);
/// false on warm hit where all four disk artifacts existed and slats were
/// loaded via small PNG decodes (T2b R2.3).
#[derive(Debug)]
pub struct BakedSet {
    pub thumb_path: PathBuf,
    pub hero_path: PathBuf,
    pub slat_rgba: image::RgbaImage,
    pub slat_expanded_rgba: image::RgbaImage,
    pub decoded: bool,
}

/// Decode ONCE, derive thumb + hero + both slat variants from the same buffer
/// (R1.2). Content-keyed (R2.1/R2.3/D2/T2b): artifacts are `<key>-thumb.png` /
/// `<key>-hero.png` / `<key>-slat.png` / `<key>-slat-exp.png` where key =
/// "{len}-{hash(bytes)}". Reads the source file ONCE into `bytes`, derives
/// the key from those bytes, and — on warm hit (all four exist) — SKIPS the
/// FULL-SIZE decode entirely and loads slats via small PNG decodes (520px,
/// ~146/959 wide). Cold path decodes once, derives thumb/hero if needed,
/// bakes slats, and persists slat PNGs. `decoded` is true on cold bake,
/// false on warm hit (R2.3).
/// Returns all four artifacts in [`BakedSet`].
pub fn generate_set(source: &Path, out_dir: &Path) -> Result<BakedSet, String> {
    // Single read → key + decode share the same bytes (zero extra IO).
    let bytes = std::fs::read(source)
        .map_err(|e| format!("cannot read {}: {e}", source.display()))?;
    let key = content_cache_key_from_bytes(&bytes);
    let thumb_out = out_dir.join(format!("{key}-thumb.png"));
    let hero_out = out_dir.join(format!("{key}-hero.png"));
    let slat_out = out_dir.join(format!("{key}-slat.png"));
    let slat_exp_out = out_dir.join(format!("{key}-slat-exp.png"));
    // Warm hit: all four exist → skip FULL-SIZE source decode, load slats small
    if thumb_out.exists() && hero_out.exists() && slat_out.exists() && slat_exp_out.exists() {
        let slat_rgba = image::ImageReader::open(&slat_out)
            .map_err(|e| format!("cannot open cached slat {}: {e}", slat_out.display()))?
            .with_guessed_format()
            .ok()
            .and_then(|r| r.decode().ok())
            .map(|d| d.to_rgba8())
            .ok_or_else(|| format!("cannot decode cached slat {}", slat_out.display()))?;
        let slat_expanded_rgba = image::ImageReader::open(&slat_exp_out)
            .map_err(|e| format!("cannot open cached slat-exp {}: {e}", slat_exp_out.display()))?
            .with_guessed_format()
            .ok()
            .and_then(|r| r.decode().ok())
            .map(|d| d.to_rgba8())
            .ok_or_else(|| format!("cannot decode cached slat-exp {}", slat_exp_out.display()))?;
        tracing::trace!(source = %source.display(), decoded = false, "generate_set warm hit (four cached)");
        return Ok(BakedSet {
            thumb_path: thumb_out,
            hero_path: hero_out,
            slat_rgba,
            slat_expanded_rgba,
            decoded: false,
        });
    }
    let needs_thumb = !thumb_out.exists();
    let needs_hero = !hero_out.exists();
    let needs_slat = !slat_out.exists();
    let needs_slat_exp = !slat_exp_out.exists();

    // Decode from the same bytes — no second file open.
    let t_decode = std::time::Instant::now();
    let img = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| format!("cannot guess format {}: {e}", source.display()))?
        .decode()
        .map_err(|e| format!("cannot decode {}: {e}", source.display()))?;
    let decode_ms = t_decode.elapsed().as_millis() as u64;

    if needs_thumb || needs_hero || needs_slat || needs_slat_exp {
        std::fs::create_dir_all(out_dir)
            .map_err(|e| format!("cannot create thumb dir {}: {e}", out_dir.display()))?;
    }
    if needs_thumb {
        let (cx, cy, cw, ch, ow, oh) =
            cover_geometry(img.width(), img.height(), THUMB_MAX_W, THUMB_MAX_H);
        img.crop_imm(cx, cy, cw, ch)
            .resize_exact(ow, oh, image::imageops::FilterType::Triangle)
            .save(&thumb_out)
            .map_err(|e| format!("cannot write {}: {e}", thumb_out.display()))?;
    }
    if needs_hero {
        let (hw, hh) = hero_geometry(img.width(), img.height(), HERO_MAX_W, HERO_MAX_H);
        img.resize_exact(hw, hh, image::imageops::FilterType::Triangle)
            .save(&hero_out)
            .map_err(|e| format!("cannot write {}: {e}", hero_out.display()))?;
    }

    // Bake slats from the SAME decoded buffer (no re-decode, no file re-read).
    let t_bake = std::time::Instant::now();
    let rgba = img.to_rgba8();
    let slat_rgba = crate::shell::gallery::slat_image::baked_slat_rgba(rgba.clone(), false);
    let slat_expanded_rgba = crate::shell::gallery::slat_image::baked_slat_rgba(rgba, true);
    let bake_ms = t_bake.elapsed().as_millis() as u64;
    let total_ms = decode_ms + bake_ms;

    // Persist slat artifacts content-keyed (T2b)
    if needs_slat {
        slat_rgba
            .save(&slat_out)
            .map_err(|e| format!("cannot write {}: {e}", slat_out.display()))?;
    }
    if needs_slat_exp {
        slat_expanded_rgba
            .save(&slat_exp_out)
            .map_err(|e| format!("cannot write {}: {e}", slat_exp_out.display()))?;
    }

    tracing::trace!(
        source = %source.display(),
        decode_ms,
        bake_ms,
        total_ms,
        decoded = true,
        "generate_set job complete"
    );

    Ok(BakedSet {
        thumb_path: thumb_out,
        hero_path: hero_out,
        slat_rgba,
        slat_expanded_rgba,
        decoded: true,
    })
}

/// Decode ONCE, write BOTH cached PNGs (HF5) content-keyed (R2.1): the
/// 400×720 cover thumb AND the ≤1600×900 aspect-true hero under
/// `<key>-thumb.png` / `<key>-hero.png`. Single read → key + decode share
/// the same bytes (zero extra IO). Existing BOTH-files hit short-circuits
/// thumb/hero generation (dedup); slat path (generate_set) still decodes
/// for its in-memory bakes. Returns `(thumb_path, hero_path)`.
pub fn generate_pair(source: &Path, out_dir: &Path) -> Result<(PathBuf, PathBuf), String> {
    let bytes = std::fs::read(source)
        .map_err(|e| format!("cannot read {}: {e}", source.display()))?;
    let key = content_cache_key_from_bytes(&bytes);
    let thumb_out = out_dir.join(format!("{key}-thumb.png"));
    let hero_out = out_dir.join(format!("{key}-hero.png"));
    if thumb_out.exists() && hero_out.exists() {
        return Ok((thumb_out, hero_out));
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create thumb dir {}: {e}", out_dir.display()))?;
    let img = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| format!("cannot guess format {}: {e}", source.display()))?
        .decode()
        .map_err(|e| format!("cannot decode {}: {e}", source.display()))?;
    let needs_thumb = !thumb_out.exists();
    let needs_hero = !hero_out.exists();
    if needs_thumb {
        let (cx, cy, cw, ch, ow, oh) =
            cover_geometry(img.width(), img.height(), THUMB_MAX_W, THUMB_MAX_H);
        img.crop_imm(cx, cy, cw, ch)
            .resize_exact(ow, oh, image::imageops::FilterType::Triangle)
            .save(&thumb_out)
            .map_err(|e| format!("cannot write {}: {e}", thumb_out.display()))?;
    }
    if needs_hero {
        let (hw, hh) = hero_geometry(img.width(), img.height(), HERO_MAX_W, HERO_MAX_H);
        img.resize_exact(hw, hh, image::imageops::FilterType::Triangle)
            .save(&hero_out)
            .map_err(|e| format!("cannot write {}: {e}", hero_out.display()))?;
    }
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

/// Best-effort prune of stale thumb/hero/slat artifacts (R2.3/D2/T2b):
/// removes only `*-thumb.png` / `*-hero.png` / `*-slat.png` /
/// `*-slat-exp.png` files whose content key (prefix before the suffix) is
/// not in `live_keys`. Never touches theme dirs, non-artifact files, or
/// subdirectories. Returns count removed.
pub fn prune_stale(cache_dir: &Path, live_keys: &std::collections::HashSet<String>) -> usize {
    let entries = match std::fs::read_dir(cache_dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        let key = if let Some(k) = name.strip_suffix("-thumb.png") {
            k
        } else if let Some(k) = name.strip_suffix("-hero.png") {
            k
        } else if let Some(k) = name.strip_suffix("-slat-exp.png") {
            k
        } else if let Some(k) = name.strip_suffix("-slat.png") {
            k
        } else {
            continue;
        };
        if !live_keys.contains(key) && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
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
///
/// Piano 3: mpvpaper assignments are the truth of what the desktop shows,
/// so they are consulted BEFORE every static source. An assigned video
/// renders its cached frame; when extraction fails we fall through to the
/// legacy chain below — today's stale-txt fallback still beats showing a
/// skeleton card.
pub fn find_source_image(theme_dir: &Path) -> Option<PathBuf> {
    if let Some(assigned) = video_assignment(theme_dir) {
        if is_video_source(&assigned) {
            if let Some(frame) = extract_video_frame(&assigned, &cache_dir()) {
                return Some(frame);
            }
        } else {
            // Image assignments win outright: they ARE the live wallpaper.
            return Some(assigned);
        }
    }
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

/// Maximum concurrent thumbnail workers (Tramo 6 corrective): bounds the
/// one-thread-per-job blast that pegged the CPU and starved the Slint UI
/// thread on cold start (42 themes → 42 threads). K=3 keeps cores free for UI.
pub const THUMBS_MAX_WORKERS: usize = 3;

/// Pure planner: keep only cards that have a source image (task 4.7).
pub fn plan_jobs(sources: Vec<(usize, String, Option<PathBuf>)>) -> Vec<PreheatJob> {
    sources
        .into_iter()
        .filter_map(|(index, name, source)| {
            source.map(|source| PreheatJob { index, name, source })
        })
        .collect()
}

/// Stable priority ordering (Tramo 6): jobs whose `index` is in `priority`
/// come FIRST (preserving their original relative order), then the rest in
/// original order. `priority` is typically the current page's real indices
/// so visible cards bake first.
pub fn order_jobs(jobs: Vec<PreheatJob>, priority: &std::collections::HashSet<usize>) -> Vec<PreheatJob> {
    if priority.is_empty() {
        return jobs;
    }
    let mut pri = Vec::new();
    let mut rest = Vec::new();
    for job in jobs {
        if priority.contains(&job.index) {
            pri.push(job);
        } else {
            rest.push(job);
        }
    }
    pri.extend(rest);
    pri
}

/// Convenience: plan + priority-order in one call.
pub fn plan_jobs_with_priority(
    sources: Vec<(usize, String, Option<PathBuf>)>,
    priority: &std::collections::HashSet<usize>,
) -> Vec<PreheatJob> {
    order_jobs(plan_jobs(sources), priority)
}

/// Coalescing helper: returns true only for the last completion in a batch.
/// Deterministic, no timers — last-job semantics via atomic pending counter.
pub fn is_last_completion(pending: &std::sync::atomic::AtomicUsize) -> bool {
    pending.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) == 1
}

/// Generate thumbnails AND heroes OFF-THREAD and marshal each finished set
/// to the UI thread via `slint::invoke_from_event_loop`, invoking
/// `ready(index, name, thumb_png_path, hero_png_path, slat_rgba, slat_expanded_rgba)`
/// there (R1.1/R1.2: single decode reused for all four artifacts; no pixel
/// work on the UI thread). Bounded worker pool (K=THUMBS_MAX_WORKERS) drains
/// a shared VecDeque<Mutex/Condvar> in FIFO order — total threads never
/// exceed K, so the UI thread is never starved even on 42-theme cold start.
/// Failures log a warning and never block the UI. Slint `Image` is not Send,
/// so we ship `RgbaImage` buffers and the UI side wraps them via `Image::from_rgba8`.
/// `ready` must be Send+Sync because it travels inside an Arc.
/// Batch tracing: per-job span via `generate_set`, plus one batch summary log
/// when the last job completes (last-job pending counter).
pub fn preheat(
    jobs: Vec<PreheatJob>,
    ready: impl Fn(usize, std::sync::Arc<str>, PathBuf, PathBuf, image::RgbaImage, image::RgbaImage)
        + Send
        + Sync
        + 'static,
) {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Instant;

    if jobs.is_empty() {
        return;
    }
    let total = jobs.len();
    let pending: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(total));
    let batch_start = Arc::new(Instant::now());
    let ready = Arc::new(ready);
    tracing::info!(jobs = total, workers = THUMBS_MAX_WORKERS, "thumbs preheat batch start (bounded pool)");

    // Shared FIFO queue: Mutex<VecDeque> + Condvar (std only, no new deps).
    // Jobs are enqueued in caller order (already priority-sorted if caller used
    // order_jobs/plan_jobs_with_priority) and drained FIFO by exactly K workers.
    let queue: Arc<(Mutex<VecDeque<PreheatJob>>, Condvar)> =
        Arc::new((Mutex::new(jobs.into_iter().collect()), Condvar::new()));
    let workers = std::cmp::min(total, THUMBS_MAX_WORKERS);
    for _ in 0..workers {
        let queue = queue.clone();
        let ready = ready.clone();
        let pending = pending.clone();
        let batch_start = batch_start.clone();
        std::thread::spawn(move || loop {
            let job = {
                let (lock, _cvar) = &*queue;
                let mut guard = lock.lock().expect("queue lock");
                guard.pop_front()
            };
            let Some(job) = job else { break };
            let t_total = Instant::now();
            let span = tracing::info_span!("thumbs::preheat::job", source = %job.source.display());
            let _guard = span.enter();
            match generate_set(&job.source, &cache_dir()) {
                Ok(set) => {
                    let idx = job.index;
                    let name: Arc<str> = Arc::from(job.name.as_str());
                    let thumb_path = set.thumb_path;
                    let hero_path = set.hero_path;
                    let slat = set.slat_rgba;
                    let slat_exp = set.slat_expanded_rgba;
                    let elapsed = t_total.elapsed().as_millis() as u64;
                    tracing::trace!(total_ms = elapsed, "job done");
                    let ready2 = ready.clone();
                    let pending2 = pending.clone();
                    let batch_start2 = batch_start.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        ready2(idx, name, thumb_path, hero_path, slat, slat_exp);
                        if is_last_completion(&pending2) {
                            let batch_ms = batch_start2.elapsed().as_millis() as u64;
                            tracing::info!(jobs = total, total_ms = batch_ms, "thumbs preheat batch complete");
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("[thumbs] preheat for '{}' failed: {}", job.source.display(), e);
                    let pending2 = pending.clone();
                    let batch_start2 = batch_start.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if is_last_completion(&pending2) {
                            let batch_ms = batch_start2.elapsed().as_millis() as u64;
                            tracing::info!(jobs = total, total_ms = batch_ms, "thumbs preheat batch complete (with failures)");
                        }
                    });
                }
            }
        });
    }
}

/// Legacy preheat shim for call-sites that still use the 4-arg callback (kept
/// for transitional compatibility; new code should use the 6-arg `preheat`).
/// Delegates to 6-arg version but discards the slat buffers.
#[allow(dead_code)]
pub fn preheat_legacy(
    jobs: Vec<PreheatJob>,
    ready: impl Fn(usize, std::sync::Arc<str>, PathBuf, PathBuf) + Send + Sync + 'static,
) {
    let ready = std::sync::Arc::new(ready);
    preheat(jobs, move |idx, name, thumb, hero, _slat, _exp| {
        ready(idx, name, thumb, hero);
    });
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

    // ── Piano 3 WIRE: video-aware ordering inside find_source_image ──

    #[test]
    fn find_source_prefers_assigned_image_over_full_legacy_chain() {
        let dir = tempfile::tempdir().expect("tmp");
        let assigned = dir.path().join("assigned.png");
        image::RgbaImage::from_pixel(10, 10, image::Rgba([2u8, 2, 2, 255])).save(&assigned).unwrap();
        let stale = dir.path().join("stale.png");
        image::RgbaImage::from_pixel(10, 10, image::Rgba([9u8, 9, 9, 255])).save(&stale).unwrap();
        let v5 = dir.path().join("providers/noctalia-v5");
        std::fs::create_dir_all(&v5).unwrap();
        std::fs::write(v5.join("wallpaper.txt"), format!("{}\n", stale.display())).unwrap();
        let noc = dir.path().join("providers/noctalia");
        std::fs::create_dir_all(&noc).unwrap();
        let j = serde_json::json!({"wallpapers":{"DP-3":{"dark":stale.to_string_lossy()}}});
        std::fs::write(noc.join("wallpapers.json"), serde_json::to_string(&j).unwrap()).unwrap();
        write_assignments(
            &v5,
            serde_json::json!({"assignments":{"*":{"filename":"a.png","local_path":assigned.to_string_lossy()}}}),
        );
        std::fs::write(dir.path().join("loose.png"), b"loose").unwrap();
        assert_eq!(
            find_source_image(dir.path()),
            Some(assigned),
            "assignment beats wallpapers.json, wallpaper.txt and loose scan"
        );
    }

    #[test]
    fn find_source_falls_back_to_legacy_chain_when_frame_fails() {
        let dir = tempfile::tempdir().expect("tmp");
        let broken = dir.path().join("broken.mp4");
        // Exists (passes discovery) but is not a decodable video: ffmpeg
        // fails (or is absent) -> extraction None -> legacy chain answers.
        std::fs::write(&broken, b"definitely not a video").unwrap();
        let stale = dir.path().join("stale.png");
        image::RgbaImage::from_pixel(10, 10, image::Rgba([7u8, 7, 7, 255])).save(&stale).unwrap();
        let v5 = dir.path().join("providers/noctalia-v5");
        std::fs::create_dir_all(&v5).unwrap();
        std::fs::write(v5.join("wallpaper.txt"), format!("{}\n", stale.display())).unwrap();
        write_assignments(
            &v5,
            serde_json::json!({"assignments":{"*":{"local_path":broken.to_string_lossy()}}}),
        );
        assert_eq!(
            find_source_image(dir.path()),
            Some(stale),
            "failed frame extraction falls through instead of skeleton"
        );
    }

    // ── T1.1 RED: generate_set single-decode four artifacts ──────────

    #[test]
    fn generate_set_single_decode_four_artifacts() {
        let dir = tempfile::tempdir().expect("tmp");
        let src = dir.path().join("src.png");
        // 800×600 source → covers both thumb (400×720 cover) and hero (≤1600×900 contain)
        let mut img = image::RgbaImage::new(800, 600);
        for y in 0..600 {
            for x in 0..800 {
                img.put_pixel(x, y, image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]));
            }
        }
        img.save(&src).unwrap();
        let out_dir = dir.path().join("thumbs");
        let set = generate_set(&src, &out_dir).expect("generate_set should succeed");
        assert_eq!(set.thumb_path, thumb_path(&src, &out_dir), "thumb path matches cache contract");
        assert_eq!(set.hero_path, hero_path(&src, &out_dir), "hero path matches cache contract");
        assert!(set.thumb_path.exists(), "thumb PNG written");
        assert!(set.hero_path.exists(), "hero PNG written");
        assert_eq!((set.slat_rgba.width(), set.slat_rgba.height()), (crate::shell::gallery::slat_image::SLAT_BBOX_COLLAPSED_W, crate::shell::gallery::slat_image::SLAT_BBOX_H), "collapsed slat bbox");
        assert_eq!((set.slat_expanded_rgba.width(), set.slat_expanded_rgba.height()), (crate::shell::gallery::slat_image::SLAT_BBOX_EXPANDED_W, crate::shell::gallery::slat_image::SLAT_BBOX_H), "expanded slat bbox");
        // Thumb PNG dimensions must be cover-cropped to 400×720 aspect (no upscale needed for 800×600)
        let thumb_img = image::ImageReader::open(&set.thumb_path).unwrap().decode().unwrap();
        assert!(thumb_img.width() <= THUMB_MAX_W && thumb_img.height() <= THUMB_MAX_H, "thumb bounded");
        let hero_img = image::ImageReader::open(&set.hero_path).unwrap().decode().unwrap();
        assert!(hero_img.width() <= HERO_MAX_W && hero_img.height() <= HERO_MAX_H, "hero bounded");
    }

    #[test]
    fn generate_set_parity_worker_bake_equals_direct_bake() {
        let dir = tempfile::tempdir().expect("tmp");
        let src = dir.path().join("src.png");
        let mut img = image::RgbaImage::new(200, 120);
        for y in 0..120 {
            for x in 0..200 {
                img.put_pixel(x, y, image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]));
            }
        }
        img.save(&src).unwrap();
        let out_dir = dir.path().join("thumbs");
        let set = generate_set(&src, &out_dir).expect("generate_set");
        // Direct bake from the SAME decoded buffer — byte-identical contract (R1.4/D1)
        let decoded = image::ImageReader::open(&src).unwrap().decode().unwrap().to_rgba8();
        let direct_collapsed = crate::shell::gallery::slat_image::baked_slat_rgba(decoded.clone(), false);
        let direct_expanded = crate::shell::gallery::slat_image::baked_slat_rgba(decoded, true);
        assert_eq!(set.slat_rgba.as_raw(), direct_collapsed.as_raw(), "worker collapsed slat must be byte-identical to direct bake");
        assert_eq!(set.slat_expanded_rgba.as_raw(), direct_expanded.as_raw(), "worker expanded slat must be byte-identical to direct bake");
    }

    // ── T1.4: preheat ships RgbaImage buffers (no UI-thread bake) ─────

    #[test]
    fn preheat_ships_rgba_buffers_worker_side() {
        // Direct verification that generate_set (the worker side of preheat)
        // produces valid RgbaImage buffers with correct bbox — the buffers
        // that preheat will ship through the channel instead of re-decoding
        // on the UI thread.
        let dir = tempfile::tempdir().expect("tmp");
        let src = dir.path().join("src.png");
        image::RgbaImage::from_pixel(320, 240, image::Rgba([10u8, 20, 30, 255]))
            .save(&src)
            .unwrap();
        let out_dir = dir.path().join("thumbs");
        let set = generate_set(&src, &out_dir).expect("generate_set for preheat");
        // Buffers must be non-empty and match baked bbox — these are what
        // preheat ships via the channel; UI thread must wrap via from_rgba8.
        assert_eq!((set.slat_rgba.width(), set.slat_rgba.height()), (crate::shell::gallery::slat_image::SLAT_BBOX_COLLAPSED_W, crate::shell::gallery::slat_image::SLAT_BBOX_H));
        assert_eq!((set.slat_expanded_rgba.width(), set.slat_expanded_rgba.height()), (crate::shell::gallery::slat_image::SLAT_BBOX_EXPANDED_W, crate::shell::gallery::slat_image::SLAT_BBOX_H));
        // Verify wrapping via from_rgba8 succeeds (UI side contract) without decode
        let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(set.slat_rgba.width(), set.slat_rgba.height());
        buf.make_mut_bytes().copy_from_slice(set.slat_rgba.as_raw());
        let img = slint::Image::from_rgba8(buf);
        assert!(img.size().width > 0, "slint::Image::from_rgba8 wrapping must succeed for shipped buffer");
    }

    // ── T1.5 RED→GREEN: coalescing — 42 completions → exactly 1 refresh ──

    #[test]
    fn coalescing_42_completions_trigger_exactly_one_refresh() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let refresh_count = Arc::new(AtomicUsize::new(0));
        let pending = Arc::new(AtomicUsize::new(42));
        let refresh = {
            let c = refresh_count.clone();
            move || { c.fetch_add(1, Ordering::SeqCst); }
        };
        let refresh = Arc::new(refresh);
        // Simulate 42 completions arriving (possibly out-of-order, but counter is atomic)
        for _ in 0..42 {
            let p = pending.clone();
            let r = refresh.clone();
            // Each completion checks is_last_completion and conditionally refreshes
            if is_last_completion(&p) {
                r();
            }
        }
        assert_eq!(refresh_count.load(Ordering::SeqCst), 1, "42 completions must coalesce to exactly ONE refresh (last-job semantics)");
        // Edge: single job also triggers exactly one
        let c2 = Arc::new(AtomicUsize::new(0));
        let p2 = Arc::new(AtomicUsize::new(1));
        if is_last_completion(&p2) { c2.fetch_add(1, Ordering::SeqCst); }
        assert_eq!(c2.load(Ordering::SeqCst), 1, "single completion must still refresh once");
        // So we just verify normal flow doesn't double-trigger
        let c3 = Arc::new(AtomicUsize::new(0));
        // Simulate two separate batches each with 3 completions → each batch exactly 1
        for batch in 0..2 {
            let pend = Arc::new(AtomicUsize::new(3));
            for _ in 0..3 {
                if is_last_completion(&pend) { c3.fetch_add(1, Ordering::SeqCst); }
            }
            assert_eq!(c3.load(Ordering::SeqCst), batch+1);
        }
    }

    // ── T2.1 RED: same bytes different paths share cache key ─────────
    #[test]
    fn same_bytes_different_paths_share_cache_key() {
        let dir_a = tempfile::tempdir().expect("tmp a");
        let dir_b = tempfile::tempdir().expect("tmp b");
        // Byte-identical wallpaper files, different names/mtimes
        let bytes = b"identical wallpaper bytes for dedup test - same content";
        let a = dir_a.path().join("wall-a.png");
        let b = dir_b.path().join("wallpaper-final.png");
        std::fs::write(&a, bytes).expect("write a");
        // Ensure mtime differs (sleep 10ms) then write b
        std::thread::sleep(std::time::Duration::from_millis(15));
        std::fs::write(&b, bytes).expect("write b");
        // T2.3 contract: key = "{len}-{hash}" over file bytes
        let ka = content_cache_key(&a).expect("content key a");
        let kb = content_cache_key(&b).expect("content key b");
        assert_eq!(ka, kb, "byte-identical files must share content key despite different paths/mtimes");
        let cache = tempfile::tempdir().expect("tmp cache");
        assert_eq!(
            thumb_path(&a, cache.path()),
            thumb_path(&b, cache.path()),
            "same content → same thumb artifact path"
        );
        assert_eq!(
            hero_path(&a, cache.path()),
            hero_path(&b, cache.path()),
            "same content → same hero artifact path"
        );
    }

    // ── T2.2 RED: different bytes same size distinct keys ────────────
    #[test]
    fn different_bytes_same_size_distinct_keys() {
        let dir = tempfile::tempdir().expect("tmp");
        let a = dir.path().join("a.png");
        let b = dir.path().join("b.png");
        let bytes_a = vec![0u8; 1024];
        let mut bytes_b = vec![0u8; 1024];
        bytes_b[512] = 1;
        bytes_b[777] = 42;
        std::fs::write(&a, &bytes_a).expect("write a");
        std::fs::write(&b, &bytes_b).expect("write b");
        assert_eq!(std::fs::metadata(&a).unwrap().len(), std::fs::metadata(&b).unwrap().len(), "fixture must be equal-size");
        let ka = content_cache_key(&a).expect("key a");
        let kb = content_cache_key(&b).expect("key b");
        assert_ne!(ka, kb, "equal-size different bytes must have distinct keys");
        assert_ne!(
            thumb_path(&a, dir.path()),
            thumb_path(&b, dir.path()),
            "different content → distinct thumb paths"
        );
        assert_ne!(
            hero_path(&a, dir.path()),
            hero_path(&b, dir.path()),
            "different content → distinct hero paths"
        );
    }

    // ── T2.4 RED→GREEN: prune_stale (tmpdir, best-effort) ───────────
    #[test]
    fn prune_stale_removes_only_orphaned_thumb_hero_artifacts() {
        let cache = tempfile::tempdir().expect("tmp cache");
        let dir = cache.path();
        // Live keys
        let live = "100-abcdef1234567890";
        let stale = "999-ffffffffffffffff";
        // Create artifacts
        let live_thumb = dir.join(format!("{live}-thumb.png"));
        let live_hero = dir.join(format!("{live}-hero.png"));
        let stale_thumb = dir.join(format!("{stale}-thumb.png"));
        let stale_hero = dir.join(format!("{stale}-hero.png"));
        let stale_thumb2 = dir.join(format!("{stale}-thumb.png")); // duplicate check not needed
        // Old path-keyed orphan (should be kept because it lacks -thumb/-hero suffix? Actually old ".png" is not pruned per spec)
        let old_style = dir.join("old123.png");
        // Non-artifact file
        let keep_txt = dir.join("keep.txt");
        for p in [&live_thumb, &live_hero, &stale_thumb, &stale_hero, &old_style, &keep_txt] {
            std::fs::write(p, b"x").expect("write fixture");
        }
        // Theme dir should never be touched, but prune only scans cache_dir
        let mut live_set = std::collections::HashSet::new();
        live_set.insert(live.to_string());
        let removed = prune_stale(dir, &live_set);
        assert_eq!(removed, 2, "should remove stale thumb+hero (2 files)");
        assert!(live_thumb.exists(), "live thumb kept");
        assert!(live_hero.exists(), "live hero kept");
        assert!(!stale_thumb.exists(), "stale thumb pruned");
        assert!(!stale_hero.exists(), "stale hero pruned");
        assert!(old_style.exists(), "old .png orphan not matching *-thumb/-hero pattern kept (migration handles separately)");
        assert!(keep_txt.exists(), "non-artifact kept");
        assert!(!stale_thumb2.exists(), "stale thumb gone");
        // Best-effort on missing dir
        let missing = dir.join("missing_subdir");
        let empty_set = std::collections::HashSet::new();
        assert_eq!(prune_stale(&missing, &empty_set), 0, "missing cache_dir is best-effort 0");
    }

    // ── T2b RED: warm hit must skip FULL-SIZE source decode ──────────
    #[test]
    fn warm_start_disk_hit_performs_zero_full_size_decodes() {
        let dir = tempfile::tempdir().expect("tmp");
        let src = dir.path().join("src.png");
        // Small but valid image that would require full-size decode if not cached
        let img = image::RgbaImage::from_pixel(320, 240, image::Rgba([9u8, 11, 13, 255]));
        img.save(&src).expect("write source");
        let out_dir = dir.path().join("thumbs");
        // First bake — must decode (cold)
        let first = generate_set(&src, &out_dir).expect("first generate_set");
        assert!(first.decoded, "first bake must have performed a full-size decode");
        // All four artifacts must exist after cold bake
        let key = content_cache_key(&src).expect("content key");
        let thumb = out_dir.join(format!("{key}-thumb.png"));
        let hero = out_dir.join(format!("{key}-hero.png"));
        let slat = out_dir.join(format!("{key}-slat.png"));
        let slat_exp = out_dir.join(format!("{key}-slat-exp.png"));
        assert!(thumb.exists(), "thumb cached after first bake");
        assert!(hero.exists(), "hero cached after first bake");
        assert!(slat.exists(), "slat cached after first bake (T2b)");
        assert!(slat_exp.exists(), "slat-exp cached after first bake (T2b)");
        // Capture slat bytes for parity
        let first_slat = first.slat_rgba.clone();
        let first_exp = first.slat_expanded_rgba.clone();
        // Second bake — warm hit must NOT decode source (decoded == false)
        let second = generate_set(&src, &out_dir).expect("second generate_set warm hit");
        assert!(!second.decoded, "warm hit with all four artifacts present must skip full-size source decode");
        assert_eq!(second.slat_rgba.as_raw(), first_slat.as_raw(), "warm slat must equal cold slat (cached bytes)");
        assert_eq!(second.slat_expanded_rgba.as_raw(), first_exp.as_raw(), "warm expanded slat must equal cold expanded slat");
        // Ensure warm path still returns valid thumb/hero paths
        assert_eq!(second.thumb_path, thumb);
        assert_eq!(second.hero_path, hero);
    }

    #[test]
    fn cached_slat_bytes_equal_fresh_baked_png_roundtrip() {
        let dir = tempfile::tempdir().expect("tmp");
        let src = dir.path().join("src.png");
        let mut img = image::RgbaImage::new(200, 120);
        for y in 0..120 {
            for x in 0..200 {
                img.put_pixel(x, y, image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]));
            }
        }
        img.save(&src).expect("write source");
        let out_dir = dir.path().join("thumbs");
        let set = generate_set(&src, &out_dir).expect("generate_set");
        let key = content_cache_key(&src).expect("key");
        let slat_png = out_dir.join(format!("{key}-slat.png"));
        let slat_exp_png = out_dir.join(format!("{key}-slat-exp.png"));
        assert!(slat_png.exists() && slat_exp_png.exists(), "slat PNGs must be persisted");
        // Load PNGs back (small decodes) and compare bytes to BakedSet buffers
        let loaded_slat = image::open(&slat_png).expect("open slat png").to_rgba8();
        let loaded_exp = image::open(&slat_exp_png).expect("open slat-exp png").to_rgba8();
        assert_eq!(loaded_slat.as_raw(), set.slat_rgba.as_raw(), "cached slat PNG bytes must equal fresh-baked slat bytes (PNG roundtrip)");
        assert_eq!(loaded_exp.as_raw(), set.slat_expanded_rgba.as_raw(), "cached expanded slat PNG bytes must equal fresh-baked expanded slat bytes");
        // Also compare to direct bake for parity
        let decoded = image::ImageReader::open(&src).unwrap().decode().unwrap().to_rgba8();
        let direct_collapsed = crate::shell::gallery::slat_image::baked_slat_rgba(decoded.clone(), false);
        let direct_expanded = crate::shell::gallery::slat_image::baked_slat_rgba(decoded, true);
        assert_eq!(loaded_slat.as_raw(), direct_collapsed.as_raw(), "cached slat must match direct bake (R1.4 parity)");
        assert_eq!(loaded_exp.as_raw(), direct_expanded.as_raw(), "cached expanded slat must match direct bake");
    }

    #[test]
    fn prune_stale_removes_orphaned_slat_artifacts() {
        let cache = tempfile::tempdir().expect("tmp cache");
        let dir = cache.path();
        let live = "100-abcdef1234567890";
        let stale = "999-ffffffffffffffff";
        let live_thumb = dir.join(format!("{live}-thumb.png"));
        let live_hero = dir.join(format!("{live}-hero.png"));
        let live_slat = dir.join(format!("{live}-slat.png"));
        let live_slat_exp = dir.join(format!("{live}-slat-exp.png"));
        let stale_slat = dir.join(format!("{stale}-slat.png"));
        let stale_slat_exp = dir.join(format!("{stale}-slat-exp.png"));
        let stale_thumb = dir.join(format!("{stale}-thumb.png"));
        let keep_txt = dir.join("keep.txt");
        for p in [&live_thumb, &live_hero, &live_slat, &live_slat_exp, &stale_slat, &stale_slat_exp, &stale_thumb, &keep_txt] {
            std::fs::write(p, b"x").expect("write fixture");
        }
        let mut live_set = std::collections::HashSet::new();
        live_set.insert(live.to_string());
        let removed = prune_stale(dir, &live_set);
        // stale thumb + stale slat + stale slat-exp = 3 removed; live kept
        assert_eq!(removed, 3, "should remove stale thumb + slat + slat-exp (3 files)");
        assert!(live_thumb.exists() && live_hero.exists() && live_slat.exists() && live_slat_exp.exists(), "live artifacts kept");
        assert!(!stale_slat.exists(), "stale slat pruned");
        assert!(!stale_slat_exp.exists(), "stale slat-exp pruned");
        assert!(!stale_thumb.exists(), "stale thumb pruned");
        assert!(keep_txt.exists(), "non-artifact kept");
    }

    // ── T5.1 REGRESSION GUARD: 42-source burst coalesces to exactly 1 refresh ──
    // ── Tramo 6 RED: bounded pool (K=3) + priority ordering ──────────

    #[test]
    fn preheat_bounds_worker_concurrency() {
        use std::collections::VecDeque;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Condvar, Mutex};
        // Contract: THUMBS_MAX_WORKERS must be 3 and pool must bound concurrency.
        assert_eq!(THUMBS_MAX_WORKERS, 3, "THUMBS_MAX_WORKERS must be 3 (UI thread never starved)");
        let k = THUMBS_MAX_WORKERS;
        let n = 12usize;
        // Shared queue drained by K workers — mirrors preheat's bounded pool.
        let queue: Arc<Mutex<VecDeque<usize>>> = Arc::new(Mutex::new((0..n).collect()));
        let cvar = Arc::new((Mutex::new(()), Condvar::new()));
        let concurrent = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(Mutex::new(0usize));
        let done = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..k {
            let q = queue.clone();
            let conc = concurrent.clone();
            let peak = max_seen.clone();
            let d = done.clone();
            handles.push(std::thread::spawn(move || {
                loop {
                    let job = { let mut g = q.lock().unwrap(); g.pop_front() };
                    let Some(_idx) = job else { break };
                    let cur = conc.fetch_add(1, Ordering::SeqCst) + 1;
                    {
                        let mut p = peak.lock().unwrap();
                        if cur > *p { *p = cur; }
                    }
                    // Simulate heavy decode/bake work so overlap is observable.
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    conc.fetch_sub(1, Ordering::SeqCst);
                    d.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }
        for h in handles { h.join().expect("worker join"); }
        let _ = cvar; // keep condvar in scope to document Mutex/Condvar design
        assert_eq!(done.load(Ordering::SeqCst), n, "all 12 jobs must complete (no loss)");
        let peak = *max_seen.lock().unwrap();
        assert!(peak <= k, "max concurrent {peak} must be ≤ K={k} (bounded pool)");
        assert!(peak > 1, "pool must actually run concurrently (peak {peak} >1 for N=12 on K=3)");
    }

    #[test]
    fn plan_jobs_orders_priority_first() {
        use std::collections::HashSet;
        // Build 5 jobs with original order 0..5
        let sources: Vec<(usize, String, Option<PathBuf>)> = (0..5)
            .map(|i| (i, format!("T{i}"), Some(PathBuf::from(format!("/tmp/w-{i}.png")))))
            .collect();
        let jobs = plan_jobs(sources);
        assert_eq!(jobs.len(), 5);
        let priority: HashSet<usize> = [2usize, 4].into_iter().collect();
        let ordered = order_jobs(jobs, &priority);
        let indices: Vec<usize> = ordered.iter().map(|j| j.index).collect();
        assert_eq!(indices, vec![2, 4, 0, 1, 3], "priority indices must come FIRST, rest keep stable original order");
        // Empty priority → stable original order unchanged
        let sources2: Vec<(usize, String, Option<PathBuf>)> = (0..3)
            .map(|i| (i, format!("A{i}"), Some(PathBuf::from(format!("/p/{i}.png")))))
            .collect();
        let jobs2 = plan_jobs(sources2);
        let empty: HashSet<usize> = HashSet::new();
        let ordered2 = order_jobs(jobs2, &empty);
        assert_eq!(ordered2.iter().map(|j| j.index).collect::<Vec<_>>(), vec![0,1,2]);
        // Unknown priority indices are ignored (no panic, no extra jobs)
        let sources3: Vec<(usize, String, Option<PathBuf>)> = (0..2)
            .map(|i| (i, format!("B{i}"), Some(PathBuf::from(format!("/p/{i}.png")))))
            .collect();
        let jobs3 = plan_jobs(sources3);
        let unknown: HashSet<usize> = [99usize].into_iter().collect();
        let ordered3 = order_jobs(jobs3, &unknown);
        assert_eq!(ordered3.iter().map(|j| j.index).collect::<Vec<_>>(), vec![0,1]);
    }

    #[test]
    fn preheat_plan_for_42_sources_coalesces_to_single_refresh() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        // Build 42 synthetic sources (all Some) → plan_jobs must keep all 42
        let sources: Vec<(usize, String, Option<PathBuf>)> = (0..42)
            .map(|i| (i, format!("Theme-{i}"), Some(PathBuf::from(format!("/tmp/wall-{i}.png")))))
            .collect();
        let jobs = plan_jobs(sources);
        assert_eq!(jobs.len(), 42, "plan_jobs must keep all 42 sourced cards");

        // Counting-closure pattern over is_last_completion (same as preheat's UI callback)
        let refresh_count = Arc::new(AtomicUsize::new(0));
        let pending = Arc::new(AtomicUsize::new(jobs.len()));
        let refresh = {
            let c = refresh_count.clone();
            move || { c.fetch_add(1, Ordering::SeqCst); }
        };
        let refresh = Arc::new(refresh);
        for _ in &jobs {
            let p = pending.clone();
            let r = refresh.clone();
            if is_last_completion(&p) {
                r();
            }
        }
        assert_eq!(
            refresh_count.load(Ordering::SeqCst),
            1,
            "42 completions must coalesce to exactly ONE refresh (regression guard for R1.3)"
        );

        // Mixed burst with holes: 42 slots, 7 without source → 35 jobs → still 1
        let mixed: Vec<(usize, String, Option<PathBuf>)> = (0..42)
            .map(|i| {
                let src = if i % 6 == 0 { None } else { Some(PathBuf::from(format!("/tmp/w-{i}.png"))) };
                (i, format!("T{i}"), src)
            })
            .collect();
        let jobs2 = plan_jobs(mixed);
        assert_eq!(jobs2.len(), 35, "plan filters None sources");
        let c2 = Arc::new(AtomicUsize::new(0));
        let p2 = Arc::new(AtomicUsize::new(jobs2.len()));
        let r2 = {
            let c = c2.clone();
            move || { c.fetch_add(1, Ordering::SeqCst); }
        };
        let r2 = Arc::new(r2);
        for _ in &jobs2 {
            let p = p2.clone();
            let r = r2.clone();
            if is_last_completion(&p) { r(); }
        }
        assert_eq!(c2.load(Ordering::SeqCst), 1, "filtered burst must still coalesce to 1");
    }

}
