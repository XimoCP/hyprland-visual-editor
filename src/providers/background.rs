//! The `apply-background` capability router.
//!
//! Contract: `openspec/specs/capability-routing/spec.md` — the core states
//! WHAT it wants painted and this module routes that request to an
//! independent backend. A backend never calls another backend (the
//! siblings rule), so the provider-side chains that used to drive the
//! other wallpaper backends from inside a provider end HERE: one neutral
//! module that names the backends, while the caller names only this.
//!
//! Routing only: no Noctalia policy lives here (the provider keeps its own
//! settings restore, plugin re-assert and saved records), no skwd engine
//! knowledge beyond selecting it and holding its colour authority off
//! around an apply (the mechanism itself stays in `skwd_policy`), and no
//! mpvpaper internals beyond its public API.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::providers::bg_info;

// ── Declared backends ────────────────────────────────────────────────

/// The backends that can paint a theme's background, declared here so the
/// caller never names one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundBackend {
    /// The skwd-wall-v2 / skwd-helm engine pair: paints one exact path on
    /// request (`apply <path>`).
    SkwdEngine,
    /// The Noctalia mpvpaper plugin: plays the theme's saved manifest.
    MpvpaperPlugin,
}

/// WHAT one hand-off wants painted — the declared choice the selector
/// reads. The selector never guesses from side effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundSource {
    /// One exact path on disk (a painter video or a static wallpaper).
    ExactPath,
    /// The theme's saved animated manifest (its absence means "stop
    /// whatever is running", never "paint").
    SavedManifest,
}

/// Pure selection over the declared backends: no I/O, no probing, no
/// policy — the declared choice alone decides who paints, so the routing
/// rule is testable on its own.
pub fn select_backend(source: BackgroundSource) -> BackgroundBackend {
    match source {
        // A bare path is what the engine speaks natively (`apply <path>`);
        // the manifest backend plays saved assignments, never a path.
        BackgroundSource::ExactPath => BackgroundBackend::SkwdEngine,
        // Only the plugin understands the theme's saved manifest format.
        BackgroundSource::SavedManifest => BackgroundBackend::MpvpaperPlugin,
    }
}

// ── Request ──────────────────────────────────────────────────────────

/// The inputs of one `apply-background` request: exactly the facts the old
/// provider-driven chains consumed, stated by the caller instead of being
/// reached into another backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyBackgroundRequest {
    /// The theme dir: where the manifest backend resolves the records it
    /// plays (`theme/providers/<id>/…`).
    pub theme_dir: PathBuf,
    /// The theme's provider dir: where the saved background records live
    /// (manifest, painter video record, static wallpaper record).
    pub provider_dir: PathBuf,
    /// Whether an exact painter video was already restored in this apply
    /// — the fact that gates the manifest leg.
    pub animated_applied: bool,
    /// The saved static wallpaper path, if the theme carries one.
    pub static_wallpaper: Option<PathBuf>,
    /// T6: HOW the theme wants its video (none, an exact path, or a url), so
    /// the video-backend notice can tell which backend could paint it. The
    /// notice reads it.
    pub video_want: bg_info::VideoWant,
}

// ── Outcome ──────────────────────────────────────────────────────────

/// The user-visible notification state the plugin probe derived: the
/// theme carries animated backgrounds but the plugin is missing or
/// disabled, so the user was told before the manifest leg ran (the same
/// notification the provider used to send itself, in the same order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundNotification {
    /// The plugin is missing or disabled; the user has been notified.
    PluginUnavailable,
    /// Nothing to tell the user.
    NoNotification,
}

/// Which leg ran and how it went — exhaustive enough that the caller
/// keeps every one of its current log lines and control-flow branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutedLeg {
    /// An exact painter video was already restored: nothing was routed.
    AlreadyApplied,
    /// The manifest backend painted the theme's saved videos.
    ManifestApplied,
    /// The manifest backend refused (reason = the caller's warn text).
    ManifestRefused(String),
    /// No manifest: running videos were told to stop.
    Cleared,
    /// The stop itself failed (reason = the caller's warn text).
    ClearFailed(String),
}

/// The ONE outcome the caller can act on after a routed hand-off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyBackgroundOutcome {
    /// Whether an animated background ended up applied by this call.
    pub animated_applied: bool,
    /// The user-visible notification state derived from the probe.
    pub notification: BackgroundNotification,
    /// Which routed leg ran and how it went.
    pub leg: RoutedLeg,
}

// ── Routing entry points ─────────────────────────────────────────────

/// Hand one exact path (a painter video, a static wallpaper) to the
/// backend declared for a path. Best-effort: `false` unless the backend
/// really painted it, so the caller keeps its fallback routes.
pub fn hand_off_path(path: &Path) -> bool {
    match select_backend(BackgroundSource::ExactPath) {
        BackgroundBackend::SkwdEngine => {
            crate::providers::skwd_engine::delegate_to_skwd_walld(path)
        }
        // Unreachable while the selector declares a path hand-off to the
        // engine (pinned by `select_backend_routes_by_declared_choice`);
        // still a refusal, never a panic or a silent claim of success —
        // the manifest backend paints no bare path.
        other => {
            tracing::warn!(
                "[background] path hand-off routed to {:?}, which paints no bare path",
                other
            );
            false
        }
    }
}

/// Hand the request's saved static wallpaper to the backend declared for
/// it. `false` when the theme saved no static wallpaper (nothing to hand
/// over) or when the backend did not paint it.
pub fn hand_off_static(req: &ApplyBackgroundRequest) -> bool {
    match req.static_wallpaper.as_deref() {
        Some(path) => hand_off_path(path),
        None => false,
    }
}

/// The manifest leg of `apply-background`: restore the theme's saved
/// animated manifest — or, with none saved, stop running video
/// wallpapers — through the declared backend. Never fails the apply:
/// every refusal comes back as an outcome the caller logs.
pub fn apply_animated(req: &ApplyBackgroundRequest) -> ApplyBackgroundOutcome {
    if req.animated_applied {
        // The exact video is already back with its own manager: nothing is
        // routed (no probe, no notification, no manifest touched — the
        // caller keeps its own stale-manifest delete for this case).
        return ApplyBackgroundOutcome {
            animated_applied: true,
            notification: BackgroundNotification::NoNotification,
            leg: RoutedLeg::AlreadyApplied,
        };
    }
    match select_backend(BackgroundSource::SavedManifest) {
        BackgroundBackend::MpvpaperPlugin => mpvpaper_manifest_leg(req),
        // Unreachable while the selector declares the saved manifest to
        // the plugin backend; a refusal the caller can still log, never a
        // panic in the theme apply.
        other => {
            tracing::warn!(
                "[background] manifest leg routed to {:?}, which plays no saved manifest",
                other
            );
            ApplyBackgroundOutcome {
                animated_applied: false,
                notification: BackgroundNotification::NoNotification,
                leg: RoutedLeg::ManifestRefused(
                    "no backend declared for the saved manifest".into(),
                ),
            }
        }
    }
}

/// The mpvpaper-plugin leg, in the order the old chain ran it: warn the
/// user up-front when the theme has animated wallpapers but the plugin is
/// not available — otherwise they would apply the theme and silently lose
/// the videos — then restore the manifest, or (no manifest) stop running
/// video wallpapers.
fn mpvpaper_manifest_leg(req: &ApplyBackgroundRequest) -> ApplyBackgroundOutcome {
    let manifest_exists = req.provider_dir.join(bg_info::MANIFEST_FILE).exists();
    let notification = notify_missing_video_backend(req, manifest_exists);
    if !manifest_exists {
        return match crate::providers::mpvpaper::clear_all() {
            Ok(()) => ApplyBackgroundOutcome {
                animated_applied: false,
                notification,
                leg: RoutedLeg::Cleared,
            },
            Err(reason) => ApplyBackgroundOutcome {
                animated_applied: false,
                notification,
                leg: RoutedLeg::ClearFailed(reason),
            },
        };
    }
    match crate::providers::mpvpaper::apply_manifest(&req.theme_dir) {
        Ok(()) => ApplyBackgroundOutcome {
            animated_applied: true,
            notification,
            leg: RoutedLeg::ManifestApplied,
        },
        Err(reason) => ApplyBackgroundOutcome {
            animated_applied: false,
            notification,
            leg: RoutedLeg::ManifestRefused(reason),
        },
    }
}

/// Whether the wallpaper engine's socket exists — the engine can paint a
/// video. Read here so no provider names the engine backend directly.
pub fn engine_socket_available() -> bool {
    crate::providers::skwd_engine::socket_available()
}

/// T6: tell the user when a theme wants a video and no backend can paint it.
///
/// The decision and the copy come from the pure
/// [`bg_info::video_backend_notice`]; this only gathers the facts and raises
/// the shared toast. A static theme (nothing wants a video, no manifest)
/// short-circuits before any probe, so it never spawns the plugin probe.
fn notify_missing_video_backend(
    req: &ApplyBackgroundRequest,
    manifest_exists: bool,
) -> BackgroundNotification {
    if req.video_want == bg_info::VideoWant::None && !manifest_exists {
        return BackgroundNotification::NoNotification;
    }
    let facts = bg_info::VideoBackendFacts {
        video_want: req.video_want,
        has_manifest: manifest_exists,
        engine_socket: engine_socket_available(),
        mpvpaper_enabled: crate::providers::mpvpaper::mpvpaper_enabled(),
    };
    if let Some(notice) = bg_info::video_backend_notice(facts) {
        bg_info::notify_video_backend(&notice);
        BackgroundNotification::PluginUnavailable
    } else {
        BackgroundNotification::NoNotification
    }
}

// ── Colour-authority capability (the engine held off around an apply) ─

/// Bounds for the observe wait between the flip and the first wallpaper
/// hand-off. The engine reloads its config on change, not on write:
/// measured from `~/Proyectos/skwd-deck` (2026-09-23) — `skwd-walld`
/// native-watches the config dir and calls `reload_config()` inline on
/// the config-file event
/// (`crates/skwd-walld/src/infrastructure/watcher.rs:688`), every
/// `wall.apply` RPC reloads at the handler top
/// (`crates/skwd-walld/src/infrastructure/rpc/wallpaper.rs:321`), and the
/// periodic fallbacks tick at ≥50 ms while playlist assignments exist
/// (`crates/skwd-walld/src/composition/runtime/playlist.rs:15`
/// MIN_TICK_WAIT). One 250 ms sleep covers the worst scheduling jitter of
/// those paths with a wide margin, costs once per hold, and the measured
/// elapsed time is returned to the caller so the keeper's live test can
/// tune it.
const COLOR_YIELD_OBSERVE_WAIT: Duration = Duration::from_millis(250);

/// Sleep the bounded observe wait so the engine's config watcher sees the
/// flip before the wallpaper hand-off; returns the elapsed time so the
/// caller keeps its "flip observed in N ms" log. The fast test profile
/// (`HVE_REASSERT_PROFILE=fast`) shortens the wait so tests observe the
/// whole race without real delays.
fn wait_for_engine_to_observe_flip() -> Duration {
    let wait = if std::env::var("HVE_REASSERT_PROFILE").as_deref() == Ok("fast") {
        Duration::from_millis(5)
    } else {
        COLOR_YIELD_OBSERVE_WAIT
    };
    let start = Instant::now();
    std::thread::sleep(wait);
    start.elapsed()
}

/// One open window of the engine's colour authority — the colour-authority
/// yield (W2/W4) made into a routed capability. While this value lives the
/// engine is held off (`theme.policy` reads `off`), so it cannot regenerate
/// its wallpaper-derived palette and stomp the colours the apply is about
/// to paint.
///
/// Dropped armed → the previous value comes back on that scope's exit (the
/// early-error path of an apply). [`ColourAuthorityHold::disarm`] hands the
/// restore to the caller's palette-verification worker instead, which
/// pairs it with [`release_color_authority`].
#[must_use = "the hold restores the engine's colour authority when it drops; disarm it to hand the restore over"]
pub struct ColourAuthorityHold {
    guard: crate::providers::skwd_policy::YieldGuard,
    observed_in: Duration,
}

impl ColourAuthorityHold {
    /// How long the engine was given to observe the flip before the first
    /// wallpaper hand-off (only meaningful under the production profile;
    /// the `fast` test profile shortens it).
    pub fn observed_in(&self) -> Duration {
        self.observed_in
    }

    /// Detach the restore from this scope: the caller's async
    /// palette-verification worker takes ownership and must put the
    /// returned previous value back with [`release_color_authority`] once
    /// the palette is verified (or at the bounded cap). `None` means there
    /// was no previous value to restore — nothing to arrange.
    pub fn disarm(&mut self) -> Option<String> {
        self.guard.disarm()
    }
}

impl Drop for ColourAuthorityHold {
    fn drop(&mut self) {
        // The apply's hand-off phase is over: HVE is no longer acting, so a
        // later engine background change can be told apart from its own.
        HVE_APPLY_ACTIVE.store(false, Ordering::SeqCst);
    }
}

/// Hold the engine's colour authority off for one apply: flip
/// `theme.policy` to `off` (engine's own mechanism, never touched from
/// here), give the engine's config watcher the bounded moment to observe
/// it, and hand back the armed hold.
///
/// `Ok(None)`: nothing to hold (missing/unparsable config, or the engine
/// does not own the scheme) — nothing was written, and no wait runs, so
/// the caller behaves exactly as today. `Ok(Some(hold))`: the engine is
/// held off until the hold drops or is disarmed. `Err`: honest failure;
/// the caller logs and carries on — a failed write never aborts an apply.
pub fn hold_color_authority() -> Result<Option<ColourAuthorityHold>, String> {
    let previous = match crate::providers::skwd_policy::yield_color_authority() {
        Ok(Some(previous)) => previous,
        Ok(None) => return Ok(None),
        Err(e) => return Err(e),
    };
    let observed_in = wait_for_engine_to_observe_flip();
    // The hold is alive until its guard drops: while it is, HVE is the one
    // painting, so its own hand-off is never a foreign change.
    HVE_APPLY_ACTIVE.store(true, Ordering::SeqCst);
    Ok(Some(ColourAuthorityHold {
        guard: crate::providers::skwd_policy::YieldGuard::new(previous),
        observed_in,
    }))
}

/// Put the engine's colour authority back EXACTLY as it was — the paired
/// release for a hold that was disarmed into a worker. `Ok(true)` when the
/// config was rewritten, `Ok(false)` when there was nothing to do, `Err` an
/// honest failure the caller logs and carries on from.
///
/// `Ok(false)` covers three cases: the config is gone, a concurrent write
/// won (the current value is no longer the `off` this hold wrote), or a
/// valid colour-authority descriptor says a theme still owns the palette —
/// in that last case the mute is DELIBERATE and is kept, along with its
/// crash marker, so the engine stays `off` while the theme rules.
pub fn release_color_authority(previous_value: &str) -> Result<bool, String> {
    crate::providers::skwd_policy::restore_color_authority(previous_value)
}

/// Whether a hold is in flight (its crash marker is present): while the
/// engine is held off, the apply's own worker owns the colours, so a
/// watcher-driven re-assert must stay quiet instead of fighting it.
pub fn color_authority_held() -> bool {
    crate::providers::skwd_policy::marker_path().exists()
}

/// Whether the engine currently owns the colour scheme (it would
/// regenerate and re-impose its own palette asynchronously). Read through
/// the engine's own config resolver — a provider asks THIS module, never
/// the engine's file. Missing or unparsable config answers `false`.
pub fn engine_owns_color_scheme() -> bool {
    crate::providers::skwd_policy::engine_owns_color_scheme()
}

// ── Foreign-background step-aside (W2 of palette-authority-mute-and-yield) ─

/// What the detector decided about the live background while a theme owns
/// the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAuthorityVerdict {
    /// Keep the claim: nothing proves the background change was foreign.
    Hold,
    /// A background the theme does not declare is live: HVE steps aside.
    StepAside,
}

/// The pure step-aside decision.
///
/// Inputs are exactly the facts the rule needs:
/// - `live`: the engine's own live background paths (`None` = the engine
///   could not be read — a read failure is NEVER evidence of a foreign
///   change);
/// - `declared`: the background paths the applied theme declares as its own;
/// - `hve_acting`: HVE is inside its own apply (its own hand-off is never a
///   foreign change);
/// - `descriptor_present`: a theme currently owns the palette. Without it
///   there is nothing to step aside from.
///
/// Conservative by construction: an unreadable engine, an empty live set or
/// a theme that declares nothing all `Hold` — the detector only steps aside
/// when it can prove a live path is not one the theme declared.
pub fn decide_palette_authority(
    live: Option<&[String]>,
    declared: &[String],
    hve_acting: bool,
    descriptor_present: bool,
) -> PaletteAuthorityVerdict {
    // No owner: nothing to step aside from.
    if !descriptor_present {
        return PaletteAuthorityVerdict::Hold;
    }
    // HVE's own apply is painting the theme's background right now.
    if hve_acting {
        return PaletteAuthorityVerdict::Hold;
    }
    // A failed read, no live outputs, or a theme that declares nothing:
    // HVE cannot prove a foreign change, so it never acts on a guess.
    let Some(live) = live else {
        return PaletteAuthorityVerdict::Hold;
    };
    if live.is_empty() || declared.is_empty() {
        return PaletteAuthorityVerdict::Hold;
    }
    // Hold only while EVERY live background is one the theme declared. A
    // single path outside the declared set is the keeper's own change.
    let all_declared = live.iter().all(|path| declared.iter().any(|d| d == path));
    if all_declared {
        PaletteAuthorityVerdict::Hold
    } else {
        PaletteAuthorityVerdict::StepAside
    }
}

/// Whether HVE is inside its own apply right now (a hold is alive). Process
/// global, set by [`hold_color_authority`] and cleared when the hold guard
/// drops: the whole hand-off phase of an apply is "HVE acting", so a
/// background the engine shows during it is never mistaken for the keeper's.
static HVE_APPLY_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether HVE is currently applying (see [`HVE_APPLY_ACTIVE`]).
pub fn hve_is_acting() -> bool {
    HVE_APPLY_ACTIVE.load(Ordering::SeqCst)
}

/// What one step-aside check found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepAsideOutcome {
    /// No theme owns the palette: nothing to step aside from.
    NothingClaimed,
    /// A theme owns the palette and the live background is still its own.
    Held,
    /// A foreign background is live: the claim was released.
    SteppedAside,
}

/// The background paths the applied theme declares as its own, resolved from
/// its colour-authority descriptor.
///
/// The descriptor's `palette_file` lives at
/// `{theme}/providers/{id}/palette.json` (written by the provider's apply),
/// so the theme and provider dirs are derived from it. A descriptor whose
/// path does not have that shape declares nothing: the detector then holds.
fn declared_backgrounds_for(authority: &crate::color_authority::ColorAuthority) -> Vec<String> {
    let palette = Path::new(&authority.palette_file);
    let Some(provider_dir) = palette.parent() else {
        return Vec::new();
    };
    let Some(theme_dir) = provider_dir.parent().and_then(Path::parent) else {
        return Vec::new();
    };
    crate::theme_media::declared_backgrounds(theme_dir, provider_dir)
}

/// One step-aside check: read the engine's own live background, compare it
/// with what the applied theme declares, and — only when it proves the
/// change was not HVE's — release the claim (descriptor cleared) and the
/// engine mute.
///
/// Nothing happens without a descriptor (no theme owns the palette), while
/// HVE is acting, when the engine cannot be read, or when the live
/// background is one the theme declared. Idempotent: once aside, the absent
/// descriptor keeps every later call a `NothingClaimed` no-op.
pub fn step_aside_if_foreign() -> Result<StepAsideOutcome, String> {
    let Some(descriptor) = crate::color_authority::read_descriptor() else {
        return Ok(StepAsideOutcome::NothingClaimed);
    };
    let declared = declared_backgrounds_for(&descriptor);
    let live = crate::providers::wallpaper_authority::query_live_backgrounds();
    if decide_palette_authority(live.as_deref(), &declared, hve_is_acting(), true)
        == PaletteAuthorityVerdict::Hold
    {
        return Ok(StepAsideOutcome::Held);
    }
    // Step aside. ORDER MATTERS: with the descriptor still present
    // `restore_color_authority` would keep the mute (a theme owns the
    // palette); clearing it first makes the release genuine.
    crate::color_authority::clear_descriptor()
        .map_err(|e| format!("[background] cannot clear colour-authority descriptor: {e}"))?;
    if let Some(previous) = crate::providers::skwd_policy::held_previous_value() {
        release_color_authority(&previous)?;
    }
    tracing::info!(
        "[background] a background HVE did not paint is live: stepping aside from \
         the palette (descriptor cleared, engine mute released)"
    );
    Ok(StepAsideOutcome::SteppedAside)
}

/// How often the background watch samples the engine's own state.
///
/// The keeper changes the background by hand, so a few seconds of lag is
/// harmless; the read itself is bounded by the authority timeout. One sample
/// per interval, and only while a theme owns the palette.
const FOREIGN_CHANGE_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Start the process-lifetime watch that steps aside when a background HVE
/// did not paint becomes live.
///
/// Idempotent: a second call is a no-op. The thread only samples the engine
/// while a colour-authority descriptor exists (a theme owns the palette), so
/// an idle HVE never runs the engine CLI. Spawn failure is logged and leaves
/// the watch unstarted (a later call retries); it never panics.
pub fn start_foreign_change_watch() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("hve-foreign-bg-watch".into())
        .spawn(|| loop {
            std::thread::sleep(FOREIGN_CHANGE_POLL_INTERVAL);
            // No owner -> nothing to defend; skip the engine read entirely.
            if crate::color_authority::read_descriptor().is_none() {
                continue;
            }
            if let Err(e) = step_aside_if_foreign() {
                tracing::warn!("[background] foreign-background check failed: {e}");
            }
        });
    if let Err(e) = spawned {
        STARTED.store(false, Ordering::SeqCst);
        tracing::warn!("[background] cannot start the foreign-background watch: {e}");
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    /// The routing rule itself, pure: a declared exact path goes to the
    /// engine backend; the theme's saved manifest goes to the plugin
    /// backend. No I/O, no probing — this is the seam a future backend
    /// declaration plugs into.
    #[test]
    fn select_backend_routes_by_declared_choice() {
        assert_eq!(
            select_backend(BackgroundSource::ExactPath),
            BackgroundBackend::SkwdEngine,
            "a bare path is what the engine speaks natively"
        );
        assert_eq!(
            select_backend(BackgroundSource::SavedManifest),
            BackgroundBackend::MpvpaperPlugin,
            "only the plugin understands the saved manifest format"
        );
    }

    /// Hermetic probe sandbox: a stub `noctalia` on PATH (the plugin probe
    /// must never reach a live desktop) whose `plugins list` answers per
    /// the requested state, plus a stub `notify-send` that records the
    /// notification into a marker file instead of raising one on the
    /// keeper's screen.
    struct ProbeSandbox {
        /// Restored first: `Drop` puts PATH back before any field goes.
        old_path: Option<String>,
        /// Restored with PATH: the engine socket seam, so a live keeper socket
        /// can never leak into the notice decision.
        old_runtime_dir: Option<String>,
        old_skwd_sock: Option<String>,
        /// Written by the stub `notify-send` when a notification fires.
        notify_marker: PathBuf,
        /// Lives on PATH until `old_path` is restored above.
        _bin: PathBuf,
        _tmp: TempDir,
        /// Shared env lock, released last.
        _env: crate::test_utils::TempEnv,
    }

    impl ProbeSandbox {
        fn new(plugin_enabled: bool) -> Self {
            let env = crate::test_utils::TempEnv::new();
            let tmp = TempDir::new().unwrap();
            let bin = tmp.path().join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let noctalia = if plugin_enabled {
                "#!/bin/sh\n\
                 if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"plugins\" ] && [ \"$3\" = \"list\" ]; then\n\
                 printf 'noctalia/mpvpaper enabled\\n'\n\
                 exit 0\n\
                 fi\n\
                 exit 0\n"
            } else {
                "#!/bin/sh\nexit 0\n"
            };
            std::fs::write(bin.join("noctalia"), noctalia).unwrap();
            let notify_marker = tmp.path().join("notified");
            let notify_send = format!(
                "#!/bin/sh\nprintf 'notified\\n' >> '{}'\nexit 0\n",
                notify_marker.display()
            );
            std::fs::write(bin.join("notify-send"), notify_send).unwrap();
            for name in ["noctalia", "notify-send"] {
                let p = bin.join(name);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
                }
            }
            let old_path = std::env::var("PATH").ok();
            std::env::set_var(
                "PATH",
                format!("{}:{}", bin.display(), old_path.clone().unwrap_or_default()),
            );
            // Isolate the engine's socket seam: no live keeper socket may make
            // the notice decision think a video backend is available.
            let old_runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok();
            let old_skwd_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
            std::env::set_var("XDG_RUNTIME_DIR", tmp.path());
            std::env::remove_var("SKWD_WALL_V2_SOCK");
            Self {
                old_path,
                old_runtime_dir,
                old_skwd_sock,
                notify_marker,
                _bin: bin,
                _tmp: tmp,
                _env: env,
            }
        }

        fn notified(&self) -> bool {
            self.notify_marker.exists()
        }
    }

    impl Drop for ProbeSandbox {
        fn drop(&mut self) {
            match &self.old_path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
            match &self.old_runtime_dir {
                Some(p) => std::env::set_var("XDG_RUNTIME_DIR", p),
                None => std::env::remove_var("XDG_RUNTIME_DIR"),
            }
            match &self.old_skwd_sock {
                Some(p) => std::env::set_var("SKWD_WALL_V2_SOCK", p),
                None => std::env::remove_var("SKWD_WALL_V2_SOCK"),
            }
        }
    }

    /// A theme dir whose provider dir carries a manifest the backend
    /// cannot play (a video that resolves nowhere): the manifest leg must
    /// reach the plugin, refuse, and report the refusal instead of
    /// failing the apply. No live state is touched — the backend gives up
    /// while resolving, before any write, bounce or kill.
    fn manifest_theme() -> TempDir {
        let theme = TempDir::new().unwrap();
        let provider_dir = theme.path().join("providers").join("noctalia-v5");
        std::fs::create_dir_all(&provider_dir).unwrap();
        std::fs::write(
            provider_dir.join(bg_info::MANIFEST_FILE),
            r#"{"version":1,"assignments":{"*":{"filename":"missing.mp4","local_path":"/nonexistent/hve-test-missing.mp4"}}}"#,
        )
        .unwrap();
        theme
    }

    fn request(theme: &TempDir, animated_applied: bool) -> ApplyBackgroundRequest {
        ApplyBackgroundRequest {
            theme_dir: theme.path().to_path_buf(),
            provider_dir: theme.path().join("providers").join("noctalia-v5"),
            animated_applied,
            static_wallpaper: None,
            video_want: bg_info::VideoWant::None,
        }
    }

    /// Fall-through contract, first half: when an exact painter video is
    /// already restored, the manifest leg stays out entirely — no probe,
    /// no notification, no backend touching the saved manifest (the
    /// caller keeps its stale-manifest delete for that case).
    #[test]
    #[serial]
    fn exact_video_already_restored_routes_nothing() {
        let sandbox = ProbeSandbox::new(false);
        let theme = manifest_theme();
        let outcome = apply_animated(&request(&theme, true));
        assert_eq!(
            outcome.leg,
            RoutedLeg::AlreadyApplied,
            "the exact video is already back: nothing else may run"
        );
        assert!(outcome.animated_applied);
        assert_eq!(outcome.notification, BackgroundNotification::NoNotification);
        assert!(
            theme
                .path()
                .join("providers")
                .join("noctalia-v5")
                .join(bg_info::MANIFEST_FILE)
                .exists(),
            "no backend may consume the manifest when the exact video is back"
        );
        assert!(!sandbox.notified(), "no probe ran, so nothing was notified");
    }

    /// Fall-through contract, second half: a theme WITH a manifest and an
    /// available plugin must go to the plugin, get refused (the manifest
    /// resolves nowhere), and stay silent — no notification when the
    /// plugin can deliver.
    #[test]
    #[serial]
    fn manifest_leg_reaches_the_plugin_when_it_can_deliver() {
        let sandbox = ProbeSandbox::new(true);
        let theme = manifest_theme();
        let outcome = apply_animated(&request(&theme, false));
        assert!(
            matches!(
                &outcome.leg,
                RoutedLeg::ManifestRefused(reason)
                    if reason.contains("No mpvpaper videos could be resolved")
            ),
            "the manifest leg must reach the plugin and report its refusal, got {:?}",
            outcome.leg
        );
        assert!(!outcome.animated_applied);
        assert_eq!(outcome.notification, BackgroundNotification::NoNotification);
        assert!(
            !sandbox.notified(),
            "an available plugin needs no notification"
        );
    }

    /// The notification state, both sides of it: a theme WITH a manifest
    /// while the plugin is missing or disabled must carry
    /// `PluginUnavailable` in the outcome AND notify the user before the
    /// refusal — the exact notification the provider used to send
    /// itself, still never fatal to the apply.
    #[test]
    #[serial]
    fn manifest_leg_notifies_when_the_plugin_is_unavailable() {
        let sandbox = ProbeSandbox::new(false);
        let theme = manifest_theme();
        let outcome = apply_animated(&request(&theme, false));
        assert_eq!(
            outcome.notification,
            BackgroundNotification::PluginUnavailable,
            "an unplayable manifest must carry the notification state to the caller"
        );
        assert!(
            sandbox.notified(),
            "the user must be told the plugin is missing or disabled"
        );
        assert!(
            matches!(outcome.leg, RoutedLeg::ManifestRefused(_)),
            "the refusal still comes back to the caller, got {:?}",
            outcome.leg
        );
        assert!(!outcome.animated_applied);
    }

    /// The clear-all leg: a theme WITHOUT a manifest stops running video
    /// wallpapers through the same backend and reports it as `Cleared` —
    /// and it never notifies, exactly as before (the notification only
    /// ever fired for a manifest the theme actually carries).
    #[test]
    #[serial]
    fn manifest_leg_clears_when_the_theme_has_no_manifest() {
        let sandbox = ProbeSandbox::new(false);
        let theme = TempDir::new().unwrap();
        std::fs::create_dir_all(theme.path().join("providers").join("noctalia-v5")).unwrap();
        let outcome = apply_animated(&request(&theme, false));
        assert_eq!(
            outcome.leg,
            RoutedLeg::Cleared,
            "no manifest means: stop running videos, got {:?}",
            outcome.leg
        );
        assert!(!outcome.animated_applied);
        assert_eq!(outcome.notification, BackgroundNotification::NoNotification);
        assert!(!sandbox.notified(), "clear-all must never notify");
    }

    /// T6 new case: a theme that wants a video (a packaged video, or a
    /// recorded video/url) but carries NO manifest and has no backend at all
    /// must be told the backend is missing — while the clear leg still runs
    /// and `animated_applied` stays false, so the caller keeps painting the
    /// poster statically.
    #[test]
    #[serial]
    fn packaged_video_without_any_backend_notifies_and_keeps_the_clear_leg() {
        let sandbox = ProbeSandbox::new(false);
        let theme = TempDir::new().unwrap();
        std::fs::create_dir_all(theme.path().join("providers").join("noctalia-v5")).unwrap();
        let req = ApplyBackgroundRequest {
            theme_dir: theme.path().to_path_buf(),
            provider_dir: theme.path().join("providers").join("noctalia-v5"),
            animated_applied: false,
            static_wallpaper: None,
            video_want: bg_info::VideoWant::ExactPath,
        };

        let outcome = apply_animated(&req);

        assert_eq!(
            outcome.notification,
            BackgroundNotification::PluginUnavailable,
            "a video with no backend must carry the notification state"
        );
        assert!(
            sandbox.notified(),
            "the user must be told no backend can paint the theme's video"
        );
        assert_eq!(
            outcome.leg,
            RoutedLeg::Cleared,
            "the clear leg still runs after the notice, got {:?}",
            outcome.leg
        );
        assert!(
            !outcome.animated_applied,
            "no video was painted: the poster fallback stays in charge"
        );
    }

    /// Defect 1 (verifier): the plugin plays only a SAVED manifest, never a
    /// bare path, so an enabled plugin is NOT a backend for the packaged video
    /// the reference theme ships. With no engine socket the notice MUST fire.
    #[test]
    #[serial]
    fn packaged_video_with_only_the_plugin_enabled_still_notifies_without_an_engine_socket() {
        let sandbox = ProbeSandbox::new(true);
        let theme = TempDir::new().unwrap();
        std::fs::create_dir_all(theme.path().join("providers").join("noctalia-v5")).unwrap();
        let req = ApplyBackgroundRequest {
            theme_dir: theme.path().to_path_buf(),
            provider_dir: theme.path().join("providers").join("noctalia-v5"),
            animated_applied: false,
            static_wallpaper: None,
            video_want: bg_info::VideoWant::ExactPath,
        };

        let outcome = apply_animated(&req);

        assert_eq!(
            outcome.notification,
            BackgroundNotification::PluginUnavailable,
            "the plugin cannot paint a bare path: the notice must fire"
        );
        assert!(
            sandbox.notified(),
            "the user must be told no backend can paint the theme's packaged video"
        );
    }

    /// T6: a STATIC theme (nothing wants a video) must never notify, even with
    /// no backend at all.
    #[test]
    #[serial]
    fn static_theme_never_notifies_for_the_video_backend() {
        let sandbox = ProbeSandbox::new(false);
        let theme = TempDir::new().unwrap();
        std::fs::create_dir_all(theme.path().join("providers").join("noctalia-v5")).unwrap();
        let req = ApplyBackgroundRequest {
            theme_dir: theme.path().to_path_buf(),
            provider_dir: theme.path().join("providers").join("noctalia-v5"),
            animated_applied: false,
            static_wallpaper: None,
            video_want: bg_info::VideoWant::None,
        };

        let outcome = apply_animated(&req);

        assert_eq!(outcome.leg, RoutedLeg::Cleared);
        assert_eq!(outcome.notification, BackgroundNotification::NoNotification);
        assert!(!sandbox.notified(), "a static theme must never notify");
    }

    /// The engine's gate is the socket file: with it absent the routed
    /// path hand-off must report `false` (never panic, never claim a
    /// paint), so the caller keeps every fallback route (D1).
    #[test]
    #[serial]
    fn path_hand_off_reports_failure_when_the_engine_is_absent() {
        let _env = crate::test_utils::env_guard();
        let tmp = TempDir::new().unwrap();
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        std::env::set_var("XDG_RUNTIME_DIR", tmp.path());
        std::env::remove_var("SKWD_WALL_V2_SOCK");
        assert!(!hand_off_path(Path::new("/tmp/absent-engine.jpg")));
        if let Some(v) = orig_runtime {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        if let Some(v) = orig_sock {
            std::env::set_var("SKWD_WALL_V2_SOCK", v);
        } else {
            std::env::remove_var("SKWD_WALL_V2_SOCK");
        }
    }

    /// The routing proof for a bare path: with the engine's gate open (an
    /// existing socket file) and stub engine binaries on PATH, BOTH path
    /// entry points return `true` — the declared choice reached the
    /// engine backend and reported what it painted.
    #[test]
    #[serial]
    fn path_hand_offs_reach_the_engine_backend() {
        let _env = crate::test_utils::env_guard();
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("wall.sock");
        std::fs::write(&sock, b"x").unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["skwd-helm", "skwd-wall-v2"] {
            let p = bin.join(name);
            std::fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let orig_path = std::env::var("PATH").unwrap_or_default();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        std::env::set_var("PATH", format!("{}:{}", bin.display(), orig_path));
        std::env::set_var("SKWD_WALL_V2_SOCK", &sock);
        std::env::remove_var("XDG_RUNTIME_DIR");

        assert!(
            hand_off_path(Path::new("/tmp/a-still.png")),
            "the declared path choice must reach the engine and report its success"
        );
        let req = ApplyBackgroundRequest {
            theme_dir: tmp.path().to_path_buf(),
            provider_dir: tmp.path().join("providers").join("noctalia-v5"),
            animated_applied: false,
            static_wallpaper: Some(PathBuf::from("/tmp/a-still.png")),
            video_want: bg_info::VideoWant::None,
        };
        assert!(
            hand_off_static(&req),
            "the request's saved static path must reach the same engine backend"
        );

        std::env::set_var("PATH", &orig_path);
        if let Some(v) = orig_sock {
            std::env::set_var("SKWD_WALL_V2_SOCK", v);
        } else {
            std::env::remove_var("SKWD_WALL_V2_SOCK");
        }
        if let Some(v) = orig_runtime {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
    }

    /// A request with no saved static wallpaper declares nothing to hand
    /// over: `false`, no spawn, no side effect.
    #[test]
    fn hand_off_static_without_a_saved_wallpaper_paints_nothing() {
        let req = ApplyBackgroundRequest {
            theme_dir: PathBuf::from("/nonexistent-theme"),
            provider_dir: PathBuf::from("/nonexistent-theme/providers/noctalia-v5"),
            animated_applied: false,
            static_wallpaper: None,
            video_want: bg_info::VideoWant::None,
        };
        assert!(!hand_off_static(&req));
    }

    // ── Colour-authority capability: the fake-engine harness ──────────
    //
    // WHY these exist: the hold/release IS the colour-authority yield
    // (W2/W4) the provider used to drive straight into the engine's
    // config. The router owns the capability now, so every call is
    // recorded against a FAKE engine — a private `SKWD_WALL_V2_CONFIG`
    // file — and never against the keeper's live engine config.

    /// The fake engine's config while it owns the colour scheme.
    const FAKE_ENGINE_OWNER: &str =
        r#"{"theme": {"policy": "wallpaper"}, "noctalia": {"themeMode": "manual"}}"#;

    /// Hermetic fake engine: `SKWD_WALL_V2_CONFIG` points at a private
    /// temp config (or at nothing, for the engine-absent case), the shared
    /// env lock is held for the whole test, and `HVE_REASSERT_PROFILE=fast`
    /// keeps the observe wait off the real bound. Restores both env vars on
    /// drop, exactly like the other harnesses.
    struct FakeEngine {
        config: PathBuf,
        saved: Vec<(&'static str, Option<String>)>,
        /// Owns the sandbox directory for the whole test.
        _tmp: tempfile::TempDir,
        /// Shared env lock, released last (after `Drop` restored the keys
        /// and the sandbox dir went away).
        _env: crate::test_utils::TempEnv,
    }

    impl FakeEngine {
        fn new(config_text: Option<&str>) -> Self {
            let env = crate::test_utils::TempEnv::new();
            let saved: Vec<(&'static str, Option<String>)> =
                ["SKWD_WALL_V2_CONFIG", "HVE_REASSERT_PROFILE"]
                    .iter()
                    .map(|k| (*k, std::env::var(k).ok()))
                    .collect();
            let dir = tempfile::tempdir().expect("create fake-engine sandbox");
            let config = dir.path().join("config.json");
            if let Some(text) = config_text {
                std::fs::write(&config, text).unwrap();
            }
            std::env::set_var("SKWD_WALL_V2_CONFIG", &config);
            std::env::set_var("HVE_REASSERT_PROFILE", "fast");
            Self {
                config,
                saved,
                _tmp: dir,
                _env: env,
            }
        }

        fn text(&self) -> String {
            std::fs::read_to_string(&self.config).unwrap_or_default()
        }
    }

    impl Drop for FakeEngine {
        fn drop(&mut self) {
            for (k, v) in &self.saved {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    /// The hold/release pair, recorded end to end against the fake engine:
    /// hold flips `theme.policy` to `off`, disarm hands the previous value
    /// over without restoring, and release puts the exact bytes back and
    /// clears the held state.
    #[test]
    fn hold_then_release_restores_the_fake_engine_byte_for_byte() {
        let engine = FakeEngine::new(Some(FAKE_ENGINE_OWNER));
        let mut hold = hold_color_authority()
            .expect("hold is an honest Result")
            .expect("a wallpaper policy must be held");
        assert!(
            engine.text().contains("\"policy\": \"off\""),
            "the hold must flip the engine config, got: {}",
            engine.text()
        );
        assert!(
            color_authority_held(),
            "the held state must be observable while the hold is open"
        );
        assert!(
            hold.observed_in() < Duration::from_millis(100),
            "the fast profile must shorten the observe wait, took {:?}",
            hold.observed_in()
        );
        let previous = hold
            .disarm()
            .expect("disarm must hand the previous value over");
        assert_eq!(previous, "wallpaper");
        drop(hold); // disarmed: the worker owns the restore now
        assert!(
            engine.text().contains("\"policy\": \"off\""),
            "a disarmed hold must not restore by itself"
        );
        let wrote = release_color_authority(&previous).expect("release is an honest Result");
        assert!(wrote, "release must rewrite the engine config");
        assert_eq!(
            engine.text(),
            FAKE_ENGINE_OWNER,
            "release must restore the exact original bytes"
        );
        assert!(
            !color_authority_held(),
            "release must clear the held state"
        );
    }

    /// Restore-on-every-exit-path, through the new owner: an ARMED hold
    /// restores the previous value when its scope ends — the early-error
    /// path of any apply — and clears the held state behind it.
    #[test]
    fn an_armed_hold_restores_on_scope_exit() {
        let engine = FakeEngine::new(Some(FAKE_ENGINE_OWNER));
        {
            let _hold = hold_color_authority()
                .expect("hold is an honest Result")
                .expect("a wallpaper policy must be held");
            assert!(
                engine.text().contains("\"policy\": \"off\""),
                "the hold must be in effect inside its scope"
            );
        }
        assert_eq!(
            engine.text(),
            FAKE_ENGINE_OWNER,
            "an armed hold must restore the exact bytes on scope exit"
        );
        assert!(!color_authority_held(), "scope exit must clear the held state");
    }

    /// Nothing to hold: a `fixed` policy — and an absent engine config —
    /// must write NOTHING, report no hold and leave no held state, so the
    /// caller behaves exactly as today.
    #[test]
    fn hold_without_ownership_writes_nothing() {
        let fixed = r#"{"theme": {"policy": "fixed"}, "noctalia": {"themeMode": "manual"}}"#;
        {
            let engine = FakeEngine::new(Some(fixed));
            assert!(
                hold_color_authority().expect("hold is an honest Result").is_none(),
                "a fixed policy has no authority to hold"
            );
            assert_eq!(engine.text(), fixed, "the config must stay byte-identical");
            assert!(!color_authority_held(), "no hold -> no held state");
        }
        {
            let engine = FakeEngine::new(None);
            assert!(
                hold_color_authority().expect("hold is an honest Result").is_none(),
                "a missing config has no authority to hold"
            );
            assert!(
                !engine.config.exists(),
                "a missing config must never be created"
            );
            assert!(!color_authority_held(), "no hold -> no held state");
        }
    }

    /// The ownership read goes through the engine's ONE config resolver
    /// (single-resolver rule): both seam states must answer from it —
    /// env override set, and env override unset (platform config dir under
    /// the TempEnv-redirected HOME, where no engine config exists).
    #[test]
    fn engine_owns_color_scheme_reads_the_engine_config_seam() {
        let _env = crate::test_utils::TempEnv::new();
        let saved = std::env::var("SKWD_WALL_V2_CONFIG").ok();
        std::env::remove_var("SKWD_WALL_V2_CONFIG");
        assert!(
            !engine_owns_color_scheme(),
            "no engine config under the sandbox HOME: no ownership"
        );
        let override_path = std::env::temp_dir().join("hve-background-owns-seam.json");
        std::fs::write(&override_path, FAKE_ENGINE_OWNER).unwrap();
        std::env::set_var("SKWD_WALL_V2_CONFIG", &override_path);
        assert!(
            engine_owns_color_scheme(),
            "the env override must reach the engine's single config resolver"
        );
        std::env::remove_var("SKWD_WALL_V2_CONFIG");
        let _ = std::fs::remove_file(&override_path);
        match &saved {
            Some(v) => std::env::set_var("SKWD_WALL_V2_CONFIG", v),
            None => std::env::remove_var("SKWD_WALL_V2_CONFIG"),
        }
    }

    /// The observe wait keeps its measured bound: one 250 ms sleep per
    /// hold (the worst scheduling jitter of the engine's config reload
    /// paths with a wide margin), shortened only by the `fast` test
    /// profile the harness installs.
    #[test]
    fn the_observe_wait_keeps_its_measured_bound() {
        assert_eq!(COLOR_YIELD_OBSERVE_WAIT, Duration::from_millis(250));
    }

    // ── W2: the foreign-background step-aside decision (pure) ─────────
    //
    // odd/tasks/palette-authority-mute-and-yield.md W2: while a theme owns
    // the palette, a background the theme does not declare means the keeper
    // changed it and does not want the theme — HVE steps aside. The table
    // below is the whole rule; the impure shell only gathers the inputs.

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    /// Equal, foreign, and mixed live sets: the declared paths hold the
    /// claim, any other path steps aside (a mixed set already proves one
    /// live path is foreign).
    #[test]
    fn a_foreign_live_background_steps_aside_and_a_declared_one_holds() {
        let declared = paths(&["/theme/poster.png", "/theme/loop.mp4"]);
        for live in [
            paths(&["/theme/poster.png"]),
            paths(&["/theme/loop.mp4"]),
            paths(&["/theme/poster.png", "/theme/loop.mp4"]),
        ] {
            assert_eq!(
                decide_palette_authority(Some(&live), &declared, false, true),
                PaletteAuthorityVerdict::Hold,
                "a live background the theme declares must hold: {live:?}"
            );
        }
        assert_eq!(
            decide_palette_authority(Some(&paths(&["/keeper/car7.jpg"])), &declared, false, true),
            PaletteAuthorityVerdict::StepAside,
            "a live background the theme does not declare must step aside"
        );
        assert_eq!(
            decide_palette_authority(
                Some(&paths(&["/theme/poster.png", "/keeper/car7.jpg"])),
                &declared,
                false,
                true
            ),
            PaletteAuthorityVerdict::StepAside,
            "one foreign live path is enough to step aside"
        );
    }

    /// Unknown is conservative: an unreadable engine, no live outputs, or a
    /// theme that declares nothing can never prove a foreign change.
    #[test]
    fn an_unknown_background_is_never_evidence_of_a_foreign_change() {
        let declared = paths(&["/theme/poster.png"]);
        assert_eq!(
            decide_palette_authority(None, &declared, false, true),
            PaletteAuthorityVerdict::Hold,
            "a failed engine read must never step aside"
        );
        assert_eq!(
            decide_palette_authority(Some(&[]), &declared, false, true),
            PaletteAuthorityVerdict::Hold,
            "an empty live set must never step aside"
        );
        assert_eq!(
            decide_palette_authority(Some(&paths(&["/keeper/car7.jpg"])), &[], false, true),
            PaletteAuthorityVerdict::Hold,
            "a theme that declares no background cannot be compared: hold"
        );
    }

    /// HVE acting is never a foreign change; with no descriptor there is
    /// nothing claimed to step aside from.
    #[test]
    fn hve_acting_and_no_descriptor_both_hold() {
        let declared = paths(&["/theme/poster.png"]);
        assert_eq!(
            decide_palette_authority(
                Some(&paths(&["/keeper/car7.jpg"])),
                &declared,
                true,
                true
            ),
            PaletteAuthorityVerdict::Hold,
            "HVE's own hand-off is never a foreign change"
        );
        assert_eq!(
            decide_palette_authority(
                Some(&paths(&["/keeper/car7.jpg"])),
                &declared,
                false,
                false
            ),
            PaletteAuthorityVerdict::Hold,
            "with no descriptor nothing owns the palette: nothing to step aside from"
        );
    }

    // ── W2: the step-aside shell (descriptor + mute lifecycle) ────────
    //
    // The engine is a FAKE one: a private `SKWD_WALL_V2_CONFIG` and a stub
    // `skwd-helm` / `skwd-wall-v2` on PATH answer `current --json`. The
    // keeper's live engine and config are never touched.

    /// The fake engine's config while it owns the colour scheme.
    const FOREIGN_OWNER: &str =
        r#"{"theme": {"policy": "wallpaper"}, "noctalia": {"themeMode": "manual"}}"#;

    /// Install stub engine binaries answering `current --json` with `live`
    /// (one connected output per path); `None` installs binaries that fail,
    /// for the unreadable-engine case.
    fn install_current_stub(bin_dir: &Path, live: Option<&[&str]>) {
        std::fs::create_dir_all(bin_dir).unwrap();
        let script = match live {
            None => "#!/bin/sh\nexit 1\n".to_string(),
            Some(paths) => {
                let outputs: Vec<String> = paths
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        format!(
                            r#"{{"name":"OUT-{i}","type":"static","connected":true,"current":"{p}"}}"#
                        )
                    })
                    .collect();
                let json = format!(r#"{{"outputs":[{}]}}"#, outputs.join(","));
                let payload = bin_dir.join("current.json");
                std::fs::write(&payload, json).unwrap();
                format!(
                    "#!/bin/sh\nif [ \"$1\" = current ]; then cat '{}'; exit 0; fi\nexit 1\n",
                    payload.display()
                )
            }
        };
        for bin in ["skwd-helm", "skwd-wall-v2"] {
            let path = bin_dir.join(bin);
            std::fs::write(&path, &script).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    }

    /// Hermetic step-aside sandbox: `TempEnv` redirects HOME (the descriptor
    /// and cache live under it), `SKWD_WALL_V2_CONFIG` points at a private
    /// engine config, stub engine binaries answer on PATH and the `fast`
    /// profile keeps the observe wait off the real bound. Restores every
    /// process-global on drop, before the env lock is released.
    struct ForeignSandbox {
        config: PathBuf,
        saved: Vec<(&'static str, Option<String>)>,
        _tmp: tempfile::TempDir,
        _env: crate::test_utils::TempEnv,
    }

    impl ForeignSandbox {
        fn new(engine_config: Option<&str>, live: Option<&[&str]>) -> Self {
            let env = crate::test_utils::TempEnv::new();
            let saved: Vec<(&'static str, Option<String>)> =
                ["SKWD_WALL_V2_CONFIG", "HVE_REASSERT_PROFILE", "PATH"]
                    .iter()
                    .map(|k| (*k, std::env::var(k).ok()))
                    .collect();
            let tmp = tempfile::tempdir().expect("create foreign sandbox");
            let config = tmp.path().join("engine-config.json");
            if let Some(text) = engine_config {
                std::fs::write(&config, text).unwrap();
            }
            let bin = tmp.path().join("bin");
            install_current_stub(&bin, live);
            std::env::set_var("SKWD_WALL_V2_CONFIG", &config);
            std::env::set_var("HVE_REASSERT_PROFILE", "fast");
            let old_path = saved[2].1.clone().unwrap_or_default();
            std::env::set_var("PATH", format!("{}:{}", bin.display(), old_path));
            Self {
                config,
                saved,
                _tmp: tmp,
                _env: env,
            }
        }

        fn read_config(&self) -> String {
            std::fs::read_to_string(&self.config).unwrap_or_default()
        }
    }

    impl Drop for ForeignSandbox {
        fn drop(&mut self) {
            for (k, v) in &self.saved {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    /// A theme whose provider dir declares the packaged `poster_rel` poster,
    /// plus a real `palette.json` so the descriptor's path chain is valid.
    /// Returns `(theme_dir, provider_dir, poster_abs)`.
    fn declared_theme(root: &Path, poster_rel: &str) -> (PathBuf, PathBuf, PathBuf) {
        let theme = root.join("theme");
        let provider = theme.join("providers").join("noctalia-v5");
        let media = theme.join("media");
        std::fs::create_dir_all(&media).unwrap();
        let poster = media.join(Path::new(poster_rel).file_name().unwrap());
        std::fs::write(&poster, b"\x89PNG").unwrap();
        crate::theme_media::write_poster_record(&provider, Path::new(poster_rel)).unwrap();
        std::fs::write(provider.join("palette.json"), b"{}").unwrap();
        (theme, provider, poster)
    }

    fn descriptor_for(provider: &Path) -> crate::color_authority::ColorAuthority {
        crate::color_authority::ColorAuthority {
            backend: "noctalia-v5".to_string(),
            theme: "JoKer".to_string(),
            palette_file: provider.join("palette.json").display().to_string(),
            palette_name: "JokerTheme".to_string(),
        }
    }

    /// Put HVE in the CLAIMED state exactly as a real apply leaves it: the
    /// engine held off (`theme.policy` off, pending marker) and the theme's
    /// descriptor present. HVE is not acting afterwards.
    fn enter_claimed_state(sandbox: &ForeignSandbox, provider: &Path) {
        let mut hold = hold_color_authority()
            .expect("hold is an honest Result")
            .expect("the owner config must yield");
        hold.disarm();
        drop(hold); // the hand-off phase is over: HVE is no longer acting
        crate::color_authority::write_descriptor(&descriptor_for(provider)).unwrap();
        assert!(
            sandbox.read_config().contains("\"policy\": \"off\""),
            "precondition: the engine is held off"
        );
        assert!(color_authority_held(), "precondition: the marker is pending");
        assert!(
            crate::color_authority::read_descriptor().is_some(),
            "precondition: a theme owns the palette"
        );
    }

    /// The core of W2: a foreign live background clears the descriptor AND
    /// releases the mute, so the keeper's background and its palette win.
    #[test]
    fn a_foreign_background_clears_the_descriptor_and_releases_the_mute() {
        let sandbox = ForeignSandbox::new(Some(FOREIGN_OWNER), Some(&["/keeper/car7.jpg"]));
        let theme_tmp = tempfile::tempdir().unwrap();
        let (_theme, provider, _poster) = declared_theme(theme_tmp.path(), "media/poster.png");
        enter_claimed_state(&sandbox, &provider);

        let outcome = step_aside_if_foreign().expect("the check is an honest Result");

        assert_eq!(outcome, StepAsideOutcome::SteppedAside);
        assert!(
            crate::color_authority::read_descriptor().is_none(),
            "the theme's claim must be released"
        );
        assert_eq!(
            sandbox.read_config(),
            FOREIGN_OWNER,
            "the engine's own value must come back: the keeper's background wins"
        );
        assert!(
            !color_authority_held(),
            "the pending marker must be cleared with the mute"
        );
    }

    /// A live background the theme declares keeps the claim and the mute.
    #[test]
    fn a_declared_live_background_keeps_the_claim_and_the_mute() {
        let theme_tmp = tempfile::tempdir().unwrap();
        let (_theme, provider, poster) = declared_theme(theme_tmp.path(), "media/poster.png");
        let poster = poster.display().to_string();
        let sandbox = ForeignSandbox::new(Some(FOREIGN_OWNER), Some(&[poster.as_str()]));
        enter_claimed_state(&sandbox, &provider);

        let outcome = step_aside_if_foreign().expect("the check is an honest Result");

        assert_eq!(outcome, StepAsideOutcome::Held);
        assert!(
            crate::color_authority::read_descriptor().is_some(),
            "the theme still owns the palette"
        );
        assert!(
            sandbox.read_config().contains("\"policy\": \"off\""),
            "the mute must survive a declared background"
        );
        assert!(color_authority_held(), "the marker stays pending");
    }

    /// An unreadable engine is never evidence: HVE holds, conservatively.
    #[test]
    fn an_unreadable_engine_never_steps_aside() {
        let sandbox = ForeignSandbox::new(Some(FOREIGN_OWNER), None);
        let theme_tmp = tempfile::tempdir().unwrap();
        let (_theme, provider, _poster) = declared_theme(theme_tmp.path(), "media/poster.png");
        enter_claimed_state(&sandbox, &provider);

        let outcome = step_aside_if_foreign().expect("the check is an honest Result");

        assert_eq!(outcome, StepAsideOutcome::Held);
        assert!(
            crate::color_authority::read_descriptor().is_some(),
            "a read failure must not release the claim"
        );
        assert!(sandbox.read_config().contains("\"policy\": \"off\""));
    }

    /// While HVE is acting, its own hand-off is never a foreign change.
    #[test]
    fn hve_acting_never_steps_aside() {
        let sandbox = ForeignSandbox::new(Some(FOREIGN_OWNER), Some(&["/keeper/car7.jpg"]));
        let theme_tmp = tempfile::tempdir().unwrap();
        let (_theme, provider, _poster) = declared_theme(theme_tmp.path(), "media/poster.png");
        let hold = hold_color_authority()
            .expect("hold is an honest Result")
            .expect("the owner config must yield");
        crate::color_authority::write_descriptor(&descriptor_for(&provider)).unwrap();
        assert!(hve_is_acting(), "precondition: a hold is alive");

        let outcome = step_aside_if_foreign().expect("the check is an honest Result");

        assert_eq!(outcome, StepAsideOutcome::Held);
        assert!(
            crate::color_authority::read_descriptor().is_some(),
            "HVE's own apply must not release the claim"
        );
        assert!(sandbox.read_config().contains("\"policy\": \"off\""));
        drop(hold);
    }

    /// Once aside, every later check is a no-op: nothing claims the palette,
    /// so nothing can be released twice.
    #[test]
    fn once_aside_further_background_changes_are_a_noop() {
        let sandbox = ForeignSandbox::new(Some(FOREIGN_OWNER), Some(&["/keeper/car7.jpg"]));
        let theme_tmp = tempfile::tempdir().unwrap();
        let (_theme, provider, _poster) = declared_theme(theme_tmp.path(), "media/poster.png");
        enter_claimed_state(&sandbox, &provider);
        assert_eq!(
            step_aside_if_foreign().unwrap(),
            StepAsideOutcome::SteppedAside
        );

        let again = step_aside_if_foreign().expect("the check is an honest Result");

        assert_eq!(again, StepAsideOutcome::NothingClaimed);
        assert_eq!(
            sandbox.read_config(),
            FOREIGN_OWNER,
            "the engine's own value must stay untouched"
        );
    }

    /// Applying a theme re-claims: a new hold mutes the engine again and the
    /// theme's descriptor makes it own the palette once more.
    #[test]
    fn applying_a_theme_reclaims_and_remutes() {
        let sandbox = ForeignSandbox::new(Some(FOREIGN_OWNER), Some(&["/keeper/car7.jpg"]));
        let theme_tmp = tempfile::tempdir().unwrap();
        let (_theme, provider, _poster) = declared_theme(theme_tmp.path(), "media/poster.png");
        enter_claimed_state(&sandbox, &provider);
        assert_eq!(
            step_aside_if_foreign().unwrap(),
            StepAsideOutcome::SteppedAside
        );

        // A new apply: the hold mutes the engine and the theme writes its
        // descriptor — the claim is back.
        let mut hold = hold_color_authority()
            .expect("hold is an honest Result")
            .expect("the engine must yield again");
        assert!(sandbox.read_config().contains("\"policy\": \"off\""));
        assert!(color_authority_held());
        crate::color_authority::write_descriptor(&descriptor_for(&provider)).unwrap();
        let previous = hold.disarm().expect("hand the restore to the worker");
        drop(hold);

        assert_eq!(previous, "wallpaper");
        assert!(
            crate::color_authority::read_descriptor().is_some(),
            "the re-applied theme owns the palette again"
        );
        assert!(
            color_authority_held(),
            "the mute outlives the apply while the theme rules"
        );
    }

    /// The descriptor→declared resolution derives the theme and provider
    /// dirs from the descriptor's `palette_file` (the layout the provider's
    /// apply writes).
    #[test]
    fn declared_backgrounds_are_resolved_from_the_descriptor() {
        let theme_tmp = tempfile::tempdir().unwrap();
        let (_theme, provider, poster) = declared_theme(theme_tmp.path(), "media/poster.png");
        let declared = declared_backgrounds_for(&descriptor_for(&provider));
        assert!(
            declared.contains(&poster.display().to_string()),
            "the theme's packaged poster must be declared: {declared:?}"
        );
    }
}
