//! mpvpaper animated-wallpaper integration for Noctalia v5 themes.
//!
//! The official Noctalia `noctalia/mpvpaper` plugin supervises one `mpvpaper`
//! instance per output and persists per-output assignments in
//! `~/.local/state/noctalia/mpvpaper/assignments.json`:
//!
//! ```json
//! { "assignments": { "<connector>": "/abs/path/video.mp4", "*": "/abs/path/video.mp4" },
//!   "launchedAsSystemd": { "<connector>": false } }
//! ```
//!
//! The plugin reads that file ONLY at boot: `loadAssignments()` + `applyAll()`.
//! Its external IPC surface (`onIpc`) has no `set` event — only clear/clear-all/
//! pause/resume/toggle. Therefore, to make the plugin apply videos from a theme
//! WITHOUT patching it, we write `assignments.json` and then bounce the plugin:
//!
//! ```text
//! noctalia msg plugins disable noctalia/mpvpaper
//! noctalia msg plugins enable  noctalia/mpvpaper
//! ```
//!
//! This replicates exactly what clicking a video in the plugin picker does.
//!
//! Themes store a lightweight manifest (references only, no video bytes):
//! `{theme_dir}/providers/noctalia-v5/mpvpaper-assignments.json`:
//!
//! ```json
//! { "version": 1,
//!   "assignments": {
//!     "*": { "filename": "movie.mp4",
//!            "local_path": "/abs/path/movie.mp4",   // local save
//!            "url": "https://...",                  // distributed theme
//!            "sha256": "..." } } }                  // optional integrity
//! ```
//!
//! Resolution cascade at apply time:
//!   1. `local_path` exists on disk          -> use it (zero download)
//!   2. `filename` present in video_directory -> use it (already downloaded)
//!   3. `url` present                         -> curl into video_directory,
//!      verify sha256 when provided
//!   4. otherwise                             -> warn + skip (never fail apply)
//!
//! Themes WITHOUT a manifest send `clear-all` first (the programmatic STOP)
//! so a previously running video cannot hijack the screen.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::providers::noctalia_runtime::{noctalia_msg, noctalia_state_dir};

/// File HVE stores inside the theme provider dir.
pub const MANIFEST_FILE: &str = "mpvpaper-assignments.json";

/// How long to wait after `clear-all` for the plugin to extract the static
/// frame (async ffmpeg) before HVE applies its own static wallpaper.
const CLEAR_SETTLE_MS: u64 = 1500;

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
struct LiveAssignments {
    assignments: HashMap<String, String>,
    #[serde(default, rename = "launchedAsSystemd")]
    launched_as_systemd: HashMap<String, bool>,
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn live_assignments_path() -> Option<PathBuf> {
    Some(noctalia_state_dir()?.join("mpvpaper").join("assignments.json"))
}

/// Parse `video_directory` from the real (restored) settings.toml section
/// `[plugin_settings."noctalia/mpvpaper"]`. Falls back to `~/Videos`,
/// mirroring the plugin default. No `toml` crate: settings.toml is
/// machine-written, line `video_directory = "..."` inside the section.
fn video_directory_from_settings() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let default = PathBuf::from(&home).join("Videos");

    let Some(state_dir) = noctalia_state_dir() else {
        return default;
    };
    let settings_path = state_dir.join("settings.toml");
    let Ok(raw) = fs::read_to_string(&settings_path) else {
        return default;
    };

    parse_video_directory(&raw, default, &home)
}

/// Pure parser: extract `video_directory` from the mpvpaper plugin section of
/// a settings.toml string. Returns `default` when absent or unparseable.
fn parse_video_directory(raw: &str, default: PathBuf, home: &str) -> PathBuf {
    let mut in_mpvpaper_section = false;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_mpvpaper_section = line == "[plugin_settings.\"noctalia/mpvpaper\"]";
            continue;
        }
        if in_mpvpaper_section {
            if let Some(rest) = line.strip_prefix("video_directory") {
                let rest = rest.trim_start();
                if let Some(value) = rest.strip_prefix('=') {
                    let value = value.trim().trim_matches('"');
                    if !value.is_empty() {
                        if let Some(expanded) = expand_home(value, home) {
                            return expanded;
                        }
                    }
                }
            }
        }
    }

    default
}

/// Expand a leading `~` in a configured path.
fn expand_home(path: &str, home: &str) -> Option<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        Some(PathBuf::from(home).join(rest))
    } else if path == "~" {
        Some(PathBuf::from(home))
    } else {
        Some(PathBuf::from(path))
    }
}

/// Check whether the mpvpaper plugin is installed AND enabled, by parsing
/// `noctalia msg plugins list` (line suffix `enabled`).
pub(crate) fn mpvpaper_enabled() -> bool {
    match noctalia_msg(&["msg", "plugins", "list"]) {
        Ok(out) => out
            .lines()
            .any(|l| l.trim_start().starts_with("noctalia/mpvpaper") && l.trim_end().ends_with("enabled")),
        Err(_) => false,
    }
}

/// Notify the user that the mpvpaper plugin is required but not available,
/// so an animated theme cannot be applied.
pub(crate) fn notify_plugin_required() {
    let _ = std::process::Command::new("notify-send")
        .arg("--app-name=HVE")
        .arg("⚠️ Fondo animado no aplicado")
        .arg("El tema incluye fondos animados, pero el plugin noctalia/mpvpaper no está instalado o está desactivado. Activá el plugin en los ajustes de Noctalia para poder aplicarlos.")
        .output();
}

/// Kill every running mpvpaper instance. The plugin's own kill is async and
/// races with its start (`onConfigChanged -> applyAll -> startMpvpaper` does
/// an async pkill then an async start), so a plain bounce can leave stale
/// duplicates. Killing synchronously here guarantees a clean slate before the
/// plugin boots and launches exactly one instance per assignment.
fn kill_all_instances() {
    let _ = std::process::Command::new("pkill")
        .arg("-9")
        .arg("-x")
        .arg("mpvpaper")
        .status();
}

/// Bounce the plugin so it re-reads assignments.json at boot and launches
/// the configured mpvpaper instances (the programmatic "click").
///
/// G3: refused while the plugin is currently disabled — the enable step
/// would RE-ENABLE a plugin the user turned off and resurrect its old
/// video on the next theme apply. Callers check first; this guard is the
/// backstop so no path can bounce a disabled plugin by accident.
fn bounce_plugin() -> Result<(), String> {
    if !mpvpaper_enabled() {
        return Err(
            "noctalia/mpvpaper plugin is not enabled; refusing to bounce \
             (would re-enable a plugin the user disabled)"
                .into(),
        );
    }
    noctalia_msg(&["msg", "plugins", "disable", "noctalia/mpvpaper"])?;
    noctalia_msg(&["msg", "plugins", "enable", "noctalia/mpvpaper"])?;
    Ok(())
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

// ── Apply ───────────────────────────────────────────────────────────────

/// SECURITY: strict allowlist for manifest `filename` values. Only letters,
/// digits, `-`, `_`, `.` and spaces are accepted. Anything that could escape
/// `video_dir` (slashes, `..`, leading dots, NUL) or otherwise break the
/// download path is rejected before any path/command use.
fn validate_filename(filename: &str) -> Result<(), String> {
    if filename.is_empty() {
        return Err("manifest filename is empty".into());
    }
    if filename.contains('/') || filename.contains('\\') || filename.contains('\0') {
        return Err(format!(
            "manifest filename '{filename}' contains path separators or NUL"
        ));
    }
    if filename.starts_with('.') {
        return Err(format!("manifest filename '{filename}' starts with a dot"));
    }
    if filename.contains("..") {
        return Err(format!("manifest filename '{filename}' contains '..'"));
    }
    if !filename
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
    {
        return Err(format!(
            "manifest filename '{filename}' contains disallowed characters"
        ));
    }
    Ok(())
}

/// SECURITY: unpredictable temp path next to `dest` (filename + pid + ns) so
/// a pre-existing symlink at a predictable location cannot be followed.
fn unique_tmp_path(dest: &Path) -> Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let base = dest
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "video".to_string());
    Ok(dest.with_file_name(format!("{base}.tmp.{}.{}", std::process::id(), nanos)))
}

/// Resolve one video reference to an absolute path that exists on disk.
fn resolve_video(video: &MpvpaperVideo, video_dir: &Path) -> Result<PathBuf, String> {
    // SECURITY: reject unsafe filenames before they touch any path.
    validate_filename(&video.filename)?;

    // 1. Local save still present.
    if let Some(lp) = &video.local_path {
        let p = PathBuf::from(lp);
        if p.exists() {
            return Ok(p);
        }
    }

    // 2. Already present inside the plugin's video_directory.
    let local = video_dir.join(&video.filename);
    if local.exists() {
        return Ok(local);
    }

    // 3. Remote source: download into video_directory. sha256 is REQUIRED —
    //    an unverified remote download is never accepted.
    if let Some(url) = &video.url {
        if !url.is_empty() {
            let sha = video.sha256.as_deref().filter(|s| !s.is_empty());
            let Some(sha) = sha else {
                return Err(format!(
                    "video '{}': url present but manifest omits sha256; refusing to download",
                    video.filename
                ));
            };
            let downloaded = download_video(url, &local, sha)?;
            return Ok(downloaded);
        }
    }

    Err(format!("video '{}' not found locally and has no usable url", video.filename))
}

/// Download `url` to `dest` using `curl`, verifying sha256.
///
/// SECURITY: only `https://` URLs are accepted and curl is restricted to the
/// https protocol (defense in depth against redirect downgrades); the sha256
/// hash is mandatory, so an unverified file is never placed on disk.
fn download_video(url: &str, dest: &Path, expected_sha: &str) -> Result<PathBuf, String> {
    if !url.starts_with("https://") {
        return Err(format!("refusing to download non-https url: {}", url));
    }

    let parent = dest.parent().ok_or("Invalid destination path")?;
    fs::create_dir_all(parent).map_err(|e| format!("Cannot create video dir: {}", e))?;

    // SECURITY: claim a unique temp path with O_EXCL so a pre-existing
    // symlink at a predictable temp name cannot be followed.
    let tmp = unique_tmp_path(dest)?;
    let created = fs::File::create_new(&tmp)
        .map_err(|e| format!("Cannot create unique temp file: {}", e))?;
    drop(created);

    let status = std::process::Command::new("curl")
        .args([
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "--max-time",
            "300",
            "--proto",
            "=https",
            "-o",
        ])
        .arg(&tmp)
        .arg(url)
        .status()
        .map_err(|e| format!("Cannot run curl: {}", e))?;

    if !status.success() {
        let _ = fs::remove_file(&tmp);
        return Err(format!("Download failed for {}", url));
    }

    let actual = sha256_of(&tmp)?;
    if actual != expected_sha {
        let _ = fs::remove_file(&tmp);
        return Err(format!(
            "sha256 mismatch for {}: expected {}, got {}",
            url, expected_sha, actual
        ));
    }

    fs::rename(&tmp, dest).map_err(|e| format!("Cannot move downloaded video: {}", e))?;
    Ok(dest.to_path_buf())
}

/// Compute the lowercase hex sha256 of a file via `sha256sum`.
fn sha256_of(path: &Path) -> Result<String, String> {
    let out = std::process::Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|e| format!("Cannot run sha256sum: {}", e))?;
    if !out.status.success() {
        return Err("sha256sum failed".into());
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(stdout
        .split_whitespace()
        .next()
        .map(|s| s.to_lowercase())
        .unwrap_or_default())
}

/// Apply a theme's mpvpaper manifest:
/// 1. resolve/download every referenced video,
/// 2. write the live plugin assignments file,
/// 3. bounce the plugin so it launches the videos.
pub fn apply_manifest(theme_dir: &Path) -> Result<(), String> {
    let provider_dir = theme_dir.join("providers").join("noctalia-v5");
    let manifest_path = provider_dir.join(MANIFEST_FILE);
    if !manifest_path.exists() {
        return Err("Theme has no mpvpaper manifest".into());
    }

    let raw = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Cannot read manifest: {}", e))?;
    let manifest: MpvpaperManifest = serde_json::from_str(&raw)
        .map_err(|e| format!("Cannot parse manifest: {}", e))?;

    // settings.toml was already restored → video_directory reflects the theme.
    let video_dir = video_directory_from_settings();

    let mut resolved: HashMap<String, String> = HashMap::new();
    let mut failures: Vec<String> = Vec::new();
    for (connector, video) in &manifest.assignments {
        match resolve_video(video, &video_dir) {
            Ok(abs) => {
                resolved.insert(connector.clone(), abs.to_string_lossy().to_string());
            }
            Err(e) => {
                failures.push(format!("{}: {}", connector, e));
                tracing::warn!("[mpvpaper] Skipping {}: {}", connector, e);
            }
        }
    }

    if resolved.is_empty() {
        return Err(format!(
            "No mpvpaper videos could be resolved{}",
            if failures.is_empty() { String::new() } else { format!(" ({} failed)", failures.len()) }
        ));
    }

    // G3: refuse BEFORE writing any live state or killing anything. A
    // disabled plugin must never be re-enabled by a theme apply, and no
    // half-written assignments file may linger for its next boot.
    if !mpvpaper_enabled() {
        return Err(
            "noctalia/mpvpaper plugin is not enabled; videos not launched (live state untouched)"
                .into(),
        );
    }

    // Persist live state so the plugin picks it up at boot.
    let Some(state_file) = live_assignments_path() else {
        return Err("Cannot resolve Noctalia state dir".into());
    };
    if let Some(parent) = state_file.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create state dir: {}", e))?;
    }
    let applied_count = resolved.len();
    let live = LiveAssignments {
        assignments: resolved,
        launched_as_systemd: HashMap::new(),
    };
    let json = serde_json::to_string(&live)
        .map_err(|e| format!("Live assignments serialization error: {}", e))?;
    fs::write(&state_file, json)
        .map_err(|e| format!("Cannot write live assignments: {}", e))?;

    // Kill every current instance BEFORE bouncing. Without this, the plugin's
    // async kill/start race (triggered by onConfigChanged -> applyAll during
    // the config-reload that precedes this apply) leaves duplicate mpvpaper
    // processes decoding the same video at full CPU.
    kill_all_instances();

    // The plugin is present and enabled (checked above, before any write):
    // bounce it so its boot-time applyAll() picks up the new assignments.
    bounce_plugin()?;

    // Give the plugin's boot-time applyAll() time to spawn the new instances
    // before we return, so a subsequent static theme apply cannot clear-all
    // into a half-booted plugin.
    std::thread::sleep(Duration::from_millis(1200));

    if !failures.is_empty() {
        tracing::warn!(
            "[mpvpaper] Applied {} of {} assignment(s); {} failed",
            applied_count,
            manifest.assignments.len(),
            failures.len()
        );
    } else {
        tracing::info!("[mpvpaper] Applied {} assignment(s)", applied_count);
    }

    Ok(())
}

/// Programmatic STOP: remove every active video wallpaper and restore the
/// host (static) wallpaper. Used when a theme has NO animated wallpapers.
pub fn clear_all() -> Result<(), String> {
    if !mpvpaper_enabled() {
        return Ok(());
    }
    noctalia_msg(&["msg", "plugin", "noctalia/mpvpaper:service", "all", "clear-all"])?;
    // Let the plugin's async ffmpeg frame extraction settle before the caller
    // applies its static wallpaper, so the extracted frame cannot win the race.
    std::thread::sleep(Duration::from_millis(CLEAR_SETTLE_MS));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_settings() -> String {
        r#"# noctalia settings
[wallpaper]
directory = "/home/user/Pictures/Wallpapers"

[plugin_settings."noctalia/mpvpaper"]
picker_placement = "floating"
video_directory = "/home/ximo/Pictures/LiveWallpapers"

[plugin_settings."yuuto/arch-updater"]
auto_check_hours = 60
"#
        .to_string()
    }

    #[test]
    fn test_parse_video_directory_found() {
        let default = PathBuf::from("/fallback");
        let dir = parse_video_directory(&sample_settings(), default.clone(), "/home/ximo");
        assert_eq!(dir, PathBuf::from("/home/ximo/Pictures/LiveWallpapers"));
    }

    #[test]
    fn test_parse_video_directory_missing_section() {
        let raw = "[wallpaper]\ndirectory = \"/x\"\n";
        let default = PathBuf::from("/fallback");
        let dir = parse_video_directory(raw, default.clone(), "/home/ximo");
        assert_eq!(dir, default);
    }

    #[test]
    fn test_parse_video_directory_other_section_first() {
        // A `video_directory` in a DIFFERENT plugin section must not leak.
        let raw = r#"[plugin_settings."other/plugin"]
video_directory = "/wrong"

[plugin_settings."noctalia/mpvpaper"]
video_directory = "/right"
"#;
        let dir = parse_video_directory(raw, PathBuf::from("/fallback"), "/home/ximo");
        assert_eq!(dir, PathBuf::from("/right"));
    }

    #[test]
    fn test_parse_video_directory_tilde_expansion() {
        let raw = r#"[plugin_settings."noctalia/mpvpaper"]
video_directory = "~/Videos"
"#;
        let dir = parse_video_directory(raw, PathBuf::from("/fallback"), "/home/ximo");
        assert_eq!(dir, PathBuf::from("/home/ximo/Videos"));
    }

    #[test]
    fn test_expand_home() {
        assert_eq!(
            expand_home("~/x/y.mp4", "/home/ximo"),
            Some(PathBuf::from("/home/ximo/x/y.mp4"))
        );
        assert_eq!(expand_home("~", "/home/ximo"), Some(PathBuf::from("/home/ximo")));
        assert_eq!(expand_home("/abs/path.mp4", "/home/ximo"), Some(PathBuf::from("/abs/path.mp4")));
    }

    #[test]
    fn test_resolve_video_uses_local_path() {
        let dir = std::env::temp_dir().join("mpvpaper-test-local");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("video.mp4");
        fs::write(&file, b"fake video").unwrap();

        let video = MpvpaperVideo {
            filename: "video.mp4".into(),
            local_path: Some(file.to_string_lossy().to_string()),
            url: None,
            sha256: None,
        };
        let resolved = resolve_video(&video, &dir).unwrap();
        assert_eq!(resolved, file);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_video_uses_video_dir_when_local_missing() {
        let dir = std::env::temp_dir().join("mpvpaper-test-vdir");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("video.mp4");
        fs::write(&file, b"fake video").unwrap();

        let video = MpvpaperVideo {
            filename: "video.mp4".into(),
            local_path: Some("/nonexistent/video.mp4".into()),
            url: None,
            sha256: None,
        };
        let resolved = resolve_video(&video, &dir).unwrap();
        assert_eq!(resolved, file);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_video_fails_when_unavailable() {
        let dir = std::env::temp_dir().join("mpvpaper-test-missing");
        let _ = fs::create_dir_all(&dir);

        let video = MpvpaperVideo {
            filename: "ghost.mp4".into(),
            local_path: None,
            url: None,
            sha256: None,
        };
        assert!(resolve_video(&video, &dir).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_video_rejects_traversal_filename() {
        let dir = std::env::temp_dir().join("mpvpaper-test-traversal");
        let _ = fs::create_dir_all(&dir);

        // A traversal filename must never reach the filesystem (would let a
        // crafted manifest overwrite arbitrary user-writable files).
        for bad in [
            "../../.config/hypr/hyprland.conf",
            "..",
            ".hidden.mp4",
            "a/b.mp4",
            "a\\b.mp4",
            "",
            "a;rm -rf ~",
        ] {
            let video = MpvpaperVideo {
                filename: bad.to_string(),
                local_path: None,
                url: Some("https://example.com/movie.mp4".into()),
                sha256: Some("a".repeat(64)),
            };
            let err = resolve_video(&video, &dir).unwrap_err();
            assert!(err.contains("filename"), "unexpected error for {bad:?}: {err}");
        }

        // A safe filename with spaces is accepted by the validator, and a
        // local path resolves without any network access.
        assert!(validate_filename("my movie.mp4").is_ok());
        let local = dir.join("my movie.mp4");
        fs::write(&local, b"x").expect("write local fixture");
        let video = MpvpaperVideo {
            filename: "my movie.mp4".into(),
            local_path: Some(local.to_string_lossy().to_string()),
            url: None,
            sha256: None,
        };
        let resolved = resolve_video(&video, &dir).unwrap();
        assert_eq!(resolved, local);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_video_rejects_non_https_url() {
        let dir = std::env::temp_dir().join("mpvpaper-test-http");
        let _ = fs::create_dir_all(&dir);

        let video = MpvpaperVideo {
            filename: "movie.mp4".into(),
            local_path: None,
            url: Some("http://example.com/movie.mp4".into()),
            sha256: Some("a".repeat(64)),
        };
        let err = resolve_video(&video, &dir).unwrap_err();
        assert!(err.contains("https"), "unexpected error: {err}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_download_video_rejects_file_scheme() {
        // Defense in depth: even if a caller bypassed resolve_video, the
        // download layer itself refuses non-https URLs.
        let dir = std::env::temp_dir().join("mpvpaper-test-filescheme");
        let _ = fs::create_dir_all(&dir);

        let dest = dir.join("movie.mp4");
        let err = download_video("file:///etc/passwd", &dest, "abc").unwrap_err();
        assert!(err.contains("https"), "unexpected error: {err}");
        assert!(!dest.exists(), "file scheme must not write anything");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_video_requires_sha256_with_url() {
        let dir = std::env::temp_dir().join("mpvpaper-test-nosha");
        let _ = fs::create_dir_all(&dir);

        // Missing sha256 -> fail BEFORE any download attempt.
        let video = MpvpaperVideo {
            filename: "movie.mp4".into(),
            local_path: None,
            url: Some("https://example.com/movie.mp4".into()),
            sha256: None,
        };
        let err = resolve_video(&video, &dir).unwrap_err();
        assert!(err.contains("sha256"), "unexpected error: {err}");

        // Empty sha256 is treated the same as missing.
        let video2 = MpvpaperVideo {
            filename: "movie.mp4".into(),
            local_path: None,
            url: Some("https://example.com/movie.mp4".into()),
            sha256: Some(String::new()),
        };
        let err2 = resolve_video(&video2, &dir).unwrap_err();
        assert!(err2.contains("sha256"), "unexpected error: {err2}");

        // With a hash present the error moves past the sha gate (curl not run
        // here — this merely proves the hash requirement is what fires).
        let video3 = MpvpaperVideo {
            filename: "movie.mp4".into(),
            local_path: None,
            url: Some("https://example.com/movie.mp4".into()),
            sha256: Some("a".repeat(64)),
        };
        let err3 = resolve_video(&video3, &dir).unwrap_err();
        assert!(!err3.contains("sha256"), "unexpected error: {err3}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_manifest_roundtrip() {
        let manifest = MpvpaperManifest {
            version: 1,
            assignments: HashMap::from([(
                "*".to_string(),
                MpvpaperVideo {
                    filename: "movie.mp4".into(),
                    local_path: Some("/a/b/movie.mp4".into()),
                    url: Some("https://example.com/movie.mp4".into()),
                    sha256: None,
                },
            )]),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: MpvpaperManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, 1);
        assert_eq!(back.assignments.len(), 1);
        let v = &back.assignments["*"];
        assert_eq!(v.filename, "movie.mp4");
        assert_eq!(v.local_path.as_deref(), Some("/a/b/movie.mp4"));
        assert_eq!(v.url.as_deref(), Some("https://example.com/movie.mp4"));
    }

    /// G3: restoring a video must never re-enable a plugin the user
    /// disabled. The disable+enable bounce is therefore guarded by the
    /// live enabled check, and the live assignments file is only written
    /// after that check passes — a disabled plugin means early refusal
    /// before any write or kill. Comment lines are stripped first so a
    /// commented-out guard cannot satisfy this.
    #[test]
    fn apply_never_enables_disabled_plugin_by_construction() {
        let src = std::fs::read_to_string("src/providers/mpvpaper.rs")
            .expect("src/providers/mpvpaper.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let bounce_at = code.find("fn bounce_plugin").expect("bounce_plugin must exist");
        let after_bounce = &code[bounce_at..];
        let guard_at = after_bounce
            .find("mpvpaper_enabled")
            .expect("bounce_plugin must consult the live enabled check");
        let enable_at = after_bounce
            .find("\"enable\"")
            .expect("bounce_plugin must still contain its enable step");
        assert!(
            guard_at < enable_at,
            "the enabled check must dominate the enable step inside bounce_plugin"
        );
        let apply_at = code.find("pub fn apply_manifest").expect("apply_manifest must exist");
        let after_apply = &code[apply_at..];
        let apply_guard = after_apply
            .find("mpvpaper_enabled")
            .expect("apply_manifest must consult the live enabled check");
        let live_write = after_apply
            .find("Cannot write live assignments")
            .expect("apply_manifest must still persist the live assignments file");
        assert!(
            apply_guard < live_write,
            "apply_manifest must refuse a disabled plugin before writing any live state"
        );
    }
}