pub mod background;
pub mod bg_info;
pub mod hve_presets;
pub mod mpvpaper;
pub mod noctalia;
pub mod noctalia_paths;
pub mod noctalia_runtime;
pub mod shell_capabilities;
pub mod skwd_engine;
pub mod skwd_policy;
pub mod wallpaper_authority;

/// Composition-root re-export (W2 of
/// `odd/tasks/palette-authority-mute-and-yield.md`): `main` starts the
/// foreign-background watch through the registration router, the same way it
/// reaches the other provider entry points, so the core never names the
/// `background` router directly.
pub use background::start_foreign_change_watch;

/// W6 re-export: `main` starts the picker-voice watch through the same router.
pub use background::start_picker_voice_watch;

/// W6 re-export: a backend-neutral capability for the CORE. `ipc.rs` must not
/// name the engine module whose policy this reads, so it asks a capability
/// through the router (deliberately un-scanned: naming backends is its job),
/// exactly like `start_foreign_change_watch`.
pub(crate) use skwd_policy::color_authority_recently_suspended;

use crate::config::Config;
use crate::engine::Engine;
use crate::theme_manager::{ThemeManager, ThemeProvider};

/// One shipped desktop-shell backend. Adding a shell is ONE entry here plus
/// its provider module: the registration router never needs an `if` for it.
struct ShellBackend {
    /// The provider id a theme records (`noctalia-v5`, `noctalia`).
    id: &'static str,
    /// True when this shell is running now.
    detect: fn() -> bool,
    /// The historical fallback: used when no backend detects. The LAST entry
    /// carries it.
    fallback: bool,
    /// Build the live provider (save/apply/list).
    make: fn() -> Box<dyn ThemeProvider>,
    /// Build the side-effect-free read-only declarer for cleanup and preview.
    declare: fn() -> Box<dyn ThemeProvider>,
}

/// The shipped shell backends, in detection order. v5 is tried first; v4 is
/// the historical fallback when nothing detects.
fn shell_backends() -> [ShellBackend; 2] {
    [
        ShellBackend {
            id: "noctalia-v5",
            detect: || noctalia_paths::NoctaliaV5Paths.is_active(),
            fallback: false,
            make: || Box::new(noctalia::NoctaliaV5Provider::new()) as Box<dyn ThemeProvider>,
            declare: || Box::new(noctalia::NoctaliaV5Provider::new()) as Box<dyn ThemeProvider>,
        },
        ShellBackend {
            id: "noctalia",
            detect: || noctalia_paths::NoctaliaV4Paths.is_active(),
            fallback: true,
            make: || Box::new(noctalia::NoctaliaV4Provider::new()) as Box<dyn ThemeProvider>,
            declare: || Box::new(noctalia::NoctaliaV4Provider::new()) as Box<dyn ThemeProvider>,
        },
    ]
}

/// Register every provider shipped with HVE, in the canonical order.
///
/// Detection order decides WHICH backend is wanted: the first entry whose
/// detector fires wins; the fallback entry is used when none detects. The
/// `Config::disabled_providers` list only SUBTRACTS from that decision: a
/// disabled wanted backend registers nothing — the other version is never
/// substituted for it, because the keeper turned that backend off. Unknown
/// ids match nothing.
///
/// `hve-presets` is a provider, not a shell, so it is not part of the backend
/// list; it registers unless its own id is disabled.
pub fn register_default_providers(tm: &mut ThemeManager, engine: &Engine) {
    let disabled = Config::load().disabled_providers;
    let backends = shell_backends();

    let wanted = backends
        .iter()
        .find(|b| (b.detect)())
        .or_else(|| backends.iter().find(|b| b.fallback));

    match wanted {
        Some(b) if disabled.iter().any(|d| d == b.id) => {
            tracing::info!(
                "[providers] provider '{}' disabled in config — skipping registration",
                b.id
            );
        }
        Some(b) => {
            tm.register_provider((b.make)());
            tracing::info!("[providers] registered provider '{}'", b.id);
        }
        None => {
            tracing::warn!("[providers] no shipped shell backend to register");
        }
    }

    if disabled.iter().any(|d| d == "hve-presets") {
        tracing::info!("[providers] provider 'hve-presets' disabled in config — skipping registration");
    } else {
        tm.register_provider(Box::new(hve_presets::HvePresetsProvider::new(engine.clone())));
    }
}

/// The capability adapter for the shell that is running. Today the only
/// shipped shell is Noctalia; a future shell adds its own adapter here. The
/// core calls only this, never a shell name, path or CLI.
pub fn active_shell() -> Box<dyn shell_capabilities::ShellCapabilities> {
    Box::new(crate::providers::noctalia::NoctaliaShell)
}

/// A READ-ONLY declaration instance of a shipped provider, obtained by id for
/// cleanup decisions about a theme that RECORDED that id when it was saved —
/// and for the gallery's `read-preview-source` declarations, which the core
/// resolves from the theme's own `providers/` directory names the same way.
///
/// Registration gates behaviour (save/apply/list) — it must not gate
/// knowledge: the backend contract's `deletable-artefacts` applies "when a
/// theme that used this backend is removed"
/// (`openspec/specs/capability-routing/spec.md`), whether or not that backend
/// is active — or enabled — today. The core calls this with the provider ids a
/// theme's own `providers/` tree records; it never names a backend itself.
/// `None` for an id this product does not ship and for `hve-presets`
/// (constructing it needs an `Engine` handle, and it declares no artifacts).
///
/// Constructing a declarer must stay side-effect free — the only methods the
/// cleanup and preview seams call are `deletable_artifacts` and
/// `preview_sources`, both of which only read the theme's own record.
pub fn declaration_provider(id: &str) -> Option<Box<dyn ThemeProvider>> {
    // The retired `wallpaper` id (see `src/providers/wallpaper.rs`, inert)
    // stored a byte-copy of Noctalia v4's OWN cache record — the same
    // `wallpapers.json` file v4 saves from — so v4 is its read-only
    // declarer: it parses the record format it owns in whatever provider
    // directory the core hands it. Themes on disk still carry
    // `providers/wallpaper/` trees; without this alias their preview source
    // would silently disappear from the gallery.
    let lookup = if id == "wallpaper" { "noctalia" } else { id };
    shell_backends()
        .iter()
        .find(|b| b.id == lookup)
        .map(|b| (b.declare)())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempEnv;
    use std::path::Path;

    /// Hermetic shell-detection sandbox for the registration seam.
    ///
    /// `TempEnv` redirects `HOME`/`XDG_*`, so `Config::load()` and the v5
    /// `profiles` marker read a throwaway home and never the keeper's config.
    /// A stub `pgrep` prepended to `PATH` makes `NoctaliaV5Paths::is_active()`
    /// deterministic instead of depending on whatever is running on the box;
    /// every other `pgrep` query is delegated to the real binary, so the PATH
    /// shadow only moves the one probe under test. `Drop` restores `PATH` and
    /// the removed override while the shared environment lock is still held —
    /// the same shape as `StubNoctalia` in `providers/noctalia_runtime.rs`.
    struct ShellSandbox {
        /// Restored first: `Drop` puts these back before any field goes.
        old_path: Option<String>,
        old_noctalia_config: Option<String>,
        /// Lives on `PATH` until `old_path` is restored above.
        _bin: tempfile::TempDir,
        /// Shared env lock, released last.
        _env: TempEnv,
    }

    impl ShellSandbox {
        /// `v5_active` decides what `pgrep -x noctalia` answers: `true` makes
        /// shell detection pick the v5 provider (the sandboxed
        /// `~/.config/noctalia/profiles` marker is seeded by `write_config`),
        /// `false` leaves the v5 probe negative so registration reaches the
        /// v4 side of the decision.
        fn new(v5_active: bool) -> Self {
            let env = TempEnv::new();
            let bin = tempfile::tempdir().expect("stub bin dir");

            // Locate the real `pgrep` on the ORIGINAL PATH before shadowing it.
            let mut real_pgrep = String::new();
            if let Ok(paths) = std::env::var("PATH") {
                for dir in paths.split(':') {
                    let candidate = Path::new(dir).join("pgrep");
                    if candidate.is_file() {
                        real_pgrep = candidate.to_string_lossy().into_owned();
                        break;
                    }
                }
            }
            let answer = if v5_active { "0" } else { "1" };
            let body = if real_pgrep.is_empty() {
                format!(
                    "#!/bin/bash\n\
                     if [ \"$1\" = \"-x\" ] && [ \"$2\" = \"noctalia\" ]; then exit {answer}; fi\n\
                     exit 1\n"
                )
            } else {
                format!(
                    "#!/bin/bash\n\
                     if [ \"$1\" = \"-x\" ] && [ \"$2\" = \"noctalia\" ]; then exit {answer}; fi\n\
                     exec {real_pgrep} \"$@\"\n"
                )
            };
            let stub = bin.path().join("pgrep");
            std::fs::write(&stub, body).expect("stub pgrep script");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
                    .expect("stub must be executable");
            }

            // The v5 detector consults this override BEFORE the sandboxed
            // HOME; drop it so the seeded marker is the only profiles source.
            let old_noctalia_config = std::env::var("HVE_NOCTALIA_CONFIG").ok();
            std::env::remove_var("HVE_NOCTALIA_CONFIG");

            let old_path = std::env::var("PATH").ok();
            let new_path = match &old_path {
                Some(prev) => format!("{}:{}", bin.path().display(), prev),
                None => bin.path().display().to_string(),
            };
            std::env::set_var("PATH", &new_path);

            Self {
                old_path,
                old_noctalia_config,
                _bin: bin,
                _env: env,
            }
        }

        /// Seed the v5-only `profiles` marker under the sandboxed HOME and
        /// write the sandbox `config.json` — never the keeper's file — with
        /// the given `disabled_providers`.
        fn write_config(&self, disabled: &[&str]) {
            let config_dir = dirs::config_dir().expect("sandbox config dir");
            std::fs::create_dir_all(config_dir.join("noctalia").join("profiles"))
                .expect("seed v5 profiles marker");
            crate::config::Config {
                disabled_providers: disabled.iter().map(|s| (*s).to_string()).collect(),
                ..Default::default()
            }
            .save()
            .expect("sandbox config must save");
        }
    }

    impl Drop for ShellSandbox {
        fn drop(&mut self) {
            match &self.old_path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
            match &self.old_noctalia_config {
                Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
                None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
            }
        }
    }

    /// Register through the real seam and return the resulting ids.
    fn registered_ids(_sandbox: &ShellSandbox) -> Vec<String> {
        let config_dir = dirs::config_dir().expect("sandbox config dir");
        let mut tm = ThemeManager::new(&config_dir);
        let engine = Engine::new(Path::new("/nonexistent-hve-project"));
        register_default_providers(&mut tm, &engine);
        tm.provider_ids()
    }

    /// `disabled_providers` must gate registration at the seam: with the
    /// shell selecting v5, disabling `noctalia-v5` skips it — and the v4
    /// provider must NOT be substituted for it, because the keeper turned
    /// that backend off. The presets provider must survive untouched.
    #[test]
    fn disabled_noctalia_v5_is_skipped_without_v4_substitution() {
        let sandbox = ShellSandbox::new(true);
        sandbox.write_config(&["noctalia-v5"]);

        let ids = registered_ids(&sandbox);
        assert_eq!(
            ids,
            vec!["hve-presets"],
            "a disabled provider must not register, and the other Noctalia \
             version must not be substituted for it"
        );
    }

    /// Same gate on the v4 side of the decision: both the `else if v4 active`
    /// branch and the `else` fallback register `NoctaliaV4Provider`, whose id
    /// is `noctalia`.
    #[test]
    fn disabled_noctalia_v4_provider_is_skipped() {
        let sandbox = ShellSandbox::new(false);
        sandbox.write_config(&["noctalia"]);

        let ids = registered_ids(&sandbox);
        assert_eq!(
            ids,
            vec!["hve-presets"],
            "the v4 provider must be skipped when its id is disabled"
        );
    }

    /// `hve-presets` is a provider too: the same list disables it.
    #[test]
    fn disabled_hve_presets_is_skipped() {
        let sandbox = ShellSandbox::new(true);
        sandbox.write_config(&["hve-presets"]);

        let ids = registered_ids(&sandbox);
        assert_eq!(ids, vec!["noctalia-v5"], "presets must not register when disabled");
    }

    /// An empty list registers exactly what the seam registers today: the
    /// shell-selected v5 provider plus presets.
    #[test]
    fn empty_disabled_list_registers_the_v5_selection() {
        let sandbox = ShellSandbox::new(true);
        sandbox.write_config(&[]);

        let ids = registered_ids(&sandbox);
        assert_eq!(
            ids,
            vec!["noctalia-v5", "hve-presets"],
            "an empty disabled list must not change today's registration"
        );
    }

    /// An empty list also preserves the historical v4 fallback selection.
    #[test]
    fn empty_disabled_list_registers_the_v4_fallback() {
        let sandbox = ShellSandbox::new(false);
        sandbox.write_config(&[]);

        let ids = registered_ids(&sandbox);
        assert_eq!(
            ids,
            vec!["noctalia", "hve-presets"],
            "the v4 fallback must keep registering when nothing is disabled"
        );
    }

    /// Unknown (or malformed) ids in the list simply match nothing: no panic,
    /// no change to the registered set.
    #[test]
    fn unknown_disabled_id_changes_nothing() {
        let sandbox = ShellSandbox::new(false);
        sandbox.write_config(&["not-a-real-provider", ""]);

        let ids = registered_ids(&sandbox);
        assert_eq!(
            ids,
            vec!["noctalia", "hve-presets"],
            "an unknown id must not skip, panic, or alter registration"
        );
    }

    /// `read-preview-source` coverage proof: the gallery resolves a
    /// theme's preview sources by listing `providers/` dirs (sorted) and
    /// asking the registry — so every shipped id whose provider writes a
    /// preview record MUST resolve a read-only declarer, or its record
    /// would silently vanish from the gallery. This is the "no shipped
    /// dir left unrepresented" obligation of that convention.
    #[test]
    fn every_shipped_preview_record_id_resolves_a_declarer() {
        // The three ids that appear under a theme's `providers/` tree and
        // own a preview record: `noctalia-v5` (assignment manifest,
        // painter video, wallpaper text), `noctalia` (v4's own
        // `wallpapers.json`), `wallpaper` (the retired id's byte-copy of
        // that same record).
        for id in ["noctalia-v5", "noctalia", "wallpaper"] {
            assert!(
                declaration_provider(id).is_some(),
                "the shipped id '{id}' owns a preview record and must resolve a declarer"
            );
        }

        // `hve-presets` is deliberately absent from the registry (its
        // constructor needs an `Engine` handle) and its save writes only
        // `state.json` — a config record, never a preview source. Whether
        // it stays absent or gains a declarer later, `state.json` must
        // never become one.
        let presets_dir = tempfile::tempdir().expect("presets dir");
        std::fs::write(
            presets_dir.path().join("state.json"),
            r#"{"border_radius":12}"#,
        )
        .expect("record");
        if let Some(declarer) = declaration_provider("hve-presets") {
            assert!(
                declarer.preview_sources(presets_dir.path()).is_empty(),
                "hve-presets owns no preview record; a declarer for it must declare none"
            );
        }
    }

    /// A theme snapshot must never own the keeper's system state.
    ///
    /// Regression guard for the removed `hyprland-settings` provider: applying
    /// a saved theme whose `meta.json` still lists that provider (as every
    /// stale snapshot on disk does) must leave the WINDOW RULES, KEYBINDS and
    /// AUTOSTART blocks of `hve-settings.lua` byte-identical.
    #[test]
    fn applying_a_theme_does_not_touch_settings_system_blocks() {
        let _env = TempEnv::new();

        // 1. The keeper's live settings file: all three system blocks present,
        //    the autostart block pointing at the historical dead dev path.
        let settings_path = crate::config::hve_settings_path();
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let before = "\
-- >>> HVE WINDOW RULES <<<\n\
-- HVE manages its own window state via the Composer trait\n\
-- >>> HVE WINDOW RULES END <<<\n\
-- >>> HVE KEYBINDS <<<\n\
hl.bind(\"SUPER + H\", hl.dsp.exec_cmd(\"hve-ipc toggle-tray\"))\n\
-- >>> HVE KEYBINDS END <<<\n\
-- >>> HVE AUTOSTART <<<\n\
hl.on(\"hyprland.start\", function()\n\
hl.exec_cmd(\"/home/ximo/hve/target/debug/hve --tray\")\n\
end)\n\
-- >>> HVE AUTOSTART END <<<\n";
        std::fs::write(&settings_path, before).unwrap();

        // 2. A stale theme snapshot: `meta.json` still lists the removed
        //    provider and its `state.json` carries the old system-state payload
        //    (disabled autostart + the 5 stale keybinds).
        let config_dir = dirs::config_dir().unwrap();
        let theme_dir = config_dir.join("hve").join("themes").join("Stale");
        let provider_dir = theme_dir.join("providers").join("hyprland-settings");
        std::fs::create_dir_all(&provider_dir).unwrap();
        std::fs::write(
            theme_dir.join("meta.json"),
            r#"{
                "saved_at": "2026-09-06T22:24:00.000Z",
                "description": "",
                "providers": ["hyprland-settings"]
            }"#,
        )
        .unwrap();
        std::fs::write(
            provider_dir.join("state.json"),
            r#"{
                "keybinds": "hl.bind(\"SUPER + Q\", hl.dsp.exec_cmd(\"kitty\"))\nhl.bind(\"SUPER + E\", hl.dsp.exec_cmd(\"thunar\"))\nhl.bind(\"SUPER + R\", hl.dsp.exec_cmd(\"rofi -show drun\"))\nhl.bind(\"SUPER + T\", hl.dsp.exec_cmd(\"telegram\"))\nhl.bind(\"SUPER + B\", hl.dsp.exec_cmd(\"firefox\"))",
                "autostart": null,
                "window_rules": null
            }"#,
        )
        .unwrap();

        // 3. Apply through the real provider list the app registers.
        let mut tm = ThemeManager::new(&config_dir);
        let engine = Engine::new(Path::new("/nonexistent-hve-project"));
        register_default_providers(&mut tm, &engine);

        // 3a. Structural prevention: no shipped provider may own system state.
        //     This half is load-bearing. If the retired `hyprland-settings`
        //     provider were ever re-registered, this fails even if the
        //     byte-identity check below stayed green — for instance because
        //     the seeded meta no longer lists the id and `apply` therefore
        //     selects no provider at all.
        let ids = tm.provider_ids();
        assert!(
            !ids.iter().any(|id| id == "hyprland-settings"),
            "no provider may own system state: 'hyprland-settings' must stay \
             unregistered by register_default_providers, got {ids:?}"
        );

        // 3b. Behavioural evidence: with the id absent from the registration,
        //     applying the stale snapshot leaves the system blocks untouched.
        tm.apply("Stale", || Ok(()))
            .expect("applying the stale theme should succeed");

        // 4. The system blocks must be byte-identical. This is the behavioural
        //    evidence that pairs with the structural assertion above: the
        //    assertion prevents the provider from coming back, this proves the
        //    live registration truly does not touch system state today.
        let after = std::fs::read_to_string(&settings_path).unwrap();
        assert_eq!(
            after, before,
            "applying a theme must not modify the system blocks of hve-settings.lua"
        );
    }

    /// The registry is data-driven: adding a shell is one entry, and the
    /// shipped list carries the detection order plus exactly one fallback,
    /// which must be the LAST entry (v4).
    #[test]
    fn shipped_backends_list_owns_detection_and_construction() {
        let backends = shell_backends();
        assert_eq!(
            backends.iter().map(|b| b.id).collect::<Vec<_>>(),
            vec!["noctalia-v5", "noctalia"]
        );
        assert_eq!(
            backends.iter().filter(|b| b.fallback).count(),
            1,
            "exactly one backend is the historical fallback"
        );
        assert!(
            !backends[0].fallback && backends[1].fallback,
            "the fallback must be the last entry"
        );
    }
}
