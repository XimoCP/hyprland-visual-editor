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
}
