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
    //    capture did at save time. W10: bounded — the record is untrusted
    //    theme input, and an oversized one is refused like a missing one.
    if let Some(path) = read_theme_text(&provider_dir.join(
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

    // 2. Static wallpaper record (W10: same bounded read).
    if let Ok(text) = read_theme_text(&provider_dir.join("wallpaper.txt")) {
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
/// W10: bounded — an oversized (untrusted) state file refuses like a
/// missing one (no presets), never an unbounded read.
fn read_active_presets(state_path: &Path) -> crate::theme_presets::ActivePresets {
    let Ok(raw) = read_theme_text(state_path) else {
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
/// background record, or when `theme_dir` is a SYMLINK (refused before any
/// read or write — see the guard below). Otherwise the poster, the video and
/// the custom presets are (re)packaged exactly like a save. The only files
/// ever written live inside `theme_dir`, which the symlink guard keeps a real
/// directory.
pub fn package_theme_dir(
    theme_dir: &Path,
    assets_dir: &Path,
    user_presets_root: &Path,
    max_video_bytes: u64,
) -> PackageStatus {
    // W10c-c (CRITICAL-class): the scan's own symlink-root refusal
    // (`scan_theme_dir`) is NOT this operation's guard — it only stops the
    // walk, and this function kept going, so `sanitize_settings_file` still
    // wrote through `theme_dir` (`noctalia.rs`) and the poster/preset writes
    // landed in the symlink target — OUTSIDE the themes root. A symlinked
    // theme dir is refused HERE, before the first read or write, with ONE
    // warning, so nothing escapes: `is_symlink()` inspects the path itself
    // (it does not follow the final link), which is exactly the property that
    // must be refused. The honest limit, stated for the record: this guard
    // covers a symlinked theme directory handed in as the root; a symlinked
    // CHILD inside a real theme dir is the walk's job (`scan_theme_dir`).
    if theme_dir.is_symlink() {
        tracing::warn!(
            "[theme-package] refusing to package {}: a symlinked theme directory \
             must not read or write outside the themes root",
            theme_dir.display()
        );
        return PackageStatus::Skipped(
            "symlinked theme directory (refused, nothing read or written)".into(),
        );
    }
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

    // The theme's saved settings are cleaned whether or not there is a
    // background left to (re)package: a pass that has nothing else to do must
    // still not leave a personal path in the theme.
    let settings = provider_dir.join("settings.toml");
    if settings.is_file() {
        crate::providers::noctalia::sanitize_settings_file(&settings, theme_dir);
    }

    let Some(poster_source) = sources.poster else {
        // No background to package, but the absolute records still name this
        // machine: they go, and the missing packaged equivalent is reported.
        crate::providers::noctalia::drop_absolute_background_records(&provider_dir, false, false);
        report_remaining_absolute_paths(theme_dir);
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

    // The absolute records are what the resolution above read: they go now,
    // after the packaged equivalents exist. `poster.txt` / `video-media.txt`
    // is what a target machine resolves.
    crate::providers::noctalia::drop_absolute_background_records(
        &provider_dir,
        poster,
        video == VideoOutcome::Packaged || video == VideoOutcome::Url,
    );

    // The safety net (U3): whatever absolute path is still in the theme is a
    // leak the sanitizer does not own, and must be visible now.
    report_remaining_absolute_paths(theme_dir);

    PackageStatus::Packaged(PackageSummary {
        poster,
        video,
        presets,
    })
}

// ── W10 (theme-package trust): bounded reads of an untrusted package ──
//
// A theme package is UNTRUSTED INPUT: anyone can hand HVE a directory that
// claims to be a theme. Before these bounds, one file of many GB exhausted
// memory on apply or on `--package-themes`, and a pathologically deep tree
// could drive the recursive scan toward the stack limit.
//
// WHY THESE NUMBERS, in one place:
//
// - 4 MiB PER FILE: the largest legitimate text record a theme carries is
//   its `settings.toml` (a full bar/layout config), and it measures in the
//   tens of KiB — 4 MiB is ~100x that headroom, while a multi-GB file is
//   refused after reading bound+1 bytes instead of being slurped whole.
// - 64 MiB PER SCAN (total): a legit theme carries well under 1 MiB of
//   text; the total bounds the peak memory of ONE whole-theme scan even
//   when every file individually passes. The only overshoot is one
//   per-file cap (the refusal is decided after the capped read), so the
//   peak is total + per-file by construction.
// - DEPTH 32: a real theme tree is `theme/providers/<id>/…` and never runs
//   deeper than ~5 levels; 32 is order-of-magnitude headroom, and the walk
//   can never grow toward the 8 MiB stack (a symlink cycle also terminates,
//   because every lap spends depth AND entries).
// - 10_000 ENTRIES: real themes carry well under 200 files; the entry cap
//   bounds the loop work and the collected hits (each hit owns two Strings)
//   so a directory of millions of entries is refused early.
//
// A refusal is always a WARNING plus a graceful skip — the caller keeps
// whatever it could read, and nothing here ever panics.

/// Per-file bound: see the block above for the WHY of every number.
pub const MAX_THEME_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Total bytes one whole-theme scan may read — see the block above.
pub const MAX_THEME_TOTAL_SCAN_BYTES: u64 = 64 * 1024 * 1024;

/// Directory levels below the theme root the scan may descend — see above.
pub const MAX_THEME_SCAN_DEPTH: usize = 32;

/// Files + directories one scan may visit — see the block above.
pub const MAX_THEME_SCAN_ENTRIES: usize = 10_000;

/// The bounds an untrusted theme package is read under ([`scan_theme_for_absolute_paths`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeScanLimits {
    /// Largest single file the scan reads, in bytes.
    pub max_file_bytes: u64,
    /// Total bytes the whole scan may read.
    pub max_total_bytes: u64,
    /// Directory levels below the theme root the scan may descend.
    pub max_depth: usize,
    /// Files + directories the scan may visit.
    pub max_entries: usize,
}

impl Default for ThemeScanLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: MAX_THEME_FILE_BYTES,
            max_total_bytes: MAX_THEME_TOTAL_SCAN_BYTES,
            max_depth: MAX_THEME_SCAN_DEPTH,
            max_entries: MAX_THEME_SCAN_ENTRIES,
        }
    }
}

/// What one bounded scan has left to spend, plus the once-per-scan warning
/// flags (a pathological package must not emit a warning per entry).
struct ScanBudget {
    limits: ThemeScanLimits,
    entries_left: usize,
    bytes_left: u64,
    warned_depth: bool,
    warned_entries: bool,
    warned_bytes: bool,
    warned_symlink_dir: bool,
}

impl ScanBudget {
    fn new(limits: ThemeScanLimits) -> Self {
        Self {
            limits,
            entries_left: limits.max_entries,
            bytes_left: limits.max_total_bytes,
            warned_depth: false,
            warned_entries: false,
            warned_bytes: false,
            warned_symlink_dir: false,
        }
    }
}

/// Why a bounded theme read refused to hand back bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeReadError {
    /// The file is over the per-file bound: the read stopped after
    /// bound+1 bytes and a warning was already emitted. Callers use
    /// [`ThemeReadError::is_oversized`] to degrade gracefully (skip this
    /// file, keep working) instead of hard-failing.
    TooLarge,
    /// The file could not be opened or read (or, for [`read_theme_text`],
    /// is not valid UTF-8).
    Unreadable(String),
}

impl ThemeReadError {
    /// Whether this refusal was the size bound — as opposed to an ordinary
    /// I/O error, whose handling callers keep byte-for-byte as today.
    pub fn is_oversized(&self) -> bool {
        matches!(self, ThemeReadError::TooLarge)
    }
}

impl std::fmt::Display for ThemeReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeReadError::TooLarge => write!(
                f,
                "larger than the {MAX_THEME_FILE_BYTES}-byte bound for an untrusted theme file"
            ),
            ThemeReadError::Unreadable(detail) => write!(f, "{detail}"),
        }
    }
}

/// Read at most `max_bytes` + 1 bytes of `path`.
///
/// W10: the size bound is enforced by the READ itself (`Read::take`), not by
/// a stat-then-read — so a file that grows between the check and the read (or
/// a `/proc` entry that reports size 0) can never open an unbounded window.
///
/// W10c: the REGULAR-FILE guard lives HERE, in the lowest-level reader every
/// bounded theme read goes through (`read_theme_text`, `read_theme_bytes`
/// and the leak scan) — not in a wrapper that call sites can bypass. A FIFO,
/// socket or device named like a theme record makes `File::open` block
/// forever (FIFO) or mislead (device), so it must be refused BEFORE the
/// open, where the `Read::take` bound cannot help. A refusal warns (clear,
/// once per file) and comes back as `Err`; ordinary I/O errors come back as
/// `Err` without a warning, so callers keep their lenient "missing file is
/// not an error" behaviour.
fn read_theme_bytes_capped(path: &Path, max_bytes: u64) -> Result<Vec<u8>, ThemeReadError> {
    use std::io::Read;
    // W10c: `theme_file_is_regular` follows symlinks, so a symlink to a
    // regular file still reads (documented behaviour) while a symlink to a
    // FIFO is refused here. A path that cannot be stat'ed answers `true`,
    // so the open below reports it exactly as today.
    if !crate::theme_media::theme_file_is_regular(path) {
        tracing::warn!(
            "[theme-package] refusing to read {}: it is not a regular file (a FIFO, socket \
             or device named like a theme record would block or mislead)",
            path.display()
        );
        return Err(ThemeReadError::Unreadable(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let file = std::fs::File::open(path)
        .map_err(|e| ThemeReadError::Unreadable(format!("cannot open {}: {e}", path.display())))?;
    let mut bytes = Vec::new();
    let mut reader = file.take(max_bytes.saturating_add(1));
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| ThemeReadError::Unreadable(format!("cannot read {}: {e}", path.display())))?;
    if bytes.len() as u64 > max_bytes {
        tracing::warn!(
            "[theme-package] refusing to read {}: it is larger than the {}-byte bound \
             for an untrusted theme file",
            path.display(),
            max_bytes
        );
        return Err(ThemeReadError::TooLarge);
    }
    Ok(bytes)
}

/// Read one theme text record under the per-file bound ([`MAX_THEME_FILE_BYTES`]).
///
/// W10: apply- and package-side reads of a theme package go through this
/// instead of `fs::read_to_string`, so one oversized (or race-growing) file
/// can never exhaust memory. A refusal warns and comes back as `Err`;
/// callers treat it exactly like today's read error (skip / warn / refuse).
pub fn read_theme_text(path: &Path) -> Result<String, ThemeReadError> {
    let bytes = read_theme_bytes_capped(path, MAX_THEME_FILE_BYTES)?;
    String::from_utf8(bytes).map_err(|_| {
        ThemeReadError::Unreadable(format!("{} is not valid UTF-8 text", path.display()))
    })
}

/// Read one theme binary record under the per-file bound (same contract as
/// [`read_theme_text`], for records that are not text — e.g. `palette.json`
/// snapshots read as raw bytes).
pub fn read_theme_bytes(path: &Path) -> Result<Vec<u8>, ThemeReadError> {
    read_theme_bytes_capped(path, MAX_THEME_FILE_BYTES)
}

/// Whether `path` is safe to copy under the per-file bound — the
/// pre-flight for a `fs::copy` of untrusted theme bytes (a streamed copy
/// allocates no memory, but it would otherwise plant untrusted bytes in
/// the live config for a later unbounded read). Warns and answers `false`
/// when the file is over the bound; a file whose size cannot be
/// determined answers `true` so the caller's own error handling stays in
/// charge exactly as today. The stat/copy race it cannot close is bounded:
/// the copy itself is streamed, never a memory allocation.
///
/// W10c: a NON-REGULAR file also fails the pre-flight, with a warning
/// naming it. `fs::metadata` answers `len = 0` for a FIFO, so the
/// size-only check called it "bounded" and `fs::copy` would block on the
/// open for ever. A symlink to a regular file stays accepted (the stat
/// follows links, like the copy itself).
pub fn theme_file_is_bounded(path: &Path) -> bool {
    if !crate::theme_media::theme_file_is_regular(path) {
        tracing::warn!(
            "[theme-package] refusing to copy {}: it is not a regular file (a FIFO, socket \
             or device named like a theme record would block or mislead)",
            path.display()
        );
        return false;
    }
    match std::fs::metadata(path).map(|m| m.len()) {
        Ok(len) if len > MAX_THEME_FILE_BYTES => {
            tracing::warn!(
                "[theme-package] refusing to copy {}: it is larger than the {}-byte \
                 bound for an untrusted theme file",
                path.display(),
                MAX_THEME_FILE_BYTES
            );
            false
        }
        _ => true,
    }
}

/// Every absolute-path-looking value left in the theme's text files, as
/// `(file, value)` pairs, in a deterministic order.
///
/// The safety net of the sanitizer: a leak a future save reintroduces must be
/// visible at package time, not discovered by a user. `media/` holds the
/// packaged binaries (poster, video) and is never scanned; a file that is not
/// valid UTF-8 is skipped for the same reason.
///
/// W10 (theme-package trust): a theme package is UNTRUSTED input, so the
/// walk runs under [`ThemeScanLimits::default()`]. A file, tree or entry
/// count over the bound is refused with a warning and skipped — the scan
/// returns everything it could read instead of exhausting memory or stack.
/// W10c-b: a root that IS a symlink is refused with one warning and walked
/// not at all — see [`scan_theme_dir`].
pub fn scan_theme_for_absolute_paths(theme_dir: &Path) -> Vec<(PathBuf, String)> {
    scan_theme_for_absolute_paths_with_limits(theme_dir, ThemeScanLimits::default())
}

/// The same scan driven by EXPLICIT limits — the seam the bound tests use
/// (the production defaults are far too generous to fixture cheaply).
pub fn scan_theme_for_absolute_paths_with_limits(
    theme_dir: &Path,
    limits: ThemeScanLimits,
) -> Vec<(PathBuf, String)> {
    let mut hits: Vec<(PathBuf, String)> = Vec::new();
    let mut budget = ScanBudget::new(limits);
    scan_theme_dir(theme_dir, 0, &mut budget, &mut hits);
    hits
}

/// Walk `dir` depth-first in sorted order, collecting the hits of every text
/// file under it. The packaged `media/` directory is skipped whole, and so
/// is any symlinked DIRECTORY — the ROOT included: the walk never leaves
/// the package through a symlink, whether it is met as a child entry
/// (checked in the loop below) or handed in as the root itself (checked
/// here first, W10c-b: both sources of roots follow links — the gallery's
/// `ThemeManager::list` (`theme_manager.rs`, `is_dir()` follows links) and
/// this module's maintenance `list_theme_dirs` (same filter) — so a
/// themes-root entry like `Evil -> ~` IS handed in as a root). Exactly
/// what is NOT protected, stated for the record: a symlink to a regular
/// FILE is still read through by design (a root symlink to a file cannot
/// be walked at all — `read_dir` on it fails and nothing is read), and the
/// gallery LISTING still shows a symlinked theme, because the leak this
/// closes is the walk, not the list. W10c-c adds the operation-level
/// guard: `package_theme_dir` refuses a symlinked theme dir before any
/// read or write, so a packaging pass cannot write through the link.
///
/// W10: the walk is bounded on four axes ([`ThemeScanLimits`]) and every
/// refusal is a warning plus a skip, never a panic and never an unbounded
/// read. `depth` is the number of directory levels below the theme root the
/// walk has descended; a symlink cycle terminates because every lap spends
/// both depth and entries.
fn scan_theme_dir(
    dir: &Path,
    depth: usize,
    budget: &mut ScanBudget,
    hits: &mut Vec<(PathBuf, String)>,
) {
    // W10c-b (MEDIUM): the ROOT can be a symlink too. Both root sources
    // filter with `is_dir()`, which FOLLOWS links — the gallery's
    // `ThemeManager::list` (`theme_manager.rs`) and this module's
    // maintenance `list_theme_dirs` — so a themes-root entry `Evil -> ~`
    // is listed and passed here as `dir`, a level the child guard in the
    // loop below never sees. Without this check the walk started OUTSIDE
    // the package and `report_remaining_absolute_paths` logged one
    // absolute value per file of $HOME into the warning log on every
    // save/package and on every maintenance run.
    // Refused exactly like a symlinked child: one warning per scan, zero
    // reads. A root that is a symlink to a regular FILE is not a
    // directory to walk either: it falls through, `read_dir` fails and
    // the silent skip below applies — nothing is read.
    if dir.is_symlink() && std::fs::metadata(dir).map(|m| m.is_dir()).unwrap_or(false) {
        if !budget.warned_symlink_dir {
            budget.warned_symlink_dir = true;
            tracing::warn!(
                "[theme-package] skipping the symlinked directory {}: a symlink must \
                 not carry an untrusted theme scan out of the package",
                dir.display()
            );
        }
        return;
    }
    if depth >= budget.limits.max_depth {
        if !budget.warned_depth {
            budget.warned_depth = true;
            tracing::warn!(
                "[theme-package] refusing to scan deeper than {} levels under {}: the \
                 tree is pathologically deep for an untrusted theme package",
                budget.limits.max_depth,
                dir.display()
            );
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if budget.entries_left == 0 {
            if !budget.warned_entries {
                budget.warned_entries = true;
                tracing::warn!(
                    "[theme-package] refusing to scan past {} entries under {}: the \
                     package carries too much for an untrusted theme",
                    budget.limits.max_entries,
                    dir.display()
                );
            }
            return;
        }
        budget.entries_left -= 1;
        // W10c (MEDIUM): `is_dir()` FOLLOWS symlinks, so a package carrying
        // `providers/evil -> ~` would walk files under the keeper's home and
        // read them into the leak log. A symlinked CHILD directory is
        // skipped with one warning per scan — the ROOT is checked at the top
        // of this function (W10c-b), so both levels are covered and the
        // warning message's claim holds for directories root to leaf; a
        // symlink to a regular FILE stays accepted on purpose (documented —
        // the file it names is read exactly like a regular one, and the
        // reader's own guard follows the link, so a symlink to a FIFO is
        // refused there).
        if path.is_symlink()
            && std::fs::metadata(&path).map(|m| m.is_dir()).unwrap_or(false)
        {
            if !budget.warned_symlink_dir {
                budget.warned_symlink_dir = true;
                tracing::warn!(
                    "[theme-package] skipping the symlinked directory {}: a symlink must \
                     not carry an untrusted theme scan out of the package",
                    path.display()
                );
            }
            continue;
        }
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some(crate::theme_media::MEDIA_DIR) {
                continue;
            }
            scan_theme_dir(&path, depth + 1, budget, hits);
            continue;
        }
        // The per-file bound warns inside on a refusal (clear warning, no
        // read-on); an unreadable or non-UTF-8 file skips like today.
        let Ok(bytes) = read_theme_bytes_capped(&path, budget.limits.max_file_bytes) else {
            continue;
        };
        let len = bytes.len() as u64;
        if len > budget.bytes_left {
            // The refusal is decided AFTER the (already capped) read, so no
            // stat/read race can open an unbounded window: the scan may
            // overshoot the total by at most one per-file cap, which is
            // bounded by construction.
            budget.bytes_left = 0;
            if !budget.warned_bytes {
                budget.warned_bytes = true;
                tracing::warn!(
                    "[theme-package] refusing to keep scanning {}: the package already \
                     spent its {}-byte read budget",
                    path.display(),
                    budget.limits.max_total_bytes
                );
            }
            return;
        }
        budget.bytes_left -= len;
        // A file that is not valid UTF-8 is a binary, not a text record.
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        for value in absolute_paths_in_text(&text) {
            hits.push((path.clone(), value));
        }
    }
}

/// The absolute-path-looking values in one text: a token that BEGINS with `/`
/// and is not a `//` or `/*` comment marker.
///
/// Anchoring on the token's first character is what keeps a plugin id such as
/// `noctalia/mpvpaper` (a section header, a manifest key) out of the report:
/// only a value that starts at the filesystem root is a machine path.
fn absolute_paths_in_text(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let bytes = line.as_bytes();
        let mut at_token_start = true;
        let mut i = 0;
        while i < bytes.len() {
            let byte = bytes[i];
            if is_token_delimiter(byte) {
                at_token_start = true;
                i += 1;
                continue;
            }
            if !at_token_start {
                i += 1;
                continue;
            }
            at_token_start = false;
            if byte != b'/' || matches!(bytes.get(i + 1), Some(b'/') | Some(b'*')) {
                i += 1;
                continue;
            }
            let start = i;
            let mut end = i;
            while end < bytes.len() && is_path_char(bytes[end]) {
                end += 1;
            }
            if end > start + 1 {
                out.push(line[start..end].to_string());
            }
            i = end.max(i + 1);
        }
    }
    out
}

/// Whether `byte` ends a token: whitespace and the punctuation that surrounds
/// a value in the record formats a theme carries (TOML, JSON, lua).
fn is_token_delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b'"' | b'\'' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b'=' | b':' | b';' | b'|' | b'<' | b'>'
        )
}

/// Whether `byte` can appear inside an absolute path token. ASCII only, so a
/// multibyte character always ends the token at a valid char boundary.
fn is_path_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'/' | b'.' | b'-' | b'_' | b'~' | b'+' | b'@' | b'%' | b',' | b'#' | b'$' | b'!'
        )
}

/// Report every absolute path left in a packaged theme, naming the file and
/// the value. Called after the theme has been cleaned: anything reported here
/// is a leak the sanitizer does not own.
fn report_remaining_absolute_paths(theme_dir: &Path) {
    for (file, value) in scan_theme_for_absolute_paths(theme_dir) {
        tracing::warn!(
            "[theme-package] absolute path left in the packaged theme: {} names {}",
            file.display(),
            value
        );
    }
}

/// Resolve the theme dir for `name` under `themes_root`, guarding the name so
/// it can never escape the themes root.
///
/// W9: the guard itself now lives in ONE place —
/// `theme_manager::validate_theme_name` — shared with `ThemeManager`'s
/// `apply`/`delete`/`save`, so this path and the store cannot drift. The
/// error shape is unchanged.
fn theme_dir_for(themes_root: &Path, name: &str) -> Result<PathBuf, String> {
    crate::theme_manager::validate_theme_name(name)?;
    Ok(themes_root.join(name))
}

/// The theme directories under `themes_root`, sorted by name. A non-directory
/// entry is ignored.
///
/// A SYMLINK to a directory counts as one (`is_dir()` follows links) and is
/// deliberately still listed: this list serves ONLY the maintenance
/// `--package-themes` pass — the gallery's source is `ThemeManager::list`
/// (`theme_manager.rs`), not this function. A listed symlink is refused one
/// level down, twice over: `scan_theme_dir` refuses to WALK a symlinked
/// root (W10c-b) and `package_theme_dir` refuses to package through one at
/// all (W10c-c). The listing itself never reads file content.
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

    /// In-memory tracing sink, so the leak report can be asserted without a
    /// global subscriber (thread-local default only).
    #[derive(Clone, Default)]
    struct LogCapture {
        buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl std::io::Write for LogCapture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.buf.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
        type Writer = LogCapture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
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

    /// Every file under `root`, relative path + content, sorted — a
    /// read-only fingerprint that proves a refusal touched NOTHING.
    fn walk(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let relative = path.strip_prefix(root).unwrap().display().to_string();
                    let body = fs::read(&path).unwrap_or_default();
                    out.push(format!("{relative} {}", String::from_utf8_lossy(&body)));
                }
            }
        }
        out.sort();
        out
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

    // ── U2: a packaged theme carries no personal path ────────────────────

    /// The absolute wallpaper record names the machine that saved the theme.
    /// The packaged `poster.txt` is what travels, so the record goes.
    #[test]
    fn package_theme_drops_the_absolute_wallpaper_record() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"\xff\xd8\xffwallpaper";
        let (theme, _image) = write_static_theme(root.path(), "Static", bytes);

        let status = package_theme_dir(
            &theme,
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
        );

        assert!(
            matches!(status, PackageStatus::Packaged(_)),
            "the theme must still package, got {status:?}"
        );
        assert!(
            !provider_dir(&theme).join("wallpaper.txt").exists(),
            "the absolute wallpaper record must not survive packaging"
        );
        assert!(
            provider_dir(&theme).join("poster.txt").exists(),
            "the packaged poster record must survive instead"
        );
    }

    /// Same for the painter video record: `video-media.txt` travels, the
    /// absolute `video.txt` goes.
    #[test]
    fn package_theme_drops_the_absolute_video_record() {
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

        assert!(matches!(status, PackageStatus::Packaged(_)), "got {status:?}");
        assert!(
            !provider.join("video.txt").exists(),
            "the absolute video record must not survive packaging"
        );
        assert!(
            provider.join("video-media.txt").exists(),
            "the packaged video record must survive instead"
        );
    }

    /// Packaging re-cleans an OLD theme: its saved settings.toml loses every
    /// machine-only path and keeps every other byte.
    /// A settings.toml whose only absolute values point INSIDE the theme is
    /// rewritten, not dropped: the log says so and never claims a drop.
    #[test]
    fn package_theme_rewrites_an_in_theme_path_without_reporting_a_drop() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"\xff\xd8\xffwallpaper";
        let (theme, _image) = write_static_theme(root.path(), "Static", bytes);
        let inside = theme.join("media").join("poster.jpg");
        fs::write(
            provider_dir(&theme).join("settings.toml"),
            format!("[wallpaper.default]\n    path = \"{}\"\n", inside.display()),
        )
        .unwrap();

        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        package_theme_dir(
            &theme,
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
        );

        assert_eq!(
            fs::read_to_string(provider_dir(&theme).join("settings.toml")).unwrap(),
            "[wallpaper.default]\n    path = \"media/poster.jpg\"\n",
            "an in-theme asset must stay reachable as a theme-relative path"
        );
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("Rewrote 1"),
            "the rewrite must be reported, got: {text}"
        );
        assert!(
            !text.contains("machine-only value"),
            "an in-theme rewrite is not a machine-only drop, got: {text}"
        );
    }

    #[test]
    fn package_theme_sanitizes_the_saved_settings_toml() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"\xff\xd8\xffwallpaper";
        let (theme, _image) = write_static_theme(root.path(), "Static", bytes);
        let settings = provider_dir(&theme).join("settings.toml");
        fs::write(
            &settings,
            "[hooks]\n\
             started = \"/home/ximo/.local/share/sddm/themes/SilentDinamic/sddm-sync.sh\"\n\
             \n\
             [plugins]\n\
             enabled = [ \"noctalia/mpvpaper\" ]\n\
             \n\
             [wallpaper]\n\
             directory = \"/home/ximo/Pictures/Wallpapers/Cars\"\n\
             transition_on_startup = true\n",
        )
        .unwrap();

        package_theme_dir(
            &theme,
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
        );

        assert_eq!(
            fs::read_to_string(&settings).unwrap(),
            "[hooks]\n\
             \n\
             [plugins]\n\
             enabled = [ \"noctalia/mpvpaper\" ]\n\
             \n\
             [wallpaper]\n\
             transition_on_startup = true\n",
            "the machine-only lines must go and every other byte must stay"
        );
    }

    /// A theme with no resolvable background is SKIPPED, and its settings are
    /// still cleaned: the packaging pass must not leave a personal path
    /// behind just because it had nothing else to do.
    #[test]
    fn package_theme_sanitizes_settings_even_when_it_skips() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Empty");
        let provider = provider_dir(&theme);
        fs::create_dir_all(&provider).unwrap();
        let settings = provider.join("settings.toml");
        fs::write(
            &settings,
            "[shell.screenshot]\ndirectory = \"/home/ximo/Pictures/Screenshots\"\nconfirm_region = true\n",
        )
        .unwrap();

        let status = package_theme_dir(
            &theme,
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
        );

        assert!(matches!(status, PackageStatus::Skipped(_)), "got {status:?}");
        assert_eq!(
            fs::read_to_string(&settings).unwrap(),
            "[shell.screenshot]\nconfirm_region = true\n"
        );
    }

    // ── U3: the safety net over the whole theme ──────────────────────────

    /// A planted absolute path in a text file is reported with its file and
    /// value; the packaged binaries under `media/` are never scanned.
    #[test]
    fn scan_reports_a_planted_absolute_path_and_skips_media() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Cars");
        let provider = provider_dir(&theme);
        fs::create_dir_all(&provider).unwrap();
        fs::write(
            provider.join("notes.txt"),
            "directory = \"/home/someone/Pictures/Wallpapers\"\nrelative = \"media/poster.jpg\"\n",
        )
        .unwrap();
        let media = theme.join("media");
        fs::create_dir_all(&media).unwrap();
        fs::write(media.join("poster.jpg"), b"\xff\xd8\xff /home/someone/secret.png").unwrap();

        let hits = scan_theme_for_absolute_paths(&theme);

        assert_eq!(
            hits,
            vec![(
                provider.join("notes.txt"),
                "/home/someone/Pictures/Wallpapers".to_string()
            )],
            "the planted path must be reported and the media binary must not"
        );
    }

    /// A theme that carries no absolute path is reported clean.
    #[test]
    fn scan_reports_a_clean_theme_as_clean() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Cars");
        let provider = provider_dir(&theme);
        fs::create_dir_all(&provider).unwrap();
        fs::write(
            provider.join("settings.toml"),
            "[plugin_settings.\"noctalia/mpvpaper\"]\nvideo_directory = \"media/wallpapers\"\n",
        )
        .unwrap();
        fs::write(provider.join("poster.txt"), "poster=media/poster.jpg\n").unwrap();
        fs::write(provider.join("notes.lua"), "-- // a comment, not a path\n").unwrap();

        assert!(
            scan_theme_for_absolute_paths(&theme).is_empty(),
            "a clean theme must report nothing"
        );
    }

    /// The packaging pass reports a leak it did not create: a planted path in
    /// a file the sanitizer does not own still surfaces, named by file and
    /// value.
    #[test]
    fn package_theme_warns_about_a_path_it_cannot_clean() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"\xff\xd8\xffwallpaper";
        let (theme, _image) = write_static_theme(root.path(), "Static", bytes);
        fs::write(
            provider_dir(&theme).join("notes.txt"),
            "url = \"/home/someone/Pictures\"\n",
        )
        .unwrap();

        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        package_theme_dir(
            &theme,
            &root.path().join("assets"),
            &root.path().join("presets"),
            1024,
        );

        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("notes.txt") && text.contains("/home/someone/Pictures"),
            "the leak report must name the file and the value, got: {text}"
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

    // ── W10: an untrusted package is read under bounds ─────────────────

    /// Install an in-memory WARN capture around `body` and return what was
    /// logged (same seam the leak-report tests use).
    fn with_warn_capture(body: impl FnOnce()) -> String {
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        body();
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        text
    }

    /// A file one byte over the per-file bound, carrying a planted absolute
    /// path at its very START: a size-based refusal must not report it —
    /// proof the bound is enforced before the content is consumed, not after.
    #[test]
    fn scan_refuses_a_file_over_the_per_file_bound_without_reading_it_whole() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Bomb");
        let provider = provider_dir(&theme);
        fs::create_dir_all(&provider).unwrap();
        let mut content = b"/home/someone/secret.png\n".to_vec();
        content.resize(MAX_THEME_FILE_BYTES as usize + 1, b'x');
        fs::write(provider.join("notes.txt"), content).unwrap();

        let mut hits = Vec::new();
        let text = with_warn_capture(|| hits = scan_theme_for_absolute_paths(&theme));

        assert!(
            hits.is_empty(),
            "an over-bound file must be refused without being read whole, got {hits:?}"
        );
        assert!(
            text.contains("notes.txt") && text.contains("refusing to read"),
            "the refusal must be a clear warning naming the file, log was:\n{text}"
        );
    }

    /// A tree deeper than the depth bound is refused (warned + skipped)
    /// without recursing to its floor — no stack overflow, no deep read.
    #[test]
    fn scan_refuses_a_tree_deeper_than_the_depth_bound_without_overflowing() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Deep");
        let mut deep = theme.clone();
        for i in 0..(MAX_THEME_SCAN_DEPTH + 8) {
            deep = deep.join(format!("d{i:02}"));
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("notes.txt"), b"url = \"/home/someone/deep.png\"\n").unwrap();

        let mut hits = Vec::new();
        let text = with_warn_capture(|| hits = scan_theme_for_absolute_paths(&theme));

        assert!(
            hits.is_empty(),
            "a tree deeper than the bound must be refused without scanning its floor, got {hits:?}"
        );
        assert!(
            text.contains("deeper than"),
            "the refusal must be a clear warning, log was:\n{text}"
        );
    }

    /// The entry bound stops the walk early: of ten files, exactly the bound
    /// is scanned and the rest is refused with one warning.
    #[test]
    fn scan_stops_at_the_entry_bound() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Flood");
        fs::create_dir_all(&theme).unwrap();
        for i in 1..=10 {
            fs::write(
                theme.join(format!("f{i:02}.txt")),
                b"/home/someone/flooded.png\n",
            )
            .unwrap();
        }
        let limits = ThemeScanLimits {
            max_entries: 3,
            ..ThemeScanLimits::default()
        };

        let mut hits = Vec::new();
        let text = with_warn_capture(|| hits = scan_theme_for_absolute_paths_with_limits(&theme, limits));

        assert_eq!(
            hits.len(),
            3,
            "the entry bound must stop the walk after exactly its bound, got {hits:?}"
        );
        assert!(
            text.contains("entries"),
            "the refusal must be a clear warning naming the bound, log was:\n{text}"
        );
    }

    /// The total-bytes bound stops the scan once the budget is spent: the
    /// first 40-byte file fits, the second busts the remaining 20.
    #[test]
    fn scan_stops_at_the_total_bytes_bound() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Guzzler");
        fs::create_dir_all(&theme).unwrap();
        for name in ["a.txt", "b.txt"] {
            let mut content = b"/home/someone/guzzled.png\n".to_vec();
            content.resize(40, b'x');
            fs::write(theme.join(name), content).unwrap();
        }
        let limits = ThemeScanLimits {
            max_total_bytes: 60,
            ..ThemeScanLimits::default()
        };

        let mut hits = Vec::new();
        let text = with_warn_capture(|| hits = scan_theme_for_absolute_paths_with_limits(&theme, limits));

        assert_eq!(
            hits.len(),
            1,
            "only the file that fits the total budget may be scanned, got {hits:?}"
        );
        assert!(
            text.contains("read budget"),
            "the refusal must be a clear warning naming the budget, log was:\n{text}"
        );
    }

    /// Control: a legitimate record under every bound reads byte-identically
    /// — multi-byte UTF-8 included.
    #[test]
    fn read_theme_text_reads_a_legitimate_file_byte_identically() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("record.txt");
        let content = "poster = \"media/póster áéí.png\"\n";
        fs::write(&file, content).unwrap();

        assert_eq!(read_theme_text(&file).unwrap(), content);
    }

    /// An oversized file is refused with a clear message and never slurped.
    #[test]
    fn read_theme_text_refuses_an_oversized_file() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("bomb.txt");
        fs::write(&file, vec![b'x'; MAX_THEME_FILE_BYTES as usize + 1]).unwrap();

        let err = read_theme_text(&file).expect_err("an over-bound file must be refused");
        assert!(
            err.is_oversized(),
            "an oversized file must refuse as TooLarge, got: {err}"
        );
        assert!(err.to_string().contains("bound"), "the refusal must name the bound, got: {err}");
    }

    /// A missing or non-text file is an ordinary error, not a panic.
    #[test]
    fn read_theme_text_passes_ordinary_errors_through() {
        let root = tempfile::tempdir().unwrap();
        assert!(read_theme_text(&root.path().join("missing.txt")).is_err());

        let binary = root.path().join("binary.bin");
        fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
        let err = read_theme_text(&binary).expect_err("non-UTF-8 must not become text");
        assert!(!err.is_oversized(), "a binary is unreadable, not oversized: {err}");
        assert!(err.to_string().contains("UTF-8"), "got: {err}");
    }

    /// The `fs::copy` pre-flight answers honestly on both sides of the bound.
    #[test]
    fn theme_file_is_bounded_answers_by_size() {
        let root = tempfile::tempdir().unwrap();
        let small = root.path().join("small.toml");
        fs::write(&small, b"config_version = 14\n").unwrap();
        assert!(theme_file_is_bounded(&small));

        let bomb = root.path().join("bomb.toml");
        fs::write(&bomb, vec![b'x'; MAX_THEME_FILE_BYTES as usize + 1]).unwrap();
        assert!(!theme_file_is_bounded(&bomb), "an over-bound file must be refused");
    }

    // ── W10c: the regular-file guard lives in the LOWEST-level reader ────
    //
    // The W10b guard sat only in the `theme_media::read_theme_record`
    // wrapper, which ~17 apply-side call sites bypass by calling the bounded
    // readers directly — and `fs::metadata` answers `len = 0` for a FIFO, so
    // the size-only pre-flight called it "bounded" and `fs::copy` would block
    // on the open. These tests pin the three refuted holes. SAFETY NOTE: a
    // test that OPENS a FIFO blocks the whole suite forever, so every FIFO
    // probe here either asserts a STAT-only predicate or runs the reader on
    // a worker thread under a timeout; every `File::open` probe that must
    // fail fast uses a Unix socket (ENXIO), never a FIFO.

    /// Create a FIFO at `path` (no std equivalent; `libc::mkfifo`). Never
    /// opened here — stat-only fixtures are safe, open probes are not.
    fn mkfifo(path: &Path) {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
        assert_eq!(rc, 0, "mkfifo must succeed for the sandbox fixture");
    }

    /// Create a Unix socket at `path`. `File::open` on a socket fails fast
    /// with ENXIO (never blocks), so it pins "refused BEFORE the open"
    /// without the FIFO hazard.
    fn unix_socket(path: &Path) {
        let listener = std::os::unix::net::UnixListener::bind(path).expect("socket must bind");
        // The bound listener only creates the socket file; dropping it
        // leaves the file in place, still non-regular and still ENXIO.
        drop(listener);
    }

    /// Hole (a): the bounded TEXT reader itself must refuse a non-regular
    /// file BEFORE the open, with the refusal named in the error and in the
    /// warning — the Unix socket fails fast with ENXIO if it is ever opened,
    /// so this can never hang the suite.
    #[test]
    fn read_theme_text_refuses_a_non_regular_file_instead_of_opening_it() {
        let root = tempfile::tempdir().unwrap();
        let sock = root.path().join("wallpaper.txt");
        unix_socket(&sock);

        let text = with_warn_capture(|| {
            let err = read_theme_text(&sock).expect_err("a socket must be refused");
            assert!(
                err.to_string().contains("not a regular file"),
                "the refusal must name the non-regular file, got: {err}"
            );
        });
        assert!(
            text.contains("wallpaper.txt") && text.contains("not a regular file"),
            "the refusal must warn and name the file, log was:\n{text}"
        );
    }

    /// Hole (a), same contract for the bounded BYTE reader.
    #[test]
    fn read_theme_bytes_refuses_a_non_regular_file_instead_of_opening_it() {
        let root = tempfile::tempdir().unwrap();
        let sock = root.path().join("palette.json");
        unix_socket(&sock);

        let text = with_warn_capture(|| {
            let err = read_theme_bytes(&sock).expect_err("a socket must be refused");
            assert!(
                err.to_string().contains("not a regular file"),
                "the refusal must name the non-regular file, got: {err}"
            );
        });
        assert!(
            text.contains("palette.json") && text.contains("not a regular file"),
            "the refusal must warn and name the file, log was:\n{text}"
        );
    }

    /// Hole (a) — the auditor's exact counter-example: a FIFO named like a
    /// theme record must be REFUSED, not block. The read runs on a worker
    /// thread and a timeout proves it returned before any open (a naive
    /// open on the FIFO would block the suite forever, so the guard MUST
    /// precede the open for this test to be safe at all).
    #[test]
    fn read_theme_text_refuses_a_fifo_instead_of_blocking() {
        let root = tempfile::tempdir().unwrap();
        let fifo = root.path().join("wallpaper.txt");
        mkfifo(&fifo);

        let (tx, rx) = std::sync::mpsc::channel();
        let fifo = fifo.clone();
        std::thread::spawn(move || {
            let _ = tx.send(read_theme_text(&fifo));
        });
        match rx.recv_timeout(std::time::Duration::from_secs(3)) {
            Ok(result) => assert!(result.is_err(), "a FIFO must be refused, got: {result:?}"),
            Err(_) => panic!("read_theme_text blocked on a FIFO instead of refusing it"),
        }
    }

    /// Hole (b): the `fs::copy` pre-flight answered "bounded" for a FIFO,
    /// because `fs::metadata` reports `len = 0` for one. A non-regular file
    /// must fail the pre-flight (stat-only probe — no open can hang here).
    #[test]
    fn theme_file_is_bounded_refuses_a_non_regular_file() {
        let root = tempfile::tempdir().unwrap();
        let fifo = root.path().join("settings.toml");
        mkfifo(&fifo);

        let text = with_warn_capture(|| {
            assert!(
                !theme_file_is_bounded(&fifo),
                "a FIFO must not answer bounded: fs::copy would block on its open"
            );
        });
        assert!(
            text.contains("settings.toml") && text.contains("not a regular file"),
            "the refusal must warn and name the file, log was:\n{text}"
        );
    }

    /// Hole (c): ONE FIFO anywhere in a tree fed to the leak scan hung the
    /// package path for ever, because the scan called the unguarded reader.
    /// Worker thread + timeout again: proof the scan returns without ever
    /// opening the FIFO.
    #[test]
    fn scan_skips_a_fifo_without_blocking() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Trap");
        fs::create_dir_all(&theme).unwrap();
        mkfifo(&theme.join("notes.txt"));

        let (tx, rx) = std::sync::mpsc::channel();
        let theme = theme.clone();
        std::thread::spawn(move || {
            let _ = tx.send(scan_theme_for_absolute_paths(&theme));
        });
        match rx.recv_timeout(std::time::Duration::from_secs(3)) {
            Ok(hits) => assert!(hits.is_empty(), "a FIFO must be skipped, got {hits:?}"),
            Err(_) => panic!("the scan blocked on a FIFO instead of skipping it"),
        }
    }

    /// MEDIUM (W10): `is_dir()` follows symlinks, so a package carrying
    /// `evil -> <outside dir>` walked files OUTSIDE the package into the
    /// leak log. A symlinked directory must be skipped with a warning.
    #[test]
    fn scan_skips_a_symlinked_directory_out_of_the_package() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Evil");
        fs::create_dir_all(&theme).unwrap();
        let outside = root.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "url = \"/home/someone/.ssh/id_rsa\"\n").unwrap();
        std::os::unix::fs::symlink(&outside, theme.join("evil")).unwrap();

        let mut hits = Vec::new();
        let text = with_warn_capture(|| hits = scan_theme_for_absolute_paths(&theme));

        assert!(
            hits.is_empty(),
            "a symlinked directory must never be walked out of the package, got {hits:?}"
        );
        assert!(
            text.contains("evil") && text.contains("symlink"),
            "the skip must warn and name the symlink, log was:\n{text}"
        );
    }

    /// MEDIUM (W10c-b): BOTH root sources filter with `is_dir()`, which
    /// FOLLOWS symlinks — the gallery's `ThemeManager::list`
    /// (`theme_manager.rs`) and the maintenance pass's `list_theme_dirs` —
    /// so a theme entry at the themes root like `Evil -> ~` is listed and
    /// handed to the scan as its ROOT — a level the child-directory guard
    /// never sees. The walk then started OUTSIDE the package and logged one
    /// absolute value per file of $HOME into the warning log (this scan runs
    /// on every save/package and from `maintenance`). The scan must refuse a
    /// SYMLINKED ROOT exactly like a symlinked child: warn once, walk
    /// nothing.
    #[test]
    fn scan_refuses_a_symlinked_root_instead_of_walking_out_of_the_package() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("home");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "url = \"/home/someone/.ssh/id_rsa\"\n").unwrap();
        let link = root.path().join("themes/Evil");
        fs::create_dir_all(root.path().join("themes")).unwrap();
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let mut hits = Vec::new();
        let text = with_warn_capture(|| hits = scan_theme_for_absolute_paths(&link));

        assert!(
            hits.is_empty(),
            "a symlinked ROOT must never be walked out of the package, got {hits:?}"
        );
        assert!(
            text.contains("Evil") && text.contains("symlink"),
            "the refusal must warn once and name the symlinked root, log was:\n{text}"
        );
    }

    /// Requirement 6, characterization: a symlink to a regular FILE stays
    /// accepted on purpose (documented behaviour — the record it names is
    /// read exactly like a regular file). This must stay green.
    #[test]
    fn scan_still_reads_through_a_symlink_to_a_regular_file() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/Link");
        fs::create_dir_all(&theme).unwrap();
        let outside = root.path().join("outside.txt");
        fs::write(&outside, "url = \"/home/someone/wall.png\"\n").unwrap();
        std::os::unix::fs::symlink(&outside, theme.join("notes.txt")).unwrap();

        let hits = scan_theme_for_absolute_paths(&theme);

        assert_eq!(
            hits,
            vec![(
                theme.join("notes.txt"),
                "/home/someone/wall.png".to_string()
            )],
            "a symlink to a regular file must still be read through"
        );
    }

    /// W10c-c, CRITICAL-class: the SCAN refusing a symlinked root is not the
    /// OPERATION refusing it. `package_theme_dir` ignored the scan's early
    /// return and kept going, so `sanitize_settings_file` still wrote through
    /// `theme_dir` (`noctalia.rs`) and the poster/preset writes landed in the
    /// symlink target — OUTSIDE the themes root. Packaging must refuse a
    /// symlinked theme directory EARLY, before any read or write of it, with
    /// ONE warning.
    #[test]
    fn package_refuses_a_symlinked_theme_dir_before_any_read_or_write() {
        let root = tempfile::tempdir().unwrap();
        // A real directory OUTSIDE the themes root, carrying a theme a
        // packaging pass would normally rewrite.
        let outside = root.path().join("outside/Evil");
        let provider = provider_dir(&outside);
        fs::create_dir_all(&provider).unwrap();
        fs::write(
            provider.join("settings.toml"),
            "[wallpaper]\npath = \"/home/someone/wall.png\"\n",
        )
        .unwrap();
        let themes_root = root.path().join("themes");
        fs::create_dir_all(&themes_root).unwrap();
        let link = themes_root.join("Evil");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let before = walk(&outside);

        let mut status = None;
        let text = with_warn_capture(|| {
            status = Some(package_theme_dir(
                &link,
                &root.path().join("assets"),
                &root.path().join("presets"),
                1024,
            ));
        });

        assert_eq!(
            status,
            Some(PackageStatus::Skipped(
                "symlinked theme directory (refused, nothing read or written)".into()
            )),
            "a symlinked theme dir must be refused up front, not half-packaged"
        );
        assert_eq!(
            walk(&outside),
            before,
            "nothing behind the symlink may be read or written"
        );
        assert!(
            text.contains("Evil") && text.contains("symlink"),
            "the refusal must warn and name the symlink, log was:\n{text}"
        );
        assert_eq!(
            text.matches("[theme-package]").count(),
            1,
            "the refusal is ONE warning, log was:\n{text}"
        );
    }
}
