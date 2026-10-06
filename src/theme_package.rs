//! Maintenance packaging entry point (theme-packages T5, part A).
//!
//! HVE is a GUI app with no CLI parsing for its normal use. This module adds a
//! HIDDEN maintenance flag that re-packages an EXISTING saved theme folder in
//! place, using the real T1/T3/T4 helpers (never a second implementation):
//!
//! - `hve --package-theme <name>` packages one theme.
//! - `hve --package-themes` packages every theme in the config themes dir.
//!
//! "Packages" means exactly what a save produces today: resolve the theme's
//! recorded background, write `media/poster.*` + `providers/noctalia-v5/poster.txt`
//! (T1), package the video when it fits + its record or url (T3), and bundle
//! the theme's custom presets under `presets/` (T4).
//!
//! It never starts Slint, prints one short line per theme and exits 0. A theme
//! with no resolvable background record is SKIPPED, not an error; a missing
//! theme NAME is a clear error, never a panic. The packaging path only ever
//! writes inside the given theme dir.

use std::path::{Path, PathBuf};

use crate::theme_manager::{PreviewRole, ThemeProvider};

/// The hidden flag that packages one named theme.
pub const PACKAGE_THEME_FLAG: &str = "--package-theme";

/// The hidden flag that packages every theme in the config themes dir.
pub const PACKAGE_THEMES_FLAG: &str = "--package-themes";

/// A maintenance command parsed from the process arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenanceCommand {
    PackageTheme(String),
    PackageThemes,
}

/// The outcome of scanning the process arguments for a maintenance invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenanceParse {
    /// Not a maintenance invocation: the normal GUI path continues.
    NotMaintenance,
    /// A maintenance command to run instead of starting the GUI.
    Run(MaintenanceCommand),
    /// A maintenance-looking flag that is invalid. The caller prints the
    /// message and exits non-zero; the GUI is never started.
    Invalid(String),
}

/// What happened to the theme's optional video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoOutcome {
    /// The theme has no video (a static background, or a video that could not
    /// be packaged and has no known url).
    None,
    /// The video was copied into the theme as `media/background.mp4`.
    Packaged,
    /// The video was too large to copy; its url was recorded instead.
    Url,
}

/// What a successful package produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSummary {
    /// Whether `media/poster.*` and its record were written.
    pub poster: bool,
    /// What happened to the video.
    pub video: VideoOutcome,
    /// How many custom presets were bundled under `presets/`.
    pub presets: usize,
}

/// The result of packaging one theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageStatus {
    Packaged(PackageSummary),
    /// Nothing to package, with the reason. Not an error.
    Skipped(String),
}

impl PackageStatus {
    /// A short human-readable line body (`packaged (…)` / `skipped (…)`).
    pub fn describe(&self) -> String {
        match self {
            PackageStatus::Packaged(s) => format!(
                "packaged (poster={}, video={}, presets={})",
                if s.poster { "yes" } else { "no" },
                match s.video {
                    VideoOutcome::None => "none",
                    VideoOutcome::Packaged => "file",
                    VideoOutcome::Url => "url",
                },
                s.presets
            ),
            PackageStatus::Skipped(reason) => format!("skipped ({reason})"),
        }
    }
}

/// Pure argument parser for the hidden maintenance flags.
///
/// `args` is the process arguments WITHOUT argv[0]. A recognised maintenance
/// flag yields [`MaintenanceParse::Run`]; a malformed one (a missing theme
/// name, an unknown `--package*` flag, or two commands at once) yields
/// [`MaintenanceParse::Invalid`]; anything else yields
/// [`MaintenanceParse::NotMaintenance`], so the GUI path is byte-for-byte
/// unchanged. A non-`--package` unknown flag is left to the GUI parser, which
/// refuses it without starting a window.
pub fn parse_maintenance(args: &[String]) -> MaintenanceParse {
    let mut command: Option<MaintenanceCommand> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == PACKAGE_THEMES_FLAG {
            if command.is_some() {
                return MaintenanceParse::Invalid(
                    "only one maintenance command may be given at a time".into(),
                );
            }
            command = Some(MaintenanceCommand::PackageThemes);
        } else if arg == PACKAGE_THEME_FLAG {
            if command.is_some() {
                return MaintenanceParse::Invalid(
                    "only one maintenance command may be given at a time".into(),
                );
            }
            let Some(name) = args.get(i + 1) else {
                return MaintenanceParse::Invalid(format!(
                    "{PACKAGE_THEME_FLAG} needs a theme name"
                ));
            };
            let name = name.trim();
            if name.is_empty() || name.starts_with('-') {
                return MaintenanceParse::Invalid(format!(
                    "{PACKAGE_THEME_FLAG} needs a theme name"
                ));
            }
            command = Some(MaintenanceCommand::PackageTheme(name.to_string()));
            i += 1; // consume the name
        } else if arg.starts_with("--package") {
            return MaintenanceParse::Invalid(format!("unknown maintenance flag '{arg}'"));
        }
        i += 1;
    }
    match command {
        Some(command) => MaintenanceParse::Run(command),
        None => MaintenanceParse::NotMaintenance,
    }
}

/// The background source(s) a saved theme records, in the same precedence the
/// apply path uses: the painter video record, then the static wallpaper record,
/// then the mpvpaper manifest's local candidates.
struct BackgroundSources {
    /// The file a poster can be produced from (static image or video).
    poster: Option<PathBuf>,
    /// The file when the background is a video.
    video: Option<PathBuf>,
}

/// Resolve the theme's recorded background. Every path is read from the theme's
/// own provider records; a record naming a file that no longer exists is
/// ignored, exactly like the apply fall-through.
///
/// `manifest_candidates` are the live-assignment candidates the theme's
/// provider DECLARED for its own manifest (see [`assignment_candidates`]): the
/// core never parses that record format itself.
fn resolve_background_sources(
    provider_dir: &Path,
    manifest_candidates: &[PathBuf],
) -> BackgroundSources {
    // 1. Painter-identified video record (video.txt): wins, like the authority
    //    capture did at save time.
    if let Some(path) = std::fs::read_to_string(provider_dir.join(
        crate::providers::wallpaper_authority::PAINTER_VIDEO_FILE,
    ))
    .ok()
    .and_then(|text| crate::providers::wallpaper_authority::parse_painter_video_record(&text))
    {
        if path.is_file() {
            return BackgroundSources {
                poster: Some(path.clone()),
                video: Some(path),
            };
        }
    }

    // 2. Static wallpaper record.
    if let Ok(text) = std::fs::read_to_string(provider_dir.join("wallpaper.txt")) {
        let path = PathBuf::from(text.trim());
        if path.is_file() {
            return sources_for(&path);
        }
    }

    // 3. The provider's declared live-assignment candidates.
    for candidate in manifest_candidates {
        if candidate.is_file() {
            return sources_for(candidate);
        }
    }

    BackgroundSources {
        poster: None,
        video: None,
    }
}

/// The live-assignment candidate paths the theme's provider DECLARES for
/// `provider_dir` — the `Assignment` preview role, in the record's own order.
/// The core reads only the declaration: it never parses the provider's
/// manifest format (that knowledge lives with the provider).
fn assignment_candidates(provider: &dyn ThemeProvider, provider_dir: &Path) -> Vec<PathBuf> {
    provider
        .preview_sources(provider_dir)
        .into_iter()
        .find(|source| source.role == PreviewRole::Assignment)
        .map(|source| source.paths)
        .unwrap_or_default()
}

/// Classify one existing file as a static image or a video source.
fn sources_for(path: &Path) -> BackgroundSources {
    let is_video = crate::shell::gallery::thumbs::is_video_source(path);
    BackgroundSources {
        poster: Some(path.to_path_buf()),
        video: is_video.then(|| path.to_path_buf()),
    }
}

/// Read the theme's `hve-presets` state into the active-preset selection. A
/// missing or unreadable state is not an error: no presets are bundled.
fn read_active_presets(state_path: &Path) -> crate::theme_presets::ActivePresets {
    let Ok(raw) = std::fs::read_to_string(state_path) else {
        return crate::theme_presets::ActivePresets::default();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return crate::theme_presets::ActivePresets::default();
    };
    let get = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    crate::theme_presets::ActivePresets {
        animations: get("active_anim_file"),
        borders: get("active_border_file"),
        shaders: get("active_shader_file"),
    }
}

/// Package `theme_dir` in place using the real T1/T3/T4 helpers.
///
/// Returns [`PackageStatus::Skipped`] when the theme carries no resolvable
/// background record. Otherwise the poster, the video and the custom presets
/// are (re)packaged exactly like a save. The only files ever written live
/// inside `theme_dir`.
pub fn package_theme_dir(
    theme_dir: &Path,
    assets_dir: &Path,
    user_presets_root: &Path,
    max_video_bytes: u64,
) -> PackageStatus {
    let provider_dir = theme_dir.join("providers").join("noctalia-v5");
    // The theme's background provider, asked through the registration router
    // (never a hardcoded record format): it declares its own live-assignment
    // candidates and knows the url its own manifest holds.
    let provider = crate::providers::declaration_provider("noctalia-v5");
    let manifest_candidates = provider
        .as_deref()
        .map(|p| assignment_candidates(p, &provider_dir))
        .unwrap_or_default();
    let sources = resolve_background_sources(&provider_dir, &manifest_candidates);
    let Some(poster_source) = sources.poster else {
        return PackageStatus::Skipped("no background record".into());
    };

    // T1: poster. Best-effort, like a save: a failure drops any stale poster
    // and continues, so the theme is never left declaring an image it lacks.
    let poster = match crate::theme_media::write_theme_poster(theme_dir, &poster_source) {
        Some(relative) => match crate::theme_media::write_poster_record(&provider_dir, &relative) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("[theme-package] cannot record the packaged poster: {e}");
                false
            }
        },
        None => false,
    };
    if !poster {
        crate::theme_media::remove_theme_poster(theme_dir, &provider_dir);
    }

    // T3: video, only when the background actually is a video. An over-bound
    // video records its known url instead of being copied, exactly like a save.
    let video = match sources.video.as_deref() {
        Some(source) => {
            match crate::theme_media::package_theme_video(theme_dir, source, max_video_bytes) {
                Some(relative) => {
                    match crate::theme_media::write_video_record(&provider_dir, &relative) {
                        Ok(()) => VideoOutcome::Packaged,
                        Err(e) => {
                            tracing::warn!("[theme-package] cannot record the packaged video: {e}");
                            VideoOutcome::None
                        }
                    }
                }
                None => match provider
                    .as_deref()
                    .and_then(|p| p.video_url(&provider_dir, source))
                {
                    Some(known) => {
                        match crate::theme_media::write_video_url_record(
                            &provider_dir,
                            &known.url,
                            known.filename.as_deref(),
                            known.sha256.as_deref(),
                        ) {
                            Ok(()) => VideoOutcome::Url,
                            Err(e) => {
                                tracing::warn!("[theme-package] cannot record the video url: {e}");
                                VideoOutcome::None
                            }
                        }
                    }
                    None => VideoOutcome::None,
                },
            }
        }
        None => VideoOutcome::None,
    };
    if video != VideoOutcome::Packaged && video != VideoOutcome::Url {
        crate::theme_media::remove_theme_video(theme_dir, &provider_dir);
    }

    // T4: bundle the theme's custom presets. Best-effort, like a save.
    let active = read_active_presets(&theme_dir.join("providers").join("hve-presets").join("state.json"));
    let presets = match crate::theme_presets::ShippedPresets::load(assets_dir) {
        Some(shipped) => crate::theme_presets::bundle_theme_presets(
            theme_dir,
            &active,
            &shipped,
            user_presets_root,
        )
        .len(),
        None => {
            tracing::warn!("[theme-package] cannot read the shipped preset set; skipping presets");
            0
        }
    };

    PackageStatus::Packaged(PackageSummary {
        poster,
        video,
        presets,
    })
}

/// Resolve the theme dir for `name` under `themes_root`, guarding the name so
/// it can never escape the themes root.
fn theme_dir_for(themes_root: &Path, name: &str) -> Result<PathBuf, String> {
    crate::preset_store::PresetStore::validate_name(name)
        .map_err(|reason| format!("invalid theme name '{name}': {reason}"))?;
    Ok(themes_root.join(name))
}

/// The theme directories under `themes_root`, sorted by name. A non-directory
/// entry is ignored.
fn list_theme_dirs(themes_root: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(themes_root)
        .map_err(|e| format!("cannot read the themes dir {}: {e}", themes_root.display()))?;
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    Ok(dirs)
}

/// Run one maintenance command, writing one line per theme to `out`.
///
/// Returns the process exit code: 0 on success (including skipped themes), 1
/// for a missing theme or an unreadable themes dir, 2 for an invalid theme
/// name. All roots are explicit so tests never touch the real config dir.
pub fn run_maintenance(
    command: &MaintenanceCommand,
    themes_root: &Path,
    assets_dir: &Path,
    user_presets_root: &Path,
    max_video_bytes: u64,
    out: &mut dyn std::io::Write,
) -> i32 {
    match command {
        MaintenanceCommand::PackageTheme(name) => {
            let theme_dir = match theme_dir_for(themes_root, name) {
                Ok(dir) => dir,
                Err(message) => {
                    let _ = writeln!(out, "error: {message}");
                    return 2;
                }
            };
            if !theme_dir.is_dir() {
                let _ = writeln!(
                    out,
                    "error: no theme named '{name}' in {}",
                    themes_root.display()
                );
                return 1;
            }
            let status = package_theme_dir(&theme_dir, assets_dir, user_presets_root, max_video_bytes);
            let _ = writeln!(out, "{name}: {}", status.describe());
            0
        }
        MaintenanceCommand::PackageThemes => {
            let dirs = match list_theme_dirs(themes_root) {
                Ok(dirs) => dirs,
                Err(message) => {
                    let _ = writeln!(out, "error: {message}");
                    return 1;
                }
            };
            if dirs.is_empty() {
                let _ = writeln!(out, "no themes in {}", themes_root.display());
                return 0;
            }
            for theme_dir in dirs {
                let name = theme_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?");
                let status =
                    package_theme_dir(&theme_dir, assets_dir, user_presets_root, max_video_bytes);
                let _ = writeln!(out, "{name}: {}", status.describe());
            }
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // ── Pure argument parsing ────────────────────────────────────────────

    #[test]
    fn parse_maintenance_recognizes_the_two_flags() {
        assert_eq!(
            parse_maintenance(&args(&["--package-theme", "Joker"])),
            MaintenanceParse::Run(MaintenanceCommand::PackageTheme("Joker".into()))
        );
        assert_eq!(
            parse_maintenance(&args(&["--package-themes"])),
            MaintenanceParse::Run(MaintenanceCommand::PackageThemes)
        );
    }

    #[test]
    fn parse_maintenance_leaves_the_gui_path_untouched() {
        assert_eq!(parse_maintenance(&args(&[])), MaintenanceParse::NotMaintenance);
        assert_eq!(
            parse_maintenance(&args(&["--tray"])),
            MaintenanceParse::NotMaintenance
        );
        // A non-package unknown flag is the GUI parser's business: it refuses
        // it without starting a window. The maintenance parser stays out.
        assert_eq!(
            parse_maintenance(&args(&["--frobnicate"])),
            MaintenanceParse::NotMaintenance
        );
    }

    #[test]
    fn parse_maintenance_rejects_a_missing_theme_name() {
        assert!(matches!(
            parse_maintenance(&args(&["--package-theme"])),
            MaintenanceParse::Invalid(_)
        ));
        assert!(matches!(
            parse_maintenance(&args(&["--package-theme", ""])),
            MaintenanceParse::Invalid(_)
        ));
        // A following flag is not a theme name.
        assert!(matches!(
            parse_maintenance(&args(&["--package-theme", "--package-themes"])),
            MaintenanceParse::Invalid(_)
        ));
    }

    #[test]
    fn parse_maintenance_rejects_an_unknown_maintenance_flag() {
        assert!(matches!(
            parse_maintenance(&args(&["--package-foo"])),
            MaintenanceParse::Invalid(_)
        ));
        assert!(matches!(
            parse_maintenance(&args(&["--package-them", "Joker"])),
            MaintenanceParse::Invalid(_)
        ));
    }

    #[test]
    fn parse_maintenance_rejects_two_commands_at_once() {
        assert!(matches!(
            parse_maintenance(&args(&["--package-theme", "A", "--package-themes"])),
            MaintenanceParse::Invalid(_)
        ));
        assert!(matches!(
            parse_maintenance(&args(&["--package-themes", "--package-theme", "A"])),
            MaintenanceParse::Invalid(_)
        ));
    }

    // ── Packaging one theme (temp dirs only) ─────────────────────────────

    fn provider_dir(theme: &Path) -> PathBuf {
        theme.join("providers").join("noctalia-v5")
    }

    fn write_static_theme(root: &Path, theme_name: &str, image_bytes: &[u8]) -> (PathBuf, PathBuf) {
        let theme = root.join("themes").join(theme_name);
        let provider = provider_dir(&theme);
        fs::create_dir_all(&provider).unwrap();
        let image = root.join(format!("{theme_name}.jpg"));
        fs::write(&image, image_bytes).unwrap();
        fs::write(provider.join("wallpaper.txt"), image.display().to_string()).unwrap();
        (theme, image)
    }

    #[test]
    fn package_theme_copies_a_static_wallpaper_and_records_the_poster() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"\xff\xd8\xff\xe0static-wallpaper-bytes";
        let (theme, _image) = write_static_theme(root.path(), "Static", bytes);

        let status = package_theme_dir(
            &theme,
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
        );

        assert_eq!(
            status,
            PackageStatus::Packaged(PackageSummary {
                poster: true,
                video: VideoOutcome::None,
                presets: 0,
            })
        );
        assert_eq!(
            fs::read(theme.join("media/poster.jpg")).unwrap(),
            bytes,
            "the static wallpaper must be copied byte-for-byte"
        );
        let record = fs::read_to_string(provider_dir(&theme).join("poster.txt")).unwrap();
        assert_eq!(record.trim(), "poster=media/poster.jpg");
    }

    #[test]
    fn package_theme_skips_a_theme_without_background_records() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Empty");
        fs::create_dir_all(provider_dir(&theme)).unwrap();

        let status = package_theme_dir(
            &theme,
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
        );

        assert!(
            matches!(status, PackageStatus::Skipped(_)),
            "a theme with no background record must be skipped, got {status:?}"
        );
        assert!(
            !theme.join("media").exists(),
            "a skipped theme must not gain a media dir"
        );
    }

    #[test]
    fn package_theme_packages_a_video_and_records_it() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Clip");
        let provider = provider_dir(&theme);
        fs::create_dir_all(&provider).unwrap();
        let video = root.path().join("clip.mp4");
        fs::write(&video, b"tiny-video-bytes").unwrap();
        fs::write(
            provider.join("video.txt"),
            format!("path={}\npainter=\nnamespace=\npid=0\n", video.display()),
        )
        .unwrap();

        let status = package_theme_dir(
            &theme,
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
        );

        match status {
            PackageStatus::Packaged(s) => assert_eq!(s.video, VideoOutcome::Packaged),
            other => panic!("expected a packaged video, got {other:?}"),
        }
        assert_eq!(
            fs::read(theme.join("media/background.mp4")).unwrap(),
            b"tiny-video-bytes",
            "the video must be copied byte-for-byte"
        );
        let record = fs::read_to_string(provider.join("video-media.txt")).unwrap();
        assert_eq!(record.trim(), "video=media/background.mp4");
    }

    #[test]
    fn package_theme_records_a_url_for_an_over_bound_video() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Big");
        let provider = provider_dir(&theme);
        fs::create_dir_all(&provider).unwrap();
        let video = root.path().join("big.mp4");
        fs::write(&video, b"ten-bytes!").unwrap();
        fs::write(
            provider.join("video.txt"),
            format!("path={}\n", video.display()),
        )
        .unwrap();
        fs::write(
            provider.join("mpvpaper-assignments.json"),
            format!(
                r#"{{"version":1,"assignments":{{"*":{{"filename":"big.mp4","local_path":"{}","url":"https://example.com/big.mp4"}}}}}}"#,
                video.display()
            ),
        )
        .unwrap();

        let status = package_theme_dir(
            &theme,
            &root.path().join("assets"),
            &root.path().join("presets"),
            4,
        );

        match status {
            PackageStatus::Packaged(s) => assert_eq!(s.video, VideoOutcome::Url),
            other => panic!("expected an over-bound video to record a url, got {other:?}"),
        }
        assert!(
            !theme.join("media/background.mp4").exists(),
            "an over-bound video must never be copied"
        );
        let record = fs::read_to_string(provider.join("video-media.txt")).unwrap();
        assert!(record.contains("url=https://example.com/big.mp4"), "{record}");
    }

    #[test]
    fn package_theme_bundles_custom_presets_and_skips_shipped() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"\xff\xd8\xffwallpaper";
        let (theme, _image) = write_static_theme(root.path(), "Presets", bytes);

        // The theme's saved active selection: one custom, one shipped.
        let presets_provider = theme.join("providers/hve-presets");
        fs::create_dir_all(&presets_provider).unwrap();
        fs::write(
            presets_provider.join("state.json"),
            r#"{"active_anim_file":"custom.lua","active_border_file":"shipped.lua","active_shader_file":""}"#,
        )
        .unwrap();

        // The app's shipped set and the user's custom store.
        let assets = root.path().join("assets");
        fs::create_dir_all(assets.join("borders")).unwrap();
        fs::write(assets.join("borders/shipped.lua"), "-- shipped").unwrap();
        for category in crate::theme_presets::CATEGORIES {
            fs::create_dir_all(assets.join(category)).unwrap();
        }
        let user_root = root.path().join("presets");
        fs::create_dir_all(user_root.join("animations")).unwrap();
        fs::write(user_root.join("animations/custom.lua"), b"-- custom\n").unwrap();

        let status = package_theme_dir(&theme, &assets, &user_root, 1024);

        match status {
            PackageStatus::Packaged(s) => assert_eq!(s.presets, 1),
            other => panic!("expected a packaged theme, got {other:?}"),
        }
        assert_eq!(
            fs::read(theme.join("presets/animations/custom.lua")).unwrap(),
            b"-- custom\n",
            "the custom preset must travel byte-for-byte"
        );
        assert!(
            !theme.join("presets/borders/shipped.lua").exists(),
            "a shipped preset must never be copied into the theme"
        );
    }

    #[test]
    fn package_theme_writes_nothing_outside_the_theme_dir() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"\xff\xd8\xffwallpaper";
        let (theme, _image) = write_static_theme(root.path(), "Guarded", bytes);
        let outside = root.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("sentinel.txt"), b"do not touch").unwrap();

        package_theme_dir(
            &theme,
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
        );

        assert_eq!(
            fs::read(outside.join("sentinel.txt")).unwrap(),
            b"do not touch",
            "the packaging path must never touch a file outside the theme"
        );
        assert_eq!(
            fs::read_dir(&outside).unwrap().count(),
            1,
            "no new file may appear outside the theme"
        );
    }

    // ── Running the command (temp dirs only) ─────────────────────────────

    #[test]
    fn run_maintenance_reports_a_missing_theme_as_an_error() {
        let root = tempfile::tempdir().unwrap();
        let themes = root.path().join("themes");
        fs::create_dir_all(&themes).unwrap();
        let mut out = Vec::new();

        let code = run_maintenance(
            &MaintenanceCommand::PackageTheme("Ghost".into()),
            &themes,
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
            &mut out,
        );

        assert_eq!(code, 1, "a missing theme must be a clear error, not a panic");
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Ghost"), "{text}");
    }

    #[test]
    fn run_maintenance_rejects_an_unsafe_theme_name() {
        let root = tempfile::tempdir().unwrap();
        let themes = root.path().join("themes");
        fs::create_dir_all(&themes).unwrap();
        let mut out = Vec::new();

        let code = run_maintenance(
            &MaintenanceCommand::PackageTheme("../escape".into()),
            &themes,
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
            &mut out,
        );

        assert_eq!(code, 2, "an unsafe theme name must be refused");
        assert!(
            !root.path().join("escape").exists(),
            "an unsafe name must never write outside the themes root"
        );
    }

    #[test]
    fn run_maintenance_packages_every_theme_and_reports_each() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"\xff\xd8\xffwallpaper";
        write_static_theme(root.path(), "Alpha", bytes);
        let empty = root.path().join("themes/Empty");
        fs::create_dir_all(provider_dir(&empty)).unwrap();
        let mut out = Vec::new();

        let code = run_maintenance(
            &MaintenanceCommand::PackageThemes,
            &root.path().join("themes"),
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
            &mut out,
        );

        assert_eq!(code, 0);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Alpha") && text.contains("packaged"), "{text}");
        assert!(text.contains("Empty") && text.contains("skipped"), "{text}");
    }
}
