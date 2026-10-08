//! Theme media packaging (theme-packages T1): a saved theme carries its own
//! background under `{theme_dir}/media/`, so copying the theme folder moves
//! the whole look. The providers keep their existing ABSOLUTE records
//! untouched (old themes on disk keep resolving exactly as before); a small
//! sibling record points at the theme-relative poster this module writes.
//!
//! Placement rule agreed for T1/T2/T3: theme media lives at
//! `{theme_dir}/media/`, records become theme-relative, absolute records stay
//! readable. Packaging is BEST-EFFORT by contract: a missing source, an
//! absent ffmpeg or a failed extraction yields `None` with a warning and must
//! never fail a save.

use std::path::{Path, PathBuf};

/// Theme-relative directory that carries the packaged media.
pub const MEDIA_DIR: &str = "media";

/// Record file, inside a provider dir, naming the theme-relative poster.
pub const POSTER_RECORD_FILE: &str = "poster.txt";

/// Record key for the poster path (`poster=media/poster.jpg`).
const POSTER_KEY: &str = "poster=";

/// Canonical poster file name for a video source (always JPEG).
const VIDEO_POSTER_NAME: &str = "poster.jpg";

/// File stem shared by every packaged poster (`poster.jpg`, `poster.png`).
const POSTER_STEM: &str = "poster";

/// Package `source` as the theme's own poster under `{theme_dir}/media/` and
/// return the theme-relative path for the record (see
/// [`POSTER_RECORD_FILE`]).
///
/// - A video source is frame-extracted as high-quality JPEG at the video's
///   NATIVE resolution (no scaling) and named `media/poster.jpg`.
/// - A static image is copied byte-for-byte, keeping its real extension
///   (`media/poster.jpg` for a JPEG, `media/poster.png` for a PNG, and so on),
///   so the record always points at the file that actually holds the bytes.
///
/// `None` (with a warning) when the source is missing or nothing usable could
/// be produced — the caller still succeeds. The save path then removes any
/// stale poster via [`remove_theme_poster`], so the theme never keeps a poster
/// it no longer declares; a successful write is done through a temporary file
/// so a partial artifact is never published.
pub fn write_theme_poster(theme_dir: &Path, source: &Path) -> Option<PathBuf> {
    if !source.is_file() {
        tracing::warn!(
            "[theme-media] background source missing ({}); save continues without a packaged poster",
            source.display()
        );
        return None;
    }
    let media_dir = theme_dir.join(MEDIA_DIR);
    if let Err(e) = std::fs::create_dir_all(&media_dir) {
        tracing::warn!(
            "[theme-media] cannot create media dir ({}): {}",
            media_dir.display(),
            e
        );
        return None;
    }

    let is_video = crate::shell::gallery::thumbs::is_video_source(source);
    let name = if is_video {
        VIDEO_POSTER_NAME.to_string()
    } else {
        poster_file_name(source)
    };
    let out = media_dir.join(&name);
    // Stage through a temporary file so a failed extraction never publishes a
    // partial poster and never destroys a previously packaged one. The temp
    // name KEEPS the final extension: ffmpeg picks its muxer from it.
    let tmp = media_dir.join(format!(".part-{name}"));
    let _ = std::fs::remove_file(&tmp);

    let written = if is_video {
        crate::shell::gallery::thumbs::extract_video_poster_jpeg(source, &tmp)
    } else {
        std::fs::copy(source, &tmp).is_ok()
    };
    if !written {
        tracing::warn!(
            "[theme-media] cannot package background {}; save continues without a poster",
            source.display()
        );
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    if let Err(e) = std::fs::rename(&tmp, &out) {
        tracing::warn!(
            "[theme-media] cannot publish poster {}: {}",
            out.display(),
            e
        );
        let _ = std::fs::remove_file(&tmp);
        return None;
    }

    remove_stale_posters(&media_dir, Some(&name));
    tracing::info!("[theme-media] packaged theme poster: {}", out.display());
    Some(Path::new(MEDIA_DIR).join(&name))
}

/// The on-disk name for a STATIC image: a JPEG keeps the canonical
/// `poster.jpg`; every other format keeps its own lowercased extension so the
/// bytes are never mislabelled. A source without an extension falls back to
/// `poster.jpg` (the record still points at whatever was written).
fn poster_file_name(source: &Path) -> String {
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("jpg") | Some("jpeg") | None => VIDEO_POSTER_NAME.to_string(),
        Some(other) => format!("poster.{other}"),
    }
}

/// Drop every packaged poster whose name differs from `keep` (or every
/// packaged poster at all when `keep` is `None`), so a re-save never leaves
/// two backgrounds behind and a failed re-package never leaves a stale one.
/// Best-effort: removal failures only mean a stale file survives, never a
/// broken save.
fn remove_stale_posters(media_dir: &Path, keep: Option<&str>) {
    let Ok(entries) = std::fs::read_dir(media_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str());
        if name.is_some() && name == keep {
            continue;
        }
        let is_poster = path.file_stem().and_then(|s| s.to_str()) == Some(POSTER_STEM)
            && path.extension().is_some();
        if is_poster && path.is_file() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Drop the theme's packaged poster entirely: the [`POSTER_RECORD_FILE`]
/// record and every `media/poster.*` file. Called when a save could not
/// (re)package a background — a missing source, a missing ffmpeg, a failed
/// extraction, or no background source at all — so the theme invariant holds:
/// `poster.txt` and `media/poster.*` describe the theme's CURRENT background,
/// or they do not exist. Apply then falls back to the normal chain instead of
/// painting a stale image. Best-effort, like every media removal here.
pub fn remove_theme_poster(theme_dir: &Path, provider_dir: &Path) {
    let _ = std::fs::remove_file(provider_dir.join(POSTER_RECORD_FILE));
    remove_stale_posters(&theme_dir.join(MEDIA_DIR), None);
}

/// Write the theme-relative poster record into `provider_dir`. Callers
/// downgrade a failure to a warning: the poster is best-effort and a record
/// problem must never fail a save.
pub fn write_poster_record(provider_dir: &Path, relative: &Path) -> Result<(), String> {
    std::fs::create_dir_all(provider_dir)
        .map_err(|e| format!("Cannot create provider dir: {}", e))?;
    std::fs::write(
        provider_dir.join(POSTER_RECORD_FILE),
        format!("{}{}\n", POSTER_KEY, relative.display()),
    )
    .map_err(|e| format!("Cannot write {}: {}", POSTER_RECORD_FILE, e))
}

/// Parse a poster record back to the theme-relative path. `None` when no
/// usable `poster=` line exists; an ABSOLUTE path is rejected because a
/// theme record must stay inside the theme (resolving it against the theme
/// dir is the only correct reading). A `..` component is rejected too: it
/// would escape the theme folder once resolved, so a tampered record can
/// never reach an arbitrary file. An invalid record is treated exactly like
/// a missing one — the caller falls back to the normal chain.
///
/// The read seam for T2's theme-first apply resolution; exercised by tests
/// until that task lands.
pub fn parse_poster_record(text: &str) -> Option<PathBuf> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix(POSTER_KEY)?.trim();
        if rest.is_empty() {
            return None;
        }
        let path = PathBuf::from(rest);
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            None
        } else {
            Some(path)
        }
    })
}

/// Resolve the theme's packaged poster for APPLY, as an ABSOLUTE path.
///
/// Reads the theme-relative [`POSTER_RECORD_FILE`] from `provider_dir`, and
/// when it names a valid (non-absolute, non-traversing) file that actually
/// exists inside `theme_dir`, returns `theme_dir.join(record)` resolved at
/// apply time. This is what lets a theme copied to another machine paint its
/// own background even though the absolute record it also carries points at
/// a path that does not exist there.
///
/// `None` — treated exactly like "no packaged poster" — when the record is
/// missing, invalid, absolute, traversing, or names a file that is not on
/// disk. A `media/` file with no record is deliberately ignored: a leftover
/// artifact must never resurrect a background the theme no longer declares.
pub fn resolve_theme_poster(theme_dir: &Path, provider_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(provider_dir.join(POSTER_RECORD_FILE)).ok()?;
    let relative = parse_poster_record(&text)?;
    let absolute = theme_dir.join(&relative);
    if absolute.is_file() {
        Some(absolute)
    } else {
        None
    }
}

// ── Theme video packaging (theme-packages T3) ────────────────────────────
//
// A theme may carry its video, but only when that is sensible. A video at or
// under [`MAX_PACKAGED_VIDEO_BYTES`] is copied into the theme as
// `media/background.mp4` and named by a theme-relative record
// ([`VIDEO_RECORD_FILE`]); a larger one is never copied — the url the theme's
// mpvpaper manifest already knows is recorded instead, so the video can still
// be fetched by the SAME plugin path as any saved manifest. No url means the
// theme travels with its poster alone. Best-effort, like the poster: a missing
// file, an over-bound video or a failed copy only warns and never fails a
// save.

/// Largest video HVE copies into a theme package, in bytes.
///
/// GitHub rejects files over 100 MB, so a theme's video must stay below that
/// to ship in the repository (theme-packages T5). 95 MB leaves a 5 MB safety
/// margin for repository overhead and metadata drift. A larger video is never
/// copied; the theme records a `url` instead when one is known, and otherwise
/// travels with its poster alone.
pub const MAX_PACKAGED_VIDEO_BYTES: u64 = 95 * 1024 * 1024;

/// The production packaging bound: always [`MAX_PACKAGED_VIDEO_BYTES`].
///
/// The environment is deliberately never consulted here. A stray
/// `HVE_MAX_PACKAGED_VIDEO_BYTES` in the keeper's shell must not be able to
/// widen the 95 MB guard that keeps a theme's video under GitHub's 100 MB
/// limit; the override below is reachable only from tests.
pub fn max_packaged_video_bytes() -> u64 {
    MAX_PACKAGED_VIDEO_BYTES
}

/// Test-only override seam for [`MAX_PACKAGED_VIDEO_BYTES`]: the size decision
/// is exercised with tiny files instead of a real 95 MB video. Absent or
/// unparseable -> the constant. Not a user setting: there is no UI toggle for
/// video packaging.
#[cfg(test)]
const MAX_PACKAGED_VIDEO_ENV: &str = "HVE_MAX_PACKAGED_VIDEO_BYTES";

/// Test-only packaging bound: the environment override when set, else
/// [`MAX_PACKAGED_VIDEO_BYTES`]. Compiled only under `cfg(test)`, so the
/// production save path can never reach it.
#[cfg(test)]
pub fn test_packaged_video_bytes() -> u64 {
    std::env::var(MAX_PACKAGED_VIDEO_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(MAX_PACKAGED_VIDEO_BYTES)
}

/// Canonical packaged video file name (always MP4, matching the theme layout).
pub const VIDEO_FILE_NAME: &str = "background.mp4";

/// Record file, inside a provider dir, naming the theme-relative video, or the
/// url when the video was too large to copy (`video=media/background.mp4` or
/// `url=https://…`). Mirrors the poster's [`POSTER_RECORD_FILE`].
pub const VIDEO_RECORD_FILE: &str = "video-media.txt";

/// Record key for the packaged video path (`video=media/background.mp4`).
const VIDEO_KEY: &str = "video=";

/// Record key for the fallback url (`url=https://…`).
const URL_KEY: &str = "url=";

/// Optional record keys carried alongside the url so the mpvpaper backend can
/// rebuild its own assignment (`filename=` / `sha256=`).
const FILENAME_KEY: &str = "filename=";
const SHA256_KEY: &str = "sha256=";

/// Pure size decision: a video at or UNDER `max_bytes` fits packaging. The
/// bound is a parameter so the rule is testable with tiny values, never with a
/// real large file.
pub fn video_fits_for_packaging(size: u64, max_bytes: u64) -> bool {
    size <= max_bytes
}

/// Package `source` as the theme's own video under `{theme_dir}/media/` and
/// return the theme-relative path for the record, or `None` (with a warning)
/// when the source is missing, exceeds `max_bytes`, or cannot be copied.
///
/// The copy is staged through a temporary file, so a failed copy never
/// publishes a partial video and never destroys a previously packaged one.
/// Best-effort by contract: the caller's save still succeeds.
pub fn package_theme_video(theme_dir: &Path, source: &Path, max_bytes: u64) -> Option<PathBuf> {
    if !source.is_file() {
        tracing::warn!(
            "[theme-media] video source missing ({}); save continues without a packaged video",
            source.display()
        );
        return None;
    }
    let size = std::fs::metadata(source).map(|m| m.len()).unwrap_or(u64::MAX);
    if !video_fits_for_packaging(size, max_bytes) {
        tracing::warn!(
            "[theme-media] video {} is {} bytes, over the {} byte packaging bound; not copied",
            source.display(),
            size,
            max_bytes
        );
        return None;
    }
    let media_dir = theme_dir.join(MEDIA_DIR);
    if let Err(e) = std::fs::create_dir_all(&media_dir) {
        tracing::warn!(
            "[theme-media] cannot create media dir ({}): {}",
            media_dir.display(),
            e
        );
        return None;
    }
    let out = media_dir.join(VIDEO_FILE_NAME);
    // Stage through a temporary file so a failed copy never publishes a
    // partial video and never destroys a previously packaged one.
    let tmp = media_dir.join(format!(".part-{VIDEO_FILE_NAME}"));
    let _ = std::fs::remove_file(&tmp);
    if let Err(e) = std::fs::copy(source, &tmp) {
        tracing::warn!(
            "[theme-media] cannot copy video {} into the theme: {}",
            source.display(),
            e
        );
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    if let Err(e) = std::fs::rename(&tmp, &out) {
        tracing::warn!(
            "[theme-media] cannot publish video {}: {}",
            out.display(),
            e
        );
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    tracing::info!("[theme-media] packaged theme video: {}", out.display());
    Some(Path::new(MEDIA_DIR).join(VIDEO_FILE_NAME))
}

/// Write the theme-relative packaged-video record into `provider_dir`. Callers
/// downgrade a failure to a warning: a record problem must never fail a save.
pub fn write_video_record(provider_dir: &Path, relative: &Path) -> Result<(), String> {
    std::fs::create_dir_all(provider_dir)
        .map_err(|e| format!("Cannot create provider dir: {}", e))?;
    std::fs::write(
        provider_dir.join(VIDEO_RECORD_FILE),
        format!("{}{}\n", VIDEO_KEY, relative.display()),
    )
    .map_err(|e| format!("Cannot write {}: {}", VIDEO_RECORD_FILE, e))
}

/// Write the url record (with optional filename/sha256) into `provider_dir` for
/// a video that was too large to copy. Callers downgrade a failure to a
/// warning.
pub fn write_video_url_record(
    provider_dir: &Path,
    url: &str,
    filename: Option<&str>,
    sha256: Option<&str>,
) -> Result<(), String> {
    std::fs::create_dir_all(provider_dir)
        .map_err(|e| format!("Cannot create provider dir: {}", e))?;
    let mut text = format!("{}{}\n", URL_KEY, url.trim());
    if let Some(filename) = filename.map(str::trim).filter(|f| !f.is_empty()) {
        text.push_str(&format!("{}{}\n", FILENAME_KEY, filename));
    }
    if let Some(sha256) = sha256.map(str::trim).filter(|s| !s.is_empty()) {
        text.push_str(&format!("{}{}\n", SHA256_KEY, sha256));
    }
    std::fs::write(provider_dir.join(VIDEO_RECORD_FILE), text)
        .map_err(|e| format!("Cannot write {}: {}", VIDEO_RECORD_FILE, e))
}

/// The parsed [`VIDEO_RECORD_FILE`]: a packaged theme-relative path, a url, or
/// both (the url is the fallback when the packaged file is not on disk).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ThemeVideoRecord {
    pub packaged: Option<PathBuf>,
    pub url: Option<String>,
    pub filename: Option<String>,
    pub sha256: Option<String>,
}

/// Parse a video record. An absolute or `..`-traversing packaged path is
/// rejected (the record must stay inside the theme), exactly like the poster
/// record; the url and its optional metadata are kept verbatim.
pub fn parse_video_record(text: &str) -> ThemeVideoRecord {
    let mut record = ThemeVideoRecord::default();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(VIDEO_KEY) {
            let rest = rest.trim();
            if rest.is_empty() {
                continue;
            }
            let path = PathBuf::from(rest);
            if path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                continue;
            }
            record.packaged = Some(path);
        } else if let Some(rest) = line.strip_prefix(URL_KEY) {
            let rest = rest.trim();
            if !rest.is_empty() {
                record.url = Some(rest.to_string());
            }
        } else if let Some(rest) = line.strip_prefix(FILENAME_KEY) {
            let rest = rest.trim();
            if !rest.is_empty() {
                record.filename = Some(rest.to_string());
            }
        } else if let Some(rest) = line.strip_prefix(SHA256_KEY) {
            let rest = rest.trim();
            if !rest.is_empty() {
                record.sha256 = Some(rest.to_string());
            }
        }
    }
    record
}

/// The resolved video source for APPLY, theme-first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeVideoSource {
    /// An absolute path on disk (the packaged `media/background.mp4` or the
    /// legacy absolute `video.txt` path).
    Path(PathBuf),
    /// A remote url the theme's manifest knows, with optional metadata the
    /// mpvpaper backend needs to rebuild its assignment.
    Url {
        url: String,
        filename: Option<String>,
        sha256: Option<String>,
    },
}

/// Resolve the theme's video for APPLY: the packaged `media/background.mp4`
/// first, then the existing absolute `video.txt` path, then a recorded url.
/// `None` when nothing usable exists — the caller keeps its normal chain.
pub fn resolve_theme_video(
    theme_dir: &Path,
    provider_dir: &Path,
    absolute: Option<&Path>,
) -> Option<ThemeVideoSource> {
    let record = std::fs::read_to_string(provider_dir.join(VIDEO_RECORD_FILE))
        .ok()
        .map(|text| parse_video_record(&text))
        .unwrap_or_default();
    if let Some(relative) = record.packaged {
        let packaged = theme_dir.join(&relative);
        if packaged.is_file() {
            return Some(ThemeVideoSource::Path(packaged));
        }
    }
    if let Some(absolute) = absolute {
        if absolute.is_file() {
            return Some(ThemeVideoSource::Path(absolute.to_path_buf()));
        }
    }
    if let Some(url) = record.url {
        return Some(ThemeVideoSource::Url {
            url,
            filename: record.filename,
            sha256: record.sha256,
        });
    }
    None
}

/// Drop the theme's packaged video entirely: the [`VIDEO_RECORD_FILE`] record
/// and `media/background.mp4`. Called when the background is not a video, or a
/// video could not be packaged and has no known url, so the invariant holds:
/// the record and the file describe the theme's CURRENT video, or they do not
/// exist. Best-effort.
pub fn remove_theme_video(theme_dir: &Path, provider_dir: &Path) {
    let _ = std::fs::remove_file(provider_dir.join(VIDEO_RECORD_FILE));
    let _ = std::fs::remove_file(theme_dir.join(MEDIA_DIR).join(VIDEO_FILE_NAME));
}

/// Every background path the applied theme declares as its own, as the
/// absolute paths the engine would report for them: the packaged poster, the
/// video (packaged first, then the legacy absolute record) and the saved
/// static wallpaper. Order is poster, video, wallpaper; duplicates are
/// dropped.
///
/// W2 of `odd/tasks/palette-authority-mute-and-yield.md`: the detector
/// compares the engine's live background against this set, so a path outside
/// it is the keeper's own change. An unreadable or absent record simply
/// contributes nothing — the set is a declaration, never a guess.
pub fn declared_backgrounds(theme_dir: &Path, provider_dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(poster) = resolve_theme_poster(theme_dir, provider_dir) {
        push_unique(&mut out, poster);
    }
    let legacy = legacy_painter_video_path(provider_dir);
    match resolve_theme_video(theme_dir, provider_dir, legacy.as_deref()) {
        Some(ThemeVideoSource::Path(path)) => push_unique(&mut out, path),
        // A url is not something the engine paints from disk, so it is not a
        // live background path to compare against.
        Some(ThemeVideoSource::Url { .. }) | None => {}
    }
    if let Ok(text) = std::fs::read_to_string(provider_dir.join(STATIC_RECORD_FILE)) {
        let wallpaper = text.trim();
        if !wallpaper.is_empty() {
            push_unique(&mut out, PathBuf::from(wallpaper));
        }
    }
    out
}

/// Record file the provider writes for a saved static wallpaper (mirrors the
/// provider's own layout; declared here so this core module never depends on
/// a provider module).
const STATIC_RECORD_FILE: &str = "wallpaper.txt";

/// The legacy painter-video record's absolute path (`path=<abs>`). Parsed
/// inline so this core module never reaches into a provider module; a
/// relative path is rejected exactly like the provider's own parser.
fn legacy_painter_video_path(provider_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(provider_dir.join("video.txt")).ok()?;
    text.lines().find_map(|line| {
        let rest = line.strip_prefix("path=")?.trim();
        if rest.is_empty() {
            return None;
        }
        let path = PathBuf::from(rest);
        if path.is_absolute() {
            Some(path)
        } else {
            None
        }
    })
}

/// Append `path` unless an identical entry is already present.
fn push_unique(out: &mut Vec<String>, path: PathBuf) {
    let rendered = path.display().to_string();
    if !out.iter().any(|existing| existing == &rendered) {
        out.push(rendered);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffmpeg_available() -> bool {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn synth_video(path: &Path) -> bool {
        std::process::Command::new("ffmpeg")
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=5:size=320x240:rate=10",
                "-y",
            ])
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn write_theme_poster_copies_static_jpeg_bytes_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("wall.jpg");
        let bytes = b"\xff\xd8\xff\xe0posted-bytes-are-not-re-encoded".to_vec();
        std::fs::write(&src, &bytes).unwrap();

        let theme = dir.path().join("theme");
        let rel = write_theme_poster(&theme, &src).expect("a static image must be packaged");

        assert_eq!(
            rel,
            PathBuf::from("media/poster.jpg"),
            "a JPEG source keeps the canonical poster.jpg name"
        );
        assert_eq!(
            std::fs::read(theme.join(&rel)).unwrap(),
            bytes,
            "static image bytes must be copied unchanged"
        );
    }

    #[test]
    fn write_theme_poster_keeps_the_static_image_real_format() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("art.png");
        let bytes = b"png-bytes-copied-verbatim".to_vec();
        std::fs::write(&src, &bytes).unwrap();

        let theme = dir.path().join("theme");
        let rel = write_theme_poster(&theme, &src).expect("a static image must be packaged");

        assert_eq!(
            rel,
            PathBuf::from("media/poster.png"),
            "a non-JPEG static image must be named for its real format"
        );
        assert_eq!(
            std::fs::read(theme.join(&rel)).unwrap(),
            bytes,
            "a non-JPEG static image must be copied byte-for-byte"
        );
    }

    #[test]
    fn write_theme_poster_extracts_video_frame_as_native_jpeg() {
        if !ffmpeg_available() {
            println!("ffmpeg not available — skipping video poster extraction test");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("clip.mp4");
        assert!(synth_video(&video), "failed to synthesize the fixture video");

        let theme = dir.path().join("theme");
        let rel = write_theme_poster(&theme, &video).expect("a video must yield a poster");

        assert_eq!(rel, PathBuf::from("media/poster.jpg"));
        let out = theme.join(&rel);
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Jpeg,
            "a video poster must be JPEG"
        );
        let decoded = image::ImageReader::open(&out).unwrap().decode().unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            (320, 240),
            "the poster must keep the video's native resolution (no scaling)"
        );
    }

    #[test]
    fn write_theme_poster_returns_none_for_a_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        assert!(
            write_theme_poster(&theme, &dir.path().join("nope.png")).is_none(),
            "a missing source must yield no poster"
        );
        assert!(
            !theme.join(MEDIA_DIR).exists(),
            "a missing source must not even create the media dir"
        );
    }

    #[test]
    #[serial_test::serial]
    fn write_theme_poster_returns_none_when_ffmpeg_is_unavailable() {
        // PATH is a process-global: take the shared env lock so parallel tests
        // that probe ffmpeg availability cannot observe the emptied PATH.
        let _env = crate::test_utils::env_guard();
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("clip.mp4");
        std::fs::write(&video, b"not a video").unwrap();
        let theme = dir.path().join("theme");
        let empty_bin = dir.path().join("empty-bin");
        std::fs::create_dir_all(&empty_bin).unwrap();

        let old_path = std::env::var("PATH").ok();
        std::env::set_var("PATH", &empty_bin);
        let result = write_theme_poster(&theme, &video);
        match old_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }

        assert!(
            result.is_none(),
            "without ffmpeg a video must yield no poster instead of an error"
        );
        assert!(
            !theme.join(MEDIA_DIR).join(VIDEO_POSTER_NAME).exists(),
            "no ffmpeg means no poster artifact on disk"
        );
    }

    #[test]
    fn poster_record_round_trips_the_theme_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        write_poster_record(dir.path(), Path::new("media/poster.jpg")).unwrap();

        let text = std::fs::read_to_string(dir.path().join(POSTER_RECORD_FILE)).unwrap();
        assert_eq!(text.trim(), "poster=media/poster.jpg");
        assert_eq!(
            parse_poster_record(&text),
            Some(PathBuf::from("media/poster.jpg")),
            "the record must parse back to the theme-relative path"
        );
    }

    #[test]
    fn parse_poster_record_rejects_an_absolute_path() {
        assert_eq!(parse_poster_record("poster=/etc/passwd\n"), None);
        assert_eq!(parse_poster_record(""), None);
        assert_eq!(parse_poster_record("poster=\n"), None);
    }

    #[test]
    fn parse_poster_record_rejects_traversal_outside_the_theme() {
        // T2 security guard: a theme record must stay inside the theme
        // folder. `..` components escape it, so a value that carries one —
        // even nested — is refused instead of being resolved against the
        // theme dir.
        assert_eq!(parse_poster_record("poster=../../etc/passwd\n"), None);
        assert_eq!(parse_poster_record("poster=media/../../etc/passwd\n"), None);
        assert_eq!(parse_poster_record("poster=../sibling.jpg\n"), None);
        // A normal nested relative path stays valid.
        assert_eq!(
            parse_poster_record("poster=media/poster.jpg\n"),
            Some(PathBuf::from("media/poster.jpg"))
        );
    }

    /// The "other machine" case, unit level: the record names a file inside
    /// the theme and the resolution is absolute and real, so the caller can
    /// hand it to a shell/engine that has no idea where the theme came from.
    #[test]
    fn resolve_theme_poster_resolves_the_record_inside_the_theme() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        let media = theme.join(MEDIA_DIR);
        std::fs::create_dir_all(&media).unwrap();
        let bytes = b"\xff\xd8\xffpackaged-poster-bytes".to_vec();
        std::fs::write(media.join("poster.jpg"), &bytes).unwrap();
        write_poster_record(&provider, Path::new("media/poster.jpg")).unwrap();

        let resolved =
            resolve_theme_poster(&theme, &provider).expect("a packaged poster must resolve");

        assert!(resolved.is_absolute(), "apply needs an absolute path");
        assert_eq!(resolved, media.join("poster.jpg"));
        assert_eq!(std::fs::read(&resolved).unwrap(), bytes);
    }

    #[test]
    fn resolve_theme_poster_ignores_a_stray_poster_without_a_record() {
        // T2 requirement 5: a leftover `media/poster.*` from an older save
        // must never resurrect a background the theme no longer declares.
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        let media = theme.join(MEDIA_DIR);
        std::fs::create_dir_all(&media).unwrap();
        std::fs::create_dir_all(&provider).unwrap();
        std::fs::write(media.join("poster.jpg"), b"stray leftover").unwrap();

        assert_eq!(
            resolve_theme_poster(&theme, &provider),
            None,
            "media without a record must be ignored"
        );
    }

    #[test]
    fn resolve_theme_poster_none_when_the_declared_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        write_poster_record(&provider, Path::new("media/gone.jpg")).unwrap();

        assert_eq!(
            resolve_theme_poster(&theme, &provider),
            None,
            "a declared poster with no file must fall back to the normal chain"
        );
    }

    #[test]
    fn resolve_theme_poster_rejects_a_traversal_record_even_when_the_target_exists() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        let secret = dir.path().join("secret.jpg");
        std::fs::write(&secret, b"outside the theme").unwrap();
        // theme/providers/noctalia-v5/ + ../../../ -> dir
        write_poster_record(&provider, Path::new("../../../secret.jpg")).unwrap();

        assert_eq!(
            resolve_theme_poster(&theme, &provider),
            None,
            "a traversing record must never reach a file outside the theme"
        );
    }

    #[test]
    fn write_theme_poster_replaces_a_stale_poster_of_another_format() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let media = theme.join(MEDIA_DIR);
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("poster.png"), b"stale old poster").unwrap();

        let src = dir.path().join("wall.jpg");
        std::fs::write(&src, b"fresh jpeg bytes").unwrap();
        let rel = write_theme_poster(&theme, &src).expect("a static image must be packaged");

        assert_eq!(rel, PathBuf::from("media/poster.jpg"));
        assert!(
            !media.join("poster.png").exists(),
            "a successful write must drop the stale poster of another format"
        );
        assert_eq!(std::fs::read(theme.join(&rel)).unwrap(), b"fresh jpeg bytes");
    }

    // ── Theme packages T3: the video is optional and size-aware ───────

    #[test]
    fn video_fits_for_packaging_is_inclusive_at_the_bound() {
        assert!(
            video_fits_for_packaging(95, 95),
            "a video exactly at the bound must fit (at or under)"
        );
        assert!(
            video_fits_for_packaging(94, 95),
            "a video under the bound must fit"
        );
        assert!(
            !video_fits_for_packaging(96, 95),
            "a video over the bound must not fit"
        );
    }

    /// The 95 MB guard is a hard bound: a stray `HVE_MAX_PACKAGED_VIDEO_BYTES`
    /// in the keeper's shell must not be able to widen it. The production
    /// resolver ignores the environment entirely; the override is reachable
    /// only from tests (see `test_packaged_video_bytes`).
    #[test]
    #[serial_test::serial]
    fn max_packaged_video_bytes_ignores_the_environment() {
        let _env = crate::test_utils::env_guard();
        let previous = std::env::var(MAX_PACKAGED_VIDEO_ENV).ok();
        std::env::set_var(MAX_PACKAGED_VIDEO_ENV, "999999999");
        let bound = max_packaged_video_bytes();
        match previous {
            Some(v) => std::env::set_var(MAX_PACKAGED_VIDEO_ENV, v),
            None => std::env::remove_var(MAX_PACKAGED_VIDEO_ENV),
        }
        assert_eq!(
            bound, MAX_PACKAGED_VIDEO_BYTES,
            "the production bound must be the 95 MB constant even when the env var is set"
        );
    }

    #[test]
    fn package_theme_video_copies_a_video_at_or_under_the_bound() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("clip.mp4");
        let bytes = b"tiny-video-bytes".to_vec();
        std::fs::write(&src, &bytes).unwrap();

        let theme = dir.path().join("theme");
        let rel = package_theme_video(&theme, &src, 1024)
            .expect("a video under the bound must be packaged");

        assert_eq!(rel, PathBuf::from("media/background.mp4"));
        assert_eq!(
            std::fs::read(theme.join(&rel)).unwrap(),
            bytes,
            "the packaged video must be a byte copy"
        );
    }

    #[test]
    fn package_theme_video_skips_a_video_over_the_bound() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("clip.mp4");
        std::fs::write(&src, b"five!").unwrap(); // 5 bytes

        let theme = dir.path().join("theme");
        assert!(
            package_theme_video(&theme, &src, 4).is_none(),
            "a video over the bound must never be copied"
        );
        assert!(
            !theme.join(MEDIA_DIR).join(VIDEO_FILE_NAME).exists(),
            "an over-bound video must leave no packaged file"
        );
    }

    #[test]
    fn package_theme_video_returns_none_for_a_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        assert!(
            package_theme_video(&theme, &dir.path().join("nope.mp4"), 1024).is_none(),
            "a missing source must yield no packaged video"
        );
        assert!(
            !theme.join(MEDIA_DIR).join(VIDEO_FILE_NAME).exists(),
            "a missing source must not publish a video file"
        );
    }

    #[test]
    fn video_record_round_trips_the_packaged_path() {
        let dir = tempfile::tempdir().unwrap();
        write_video_record(dir.path(), Path::new("media/background.mp4")).unwrap();

        let text = std::fs::read_to_string(dir.path().join(VIDEO_RECORD_FILE)).unwrap();
        assert_eq!(text.trim(), "video=media/background.mp4");
        let record = parse_video_record(&text);
        assert_eq!(record.packaged, Some(PathBuf::from("media/background.mp4")));
        assert_eq!(record.url, None);
    }

    #[test]
    fn video_record_round_trips_the_url_and_its_metadata() {
        let dir = tempfile::tempdir().unwrap();
        write_video_url_record(
            dir.path(),
            "https://example.com/clip.mp4",
            Some("clip.mp4"),
            Some("abc123"),
        )
        .unwrap();

        let text = std::fs::read_to_string(dir.path().join(VIDEO_RECORD_FILE)).unwrap();
        assert!(text.contains("url=https://example.com/clip.mp4"), "{text}");
        assert!(text.contains("filename=clip.mp4"), "{text}");
        assert!(text.contains("sha256=abc123"), "{text}");
        let record = parse_video_record(&text);
        assert_eq!(record.packaged, None);
        assert_eq!(record.url.as_deref(), Some("https://example.com/clip.mp4"));
        assert_eq!(record.filename.as_deref(), Some("clip.mp4"));
        assert_eq!(record.sha256.as_deref(), Some("abc123"));
    }

    #[test]
    fn parse_video_record_rejects_absolute_and_traversal_packaged_paths() {
        assert_eq!(parse_video_record("video=/etc/passwd\n").packaged, None);
        assert_eq!(parse_video_record("video=\n").packaged, None);
        assert_eq!(
            parse_video_record("video=../../etc/passwd\n").packaged,
            None
        );
        assert_eq!(
            parse_video_record("video=media/../../etc/passwd\n").packaged,
            None
        );
        assert_eq!(
            parse_video_record("video=media/background.mp4\n").packaged,
            Some(PathBuf::from("media/background.mp4"))
        );
    }

    #[test]
    fn resolve_theme_video_prefers_the_packaged_file() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        let media = theme.join(MEDIA_DIR);
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join(VIDEO_FILE_NAME), b"packaged video").unwrap();
        write_video_record(&provider, Path::new("media/background.mp4")).unwrap();
        let legacy = dir.path().join("legacy.mp4");
        std::fs::write(&legacy, b"legacy video").unwrap();

        let resolved = resolve_theme_video(&theme, &provider, Some(&legacy))
            .expect("the packaged video must resolve");
        assert_eq!(
            resolved,
            ThemeVideoSource::Path(media.join(VIDEO_FILE_NAME)),
            "the packaged theme video must win over the absolute path"
        );
    }

    #[test]
    fn resolve_theme_video_falls_back_to_the_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        let legacy = dir.path().join("legacy.mp4");
        std::fs::write(&legacy, b"legacy video").unwrap();

        let resolved = resolve_theme_video(&theme, &provider, Some(&legacy))
            .expect("an old theme's absolute path must still resolve");
        assert_eq!(resolved, ThemeVideoSource::Path(legacy));
    }

    #[test]
    fn resolve_theme_video_falls_back_to_the_url() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        write_video_url_record(
            &provider,
            "https://example.com/clip.mp4",
            Some("clip.mp4"),
            None,
        )
        .unwrap();

        let resolved = resolve_theme_video(&theme, &provider, None)
            .expect("a recorded url must resolve when nothing local exists");
        assert_eq!(
            resolved,
            ThemeVideoSource::Url {
                url: "https://example.com/clip.mp4".into(),
                filename: Some("clip.mp4".into()),
                sha256: None,
            }
        );
    }

    #[test]
    fn resolve_theme_video_falls_through_a_missing_packaged_file_to_the_url() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        // A packaged path is declared but its file is gone, and the same
        // record carries the url fallback.
        let text = "video=media/background.mp4\nurl=https://example.com/clip.mp4\n";
        std::fs::create_dir_all(&provider).unwrap();
        std::fs::write(provider.join(VIDEO_RECORD_FILE), text).unwrap();

        let resolved = resolve_theme_video(&theme, &provider, None)
            .expect("the url must be used when the packaged file is missing");
        assert_eq!(
            resolved,
            ThemeVideoSource::Url {
                url: "https://example.com/clip.mp4".into(),
                filename: None,
                sha256: None,
            }
        );
    }

    #[test]
    fn resolve_theme_video_is_none_without_any_record() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        assert_eq!(resolve_theme_video(&theme, &provider, None), None);
    }

    #[test]
    fn remove_theme_video_drops_the_record_and_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        let media = theme.join(MEDIA_DIR);
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join(VIDEO_FILE_NAME), b"packaged video").unwrap();
        write_video_record(&provider, Path::new("media/background.mp4")).unwrap();

        remove_theme_video(&theme, &provider);

        assert!(
            !provider.join(VIDEO_RECORD_FILE).exists(),
            "the video record must be dropped"
        );
        assert!(
            !media.join(VIDEO_FILE_NAME).exists(),
            "the packaged video file must be dropped"
        );
    }

    // ── W2: the background paths the theme declares ───────────────────
    //
    // odd/tasks/palette-authority-mute-and-yield.md W2: "the background
    // change was not HVE's" is defined against what the applied theme
    // declares as its own — the packaged poster, the video (packaged or the
    // legacy absolute record) and the saved static wallpaper. A live engine
    // background outside this set is the keeper's own change.

    /// Every declared record is collected as the absolute path the engine
    /// would show: the packaged poster, the legacy absolute video and the
    /// saved static wallpaper.
    #[test]
    fn declared_backgrounds_collects_poster_video_and_wallpaper() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        let media = theme.join(MEDIA_DIR);
        std::fs::create_dir_all(&media).unwrap();
        let poster = media.join("poster.png");
        std::fs::write(&poster, b"\x89PNG").unwrap();
        write_poster_record(&provider, Path::new("media/poster.png")).unwrap();
        let video = dir.path().join("loop.mp4");
        std::fs::write(&video, b"video").unwrap();
        std::fs::write(
            provider.join("video.txt"),
            format!("path={}\npainter=mpvpaper\n", video.display()),
        )
        .unwrap();
        let wallpaper = dir.path().join("fallback.png");
        std::fs::write(&wallpaper, b"\x89PNG").unwrap();
        std::fs::write(provider.join("wallpaper.txt"), wallpaper.display().to_string()).unwrap();

        let declared = declared_backgrounds(&theme, &provider);

        assert!(declared.contains(&poster.display().to_string()), "{declared:?}");
        assert!(declared.contains(&video.display().to_string()), "{declared:?}");
        assert!(
            declared.contains(&wallpaper.display().to_string()),
            "{declared:?}"
        );
    }

    /// A theme that declares nothing yields an empty set — the detector
    /// then has nothing to compare and must never step aside.
    #[test]
    fn declared_backgrounds_is_empty_without_any_record() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        std::fs::create_dir_all(&provider).unwrap();
        assert!(declared_backgrounds(&theme, &provider).is_empty());
    }

    /// The packaged video wins over a legacy absolute record, exactly like
    /// the apply resolver: the engine is handed the packaged file.
    #[test]
    fn declared_backgrounds_prefers_the_packaged_video() {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        let media = theme.join(MEDIA_DIR);
        std::fs::create_dir_all(&media).unwrap();
        let packaged = media.join(VIDEO_FILE_NAME);
        std::fs::write(&packaged, b"packaged").unwrap();
        write_video_record(&provider, Path::new("media/background.mp4")).unwrap();
        let legacy = dir.path().join("legacy.mp4");
        std::fs::write(&legacy, b"legacy").unwrap();
        std::fs::write(
            provider.join("video.txt"),
            format!("path={}\n", legacy.display()),
        )
        .unwrap();

        let declared = declared_backgrounds(&theme, &provider);

        assert!(
            declared.contains(&packaged.display().to_string()),
            "the packaged video must be declared: {declared:?}"
        );
        assert!(
            !declared.contains(&legacy.display().to_string()),
            "the legacy absolute record must not shadow the packaged video: {declared:?}"
        );
    }
}
