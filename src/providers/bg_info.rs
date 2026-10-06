//! Shared background (wallpaper) information.
//!
//! Holds the theme-manifest file name, the capture of the live background
//! state, and the user notification that an animated background could not be
//! applied — the pieces BOTH backends (the Noctalia provider and the media
//! backend) may read, so neither depends on the other's internals (the
//! capability-routing rule: a backend never calls another backend).
//!
//! It is NOT provider-neutral: it resolves the live state location through the
//! Noctalia runtime (`noctalia_state_dir`) and its notification copy names a
//! third-party plugin identifier it does not own and must not reword. The
//! module holds no provider-specific POLICY: it decides nothing about
//! enabling, bouncing, delegating to, or skipping any plugin. Callers keep
//! their own policy; this module only records and reports information.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::providers::noctalia_runtime::noctalia_state_dir;

/// File HVE stores inside the theme provider dir.
pub const MANIFEST_FILE: &str = "mpvpaper-assignments.json";

// ── Manifest types (theme side) ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpvpaperVideo {
    /// Base file name, used to detect existing copies in video_directory.
    pub filename: String,
    /// Absolute path from a local save (may be absent in distributed themes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    /// Remote source for distributed themes (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Optional sha256 of the remote file, verified after download.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpvpaperManifest {
    pub version: u32,
    /// connector ("*" = all outputs, or a monitor name) -> video reference.
    pub assignments: HashMap<String, MpvpaperVideo>,
}

// ── Live plugin state (plugin side) ────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct LiveAssignments {
    pub(crate) assignments: HashMap<String, String>,
    #[serde(default, rename = "launchedAsSystemd")]
    pub(crate) launched_as_systemd: HashMap<String, bool>,
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Live `assignments.json` the running plugin supervises.
pub(crate) fn live_assignments_path() -> Option<PathBuf> {
    Some(noctalia_state_dir()?.join("mpvpaper").join("assignments.json"))
}

// ── Save ────────────────────────────────────────────────────────────────

/// Capture the plugin's current assignments into the theme manifest.
///
/// When the plugin has no live state, this is a no-op (theme ends up with no
/// mpvpaper manifest, i.e. a non-animated theme).
pub fn save_manifest(provider_dir: &Path) -> Result<(), String> {
    let Some(src) = live_assignments_path() else {
        return Err("Cannot resolve Noctalia state dir".into());
    };
    if !src.exists() {
        tracing::debug!("[mpvpaper] No live assignments file, skipping manifest");
        return Ok(());
    }

    let raw = fs::read_to_string(&src)
        .map_err(|e| format!("Cannot read live assignments: {}", e))?;
    let live: LiveAssignments = serde_json::from_str(&raw)
        .map_err(|e| format!("Cannot parse live assignments: {}", e))?;

    let mut assignments = HashMap::new();
    for (connector, path) in live.assignments {
        let filename = Path::new(&path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        assignments.insert(
            connector,
            MpvpaperVideo {
                filename,
                local_path: Some(path),
                url: None,
                sha256: None,
            },
        );
    }

    let manifest = MpvpaperManifest { version: 1, assignments };
    fs::create_dir_all(provider_dir)
        .map_err(|e| format!("Cannot create provider dir: {}", e))?;
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Manifest serialization error: {}", e))?;
    fs::write(provider_dir.join(MANIFEST_FILE), json)
        .map_err(|e| format!("Cannot write mpvpaper manifest: {}", e))?;

    tracing::info!("[mpvpaper] Saved manifest with {} assignment(s)", manifest.assignments.len());
    Ok(())
}

// ── Preview (read-preview-source) ───────────────────────────────────────

/// The assignment paths a saved theme manifest STATES, in the record's own
/// precedence order: the wildcard `"*"` assignment first, then the
/// remaining connectors in sorted-key order. Only non-empty `local_path`
/// values are candidates; existence is the CALLER's decision (the gallery
/// core probes it and keeps the first existing candidate). An absent,
/// unreadable or malformed manifest declares nothing — it is not an error,
/// it is a theme without an animated-background record. The record format
/// knowledge lives here, next to [`MANIFEST_FILE`] and the manifest types:
/// callers never parse this file themselves.
pub fn preview_candidates(provider_dir: &Path) -> Vec<PathBuf> {
    let Ok(text) = fs::read_to_string(provider_dir.join(MANIFEST_FILE)) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(assignments) = v.get("assignments").and_then(|a| a.as_object()) else {
        return Vec::new();
    };
    // Wildcard "*" wins; remaining entries keep sorted-key order.
    let mut keys: Vec<&String> = assignments.keys().collect();
    keys.sort();
    keys.sort_by_key(|k| k.as_str() != "*");
    keys.iter()
        .filter_map(|key| {
            assignments
                .get(key.as_str())
                .and_then(|e| e.get("local_path"))
                .and_then(|l| l.as_str())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        })
        .collect()
}

// ── Remote url knowledge (theme-packages T3) ────────────────────────────

/// A remote video reference HVE learned from a theme's mpvpaper manifest: the
/// url, plus the optional metadata the backend needs to rebuild its own
/// assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoUrl {
    pub url: String,
    pub filename: Option<String>,
    pub sha256: Option<String>,
}

/// Look up the `url` a theme's mpvpaper manifest already knows for `source`,
/// matched by absolute `local_path` first and by base file name second. `None`
/// when the manifest is absent/unreadable, has no matching assignment, or that
/// assignment carries no url — the caller then records nothing and the theme
/// travels with its poster alone.
pub fn video_url_for(provider_dir: &Path, source: &Path) -> Option<VideoUrl> {
    let text = fs::read_to_string(provider_dir.join(MANIFEST_FILE)).ok()?;
    let manifest: MpvpaperManifest = serde_json::from_str(&text).ok()?;
    let source_str = source.to_string_lossy();
    let source_name = source.file_name().and_then(|n| n.to_str());
    for video in manifest.assignments.values() {
        let matches = video.local_path.as_deref() == Some(source_str.as_ref())
            || (source_name.is_some() && Some(video.filename.as_str()) == source_name);
        if !matches {
            continue;
        }
        if let Some(url) = video.url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
            return Some(VideoUrl {
                url: url.to_string(),
                filename: Some(video.filename.trim().to_string()).filter(|f| !f.is_empty()),
                sha256: video
                    .sha256
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            });
        }
    }
    None
}

/// Whether `name` is a manifest-safe video file name, mirroring the backend's
/// own allowlist: no separators, no leading dot, no `..`, only letters, digits,
/// `-`, `_`, `.` and spaces. An unsafe name is dropped and the fallback used,
/// so a tampered record can never point the download outside `video_directory`.
fn is_safe_video_filename(name: &str) -> bool {
    if name.is_empty() || name.starts_with('.') || name.contains("..") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
}

/// What [`ensure_url_manifest`] actually did: wrote a fresh manifest from the
/// url record, or left an existing manifest untouched. The caller logs the
/// truth instead of claiming a write that never happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlManifestOutcome {
    /// No manifest existed, so one was written from the url record.
    Written,
    /// A manifest already existed and was deliberately left untouched.
    KeptExisting,
}

/// Make sure the theme's manifest carries `url` for the mpvpaper backend, so an
/// over-bound video (recorded by url, never copied) is still played by the SAME
/// plugin path that plays any saved manifest. A manifest already on disk is
/// left untouched (the theme's own record wins); only its absence is filled in
/// from the url record. Best-effort: the caller downgrades a failure to a
/// warning. The returned [`UrlManifestOutcome`] says whether the manifest was
/// actually written.
pub fn ensure_url_manifest(
    provider_dir: &Path,
    filename: Option<&str>,
    url: &str,
    sha256: Option<&str>,
) -> Result<UrlManifestOutcome, String> {
    let manifest_path = provider_dir.join(MANIFEST_FILE);
    if manifest_path.exists() {
        return Ok(UrlManifestOutcome::KeptExisting);
    }
    let filename = filename
        .map(str::trim)
        .filter(|f| is_safe_video_filename(f))
        .unwrap_or("background.mp4")
        .to_string();
    let mut assignments = HashMap::new();
    assignments.insert(
        "*".to_string(),
        MpvpaperVideo {
            filename,
            local_path: None,
            url: Some(url.trim().to_string()),
            sha256: sha256
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        },
    );
    let manifest = MpvpaperManifest { version: 1, assignments };
    fs::create_dir_all(provider_dir).map_err(|e| format!("Cannot create provider dir: {}", e))?;
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Manifest serialization error: {}", e))?;
    fs::write(&manifest_path, json).map_err(|e| format!("Cannot write mpvpaper manifest: {}", e))?;
    tracing::info!("[mpvpaper] Recorded a url assignment for an over-bound theme video");
    Ok(UrlManifestOutcome::Written)
}

// ── Notification ────────────────────────────────────────────────────────

/// Raise ONE video-backend notice through the desktop mechanism already in
/// use. Thin shell: the decision and the copy live in [`video_backend_notice`]
/// / [`VideoBackendNotice::for_missing_backend`]; this only spawns
/// `notify-send`.
pub fn notify_video_backend(notice: &VideoBackendNotice) {
    let _ = std::process::Command::new("notify-send")
        .arg("--app-name=HVE")
        .arg(&notice.title)
        .arg(&notice.body)
        .output();
}

/// Historical API-compat shell: notify the user that the mpvpaper plugin is
/// required but not available. The production path raises the decision's own
/// notice through [`notify_video_backend`]; this keeps the historical
/// `bg_info::notify_plugin_required` / `mpvpaper::notify_plugin_required`
/// paths resolving (pinned by the `bg_info` compatibility test) with the
/// plugin remedy of the manifest leg. Nothing in the binary calls it, hence
/// the allow — same reasoning as the `mpvpaper` re-export's `unused_imports`.
#[allow(dead_code)]
pub fn notify_plugin_required() {
    notify_video_backend(&VideoBackendNotice::for_missing_backend(VideoWant::None));
}

// ── Video-backend notice (theme-packages T6) ────────────────────────────

/// The wiki page that explains how to install an animated-background backend.
/// Kept as data so the notice copy and its tests share one source.
pub const VIDEO_BACKEND_WIKI_URL: &str =
    "https://github.com/XimoCP/hyprland-visual-editor/wiki/Backgrounds";

/// HOW a theme wants its animated background — the fact the video-backend
/// notice decision reads, because each kind is painted by a DIFFERENT backend:
/// an exact path by the wallpaper engine, a url by the plugin through the
/// manifest. A single "wants a video" boolean cannot tell the two apart, and
/// that conflation is exactly what silenced the notice where it was needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoWant {
    /// The theme wants no video at all (a static theme).
    None,
    /// An exact path on disk (the packaged `media/background.mp4` or an
    /// absolute recorded path): only the wallpaper engine can paint it.
    ExactPath,
    /// A remote url: only the mpvpaper plugin can paint it, through the
    /// theme's manifest.
    Url,
}

/// The facts the T6 video-backend notice decision reads. Stated by the caller
/// so the decision stays pure and testable without spawning anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoBackendFacts {
    /// HOW the theme wants its video, if it wants one at all.
    pub video_want: VideoWant,
    /// The theme carries the mpvpaper manifest (the plugin's own record).
    pub has_manifest: bool,
    /// The wallpaper engine's socket exists (it can paint a video).
    pub engine_socket: bool,
    /// The mpvpaper plugin is installed and enabled.
    pub mpvpaper_enabled: bool,
}

/// The user-facing notice HVE raises when a theme wants a video and no backend
/// can paint it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoBackendNotice {
    pub title: String,
    pub body: String,
}

impl VideoBackendNotice {
    /// The ONE notice HVE raises when a theme wants a video and no backend can
    /// paint it. Names what is missing, that the theme's poster is shown
    /// instead, and where to read how to install a backend.
    ///
    /// The REMEDY matches the kind, because each kind is painted by a DIFFERENT
    /// backend: an exact path needs the background engine (named generically —
    /// this file names no engine), while a url or a saved manifest needs the
    /// mpvpaper plugin. Offering the plugin for a bare path would name a
    /// backend that cannot paint it, and is often already enabled.
    pub fn for_missing_backend(want: VideoWant) -> Self {
        let remedy = match want {
            VideoWant::ExactPath => "instala o activa un motor de fondos animados",
            VideoWant::None | VideoWant::Url => "instala el plugin noctalia/mpvpaper",
        };
        Self {
            title: "⚠️ Fondo animado no aplicado".to_string(),
            body: format!(
                "Este tema incluye un fondo animado, pero no hay un reproductor de vídeo \
                 disponible para mostrarlo. Se muestra el póster del tema en su lugar si el \
                 tema lo incluye. Para activar los fondos animados, {}; cómo hacerlo: {}",
                remedy, VIDEO_BACKEND_WIKI_URL
            ),
        }
    }
}

/// Pure decision + copy: `Some(notice)` when this apply must tell the user the
/// video backend is missing, `None` when there is nothing to say.
///
/// The answer depends on HOW the theme wants its video, because each kind is
/// painted by a DIFFERENT backend:
/// - an EXACT PATH (the packaged `media/background.mp4` or an absolute record)
///   is painted by the wallpaper engine, OR by the plugin through a saved
///   manifest when the theme carries both — the plugin plays only a saved
///   manifest, never a bare path, but that manifest leg IS a painter for such
///   a theme. Blocked only when NEITHER painter can run;
/// - a URL is painted by the plugin through the manifest: blocked iff the
///   plugin is not enabled or the manifest was not actually written;
/// - a SAVED MANIFEST (the old leg) keeps its rule: blocked iff the plugin is
///   not enabled;
/// - a static theme never notifies.
pub fn video_backend_notice(facts: VideoBackendFacts) -> Option<VideoBackendNotice> {
    let manifest_blocked = facts.has_manifest && !facts.mpvpaper_enabled;
    let want_blocked = match facts.video_want {
        VideoWant::None => false,
        // The engine's socket OR the manifest leg can paint this theme: a
        // theme carrying both a packaged exact-path video and a saved manifest
        // has its video played by the manifest when the plugin is enabled, so
        // the notice is silent unless BOTH painters are blocked.
        VideoWant::ExactPath => {
            !facts.engine_socket && !(facts.has_manifest && facts.mpvpaper_enabled)
        }
        VideoWant::Url => !facts.mpvpaper_enabled || !facts.has_manifest,
    };
    if manifest_blocked || want_blocked {
        Some(VideoBackendNotice::for_missing_backend(facts.video_want))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use crate::providers::noctalia_runtime::noctalia_state_dir;
    use crate::test_utils::TempEnv;

    /// Seed the live plugin-state file that `save_manifest` captures from,
    /// inside the sandboxed `HOME` the test holds.
    fn seed_live_assignments(home: &str) {
        let state_dir = noctalia_state_dir().expect("state dir resolves under the sandboxed HOME");
        let live_dir = state_dir.join("mpvpaper");
        fs::create_dir_all(&live_dir).expect("seed the live state dir");
        let json = format!(
            r#"{{"assignments":{{"*":"{home}/Videos/movie.mp4"}},"launchedAsSystemd":{{}}}}"#
        );
        fs::write(live_dir.join("assignments.json"), json).expect("seed live assignments");
    }

    fn read_manifest(dir: &Path) -> Vec<u8> {
        fs::read(dir.join(crate::providers::mpvpaper::MANIFEST_FILE))
            .expect("the manifest must exist under the shared file name")
    }

    /// The whole point of this module: `noctalia.rs` reads the shared
    /// background information here instead of naming `mpvpaper`, while the
    /// historical `mpvpaper::save_manifest` path stays a thin re-export of
    /// the SAME implementation. Both paths must therefore write the same
    /// file name and byte-identical content.
    #[test]
    fn save_manifest_is_identical_through_both_paths() {
        let env = TempEnv::new();
        let home = std::env::var("HOME").expect("TempEnv sets HOME");
        seed_live_assignments(&home);

        let dir_neutral = tempfile::tempdir().expect("provider dir A");
        let dir_legacy = tempfile::tempdir().expect("provider dir B");

        crate::providers::bg_info::save_manifest(dir_neutral.path())
            .expect("bg_info::save_manifest must succeed on seeded live state");
        crate::providers::mpvpaper::save_manifest(dir_legacy.path())
            .expect("mpvpaper::save_manifest (re-export) must succeed on the same state");

        let bytes_neutral =
            fs::read(dir_neutral.path().join(crate::providers::bg_info::MANIFEST_FILE))
                .expect("bg_info must write the manifest under its shared name");
        let bytes_legacy = read_manifest(dir_legacy.path());

        assert_eq!(
            bytes_neutral, bytes_legacy,
            "both paths must produce byte-identical manifests"
        );

        let parsed: crate::providers::mpvpaper::MpvpaperManifest =
            serde_json::from_slice(&bytes_neutral).expect("manifest must parse");
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.assignments.len(), 1);
        let video = &parsed.assignments["*"];
        assert_eq!(video.filename, "movie.mp4");
        assert_eq!(
            video.local_path.as_deref(),
            Some(format!("{home}/Videos/movie.mp4").as_str())
        );

        // API compatibility: the historical `mpvpaper` path must keep
        // resolving for every moved item. No call here — the binding only
        // proves the re-export exists with the same signature.
        let _notify: fn() = crate::providers::mpvpaper::notify_plugin_required;

        drop(env);
    }

    /// The chain this unit breaks: `noctalia.rs` must not reach into the
    /// mpvpaper backend for the shared background information — each of
    /// these items has exactly one implementation, in `bg_info`. The
    /// third-party plugin id, comments, and the policy calls reserved for
    /// the next unit are not this test's business.
    #[test]
    fn noctalia_reads_shared_background_info_from_bg_info() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        for item in ["save_manifest", "MANIFEST_FILE", "notify_plugin_required"] {
            let needle = format!("crate::providers::mpvpaper::{item}");
            assert!(
                !src.contains(&needle),
                "noctalia.rs must read `{item}` from bg_info, not from the mpvpaper \
                 backend (capability routing: a backend never calls another backend)"
            );
        }
    }

    /// The on-disk name is part of the saved-theme format: renaming it would
    /// strand every manifest already on disk. It must be one constant, seen
    /// through both paths.
    #[test]
    fn manifest_file_is_identical_through_both_paths() {
        assert_eq!(
            crate::providers::bg_info::MANIFEST_FILE,
            crate::providers::mpvpaper::MANIFEST_FILE,
            "mpvpaper must re-export the neutral constant instead of redefining it"
        );
        assert_eq!(
            crate::providers::mpvpaper::MANIFEST_FILE,
            "mpvpaper-assignments.json",
            "the manifest file name is part of the saved-theme format"
        );
    }

    // ── Theme packages T3: remote url knowledge ───────────────────────

    /// Write a manifest with one assignment carrying the given fields.
    fn write_manifest(
        dir: &Path,
        connector: &str,
        filename: &str,
        local_path: Option<&str>,
        url: Option<&str>,
        sha256: Option<&str>,
    ) {
        let mut assignments = std::collections::HashMap::new();
        assignments.insert(
            connector.to_string(),
            crate::providers::mpvpaper::MpvpaperVideo {
                filename: filename.to_string(),
                local_path: local_path.map(str::to_string),
                url: url.map(str::to_string),
                sha256: sha256.map(str::to_string),
            },
        );
        let manifest = crate::providers::mpvpaper::MpvpaperManifest { version: 1, assignments };
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        fs::write(dir.join(crate::providers::bg_info::MANIFEST_FILE), json).unwrap();
    }

    #[test]
    fn video_url_for_matches_by_local_path() {
        let dir = tempfile::tempdir().unwrap();
        let source = Path::new("/videos/slugcat.mp4");
        write_manifest(
            dir.path(),
            "*",
            "slugcat.mp4",
            Some("/videos/slugcat.mp4"),
            Some("https://example.com/slugcat.mp4"),
            Some("abc"),
        );

        let found = crate::providers::bg_info::video_url_for(dir.path(), source)
            .expect("the matching assignment's url must be found");
        assert_eq!(found.url, "https://example.com/slugcat.mp4");
        assert_eq!(found.filename.as_deref(), Some("slugcat.mp4"));
        assert_eq!(found.sha256.as_deref(), Some("abc"));
    }

    #[test]
    fn video_url_for_matches_by_filename() {
        let dir = tempfile::tempdir().unwrap();
        let source = Path::new("/elsewhere/slugcat.mp4");
        write_manifest(
            dir.path(),
            "*",
            "slugcat.mp4",
            None,
            Some("https://example.com/slugcat.mp4"),
            None,
        );

        let found = crate::providers::bg_info::video_url_for(dir.path(), source)
            .expect("the file name must match when the local path does not");
        assert_eq!(found.url, "https://example.com/slugcat.mp4");
        assert_eq!(found.sha256, None);
    }

    #[test]
    fn video_url_for_is_none_without_a_url() {
        let dir = tempfile::tempdir().unwrap();
        let source = Path::new("/videos/slugcat.mp4");
        write_manifest(
            dir.path(),
            "*",
            "slugcat.mp4",
            Some("/videos/slugcat.mp4"),
            None,
            None,
        );
        assert_eq!(
            crate::providers::bg_info::video_url_for(dir.path(), source),
            None,
            "a manifest without a url must record nothing"
        );
        // No manifest at all is equally honest.
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            crate::providers::bg_info::video_url_for(empty.path(), source),
            None
        );
    }

    #[test]
    fn ensure_url_manifest_writes_the_url_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = crate::providers::bg_info::ensure_url_manifest(
            dir.path(),
            Some("slugcat.mp4"),
            "https://example.com/slugcat.mp4",
            None,
        )
        .unwrap();
        assert_eq!(
            outcome,
            crate::providers::bg_info::UrlManifestOutcome::Written,
            "an absent manifest must be reported as written"
        );

        let text =
            fs::read_to_string(dir.path().join(crate::providers::bg_info::MANIFEST_FILE)).unwrap();
        let parsed: crate::providers::mpvpaper::MpvpaperManifest =
            serde_json::from_str(&text).unwrap();
        let video = &parsed.assignments["*"];
        assert_eq!(video.url.as_deref(), Some("https://example.com/slugcat.mp4"));
        assert_eq!(video.filename, "slugcat.mp4");
    }

    #[test]
    fn ensure_url_manifest_leaves_an_existing_manifest_untouched() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            "DP-3",
            "kept.mp4",
            Some("/videos/kept.mp4"),
            Some("https://example.com/kept.mp4"),
            None,
        );
        let before =
            fs::read_to_string(dir.path().join(crate::providers::bg_info::MANIFEST_FILE)).unwrap();

        let outcome = crate::providers::bg_info::ensure_url_manifest(
            dir.path(),
            Some("other.mp4"),
            "https://example.com/other.mp4",
            None,
        )
        .unwrap();
        assert_eq!(
            outcome,
            crate::providers::bg_info::UrlManifestOutcome::KeptExisting,
            "an existing manifest must be reported as kept, not written"
        );

        let after =
            fs::read_to_string(dir.path().join(crate::providers::bg_info::MANIFEST_FILE)).unwrap();
        assert_eq!(
            before, after,
            "an existing manifest must win; the url record never clobbers it"
        );
    }

    // ── Theme packages T6: the video-backend notice decision ───────────

    /// A packaged video with NO backend at all is the case T6 adds: the pure
    /// decision must fire, and its copy must name the poster fallback and the
    /// wiki page that explains how to install a backend.
    #[test]
    fn video_backend_notice_fires_for_a_theme_that_wants_a_video_with_no_backend() {
        let notice = crate::providers::bg_info::video_backend_notice(
            crate::providers::bg_info::VideoBackendFacts {
                video_want: crate::providers::bg_info::VideoWant::ExactPath,
                has_manifest: false,
                engine_socket: false,
                mpvpaper_enabled: false,
            },
        )
        .expect("a packaged video with no backend must raise the notice");

        assert!(!notice.title.is_empty(), "the notice must carry a title");
        assert!(
            notice.body.contains("póster"),
            "the notice must say the poster is shown instead, got: {}",
            notice.body
        );
        let wiki = crate::providers::bg_info::VIDEO_BACKEND_WIKI_URL;
        assert!(
            notice.body.contains(wiki),
            "the notice must point at the wiki page, got: {}",
            notice.body
        );
        assert!(
            notice.body.contains("motor de fondos animados"),
            "the exact-path notice must name the missing backend for its kind, got: {}",
            notice.body
        );
    }

    /// A backend that CAN paint the theme's KIND of video silences the notice:
    /// the engine's socket for an exact path, the plugin plus its written
    /// manifest for a url. The plugin ALONE is not a backend for an exact
    /// path — that conflation is defect 1, asserted below.
    #[test]
    fn video_backend_notice_is_silent_when_the_right_backend_is_present() {
        assert_eq!(
            crate::providers::bg_info::video_backend_notice(
                crate::providers::bg_info::VideoBackendFacts {
                    video_want: crate::providers::bg_info::VideoWant::ExactPath,
                    has_manifest: false,
                    engine_socket: true,
                    mpvpaper_enabled: false,
                },
            ),
            None,
            "an available engine socket must silence an exact-path video"
        );
        assert_eq!(
            crate::providers::bg_info::video_backend_notice(
                crate::providers::bg_info::VideoBackendFacts {
                    video_want: crate::providers::bg_info::VideoWant::Url,
                    has_manifest: true,
                    engine_socket: false,
                    mpvpaper_enabled: true,
                },
            ),
            None,
            "an enabled plugin with its written url manifest must silence a url video"
        );
    }

    /// Defect 1 (verifier): the shipped reference theme
    /// `assets/themes/Animation` carries a packaged video and NO manifest. On a
    /// machine where the plugin is enabled but the engine socket is absent, the
    /// plugin CANNOT paint the bare path, so the notice MUST fire. Treating an
    /// enabled plugin as a backend for an exact path silenced it exactly where
    /// it was needed.
    #[test]
    fn video_backend_notice_fires_for_a_packaged_video_with_only_the_plugin_enabled() {
        let notice = crate::providers::bg_info::video_backend_notice(
            crate::providers::bg_info::VideoBackendFacts {
                video_want: crate::providers::bg_info::VideoWant::ExactPath,
                has_manifest: false,
                engine_socket: false,
                mpvpaper_enabled: true,
            },
        )
        .expect("the plugin plays no bare path: a packaged video with no socket must notify");
        assert!(
            notice.body.contains("póster"),
            "the notice must name the poster fallback, got: {}",
            notice.body
        );
    }

    /// Point 1 correction: a theme that carries BOTH a packaged exact-path
    /// video and a saved manifest, with the plugin enabled and no engine
    /// socket, has its video painted by the manifest leg — so the notice must
    /// stay silent. The plugin ALONE paints no bare path (the previous test),
    /// but plugin + written manifest is a real painter for this theme.
    #[test]
    fn video_backend_notice_is_silent_when_an_exact_path_has_a_paintable_manifest() {
        assert_eq!(
            crate::providers::bg_info::video_backend_notice(
                crate::providers::bg_info::VideoBackendFacts {
                    video_want: crate::providers::bg_info::VideoWant::ExactPath,
                    has_manifest: true,
                    engine_socket: false,
                    mpvpaper_enabled: true,
                },
            ),
            None,
            "an enabled plugin with a written manifest paints this theme's video: \
             the notice must stay silent"
        );
    }

    /// A url is painted by the plugin THROUGH the manifest: a disabled plugin
    /// blocks it, and so does a manifest that was never written (the engine
    /// cannot paint a url either).
    #[test]
    fn video_backend_notice_fires_for_a_url_that_no_plugin_can_paint() {
        assert!(
            crate::providers::bg_info::video_backend_notice(
                crate::providers::bg_info::VideoBackendFacts {
                    video_want: crate::providers::bg_info::VideoWant::Url,
                    has_manifest: false,
                    engine_socket: false,
                    mpvpaper_enabled: false,
                },
            )
            .is_some(),
            "a url with the plugin disabled must notify"
        );
        assert!(
            crate::providers::bg_info::video_backend_notice(
                crate::providers::bg_info::VideoBackendFacts {
                    video_want: crate::providers::bg_info::VideoWant::Url,
                    has_manifest: false,
                    engine_socket: true,
                    mpvpaper_enabled: true,
                },
            )
            .is_some(),
            "the engine cannot paint a url: an enabled plugin with no written manifest must notify"
        );
    }

    /// Defect 3 (verifier): a theme with no poster record and no static
    /// wallpaper paints NOTHING, so the copy must not promise a poster
    /// unconditionally — the fallback is stated as conditional.
    #[test]
    fn video_backend_notice_copy_conditions_the_poster_fallback() {
        let notice = crate::providers::bg_info::VideoBackendNotice::for_missing_backend(
            crate::providers::bg_info::VideoWant::None,
        );
        assert!(
            notice.body.contains("si el tema lo incluye"),
            "the notice must state the poster fallback is conditional, got: {}",
            notice.body
        );
    }

    /// Point 3 correction: the remedy must match the kind of want. An exact
    /// path is painted by the background ENGINE — the plugin plays no bare path
    /// (and is often already enabled) — so its copy must not offer the plugin.
    /// A url is painted by the plugin, so its copy names it. Both keep the
    /// poster sentence and the wiki link.
    #[test]
    fn video_backend_notice_remedy_matches_the_kind() {
        let exact = crate::providers::bg_info::video_backend_notice(
            crate::providers::bg_info::VideoBackendFacts {
                video_want: crate::providers::bg_info::VideoWant::ExactPath,
                has_manifest: false,
                engine_socket: false,
                mpvpaper_enabled: true,
            },
        )
        .expect("an exact path with no engine must notify");
        assert!(
            exact.body.contains("motor de fondos animados"),
            "the exact-path remedy must name the background engine, got: {}",
            exact.body
        );
        assert!(
            !exact.body.contains("mpvpaper"),
            "the plugin cannot paint a bare path: the exact-path remedy must not \
             offer it, got: {}",
            exact.body
        );
        assert!(
            exact.body.contains("póster")
                && exact.body.contains(crate::providers::bg_info::VIDEO_BACKEND_WIKI_URL),
            "the exact-path copy must keep the poster sentence and the wiki link, got: {}",
            exact.body
        );

        let url = crate::providers::bg_info::video_backend_notice(
            crate::providers::bg_info::VideoBackendFacts {
                video_want: crate::providers::bg_info::VideoWant::Url,
                has_manifest: false,
                engine_socket: false,
                mpvpaper_enabled: false,
            },
        )
        .expect("a url with no plugin must notify");
        assert!(
            url.body.contains("mpvpaper"),
            "the url remedy must name the plugin, got: {}",
            url.body
        );
        assert!(
            url.body.contains("póster")
                && url.body.contains(crate::providers::bg_info::VIDEO_BACKEND_WIKI_URL),
            "the url copy must keep the poster sentence and the wiki link, got: {}",
            url.body
        );
    }

    /// The old case must keep working: a theme with an mpvpaper manifest while
    /// the plugin is missing or disabled still notifies. The engine's socket
    /// cannot unblock an mpvpaper manifest, so it stays a notice either way.
    #[test]
    fn video_backend_notice_keeps_the_manifest_leg_case() {
        assert!(
            crate::providers::bg_info::video_backend_notice(
                crate::providers::bg_info::VideoBackendFacts {
                    video_want: crate::providers::bg_info::VideoWant::None,
                    has_manifest: true,
                    engine_socket: false,
                    mpvpaper_enabled: false,
                },
            )
            .is_some(),
            "the plugin-disabled manifest leg must keep notifying"
        );
        assert!(
            crate::providers::bg_info::video_backend_notice(
                crate::providers::bg_info::VideoBackendFacts {
                    video_want: crate::providers::bg_info::VideoWant::None,
                    has_manifest: true,
                    engine_socket: true,
                    mpvpaper_enabled: false,
                },
            )
            .is_some(),
            "the engine socket must not silence the manifest leg"
        );
        // The plugin being enabled is the one thing that silences it.
        assert_eq!(
            crate::providers::bg_info::video_backend_notice(
                crate::providers::bg_info::VideoBackendFacts {
                    video_want: crate::providers::bg_info::VideoWant::None,
                    has_manifest: true,
                    engine_socket: false,
                    mpvpaper_enabled: true,
                },
            ),
            None,
            "an enabled plugin needs no notice"
        );
    }

    /// A static theme has nothing to be told — even with no backend at all.
    #[test]
    fn video_backend_notice_is_silent_for_a_static_theme() {
        assert_eq!(
            crate::providers::bg_info::video_backend_notice(
                crate::providers::bg_info::VideoBackendFacts {
                    video_want: crate::providers::bg_info::VideoWant::None,
                    has_manifest: false,
                    engine_socket: false,
                    mpvpaper_enabled: false,
                },
            ),
            None,
            "a static theme must never notify"
        );
    }
}
