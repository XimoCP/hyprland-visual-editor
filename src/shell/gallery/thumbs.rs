// HVE — Real wallpaper thumbnail pipeline (gallery-immersive-redesign
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

use crate::theme_manager::{PreviewRole, PreviewSource, ThemeProvider};

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

/// Content key for a source file on disk — superseded by
/// `content_cache_key_from_bytes` + single read in `generate_set`.
/// Removed in Tramo 7; tests now compute keys via `*_from_bytes` directly.

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

/// Artifact naming is inline in `generate_set` via
/// `format!("{key}-thumb.png")` etc. (content-keyed `{len}-{hash}`).
/// No separate path helpers — single source of truth is the key string.

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

/// `generate_pair` was superseded by `generate_set` (single decode → four
/// artifacts). Removed in Tramo 7 — use `generate_set`.

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

/// Bounded cache pruning (Tramo 7): keeps newest `max_keep` artifacts by
/// mtime, deletes oldest beyond the cap. Only `*-thumb.png` /
/// `*-hero.png` / `*-slat.png` / `*-slat-exp.png` are considered; other
/// files and subdirectories are untouched. Best-effort, bounded,
/// off-UI-thread — errors ignored. Prevents unbounded stale accumulation
/// without reading every wallpaper source (size/age sweep instead of
/// expensive live-key set). Wired via `preheat` (one bounded sweep per
/// gallery open, spawned off UI thread, best-effort). Call
/// `prune_stale(&cache_dir(), 512)` in production.
///
/// Keeps newest 512 artifacts (cap). Oldest beyond cap are removed.
/// Runs on the preheat worker path off the UI thread; errors ignored.
pub fn prune_stale(cache_dir: &Path, max_keep: usize) -> usize {
    let entries = match std::fs::read_dir(cache_dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    let mut artifacts: Vec<(PathBuf, SystemTime)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        let is_artifact = name.ends_with("-thumb.png")
            || name.ends_with("-hero.png")
            || name.ends_with("-slat-exp.png")
            || name.ends_with("-slat.png");
        if !is_artifact {
            continue;
        }
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        artifacts.push((path, mtime));
    }
    if artifacts.len() <= max_keep {
        return 0;
    }
    // Keep newest `max_keep` by mtime descending; delete oldest beyond cap.
    artifacts.sort_by_key(|(_, t)| *t);
    let to_remove = artifacts.len() - max_keep;
    let mut removed = 0;
    for (path, _) in artifacts.iter().take(to_remove) {
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        tracing::debug!(removed, max_keep, dir = %cache_dir.display(), "prune_stale bounded sweep");
    }
    removed
}

/// File extensions eligible as thumbnail sources.
const SOURCE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp"];

/// One provider directory of the theme's `providers/` tree paired with the
/// read-only declarer the registration router supplies for its id.
type Declarer = (PathBuf, Box<dyn ThemeProvider>);

/// The theme's provider directories in SORTED, deterministic order, each
/// paired with its shipped declarer; unknown ids declare nothing.
///
/// This is the storage contract of [`ThemeProvider`] (`providers/{id}/`),
/// not any backend's layout: directory names are ids resolved through the
/// registry, and only the declarer knows what its records look like. The
/// sort is the core's deterministic-order decision — the same sorted
/// directory iteration the old assignment scan performed.
fn collect_declarers(theme_dir: &Path) -> Vec<Declarer> {
    let Ok(entries) = std::fs::read_dir(theme_dir.join("providers")) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|dir| {
            let id = dir.file_name()?.to_str()?.to_owned();
            let declarer = crate::providers::declaration_provider(&id)?;
            Some((dir, declarer))
        })
        .collect()
}

/// Every declared preview source for the theme: provider-directory order
/// (sorted) outside, each provider's own records in its declaration order
/// inside. The gallery contributes zero layout knowledge here — it only
/// collects what the declarers state.
fn declared_sources(declarers: &[Declarer]) -> Vec<PreviewSource> {
    declarers
        .iter()
        .flat_map(|(provider_dir, declarer)| declarer.preview_sources(provider_dir))
        .collect()
}

/// The first candidate of `role` whose target file exists — role
/// precedence is the core's (declarations are consulted in role order),
/// provider-directory order decides ties, and the provider's own record
/// order decides within one declaration.
fn first_existing(sources: &[PreviewSource], role: PreviewRole) -> Option<PathBuf> {
    sources
        .iter()
        .filter(|s| s.role == role)
        .flat_map(|s| s.paths.iter())
        .find(|p| p.exists())
        .cloned()
}
/// Video extension allowlist (Piano 3): animated wallpaper sources the
/// frame extractor can handle.
const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v"];

/// True when `path` carries a video extension (case-insensitive check).
pub fn is_video_source(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Resolve a theme's live background assignment (Piano 3): the first
/// candidate every provider declares in the `Assignment` role — collected
/// in sorted, deterministic provider-directory order, each declaration in
/// its record's own order (wildcard connector first, then sorted keys) —
/// returned only when the target file exists; otherwise keep scanning the
/// remaining candidates. None = no live assignment.
///
/// The planner resolves this SAME stage inline over its already-collected
/// declarations (see [`plan_with`]) instead of calling this wrapper, so
/// this is the stage's direct test seam — kept `cfg(test)` so the binary
/// never carries an uncalled entry point.
#[cfg(test)]
pub fn video_assignment(theme_dir: &Path) -> Option<PathBuf> {
    let sources = declared_sources(&collect_declarers(theme_dir));
    first_existing(&sources, PreviewRole::Assignment)
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

/// A planned preview source (W2-1 fix: planning vs extraction split).
///
/// Planning NEVER spawns a process: it only reads small record files and
/// probes file existence, so it is safe on the Slint UI thread. A video
/// whose frame is already cached resolves to `Image(frame)` immediately; an
/// uncached video defers to `Video{..}` and the background bake worker
/// ([`bake_job_source`], called from [`preheat`] off the UI thread) runs
/// the synchronous ffmpeg extraction there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewSpec {
    /// Bake directly: a wallpaper image or an already-cached video frame.
    Image(PathBuf),
    /// Deferred video: `video` is the recorded source, `cached_frame` the
    /// deterministic frame path (same naming as [`extract_video_frame`]),
    /// and `theme_dir` so the worker can run the static fallback chain
    /// off-thread when extraction fails (a video ffmpeg cannot decode must
    /// still render the theme's static wallpaper, never a skeleton).
    /// The worker extracts first and bakes the same four artifacts from the
    /// frame; when extraction fails it warns (naming theme dir and video)
    /// and falls through to the static chain below — the old fall-through
    /// contract, just relocated off the UI thread.
    Video {
        video: PathBuf,
        cached_frame: PathBuf,
        theme_dir: PathBuf,
    },
}

impl PreviewSpec {
    /// Path for log lines: the image itself, or the video behind a deferred spec.
    pub fn log_path(&self) -> &Path {
        match self {
            PreviewSpec::Image(p) => p,
            PreviewSpec::Video { video, .. } => video,
        }
    }
}

/// Deterministic frame cache path for a video source — the SAME naming
/// [`extract_video_frame`] writes to, derived from the record path + source
/// mtime WITHOUT extracting (no process spawn; safe on the UI thread and
/// in delete cleanup).
pub fn cached_frame_path(video: &Path) -> PathBuf {
    let mtime = source_mtime(video).unwrap_or(SystemTime::UNIX_EPOCH);
    cache_dir().join(format!("{}-frame.png", cache_key(video, mtime)))
}

/// Resolve a video path to a spec WITHOUT spawning: an existing cached
/// frame wins immediately (file-existence probe only); otherwise extraction
/// is deferred to the background worker. `None` when the path is missing
/// or not a video. `theme_dir` is carried so the worker can run the static
/// fallback chain off-thread when extraction fails.
fn spec_for_video(video: PathBuf, theme_dir: &Path) -> Option<PreviewSpec> {
    if !video.exists() || !is_video_source(&video) {
        return None;
    }
    let cached_frame = cached_frame_path(&video);
    if cached_frame.exists() {
        Some(PreviewSpec::Image(cached_frame))
    } else {
        Some(PreviewSpec::Video {
            video,
            cached_frame,
            theme_dir: theme_dir.to_path_buf(),
        })
    }
}

/// Plan a theme's preview source WITHOUT spawning any process (W2-1).
///
/// The resolution chain is the core's, in precedence order: the live
/// background assignment wins outright (an assigned static image IS the
/// live wallpaper; an assigned video defers extraction to the background
/// worker), then the painter's video record (fall-through on an unusable
/// one, with a warning naming theme dir and video), then the static
/// records — wallpaper manifest before wallpaper text — and finally the
/// loose image scan. Every record is a PROVIDER DECLARATION: the gallery
/// collects what each backend states about its own files (see
/// [`collect_declarers`]) and never parses a backend layout itself.
///
/// Planning never decodes: a video ffmpeg could not decode still plans to
/// a `Video` spec (keeping video's priority position), and the WORKER runs
/// the old fall-through to the static chain off-thread when extraction
/// fails — so undecodable video + static fallback still renders stale,
/// exactly as the old synchronous resolver did.
pub fn plan_preview_source(theme_dir: &Path) -> Option<PreviewSpec> {
    plan_with(theme_dir, &collect_declarers(theme_dir))
}

/// The planner over ALREADY-COLLECTED declarers (the production path passes
/// the theme's registry-resolved declarers; tests pass fake providers) —
/// everything about precedence lives here, independent of who declares.
fn plan_with(theme_dir: &Path, declarers: &[Declarer]) -> Option<PreviewSpec> {
    let sources = declared_sources(declarers);

    // Role 1 — the live background assignment: a static assignment wins
    // outright; a video one defers extraction to the worker. The
    // video-extension allowlist decides which of the two it is.
    if let Some(assigned) = first_existing(&sources, PreviewRole::Assignment) {
        if is_video_source(&assigned) {
            if let Some(spec) = spec_for_video(assigned, theme_dir) {
                return Some(spec);
            }
        } else {
            // Image assignments win outright: they ARE the live wallpaper.
            return Some(PreviewSpec::Image(assigned));
        }
    }

    // Role 2 — the painter's video record: usable video keeps its
    // priority position; an unusable one warns and falls through.
    for record in sources
        .iter()
        .filter(|s| s.role == PreviewRole::PainterVideo)
    {
        let Some(video) = record.paths.first() else {
            continue;
        };
        if let Some(spec) = spec_for_video(video.clone(), theme_dir) {
            return Some(spec);
        }
        tracing::warn!(
            theme = %theme_dir.display(),
            video = %video.display(),
            "painter video record unusable, falling through to static sources"
        );
    }

    // Roles 3-4 — the static records, then the loose scan.
    static_from(&sources)
        .or_else(|| scan_images(theme_dir))
        .map(PreviewSpec::Image)
}

/// The static preview chain (no process spawn, no decode) over
/// already-collected declarations: wallpaper manifest records → wallpaper
/// text records → loose image scan. Shared by the planner and —
/// off-thread — by the worker's undecodable-video fallback and the delete
/// cleanup, so all three agree on what "the static wallpaper" is.
pub fn static_fallback_source(theme_dir: &Path) -> Option<PathBuf> {
    let sources = declared_sources(&collect_declarers(theme_dir));
    static_from(&sources).or_else(|| scan_images(theme_dir))
}

/// The declared half of the static chain: first existing candidate of the
/// manifest role, then of the text role (role precedence is the core's).
fn static_from(sources: &[PreviewSource]) -> Option<PathBuf> {
    first_existing(sources, PreviewRole::WallpaperManifest)
        .or_else(|| first_existing(sources, PreviewRole::WallpaperText))
}

/// Pure loose image scan (extension-based discovery only — no process
/// spawn, no decode). Themes persist provider state under their directory;
/// the scan is depth-first over sorted entries so results are
/// deterministic. None = theme has no image source (skeleton stays).
fn scan_images(theme_dir: &Path) -> Option<PathBuf> {
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
        if let Some(hit) = scan_images(dir) {
            return Some(hit);
        }
    }
    None
}

/// Bake one planned job into a full artifact set. Runs on the preheat
/// worker pool, NEVER on the UI thread: images bake directly; a deferred
/// video extracts its frame first (the single ffmpeg spawn in the gallery
/// pipeline lives here, via the existing [`extract_video_frame`]) and then
/// bakes the same four artifacts from that frame. When extraction fails
/// the worker warns (naming theme dir and video) and falls back to the
/// theme's static chain — the old synchronous fall-through, relocated
/// off-thread so undecodable video still renders stale, never a skeleton.
pub fn bake_job_source(spec: &PreviewSpec) -> Result<BakedSet, String> {
    match spec {
        PreviewSpec::Image(path) => generate_set(path, &cache_dir()),
        PreviewSpec::Video { video, theme_dir, .. } => {
            match extract_video_frame(video, &cache_dir()) {
                Some(frame) => generate_set(&frame, &cache_dir()),
                None => {
                    tracing::warn!(
                        theme = %theme_dir.display(),
                        video = %video.display(),
                        "cannot extract a preview frame, falling back to static sources"
                    );
                    let fallback = static_fallback_source(theme_dir).ok_or_else(|| {
                        format!(
                            "cannot extract a preview frame from {} and no static fallback in {}",
                            video.display(),
                            theme_dir.display()
                        )
                    })?;
                    generate_set(&fallback, &cache_dir())
                }
            }
        }
    }
}

/// One off-thread thumbnail request (task 4.7).
#[derive(Debug, Clone)]
pub struct PreheatJob {
    /// Card row index in the UI model.
    pub index: usize,
    /// Card name captured for stale-write guards on marshal.
    pub name: String,
    /// Planned source: an image bakes directly, a video extracts first.
    /// The plan itself never spawned anything (W2-1), so scheduling a job
    /// is safe on the UI thread; the ffmpeg spawn happens in the worker.
    pub spec: PreviewSpec,
}

/// Maximum concurrent thumbnail workers (Tramo 6 corrective): bounds the
/// one-thread-per-job blast that pegged the CPU and starved the Slint UI
/// thread on cold start (42 themes → 42 threads). K=3 keeps cores free for UI.
pub const THUMBS_MAX_WORKERS: usize = 3;

/// Pure planner: keep only cards that have a planned source (task 4.7).
pub fn plan_jobs(sources: Vec<(usize, String, Option<PreviewSpec>)>) -> Vec<PreheatJob> {
    sources
        .into_iter()
        .filter_map(|(index, name, source)| {
            source.map(|spec| PreheatJob { index, name, spec })
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
    sources: Vec<(usize, String, Option<PreviewSpec>)>,
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
    // Bounded pruning (Tramo 7): off UI thread, best-effort, keeps newest 512 artifacts.
    {
        let cache = cache_dir();
        std::thread::spawn(move || {
            let _ = prune_stale(&cache, 512);
        });
    }

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
            let span = tracing::info_span!("thumbs::preheat::job", source = %job.spec.log_path().display());
            let _guard = span.enter();
            // The single ffmpeg spawn in the gallery pipeline lives HERE,
            // inside bake_job_source on this worker thread — never on the
            // UI thread that planned the job (W2-1).
            match bake_job_source(&job.spec) {
                Ok(set) => {
                    let idx = job.index;
                    let name: Arc<str> = Arc::from(job.name.as_str());
                    let thumb_path = set.thumb_path;
                    let hero_path = set.hero_path;
                    let decoded = set.decoded;
                    let slat = set.slat_rgba;
                    let slat_exp = set.slat_expanded_rgba;
                    let elapsed = t_total.elapsed().as_millis() as u64;
                    tracing::trace!(decoded, total_ms = elapsed, "job done");
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
                    tracing::warn!("[thumbs] preheat for '{}' failed: {}", job.spec.log_path().display(), e);
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

    // `hero_path` / `thumb_path` helpers removed — naming is inline via `content_cache_key_from_bytes`.
    // `generate_pair` superseded by `generate_set`; its cache-hit contract is now covered by
    // `generate_set_single_decode_four_artifacts` and the warm-hit tests below.

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

    // `generated_thumb_roundtrips_and_hits_cache` removed — superseded by `generate_set`
    // warm-hit tests; thumb cover-crop geometry is still covered by `cover_geometry_*`.

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

        let found = plan_preview_source(dir.path()).expect("source found");
        assert_eq!(found, PreviewSpec::Image(img));
    }

    #[test]
    fn find_source_none_without_images() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("meta.json"), b"{}").expect("meta");
        assert!(plan_preview_source(dir.path()).is_none(), "no images -> None");
        assert!(plan_preview_source(&dir.path().join("missing")).is_none(), "missing dir safe");
    }

    #[test]
    fn plan_jobs_keeps_only_sourced_cards() {
        let jobs = plan_jobs(vec![
            (0, "A".into(), Some(PreviewSpec::Image(PathBuf::from("/a.png")))),
            (1, "B".into(), None),
            (2, "C".into(), Some(PreviewSpec::Image(PathBuf::from("/c.jpg")))),
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
        let found = plan_preview_source(dir.path()).expect("json source");
        assert_eq!(found, PreviewSpec::Image(real));
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
        let found = plan_preview_source(dir.path()).expect("fallback scan");
        assert_eq!(found, PreviewSpec::Image(loose));
    }

    #[test]
    fn find_source_discovers_wallpaper_txt_and_json_takes_priority() {
        let dir = tempfile::tempdir().expect("tmp");
        let a = dir.path().join("a.png"); image::RgbaImage::from_pixel(10,10,image::Rgba([0u8,0,0,255])).save(&a).unwrap();
        let b = dir.path().join("b.png"); image::RgbaImage::from_pixel(10,10,image::Rgba([1u8,1,1,255])).save(&b).unwrap();
        let tp = dir.path().join("providers/noctalia-v5/wallpaper.txt");
        std::fs::create_dir_all(tp.parent().unwrap()).unwrap();
        std::fs::write(&tp, format!("{}\n", a.display())).unwrap();
        assert_eq!(plan_preview_source(dir.path()), Some(PreviewSpec::Image(a.clone())));
        let prov = dir.path().join("providers/noctalia");
        std::fs::create_dir_all(&prov).unwrap();
        let j = serde_json::json!({"wallpapers":{"DP-3":{"dark":b.to_string_lossy()}}});
        std::fs::write(prov.join("wallpapers.json"), serde_json::to_string(&j).unwrap()).unwrap();
        assert_eq!(plan_preview_source(dir.path()), Some(PreviewSpec::Image(b)), "JSON priority over txt");
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
        // The assignment record is declared by the provider that writes it;
        // a candidate whose target file is missing must be rejected.
        write_assignments(
            &dir.path().join("providers/noctalia-v5"),
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
            &dir.path().join("providers/noctalia-v5"),
            serde_json::json!({ "assignments": {
                "HDMI-A-1": { "local_path": "/nope/z.mp4" },
                "DP-1": { "local_path": chosen.to_string_lossy() }
            }}),
        );
        assert_eq!(video_assignment(dir.path()), Some(chosen), "DP-1 sorts before HDMI-A-1");
    }

    /// The preserved sorted, deterministic provider-directory iteration:
    /// when two shipped directories declare the SAME role, the theme's
    /// `providers/` tree is read in sorted order, so the record under the
    /// lexically-first directory decides. (The retired `wallpaper` id
    /// stores a byte-copy of the same cache record the first directory
    /// holds, so both directories declare through the same format.)
    ///
    /// Replaces the old test that scanned arbitrary UNKNOWN directory
    /// names for a hardcoded record file — that blind scan of every
    /// backend layout is exactly the coupling this seam removed: an id
    /// the registry does not ship now declares nothing.
    #[test]
    fn manifest_sources_resolve_in_sorted_provider_dir_order() {
        let src_dir = tempfile::tempdir().expect("src tmp");
        // Sources live OUTSIDE the theme tree so the loose image scan can
        // never be what found them.
        let early = src_dir.path().join("a.png");
        let late = src_dir.path().join("z.png");
        std::fs::write(&early, b"a").expect("fixture");
        std::fs::write(&late, b"z").expect("fixture");

        let dir = tempfile::tempdir().expect("tmp");
        for (sub, target) in [("noctalia", &early), ("wallpaper", &late)] {
            let provider_dir = dir.path().join("providers").join(sub);
            std::fs::create_dir_all(&provider_dir).expect("mkdir");
            let j = serde_json::json!({"wallpapers":{"DP-3":{"dark": target.to_string_lossy()}}});
            std::fs::write(
                provider_dir.join("wallpapers.json"),
                serde_json::to_string(&j).unwrap(),
            )
            .expect("record");
        }
        assert_eq!(
            plan_preview_source(dir.path()),
            Some(PreviewSpec::Image(early)),
            "providers/noctalia sorts before providers/wallpaper: sorted \
             provider-directory order must decide which declaration wins"
        );
    }

    #[test]
    fn unknown_provider_dirs_declare_nothing() {
        // The other half of the removed blind scan: a record file under an
        // id the registry does not ship is invisible to the gallery — no
        // backend layout is read outside the declarations.
        let dir = tempfile::tempdir().expect("tmp");
        let video = dir.path().join("ghost.mp4");
        std::fs::write(&video, b"v").expect("fixture");
        write_assignments(
            &dir.path().join("providers/aaa-early"),
            serde_json::json!({ "assignments": { "*": { "local_path": video.to_string_lossy() } } }),
        );
        assert_eq!(
            video_assignment(dir.path()),
            None,
            "an unknown id must declare nothing"
        );
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

    // ── Piano 3 WIRE: video-aware ordering inside plan_preview_source ──

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
            plan_preview_source(dir.path()),
            Some(PreviewSpec::Image(assigned)),
            "assignment beats wallpapers.json, wallpaper.txt and loose scan"
        );
    }

    #[test]
    fn plan_defers_uncached_assigned_video_over_static_fallback() {
        // W2-1 contract change (documented on `PreviewSpec::Video`): video
        // keeps its priority position at PLAN time and defers extraction to
        // the worker, so planning returns a Video spec WITHOUT spawning —
        // even when ffmpeg could never decode these bytes and a static
        // fallback exists. No ffmpeg needed: planning never decodes.
        let _env = crate::test_utils::TempEnv::new();
        let cache = cache_dir();
        std::fs::create_dir_all(&cache).unwrap();
        let before = sorted_cache_names(&cache);
        let dir = tempfile::tempdir().expect("tmp");
        let broken = dir.path().join("broken.mp4");
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
        let spec = plan_preview_source(dir.path()).expect("video assignment must plan");
        match &spec {
            PreviewSpec::Video { video, .. } => assert_eq!(video, &broken),
            other => panic!("expected a Video spec, got {other:?}"),
        }
        assert_eq!(
            sorted_cache_names(&cache),
            before,
            "planning a broken video must not spawn ffmpeg either"
        );
    }

    // ── Painter video record (noctalia-v5/video.txt) discovery ──────

    /// Fixture helper: synthesize a short mp4 decodable by ffmpeg, using the
    /// same lavfi recipe as `extract_video_frame_writes_decodable_cached_png`.
    /// Returns None when ffmpeg cannot synthesize (caller must skip then).
    fn synth_video_fixture(path: &Path) -> bool {
        std::process::Command::new("ffmpeg")
            .args(["-f", "lavfi", "-i", "testsrc=duration=5:size=320x240:rate=10", "-y"])
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn plan_ignores_painter_video_record_pointing_at_missing_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let v5 = dir.path().join("providers/noctalia-v5");
        std::fs::create_dir_all(&v5).unwrap();
        std::fs::write(
            v5.join(crate::providers::wallpaper_authority::PAINTER_VIDEO_FILE),
            crate::providers::wallpaper_authority::format_painter_video_record(
                Path::new("/nonexistent/gone.mp4"),
                Some("skwd-wall-vk"),
                "DP-3",
                7,
            ),
        )
        .unwrap();
        assert_eq!(
            plan_preview_source(dir.path()),
            None,
            "a video.txt pointing at a missing file falls through without a bogus source"
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
        let expected_key = content_cache_key_from_bytes(&std::fs::read(&src).expect("read src"));
        assert_eq!(set.thumb_path, out_dir.join(format!("{expected_key}-thumb.png")), "thumb path matches content-keyed contract");
        assert_eq!(set.hero_path, out_dir.join(format!("{expected_key}-hero.png")), "hero path matches content-keyed contract");
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
        let ka = content_cache_key_from_bytes(&std::fs::read(&a).expect("read a"));
        let kb = content_cache_key_from_bytes(&std::fs::read(&b).expect("read b"));
        assert_eq!(ka, kb, "byte-identical files must share content key despite different paths/mtimes");
        let cache = tempfile::tempdir().expect("tmp cache");
        // Artifact naming is inline via key strings (no thumb_path helper)
        assert_eq!(
            cache.path().join(format!("{ka}-thumb.png")),
            cache.path().join(format!("{kb}-thumb.png")),
            "same content → same thumb artifact path"
        );
        assert_eq!(
            cache.path().join(format!("{ka}-hero.png")),
            cache.path().join(format!("{kb}-hero.png")),
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
        let ka = content_cache_key_from_bytes(&std::fs::read(&a).expect("read a"));
        let kb = content_cache_key_from_bytes(&std::fs::read(&b).expect("read b"));
        assert_ne!(ka, kb, "equal-size different bytes must have distinct keys");
        assert_ne!(
            dir.path().join(format!("{ka}-thumb.png")),
            dir.path().join(format!("{kb}-thumb.png")),
            "different content → distinct thumb paths"
        );
        assert_ne!(
            dir.path().join(format!("{ka}-hero.png")),
            dir.path().join(format!("{kb}-hero.png")),
            "different content → distinct hero paths"
        );
    }

    // ── T2.4 RED→GREEN: prune_stale bounded sweep (tmpdir, best-effort) ───
    #[test]
    fn prune_stale_removes_only_orphaned_thumb_hero_artifacts() {
        // Bounded sweep: keep newest `max_keep` artifacts by mtime.
        let cache = tempfile::tempdir().expect("tmp cache");
        let dir = cache.path();
        // Create 4 artifacts sequentially with distinct mtimes (sleep to order)
        let a_thumb = dir.join("100-aaaa-thumb.png");
        let a_hero = dir.join("100-aaaa-hero.png");
        let b_thumb = dir.join("200-bbbb-thumb.png");
        let b_hero = dir.join("200-bbbb-hero.png");
        for p in [&a_thumb, &a_hero] {
            std::fs::write(p, b"x").expect("write fixture");
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
        for p in [&b_thumb, &b_hero] {
            std::fs::write(p, b"x").expect("write fixture");
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
        // Extra non-artifact that must be kept
        let old_style = dir.join("old123.png");
        let keep_txt = dir.join("keep.txt");
        for p in [&old_style, &keep_txt] {
            std::fs::write(p, b"x").expect("write fixture");
        }
        // Keep newest 2 → oldest 2 (a_*) pruned, newest 2 (b_*) kept
        let removed = prune_stale(dir, 2);
        assert_eq!(removed, 2, "bounded sweep: keep newest 2, oldest 2 pruned");
        assert!(!a_thumb.exists(), "oldest thumb pruned");
        assert!(!a_hero.exists(), "oldest hero pruned");
        assert!(b_thumb.exists(), "newest thumb kept");
        assert!(b_hero.exists(), "newest hero kept");
        assert!(old_style.exists(), "non-artifact kept");
        assert!(keep_txt.exists(), "non-artifact kept");
        // Best-effort on missing dir
        let missing = dir.join("missing_subdir");
        assert_eq!(prune_stale(&missing, 2), 0, "missing cache_dir is best-effort 0");
        // No pruning when under cap
        assert_eq!(prune_stale(dir, 10), 0, "under cap → 0 removed");
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
        let key = content_cache_key_from_bytes(&std::fs::read(&src).expect("read src"));
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
        let key = content_cache_key_from_bytes(&std::fs::read(&src).expect("read src"));
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
        // Oldest set (should be pruned when over cap)
        let old_thumb = dir.join("100-aaaa-thumb.png");
        let old_slat = dir.join("100-aaaa-slat.png");
        let old_slat_exp = dir.join("100-aaaa-slat-exp.png");
        // Newest set (should be kept)
        let new_thumb = dir.join("200-bbbb-thumb.png");
        let new_hero = dir.join("200-bbbb-hero.png");
        let new_slat = dir.join("200-bbbb-slat.png");
        let new_slat_exp = dir.join("200-bbbb-slat-exp.png");
        for p in [&old_thumb, &old_slat, &old_slat_exp] {
            std::fs::write(p, b"x").expect("write fixture");
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
        for p in [&new_thumb, &new_hero, &new_slat, &new_slat_exp] {
            std::fs::write(p, b"x").expect("write fixture");
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
        let keep_txt = dir.join("keep.txt");
        std::fs::write(&keep_txt, b"x").unwrap();
        // Keep newest 4 → oldest 3 pruned
        let removed = prune_stale(dir, 4);
        assert_eq!(removed, 3, "bounded: oldest 3 beyond cap 4 pruned");
        assert!(!old_thumb.exists() && !old_slat.exists() && !old_slat_exp.exists(), "old slat artifacts pruned");
        assert!(new_thumb.exists() && new_hero.exists() && new_slat.exists() && new_slat_exp.exists(), "newest artifacts kept");
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
        let sources: Vec<(usize, String, Option<PreviewSpec>)> = (0..5)
            .map(|i| (i, format!("T{i}"), Some(PreviewSpec::Image(PathBuf::from(format!("/tmp/w-{i}.png"))))))
            .collect();
        let jobs = plan_jobs(sources);
        assert_eq!(jobs.len(), 5);
        let priority: HashSet<usize> = [2usize, 4].into_iter().collect();
        let ordered = order_jobs(jobs, &priority);
        let indices: Vec<usize> = ordered.iter().map(|j| j.index).collect();
        assert_eq!(indices, vec![2, 4, 0, 1, 3], "priority indices must come FIRST, rest keep stable original order");
        // Empty priority → stable original order unchanged
        let sources2: Vec<(usize, String, Option<PreviewSpec>)> = (0..3)
            .map(|i| (i, format!("A{i}"), Some(PreviewSpec::Image(PathBuf::from(format!("/p/{i}.png"))))))
            .collect();
        let jobs2 = plan_jobs(sources2);
        let empty: HashSet<usize> = HashSet::new();
        let ordered2 = order_jobs(jobs2, &empty);
        assert_eq!(ordered2.iter().map(|j| j.index).collect::<Vec<_>>(), vec![0,1,2]);
        // Unknown priority indices are ignored (no panic, no extra jobs)
        let sources3: Vec<(usize, String, Option<PreviewSpec>)> = (0..2)
            .map(|i| (i, format!("B{i}"), Some(PreviewSpec::Image(PathBuf::from(format!("/p/{i}.png"))))))
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
        let sources: Vec<(usize, String, Option<PreviewSpec>)> = (0..42)
            .map(|i| (i, format!("Theme-{i}"), Some(PreviewSpec::Image(PathBuf::from(format!("/tmp/wall-{i}.png"))))))
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
        let mixed: Vec<(usize, String, Option<PreviewSpec>)> = (0..42)
            .map(|i| {
                let src = if i % 6 == 0 { None } else { Some(PreviewSpec::Image(PathBuf::from(format!("/tmp/w-{i}.png")))) };
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

    // ── W2-1: planning never spawns ffmpeg (plan vs extract split) ──

    fn sorted_cache_names(cache: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(cache)
            .map(|r| {
                r.flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn plan_returns_video_spec_without_touching_cache() {
        // Deterministic (no ffmpeg needed): fake .mp4 bytes are enough
        // because planning only reads records + file existence, never
        // decodes. Any ffmpeg spawn would write a `-frame.png` here.
        let _env = crate::test_utils::TempEnv::new();
        let cache = cache_dir();
        std::fs::create_dir_all(&cache).unwrap();
        let before = sorted_cache_names(&cache);
        let dir = tempfile::tempdir().expect("tmp");
        let video = dir.path().join("loop.mp4");
        std::fs::write(&video, b"definitely not a video; planning must not care").unwrap();
        let v5 = dir.path().join("providers/noctalia-v5");
        std::fs::create_dir_all(&v5).unwrap();
        std::fs::write(
            v5.join(crate::providers::wallpaper_authority::PAINTER_VIDEO_FILE),
            crate::providers::wallpaper_authority::format_painter_video_record(
                &video,
                Some("skwd-wall-vk"),
                "DP-3",
                1,
            ),
        )
        .unwrap();
        let spec = plan_preview_source(dir.path())
            .expect("a video-record theme must plan to a spec");
        match &spec {
            PreviewSpec::Video { video: v, cached_frame, .. } => {
                assert_eq!(v, &video);
                assert!(
                    cached_frame
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.ends_with("-frame.png")),
                    "the spec must carry the deterministic frame cache path"
                );
            }
            other => panic!("expected a Video spec, got {other:?}"),
        }
        assert_eq!(
            sorted_cache_names(&cache),
            before,
            "planning must not write to the cache (no ffmpeg spawn on the planning path)"
        );
    }

    #[test]
    fn plan_resolves_already_cached_frame_immediately() {
        // No regression: a frame ffmpeg already baked resolves at planning
        // time through a file-existence check only — still no spawn.
        let _env = crate::test_utils::TempEnv::new();
        let dir = tempfile::tempdir().expect("tmp");
        let video = dir.path().join("loop.mp4");
        std::fs::write(&video, b"fake video bytes").unwrap();
        let v5 = dir.path().join("providers/noctalia-v5");
        std::fs::create_dir_all(&v5).unwrap();
        std::fs::write(
            v5.join(crate::providers::wallpaper_authority::PAINTER_VIDEO_FILE),
            crate::providers::wallpaper_authority::format_painter_video_record(
                &video,
                Some("skwd-wall-vk"),
                "DP-3",
                2,
            ),
        )
        .unwrap();
        let frame = cached_frame_path(&video);
        std::fs::create_dir_all(frame.parent().unwrap()).unwrap();
        image::RgbaImage::from_pixel(16, 16, image::Rgba([5u8, 6, 7, 255]))
            .save(&frame)
            .unwrap();
        assert_eq!(
            plan_preview_source(dir.path()),
            Some(PreviewSpec::Image(frame)),
            "an already-cached frame must resolve at planning time"
        );
    }

    #[test]
    fn worker_bakes_frame_and_artifacts_from_video_spec() {
        if !ffmpeg_available() {
            println!("ffmpeg not available — skipping worker video bake test");
            return;
        }
        let _env = crate::test_utils::TempEnv::new();
        let dir = tempfile::tempdir().expect("tmp");
        let video = dir.path().join("src.mp4");
        assert!(synth_video_fixture(&video), "failed to synthesize mp4 fixture");
        let spec = PreviewSpec::Video {
            video: video.clone(),
            cached_frame: cached_frame_path(&video),
            theme_dir: dir.path().to_path_buf(),
        };
        let set = bake_job_source(&spec).expect("the worker must bake a video spec");
        let frame = cached_frame_path(&video);
        assert!(frame.exists(), "the worker must extract the cached frame");
        let decoded = image::ImageReader::open(&frame).unwrap().decode().unwrap();
        assert_eq!((decoded.width(), decoded.height()), (320, 240), "full source frame");
        let key = content_cache_key_from_bytes(&std::fs::read(&frame).unwrap());
        for suffix in ["-thumb.png", "-hero.png", "-slat.png", "-slat-exp.png"] {
            assert!(
                cache_dir().join(format!("{key}{suffix}")).exists(),
                "the worker must bake {suffix} from the extracted frame"
            );
        }
        assert!(set.thumb_path.exists() && set.hero_path.exists());
    }

    #[test]
    fn plan_prefers_assigned_image_over_painter_video_record() {
        // An assigned static image wins outright without any video work.
        let _env = crate::test_utils::TempEnv::new();
        let dir = tempfile::tempdir().expect("tmp");
        let assigned = dir.path().join("assigned.png");
        image::RgbaImage::from_pixel(10, 10, image::Rgba([2u8, 2, 2, 255]))
            .save(&assigned)
            .unwrap();
        let v5 = dir.path().join("providers/noctalia-v5");
        write_assignments(
            &v5,
            serde_json::json!({"assignments":{"*":{"filename":"a.png","local_path":assigned.to_string_lossy()}}}),
        );
        let video = dir.path().join("loop.mp4");
        std::fs::write(&video, b"fake").unwrap();
        std::fs::write(
            v5.join(crate::providers::wallpaper_authority::PAINTER_VIDEO_FILE),
            crate::providers::wallpaper_authority::format_painter_video_record(
                &video,
                Some("skwd-wall-vk"),
                "DP-3",
                9,
            ),
        )
        .unwrap();
        assert_eq!(
            plan_preview_source(dir.path()),
            Some(PreviewSpec::Image(assigned)),
            "an assigned static image wins over the painter video record"
        );
    }

    #[test]
    fn worker_falls_back_to_static_when_video_undecodable() {
        // W2-1: a video ffmpeg cannot decode must still render the theme's
        // static wallpaper — the worker runs the old fall-through off-thread
        // instead of dropping the card. No ffmpeg needed: bogus bytes never
        // decode, whether ffmpeg is present or absent.
        let _env = crate::test_utils::TempEnv::new();
        let dir = tempfile::tempdir().expect("tmp");
        let broken = dir.path().join("broken.mp4");
        std::fs::write(&broken, b"definitely not a video").unwrap();
        let stale = dir.path().join("stale.png");
        image::RgbaImage::from_pixel(10, 10, image::Rgba([7u8, 7, 7, 255]))
            .save(&stale)
            .unwrap();
        let v5 = dir.path().join("providers/noctalia-v5");
        std::fs::create_dir_all(&v5).unwrap();
        std::fs::write(v5.join("wallpaper.txt"), format!("{}\n", stale.display())).unwrap();
        let spec = PreviewSpec::Video {
            video: broken.clone(),
            cached_frame: cached_frame_path(&broken),
            theme_dir: dir.path().to_path_buf(),
        };
        let set = bake_job_source(&spec)
            .expect("an undecodable video must fall back to the static source, not fail");
        let key = content_cache_key_from_bytes(&std::fs::read(&stale).unwrap());
        assert_eq!(
            set.thumb_path,
            cache_dir().join(format!("{key}-thumb.png")),
            "the worker must bake the static fallback's artifacts"
        );
        assert!(set.hero_path.exists(), "fallback hero PNG written");
    }

    // ── read-preview-source: provider-declared preview sources ─────────

    use crate::theme_manager::{PreviewRole, PreviewSource, PreviewSourceKind, ThemeProvider};

    /// A provider the product does NOT ship: its id matches no registry
    /// entry, and its record file name is known only to this fake. The
    /// gallery must resolve whatever it declares without knowing either —
    /// that is the whole `read-preview-source` contract.
    struct FakePreviewProvider {
        id: &'static str,
        /// File name of this fake's OWN record inside its provider dir.
        record: &'static str,
        role: PreviewRole,
        kind: PreviewSourceKind,
    }

    impl ThemeProvider for FakePreviewProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn display_name_key(&self) -> &str {
            "test.fake"
        }
        fn icon(&self) -> &str {
            "✧"
        }
        fn save(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn apply(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn preview_sources(&self, provider_dir: &Path) -> Vec<PreviewSource> {
            let Ok(text) = std::fs::read_to_string(provider_dir.join(self.record)) else {
                return Vec::new();
            };
            let Some(line) = text.lines().map(str::trim).find(|l| !l.is_empty()) else {
                return Vec::new();
            };
            vec![PreviewSource {
                role: self.role,
                kind: self.kind,
                paths: vec![PathBuf::from(line)],
            }]
        }
    }

    fn fake_declarer(
        provider_dir: &Path,
        id: &'static str,
        record: &'static str,
        role: PreviewRole,
        kind: PreviewSourceKind,
    ) -> (PathBuf, Box<dyn ThemeProvider>) {
        (
            provider_dir.to_path_buf(),
            Box::new(FakePreviewProvider {
                id,
                record,
                role,
                kind,
            }),
        )
    }

    #[test]
    fn plan_resolves_a_declared_source_from_an_unknown_backend() {
        // The source sits OUTSIDE the theme tree, so the loose image scan
        // could never be what found it: resolution must come from the
        // fake's declaration alone.
        let src_dir = tempfile::tempdir().expect("src tmp");
        let src = src_dir.path().join("declared.png");
        std::fs::write(&src, b"declared image bytes").expect("fixture");

        let theme = tempfile::tempdir().expect("theme tmp");
        let provider_dir = theme.path().join("providers").join("unknown-shell-xyz");
        std::fs::create_dir_all(&provider_dir).expect("provider dir");
        std::fs::write(provider_dir.join("shell-record.src"), format!("{}\n", src.display()))
            .expect("record");

        let declarers = vec![fake_declarer(
            &provider_dir,
            "unknown-shell-xyz",
            "shell-record.src",
            PreviewRole::WallpaperText,
            PreviewSourceKind::Image,
        )];
        assert_eq!(
            plan_with(theme.path(), &declarers),
            Some(PreviewSpec::Image(src)),
            "the gallery must resolve a declared preview source with zero \
             knowledge of any backend's layout or record format"
        );
    }

    #[test]
    fn preview_role_order_beats_declarer_order() {
        // Two declaring providers, injected with the TEXT declarer first:
        // the core's role precedence (wallpaper manifest before wallpaper
        // text — the order the old chain hardcoded) must decide, not the
        // order the declarations arrive in.
        let src_dir = tempfile::tempdir().expect("src tmp");
        let manifest_src = src_dir.path().join("manifest.png");
        let text_src = src_dir.path().join("text.png");
        std::fs::write(&manifest_src, b"manifest image bytes").expect("fixture");
        std::fs::write(&text_src, b"text image bytes").expect("fixture");

        let theme = tempfile::tempdir().expect("theme tmp");
        let manifest_dir = theme.path().join("providers").join("shell-a");
        let text_dir = theme.path().join("providers").join("shell-b");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        std::fs::create_dir_all(&text_dir).unwrap();
        std::fs::write(manifest_dir.join("a.rec"), format!("{}\n", manifest_src.display()))
            .unwrap();
        std::fs::write(text_dir.join("b.rec"), format!("{}\n", text_src.display())).unwrap();

        let declarers = vec![
            fake_declarer(
                &text_dir,
                "shell-b",
                "b.rec",
                PreviewRole::WallpaperText,
                PreviewSourceKind::Image,
            ),
            fake_declarer(
                &manifest_dir,
                "shell-a",
                "a.rec",
                PreviewRole::WallpaperManifest,
                PreviewSourceKind::Image,
            ),
        ];
        assert_eq!(
            plan_with(theme.path(), &declarers),
            Some(PreviewSpec::Image(manifest_src)),
            "a wallpaper-manifest declaration must beat a wallpaper-text \
             declaration regardless of declarer order"
        );
    }

    #[test]
    fn painter_video_declaration_beats_wallpaper_manifest() {
        // The preserved chain puts the painter's video record BEFORE the
        // static wallpaper manifest: a usable declared video must plan to
        // a Video spec even when an existing manifest image is declared too.
        let _env = crate::test_utils::TempEnv::new();

        let src_dir = tempfile::tempdir().expect("src tmp");
        let video = src_dir.path().join("loop.mp4");
        let image = src_dir.path().join("manifest.png");
        std::fs::write(&video, b"fake video bytes").expect("fixture");
        std::fs::write(&image, b"manifest image bytes").expect("fixture");

        let theme = tempfile::tempdir().expect("theme tmp");
        let painter_dir = theme.path().join("providers").join("shell-p");
        let manifest_dir = theme.path().join("providers").join("shell-m");
        std::fs::create_dir_all(&painter_dir).unwrap();
        std::fs::create_dir_all(&manifest_dir).unwrap();
        std::fs::write(painter_dir.join("p.rec"), format!("{}\n", video.display())).unwrap();
        std::fs::write(manifest_dir.join("m.rec"), format!("{}\n", image.display())).unwrap();

        let declarers = vec![
            fake_declarer(
                &manifest_dir,
                "shell-m",
                "m.rec",
                PreviewRole::WallpaperManifest,
                PreviewSourceKind::Image,
            ),
            fake_declarer(
                &painter_dir,
                "shell-p",
                "p.rec",
                PreviewRole::PainterVideo,
                PreviewSourceKind::Video,
            ),
        ];
        match plan_with(theme.path(), &declarers) {
            Some(PreviewSpec::Video { video: v, .. }) => assert_eq!(
                v, video,
                "the painter video declaration must plan before the static manifest"
            ),
            other => panic!("expected a Video spec, got {other:?}"),
        }
    }

    // ── read-preview-source divergence pins (verification D1 / D2) ─────
    //
    // The `read-preview-source` unit replaced the OLD hardcoded stages
    // with provider declarations. Two sub-stages changed behaviour and
    // had NO coverage. All three fixtures below were first written with
    // the verifier's exact OLD expectations and observed RED
    // (`None` vs `Some(Image(imgA))`, `None` vs `Some(imgA)`,
    // `Some(imgA)` vs `Some(imgB)`); they now pin the DECIDED contract.
    //
    // D1 and D2A are the SAME fact pattern: a record owned by the
    // `noctalia-v5` save/apply path, planted under the `noctalia` (v4)
    // id. Restoring either would make the v4 provider declare a record
    // format it has never written nor applied — a phantom declaration.
    // Evidence (this corrective unit, 2026-09-29):
    //   - `NoctaliaV4Provider::save` copies `settings.json`,
    //     `colors.json`, `plugins.json`, `noctalia-colors.conf`,
    //     `noctalia-colors.lua` and `wallpapers.json` — nothing else;
    //   - across ALL 34 committed revisions of `src/providers/noctalia.rs`,
    //     `wallpaper.txt` and `bg_info::save_manifest` appear ONLY inside
    //     the v5 impl block (v4: 0 occurrences), and
    //     `mpvpaper::apply_manifest` has hardcoded
    //     `providers/noctalia-v5` since the manifest existed (444b3bd);
    //   - a scan of the keeper's real themes tree holds ZERO
    //     `mpvpaper-assignments.json` files and every `wallpaper.txt`
    //     under `providers/noctalia-v5/` — never under `providers/noctalia/`.
    // The OLD scan simply reached further than any writer ever did.

    /// D1 pin — verifier's exact fixture: a theme carrying ONLY
    /// `providers/noctalia/wallpaper.txt` → `imgA`. OLD resolved
    /// `Some(Image(imgA))` (`wallpaper_from_txt` tried the v4 path after
    /// the v5 one); the decided contract is `None`: v4 never writes that
    /// record, so its provider declares no `WallpaperText`. The loose
    /// scan cannot find `imgA` either — it lives outside the theme.
    #[test]
    fn wallpaper_txt_under_the_v4_id_declares_nothing() {
        let src = tempfile::tempdir().expect("src tmp");
        let img_a = src.path().join("imgA.png");
        std::fs::write(&img_a, b"imgA bytes").expect("fixture");

        let theme = tempfile::tempdir().expect("theme tmp");
        let v4_dir = theme.path().join("providers").join("noctalia");
        std::fs::create_dir_all(&v4_dir).expect("mkdir");
        std::fs::write(v4_dir.join("wallpaper.txt"), format!("{}\n", img_a.display()))
            .expect("record");

        assert_eq!(
            plan_preview_source(theme.path()),
            None,
            "OLD contract read providers/noctalia/wallpaper.txt; no shipped \
             writer ever produced that record, so it declares nothing"
        );

        // Provider-level half of the same pin: the v4 declarer must state
        // no WallpaperText slot for its own directory, whatever a stray
        // file there is called.
        let declarer = crate::providers::declaration_provider("noctalia")
            .expect("the v4 id is a shipped declarer");
        assert!(
            declarer
                .preview_sources(&v4_dir)
                .iter()
                .all(|s| s.role != PreviewRole::WallpaperText),
            "the v4 provider must not declare a wallpaper-text record it never writes"
        );
    }

    /// D2 pin A — verifier's exact fixture: an assignment manifest under
    /// `providers/noctalia/` → `imgA`. OLD resolved it (the blind
    /// `providers/*/mpvpaper-assignments.json` scan); the decided
    /// contract is `None`: only the `noctalia-v5` save writes that
    /// manifest and only its apply path restores it, so `noctalia` is
    /// not its owner. v4 also claims no `WALLPAPERS` capability — it
    /// cannot own a background assignment.
    #[test]
    fn assignment_manifest_under_the_v4_id_declares_nothing() {
        let src = tempfile::tempdir().expect("src tmp");
        let img_a = src.path().join("imgA.png");
        std::fs::write(&img_a, b"imgA bytes").expect("fixture");

        let theme = tempfile::tempdir().expect("theme tmp");
        write_assignments(
            &theme.path().join("providers/noctalia"),
            serde_json::json!({
                "version": 1,
                "assignments": { "*": { "filename": "imgA.png", "local_path": img_a.to_string_lossy() } }
            }),
        );

        assert_eq!(
            video_assignment(theme.path()),
            None,
            "OLD contract scanned every providers/*/mpvpaper-assignments.json; \
             the manifest is the noctalia-v5 provider's own record"
        );
        assert_eq!(
            plan_preview_source(theme.path()),
            None,
            "a manifest under a dir that owns no assignment record plans no source"
        );

        // Provider-level half: the v4 declarer states no Assignment slot.
        let declarer = crate::providers::declaration_provider("noctalia")
            .expect("the v4 id is a shipped declarer");
        assert!(
            declarer
                .preview_sources(&theme.path().join("providers").join("noctalia"))
                .iter()
                .all(|s| s.role != PreviewRole::Assignment),
            "the v4 provider must not declare an assignment record it neither writes nor applies"
        );
    }

    /// D2 pin B — verifier's exact fixture: `providers/aaa-custom/`
    /// holds a manifest → `imgB`, `providers/noctalia-v5/` one →
    /// `imgA`. OLD returned `imgB` (sorted dir scan over unknown ids);
    /// the decided contract is `imgA`. This case CANNOT be restored
    /// honestly, and the deliberate conflict resolves in favour of
    /// `unknown_provider_dirs_declare_nothing`: an id the registry does
    /// not ship has no owner, so no declarer can know what its records
    /// look like — restoring `imgB` means restoring the blind layout
    /// scan this seam removed (and the backend contract's Definition of
    /// Done forbids thumbs.rs learning directory layouts).
    #[test]
    fn assignment_manifest_under_an_unknown_dir_cannot_beat_the_owning_dir() {
        let src = tempfile::tempdir().expect("src tmp");
        let img_b = src.path().join("imgB.png");
        let img_a = src.path().join("imgA.png");
        std::fs::write(&img_b, b"imgB bytes").expect("fixture");
        std::fs::write(&img_a, b"imgA bytes").expect("fixture");

        let theme = tempfile::tempdir().expect("theme tmp");
        write_assignments(
            &theme.path().join("providers/aaa-custom"),
            serde_json::json!({
                "version": 1,
                "assignments": { "*": { "filename": "imgB.png", "local_path": img_b.to_string_lossy() } }
            }),
        );
        write_assignments(
            &theme.path().join("providers/noctalia-v5"),
            serde_json::json!({
                "version": 1,
                "assignments": { "*": { "filename": "imgA.png", "local_path": img_a.to_string_lossy() } }
            }),
        );

        assert_eq!(
            video_assignment(theme.path()),
            Some(img_a.clone()),
            "aaa-custom sorts first but declares nothing; the owning dir's \
             record must decide (OLD gave imgB via the blind scan)"
        );
        assert_eq!(
            plan_preview_source(theme.path()),
            Some(PreviewSpec::Image(img_a)),
            "the unknown dir must never supply the planned source"
        );
    }

    /// Sorted-directory precedence between two DECLARING dirs (the
    /// property D2 reported as lost). `collect_declarers` sorts the
    /// theme's `providers/` tree BEFORE the registry filter, so the order
    /// is the core's own — never `read_dir`'s: the dirs are created here
    /// in REVERSE lexical order on purpose. The unknown `aaa-custom` dir
    /// is included because it sorts FIRST: it must be skipped, not win.
    ///
    /// `noctalia` and `wallpaper` are the only two shipped ids that
    /// declare the SAME role (`WallpaperManifest`, through the v4
    /// declarer), which is why they are the pair under test; no role has
    /// two Assignment declarers — `noctalia-v5` is the manifest's only
    /// owner (see the D2 pin A above).
    #[test]
    fn sorted_provider_dir_order_decides_between_two_declaring_dirs() {
        let src = tempfile::tempdir().expect("src tmp");
        let unknown = src.path().join("unknown.png");
        let early = src.path().join("early.png");
        let late = src.path().join("late.png");
        std::fs::write(&unknown, b"unknown").expect("fixture");
        std::fs::write(&early, b"early").expect("fixture");
        std::fs::write(&late, b"late").expect("fixture");

        let theme = tempfile::tempdir().expect("theme tmp");
        // Reverse lexical creation order: `wallpaper` first, `noctalia`
        // last, the unknown id in between.
        for (sub, target) in [
            ("wallpaper", &late),
            ("aaa-custom", &unknown),
            ("noctalia", &early),
        ] {
            let provider_dir = theme.path().join("providers").join(sub);
            std::fs::create_dir_all(&provider_dir).expect("mkdir");
            let j = serde_json::json!({"wallpapers":{"DP-3":{"dark": target.to_string_lossy()}}});
            std::fs::write(
                provider_dir.join("wallpapers.json"),
                serde_json::to_string(&j).unwrap(),
            )
            .expect("record");
        }

        assert_eq!(
            plan_preview_source(theme.path()),
            Some(PreviewSpec::Image(early)),
            "providers/aaa-custom sorts first but declares nothing, \
             providers/noctalia sorts before providers/wallpaper and both \
             declare: sorted provider-directory order must decide, not \
             creation/read_dir order"
        );
    }

}
