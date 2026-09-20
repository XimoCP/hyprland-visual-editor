pub mod hve_presets;
pub mod mpvpaper;
pub mod noctalia;
pub mod noctalia_runtime;
pub mod shell;
pub mod wallpaper_authority;

use crate::engine::Engine;
use crate::theme_manager::ThemeManager;
use shell::ShellDetector;

/// Register every provider shipped with HVE, in the canonical order.
///
/// Shell detection picks the Noctalia provider that matches the running shell
/// (v5 when active, v4 otherwise — the v4 fallback preserves the historical
/// startup behaviour). Both the main window's manager and the gallery's
/// manager call this, so they always expose the same provider list.
pub fn register_default_providers(tm: &mut ThemeManager, engine: &Engine) {
    let noctalia_v5 = shell::NoctaliaV5Paths;
    let noctalia_v4 = shell::NoctaliaV4Paths;

    if noctalia_v5.is_active() {
        tm.register_provider(Box::new(noctalia::NoctaliaV5Provider::new()));
        tracing::info!("[shell] Noctalia v5 detectado — registrando provider v5");
    } else if noctalia_v4.is_active() {
        tm.register_provider(Box::new(noctalia::NoctaliaV4Provider::new()));
        tracing::info!("[shell] Noctalia v4 detectado — registrando provider v4");
    } else {
        tm.register_provider(Box::new(noctalia::NoctaliaV4Provider::new()));
        tracing::warn!("[shell] Noctalia no detectado — registrando provider v4 por defecto");
    }
    tm.register_provider(Box::new(hve_presets::HvePresetsProvider::new(engine.clone())));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempEnv;
    use std::path::Path;

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
