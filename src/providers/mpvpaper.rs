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
//!
//! Supervising the plugin (probing `plugins list`, the disable/enable
//! bounce, the `clear-all` service call) is Noctalia runtime knowledge and
//! lives in `noctalia_runtime` — the plugin is Noctalia's. This backend
//! only states what it needs from that seam.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::providers::noctalia_runtime::{
    noctalia_state_dir, plugin_bounce, plugin_enabled, plugin_service_clear_all, MPVPAPER_PLUGIN_ID,
};

// The shared background information — manifest file name, manifest types,
// live-state path and capture, and the user notification — lives in the
// neutral `bg_info` module, so `noctalia.rs` can read it without naming
// this backend (capability routing: a backend never calls another one).
// These are re-exports of the ONE implementation, which sits in `bg_info`.
pub use crate::providers::bg_info::{MANIFEST_FILE, MpvpaperManifest, MpvpaperVideo};
// Compatibility re-exports: the historical `mpvpaper::save_manifest` and
// `mpvpaper::notify_plugin_required` paths must keep resolving (pinned by
// the `bg_info` compatibility tests), while production code reaches them
// through `bg_info`. Nothing in the binary calls them through THIS path,
// hence the allow — the one implementation still lives in `bg_info`.
#[allow(unused_imports)]
pub use crate::providers::bg_info::{notify_plugin_required, save_manifest};
pub(crate) use crate::providers::bg_info::{live_assignments_path, LiveAssignments};

/// How long to wait after `clear-all` for the plugin to extract the static
/// frame (async ffmpeg) before HVE applies its own static wallpaper.
const CLEAR_SETTLE_MS: u64 = 1500;

// ── Helpers ─────────────────────────────────────────────────────────────

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
    let section = format!("[plugin_settings.\"{MPVPAPER_PLUGIN_ID}\"]");
    let mut in_mpvpaper_section = false;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_mpvpaper_section = line == section;
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

/// Resolve `path` to its real location: canonicalize the longest existing
/// ancestor (following symlinks), then re-attach the not-yet-existing
/// trailing components — which, by definition, cannot be symlinks yet.
/// Refuses when a `..` or `.` component sits above a non-existent
/// component: such a path cannot be pinned to a real location, and
/// refusing is the safe direction.
fn resolve_real_path(path: &Path) -> Result<PathBuf, String> {
    let mut ancestor = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(real) = fs::canonicalize(&ancestor) {
            let mut resolved = real;
            for component in tail.iter().rev() {
                resolved.push(component);
            }
            return Ok(resolved);
        }
        match ancestor.components().next_back() {
            Some(std::path::Component::Normal(name)) => {
                tail.push(name.to_os_string());
                ancestor.pop();
            }
            _ => {
                return Err(format!("cannot resolve '{}' to a real path", path.display()));
            }
        }
    }
}

/// The known-safe roots a theme's `video_directory` may live under: the
/// user's media directories. Each root is resolved to its REAL location so
/// a root reached through a symlink is compared like-for-like against the
/// resolved candidate.
fn allowed_video_roots(home: &str) -> Vec<PathBuf> {
    ["Pictures", "Videos", "Downloads"]
        .iter()
        .map(|dir| resolve_real_path(&Path::new(home).join(dir)))
        .filter_map(Result::ok)
        .collect()
}

/// SECURITY (W4 — confinement rule, a deliberate restriction, not an
/// accident): `video_directory` comes from the theme's settings.toml, which
/// is UNTRUSTED input. Without this check a hostile theme could set it to
/// `/home/<user>/.config/hypr` and, through the download+rename in
/// `download_video`, overwrite arbitrary files of the user's (his Hyprland
/// config, his watchdog script, a systemd unit…). Only the user's media
/// roots — `$HOME/Pictures`, `$HOME/Videos`, `$HOME/Downloads` — are
/// accepted. Anything else is REFUSED with a warning and nothing is
/// created, downloaded or written.
fn confine_video_dir(dir: &Path, home: &str) -> Result<PathBuf, String> {
    let resolved = resolve_real_path(dir)?;
    for root in allowed_video_roots(home) {
        if resolved.starts_with(&root) {
            return Ok(resolved);
        }
    }
    Err(format!(
        "refusing video_directory '{}': it resolves to '{}', outside the allowed \
         media roots ($HOME/Pictures, $HOME/Videos, $HOME/Downloads)",
        dir.display(),
        resolved.display()
    ))
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

/// Check whether the mpvpaper plugin is installed AND enabled. One
/// implementation only, in the neutral `noctalia_runtime` seam (the plugin
/// is Noctalia's, so how to ask is Noctalia runtime knowledge); this
/// backend just states which plugin it feeds.
pub(crate) fn mpvpaper_enabled() -> bool {
    plugin_enabled(MPVPAPER_PLUGIN_ID)
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
/// G3 lives with the IPC it guards: `noctalia_runtime::plugin_bounce`
/// refuses while the plugin is currently disabled, so the enable step can
/// never RE-ENABLE a plugin the user turned off. This backend delegates
/// and keeps no supervisor IPC of its own.
fn bounce_plugin() -> Result<(), String> {
    plugin_bounce(MPVPAPER_PLUGIN_ID)
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
    // SECURITY (W4): that value is UNTRUSTED theme input — confine it to the
    // user's media roots BEFORE anything resolves or downloads into it. On
    // refusal the whole apply stops here: no directory is created, no file
    // downloaded, no live state written.
    let video_dir = video_directory_from_settings();
    let home = std::env::var("HOME").unwrap_or_default();
    let video_dir = match confine_video_dir(&video_dir, &home) {
        Ok(dir) => dir,
        Err(e) => {
            tracing::warn!("[mpvpaper] {e}; nothing was created or downloaded");
            return Err(e);
        }
    };

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
        return Err(format!(
            "{MPVPAPER_PLUGIN_ID} plugin is not enabled; videos not launched (live state untouched)"
        ));
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
    plugin_service_clear_all(MPVPAPER_PLUGIN_ID)?;
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

    /// Sandbox home for the confinement tests: the three media roots exist,
    /// nothing else does. Never touches the real HOME.
    fn confine_sandbox(tag: &str) -> PathBuf {
        let sandbox =
            std::env::temp_dir().join(format!("mpvpaper-test-confine-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&sandbox);
        let home = sandbox.join("home");
        for dir in ["Pictures", "Videos", "Downloads"] {
            fs::create_dir_all(home.join(dir)).expect("create media root in sandbox");
        }
        sandbox
    }

    #[test]
    fn confine_video_dir_allows_paths_under_media_roots() {
        let sandbox = confine_sandbox("ok");
        let home = sandbox.join("home");
        let home_s = home.to_string_lossy().to_string();

        // The keeper's real configuration must keep working unchanged.
        let live = home.join("Pictures").join("LiveWallpapers");
        fs::create_dir_all(&live).expect("create LiveWallpapers");
        let real_live = fs::canonicalize(&live).expect("canonicalize LiveWallpapers");
        assert_eq!(confine_video_dir(&live, &home_s).unwrap(), real_live);

        // A media root itself is allowed.
        let videos = home.join("Videos");
        let real_videos = fs::canonicalize(&videos).expect("canonicalize Videos");
        assert_eq!(confine_video_dir(&videos, &home_s).unwrap(), real_videos);

        // A not-yet-existing subdir under a root resolves and is allowed,
        // and the check itself creates nothing (download creates it later).
        let fresh = real_live.join("new-loop");
        assert_eq!(confine_video_dir(&fresh, &home_s).unwrap(), fresh);
        assert!(!fresh.exists(), "the confinement check must not create directories");

        let _ = fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn confine_video_dir_refuses_outside_every_root_and_creates_nothing() {
        let sandbox = confine_sandbox("out");
        let home = sandbox.join("home");
        let home_s = home.to_string_lossy().to_string();

        // The hostile shape from the security audit: a theme pointing
        // video_directory at the keeper's Hyprland config.
        let hostile = sandbox.join(".config").join("hypr");
        let err = confine_video_dir(&hostile, &home_s).unwrap_err();
        assert!(err.contains("allowed"), "unexpected error: {err}");
        assert!(!hostile.exists(), "a refused directory must not be created");

        // An existing directory outside every root is refused too.
        let outside = sandbox.join("elsewhere");
        fs::create_dir_all(&outside).expect("create outside dir");
        let err = confine_video_dir(&outside, &home_s).unwrap_err();
        assert!(err.contains("allowed"), "unexpected error: {err}");

        let _ = fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn confine_video_dir_refuses_dotdot_escape() {
        let sandbox = confine_sandbox("dotdot");
        let home = sandbox.join("home");
        let home_s = home.to_string_lossy().to_string();

        // Pictures/../../evil walks out of the allowed roots and must be
        // refused without creating anything.
        let escape = home.join("Pictures").join("..").join("..").join("evil-dir");
        let err = confine_video_dir(&escape, &home_s).unwrap_err();
        assert!(err.contains("allowed"), "unexpected error: {err}");
        assert!(
            !sandbox.join("evil-dir").exists(),
            "a '..' escape must not create the directory"
        );

        // A '..' over a non-existent component cannot be pinned to a real
        // location, so it is refused as well.
        let unpinnable = home.join("Pictures").join("ghost").join("..").join("..");
        assert!(confine_video_dir(&unpinnable, &home_s).is_err());

        let _ = fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn confine_video_dir_refuses_symlink_pointing_outside() {
        let sandbox = confine_sandbox("symlink");
        let home = sandbox.join("home");
        let home_s = home.to_string_lossy().to_string();

        // LiveWallpapers is a symlink escaping to a directory outside every
        // allowed root: resolving the REAL path is what catches it.
        let outside = sandbox.join("outside-videos");
        fs::create_dir_all(&outside).expect("create outside dir");
        let link = home.join("Pictures").join("LiveWallpapers");
        std::os::unix::fs::symlink(&outside, &link).expect("create escaping symlink");

        let err = confine_video_dir(&link, &home_s).unwrap_err();
        assert!(err.contains("allowed"), "unexpected error: {err}");
        assert_eq!(
            fs::read_dir(&outside).expect("read outside dir").count(),
            0,
            "a refused symlink target must receive no files"
        );

        let _ = fs::remove_dir_all(&sandbox);
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
    /// disabled. The disable+enable bounce and its guard moved to the
    /// neutral runtime seam (`noctalia_runtime::plugin_bounce`, pinned
    /// there by `plugin_bounce_guard_dominates_the_enable_step_by_construction`),
    /// so this backend must reach that guarded bounce BY DELEGATION — the
    /// property is guarded in its new location, not re-implemented here.
    /// `apply_manifest` must still refuse a disabled plugin before any
    /// write or kill. Comment lines are stripped first so a commented-out
    /// guard cannot satisfy this. Needles are built with concat() so this
    /// test's own source — it lives in the file it inspects — can never
    /// satisfy them.
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
        let delegate = ["plugin", "_bounce("].concat();
        after_bounce
            .find(&delegate)
            .expect("bounce_plugin must delegate to the guarded bounce in the runtime seam");
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

    /// The chain this unit breaks: the media backend owns mpvpaper
    /// processes, manifests, downloads and pkill — never the plugin's
    /// supervision. The probe, the bounce and the service IPC live in the
    /// neutral `noctalia_runtime` seam; the only `noctalia` knowledge left
    /// here is the plugin id as data (settings section, error copy) and
    /// comments. Needles are built with concat() so this test's own source
    /// — it lives in the file it inspects — can never satisfy them.
    #[test]
    fn media_backend_sends_no_supervisor_ipc_by_construction() {
        let src = std::fs::read_to_string("src/providers/mpvpaper.rs")
            .expect("src/providers/mpvpaper.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            ["noctalia", "_msg"].concat(),
            ["plugins\", \"dis", "able\""].concat(),
            ["plugins\", \"en", "able\""].concat(),
            ["noctalia/mpvpaper", ":service"].concat(),
        ] {
            assert!(
                !code.contains(&needle),
                "mpvpaper.rs must not contain `{needle}` — the supervisor IPC belongs to \
                 the neutral runtime seam (capability routing: the media backend owns \
                 mpvpaper processes, manifests and downloads, never supervision)"
            );
        }
    }
}