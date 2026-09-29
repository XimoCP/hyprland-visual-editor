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
    if !manifest_exists {
        return match crate::providers::mpvpaper::clear_all() {
            Ok(()) => ApplyBackgroundOutcome {
                animated_applied: false,
                notification: BackgroundNotification::NoNotification,
                leg: RoutedLeg::Cleared,
            },
            Err(reason) => ApplyBackgroundOutcome {
                animated_applied: false,
                notification: BackgroundNotification::NoNotification,
                leg: RoutedLeg::ClearFailed(reason),
            },
        };
    }
    let notification = if crate::providers::mpvpaper::mpvpaper_enabled() {
        BackgroundNotification::NoNotification
    } else {
        bg_info::notify_plugin_required();
        BackgroundNotification::PluginUnavailable
    };
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
    Ok(Some(ColourAuthorityHold {
        guard: crate::providers::skwd_policy::YieldGuard::new(previous),
        observed_in,
    }))
}

/// Put the engine's colour authority back EXACTLY as it was — the paired
/// release for a hold that was disarmed into a worker. `Ok(true)` when the
/// config was rewritten, `Ok(false)` when there was nothing to do (config
/// gone, or a concurrent write won), `Err` an honest failure the caller
/// logs and carries on from.
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
            Self {
                old_path,
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
}
