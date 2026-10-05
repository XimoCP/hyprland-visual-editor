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
/// be produced — the caller still succeeds. A previously packaged poster is
/// left untouched on failure; a successful write is done through a temporary
/// file so a partial artifact is never published.
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

    remove_stale_posters(&media_dir, &name);
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

/// Drop every packaged poster whose name differs from `keep`, so a re-save
/// never leaves two backgrounds behind. Best-effort: removal failures only
/// mean a stale file survives, never a broken save.
fn remove_stale_posters(media_dir: &Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(media_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some(keep) {
            continue;
        }
        let is_poster = path.file_stem().and_then(|s| s.to_str()) == Some(POSTER_STEM)
            && path.extension().is_some();
        if is_poster && path.is_file() {
            let _ = std::fs::remove_file(&path);
        }
    }
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
/// dir is the only correct reading).
///
/// The read seam for T2's theme-first apply resolution; exercised by tests
/// until that task lands.
#[allow(dead_code)]
pub fn parse_poster_record(text: &str) -> Option<PathBuf> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix(POSTER_KEY)?.trim();
        if rest.is_empty() {
            return None;
        }
        let path = PathBuf::from(rest);
        if path.is_absolute() {
            None
        } else {
            Some(path)
        }
    })
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
}
