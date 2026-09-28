pub mod background;
pub mod bg_info;
pub mod hve_presets;
pub mod mpvpaper;
pub mod noctalia;
pub mod noctalia_runtime;
pub mod shell;
pub mod skwd_engine;
pub mod skwd_policy;
pub mod wallpaper_authority;

use crate::config::Config;
use crate::engine::Engine;
use crate::theme_manager::ThemeManager;
use shell::ShellDetector;

/// Register every provider shipped with HVE, in the canonical order.
///
/// Shell detection picks the Noctalia provider that matches the running shell
/// (v5 when active, v4 otherwise — the v4 fallback preserves the historical
/// startup behaviour). Both the main window's manager and the gallery's
/// manager call this, so they always expose the same provider list.
///
/// `Config::disabled_providers` gates that decision: an id listed there is
/// skipped entirely, so the same list applies to every caller (main window,
/// gallery, IPC) because it is read here, inside the seam.
pub fn register_default_providers(tm: &mut ThemeManager, engine: &Engine) {
    let disabled = Config::load().disabled_providers;

    let noctalia_v5 = shell::NoctaliaV5Paths;
    let noctalia_v4 = shell::NoctaliaV4Paths;

    // Shell detection still decides WHICH Noctalia provider is wanted — v5
    // when active, v4 when active, v4 as the historical fallback — and the
    // disabled list only SUBTRACTS from that decision. The matched ids are
    // exactly the strings `ThemeProvider::id()` returns: `noctalia-v5` for
    // v5, `noctalia` for v4 (the v4 provider has no `noctalia-v4` id). When
    // the wanted version is disabled we register NO Noctalia provider: the
    // other version is never substituted for it, because the keeper turned
    // that backend off. Unknown ids match nothing.
    let v5_active = noctalia_v5.is_active();
    let v4_active = noctalia_v4.is_active();
    let wanted: &str = if v5_active { "noctalia-v5" } else { "noctalia" };

    if disabled.iter().any(|d| d == wanted) {
        tracing::info!("[providers] provider '{wanted}' disabled in config — skipping registration");
    } else if v5_active {
        tm.register_provider(Box::new(noctalia::NoctaliaV5Provider::new()));
        tracing::info!("[shell] Noctalia v5 detectado — registrando provider v5");
    } else if v4_active {
        tm.register_provider(Box::new(noctalia::NoctaliaV4Provider::new()));
        tracing::info!("[shell] Noctalia v4 detectado — registrando provider v4");
    } else {
        tm.register_provider(Box::new(noctalia::NoctaliaV4Provider::new()));
        tracing::warn!("[shell] Noctalia no detectado — registrando provider v4 por defecto");
    }

    if disabled.iter().any(|d| d == "hve-presets") {
        tracing::info!("[providers] provider 'hve-presets' disabled in config — skipping registration");
    } else {
        tm.register_provider(Box::new(hve_presets::HvePresetsProvider::new(engine.clone())));
    }
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
-- HVE 2 manages its own window state via the Composer trait\n\
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
}
