use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Current config version.
/// Bump this when making backward-incompatible changes and add a migration step.
pub const CONFIG_VERSION: u32 = 7;

fn default_config_version() -> u32 {
    0 // pre-versioning configs are treated as v0 and migrated forward
}

fn default_border_radius() -> i32 {
    32
}

fn default_gaps() -> i32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default = "Config::default")]
pub struct Config {
    #[serde(default = "default_config_version")]
    pub config_version: u32,
    pub is_system_active: bool,
    pub border_size: i32,
    #[serde(default = "default_border_radius")]
    pub border_radius: i32,
    #[serde(default = "default_gaps")]
    pub gaps_in: i32,
    #[serde(default = "default_gaps")]
    pub gaps_out: i32,
    pub active_anim_file: String,
    pub active_border_file: String,
    pub active_shader_file: String,
    pub auto_start: bool,
    pub auto_minimize_enabled: bool,
    pub minimize_seconds: i32,
    pub language: String,
    pub tiling_mode: bool,
    pub theme: String,
    pub last_applied_theme: String,
    pub keybinds_enabled: bool,
    #[serde(default)]
    pub disabled_providers: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            is_system_active: false,
            border_size: 2,
            border_radius: 32,
            gaps_in: 5,
            gaps_out: 5,
            active_anim_file: String::new(),
            active_border_file: String::new(),
            active_shader_file: String::new(),
            auto_start: false,
            auto_minimize_enabled: true,
            minimize_seconds: 4,
            language: String::new(),
            tiling_mode: false,
            theme: "system".to_string(),
            last_applied_theme: String::new(),
            keybinds_enabled: false,
            disabled_providers: Vec::new(),
        }
    }
}

/// Migrate a config from an older version to the current version.
/// Each step handles one version increment so the chain is explicit.
fn migrate(mut cfg: Config) -> Config {
    // v0 → v1: pre-versioning configs. No structural changes needed,
    // just mark them as current.
    if cfg.config_version < 1 {
        cfg.config_version = 1;
    }

    // v1 → v2: add settings panel fields (auto_minimize, language, tiling_mode, theme)
    if cfg.config_version < 2 {
        cfg.auto_minimize_enabled = true;
        cfg.minimize_seconds = 5;
        cfg.language = String::new();
        cfg.tiling_mode = false;
        cfg.theme = "system".to_string();
        cfg.config_version = 2;
    }

    // v2 → v3: add keybinds toggle
    if cfg.config_version < 3 {
        cfg.keybinds_enabled = false;
        cfg.config_version = 3;
    }

    // v3 → v4: add last_applied_theme field
    if cfg.config_version < 4 {
        cfg.last_applied_theme = String::new();
        cfg.config_version = 4;
    }

    // v4 → v5: add disabled_providers field
    if cfg.config_version < 5 {
        cfg.disabled_providers = Vec::new();
        cfg.config_version = 5;
    }

    // v5 → v6: add border_radius field
    if cfg.config_version < 6 {
        cfg.border_radius = 32;
        cfg.config_version = 6;
    }

    // v6 → v7: add gaps_in/gaps_out fields
    if cfg.config_version < 7 {
        cfg.gaps_in = 5;
        cfg.gaps_out = 5;
        cfg.config_version = 7;
    }

    cfg.config_version = CONFIG_VERSION;
    cfg
}

impl Config {
    pub fn config_path() -> PathBuf {
        let config_dir = dirs::config_dir()
            .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("/tmp/hve-config"));
        config_dir.join("hve").join("config.json")
    }

    /// Load config from disk, falling back to defaults if anything goes wrong.
    ///
    /// Warnings are printed to stderr so the user knows their config was ignored
    /// rather than silently losing settings.
    pub fn load() -> Self {
        let path = Self::config_path();

        if !path.exists() {
            return Self::default();
        }

        let data = match fs::read_to_string(&path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!(
                    "[hve] Warning: could not read config at {}: {}. Using defaults.",
                    path.display(),
                    e
                );
                return Self::default();
            }
        };

        match serde_json::from_str::<Config>(&data) {
            Ok(cfg) => {
                tracing::debug!("[config] Loaded config: version={}, theme={:?}, tiling={}, keybinds={}, active={}",
                    cfg.config_version, cfg.theme, cfg.tiling_mode, cfg.keybinds_enabled, cfg.is_system_active);
                if cfg.config_version < CONFIG_VERSION {
                    eprintln!(
                        "[hve] Info: migrating config from v{} to v{}.",
                        cfg.config_version, CONFIG_VERSION
                    );
                    let migrated = migrate(cfg);
                    tracing::info!("[config] Migrated config: version={}, theme={:?}, tiling={}, keybinds={}",
                        migrated.config_version, migrated.theme, migrated.tiling_mode, migrated.keybinds_enabled);
                    migrated
                } else {
                    cfg
                }
            }
            Err(e) => {
                eprintln!(
                    "[hve] Warning: failed to parse config at {}: {}. Using defaults.",
                    path.display(),
                    e
                );
                tracing::warn!("[config] Parse error: {} — falling back to defaults", e);
                Self::default()
            }
        }
    }

    /// Save config to disk.
    ///
    /// Returns a descriptive error string so callers can surface the issue
    /// to users instead of silently swallowing failures.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Failed to create config directory '{}': {}",
                    parent.display(),
                    e
                )
            })?;
        }
        let data = serde_json::to_string_pretty(self).map_err(|e| {
            format!(
                "Failed to serialize config to '{}': {}",
                path.display(),
                e
            )
        })?;
        fs::write(&path, data).map_err(|e| {
            format!("Failed to write config to '{}': {}", path.display(), e)
        })?;
        tracing::info!(
            "[config] Saved config: version={}, theme={:?}, tiling={}, keybinds={}, active={}, disabled_providers={:?}",
            self.config_version, self.theme, self.tiling_mode, self.keybinds_enabled, self.is_system_active, self.disabled_providers
        );
        Ok(())
    }
}

/// HVE cache directory: ~/.cache/hve/
pub fn hve_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".cache")
        })
        .join("hve")
}

/// Path to the HVE settings file: always `~/.cache/hve/hve-settings.lua`.
/// This file controls HVE's window rules and keyboard shortcuts.
///
/// The legacy `hve-windowrules.lua` is renamed to `hve-settings.lua` on first
/// call if it exists and the new name does not. The old `conf` variants are
/// intentionally left orphaned: settings are regenerable and conf bytes must
/// never be copied into the Lua file.
pub fn hve_settings_path() -> PathBuf {
    let new_path = hve_cache_dir().join("hve-settings.lua");
    let old_path = hve_cache_dir().join("hve-windowrules.lua");

    // Migrate the legacy Lua name to the current one if needed.
    if old_path.exists() && !new_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&old_path) {
            if std::fs::write(&new_path, &content).is_ok() {
                let _ = std::fs::remove_file(&old_path);
                tracing::info!(
                    "[settings] Migrated {} → {}",
                    old_path.display(),
                    new_path.display()
                );
            }
        }
    }

    new_path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempEnv;

    // ── Config::default() ────────────────────────────────────────────

    #[test]
    fn test_default_config_values() {
        let cfg = Config::default();
        assert!(!cfg.is_system_active, "system_active should default to false");
        assert_eq!(cfg.border_size, 2, "border_size should default to 2");
        assert_eq!(cfg.border_radius, 32, "border_radius should default to 32");
        assert_eq!(cfg.gaps_in, 5, "gaps_in should default to 5");
        assert_eq!(cfg.gaps_out, 5, "gaps_out should default to 5");
        assert!(!cfg.auto_start, "auto_start should default to false");
        assert!(cfg.auto_minimize_enabled, "auto_minimize_enabled should default to true");
        assert_eq!(cfg.minimize_seconds, 4, "minimize_seconds should default to 4");
        assert_eq!(cfg.theme, "system", "theme should default to system");
        assert!(!cfg.tiling_mode, "tiling_mode should default to false");
        assert!(
            !cfg.keybinds_enabled,
            "keybinds_enabled should default to false"
        );
        assert!(cfg.language.is_empty(), "language should default to empty (auto-detect)");
        assert_eq!(
            cfg.config_version, CONFIG_VERSION,
            "config_version should match CONFIG_VERSION"
        );
        assert!(cfg.active_anim_file.is_empty());
        assert!(cfg.active_border_file.is_empty());
        assert!(cfg.active_shader_file.is_empty());
    }

    // ── config_path() ────────────────────────────────────────────────

    #[test]
    fn test_config_path_ends_correctly() {
        let _env = TempEnv::new();
        let path = Config::config_path();
        let s = path.to_string_lossy();
        assert!(s.contains(".config"), "path should contain .config: {s}");
        assert!(s.ends_with("hve/config.json"), "path should end with hve/config.json: {s}");
    }

    // ── save + load roundtrip ────────────────────────────────────────

    #[test]
    fn test_save_load_roundtrip() {
        let _env = TempEnv::new();

        let cfg = Config {
            is_system_active: true,
            border_size: 6,
            border_radius: 48,
            gaps_in: 7,
            gaps_out: 9,
            auto_start: true,
            active_anim_file: "glow.json".into(),
            active_border_file: "sharp.json".into(),
            active_shader_file: "rgb.json".into(),
            config_version: CONFIG_VERSION,
            auto_minimize_enabled: false,
            minimize_seconds: 10,
            language: "es".into(),
            tiling_mode: true,
            theme: "light".into(),
            keybinds_enabled: true,
            last_applied_theme: String::new(),
            disabled_providers: vec!["noctalia".into()],
        };

        cfg.save().expect("save should succeed");

        let loaded = Config::load();
        assert!(loaded.is_system_active);
        assert_eq!(loaded.border_size, 6);
        assert_eq!(loaded.border_radius, 48);
        assert_eq!(loaded.gaps_in, 7);
        assert_eq!(loaded.gaps_out, 9);
        assert!(loaded.auto_start);
        assert!(!loaded.auto_minimize_enabled);
        assert_eq!(loaded.minimize_seconds, 10);
        assert_eq!(loaded.language, "es");
        assert!(loaded.tiling_mode);
        assert_eq!(loaded.theme, "light");
        assert!(loaded.keybinds_enabled);
        assert_eq!(loaded.active_anim_file, "glow.json");
        assert_eq!(loaded.active_border_file, "sharp.json");
        assert_eq!(loaded.active_shader_file, "rgb.json");
        assert_eq!(loaded.config_version, CONFIG_VERSION);
        assert_eq!(loaded.disabled_providers, vec!["noctalia"]);
    }

    // ── corrupt JSON → defaults (no panic) ───────────────────────────

    #[test]
    fn test_corrupt_json_returns_defaults() {
        let _env = TempEnv::new();

        let path = Config::config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");
        std::fs::write(&path, "this is not valid json").expect("write junk");

        let cfg = Config::load();
        assert!(!cfg.is_system_active, "corrupt JSON should yield defaults");
        assert_eq!(cfg.border_size, 2);
    }

    // ── migration from v0 (no config_version) → v1 ───────────────────

    #[test]
    fn test_migration_from_v0_to_v1() {
        let _env = TempEnv::new();

        let path = Config::config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");

        // A v0 config has no config_version field and no auto_start field
        // (auto_start was added later as a feature).
        let v0_json = r#"{
            "is_system_active": true,
            "border_size": 3,
            "active_anim_file": "",
            "active_border_file": "",
            "active_shader_file": ""
        }"#;
        std::fs::write(&path, v0_json).expect("write v0 config");

        let cfg = Config::load();
        assert_eq!(
            cfg.config_version, CONFIG_VERSION,
            "v0 config should be migrated to v{CONFIG_VERSION}"
        );
        assert!(cfg.is_system_active, "v0 field should be preserved");
        assert_eq!(cfg.border_size, 3, "v0 field should be preserved");
    }

    // ── migration from v2 → v3 (keybinds_enabled) ────────────────────

    #[test]
    fn test_migration_from_v2_to_v3_adds_keybinds_enabled() {
        let _env = TempEnv::new();

        let path = Config::config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");

        // A v2 config exists without keybinds_enabled
        let v2_json = r#"{
            "config_version": 2,
            "is_system_active": true,
            "border_size": 3,
            "active_anim_file": "",
            "active_border_file": "",
            "active_shader_file": "",
            "auto_start": false,
            "auto_minimize_enabled": true,
            "minimize_seconds": 5,
            "language": "",
            "tiling_mode": false,
            "theme": "system"
        }"#;
        std::fs::write(&path, v2_json).expect("write v2 config");

        let cfg = Config::load();
        assert_eq!(
            cfg.config_version, CONFIG_VERSION,
            "v2 config should be migrated to v{CONFIG_VERSION}"
        );
        assert!(cfg.is_system_active, "v2 field should be preserved");
        assert_eq!(
            cfg.keybinds_enabled, false,
            "keybinds_enabled should default to false after migration"
        );
    }

    // ── deny_unknown_fields catches unexpected fields ─────────────────

    #[test]
    fn test_deny_unknown_fields_returns_defaults() {
        let _env = TempEnv::new();

        let path = Config::config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");

        let bad_json = r#"{
            "is_system_active": true,
            "border_size": 2,
            "active_anim_file": "",
            "active_border_file": "",
            "active_shader_file": "",
            "auto_start": false,
            "unknown_field": "should cause error"
        }"#;
        std::fs::write(&path, bad_json).expect("write bad config");

        let cfg = Config::load();
        // Because of #[serde(deny_unknown_fields)] the parse should fail
        // and load() should fall back to defaults.
        assert!(!cfg.is_system_active, "unknown field should cause fallback to defaults");
        assert_eq!(cfg.border_size, 2);
    }

    // ── backward compatibility: config without disabled_providers loads ──

    #[test]
    fn test_backward_compat_no_disabled_providers() {
        let _env = TempEnv::new();

        let path = Config::config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");

        // A config written before v5 — no disabled_providers field
        let v4_json = r#"{
            "config_version": 4,
            "is_system_active": true,
            "border_size": 3,
            "active_anim_file": "",
            "active_border_file": "",
            "active_shader_file": "",
            "auto_start": false,
            "auto_minimize_enabled": true,
            "minimize_seconds": 5,
            "language": "",
            "tiling_mode": false,
            "theme": "system",
            "last_applied_theme": ""
        }"#;
        std::fs::write(&path, v4_json).expect("write v4 config");

        let cfg = Config::load();
        assert_eq!(
            cfg.config_version, CONFIG_VERSION,
            "v4 config should be migrated to v{CONFIG_VERSION}"
        );
        assert!(cfg.disabled_providers.is_empty(), "disabled_providers should default to empty");
    }

    // ── migration from v4 → v5 (disabled_providers) ───────────────────

    #[test]
    fn test_migration_from_v4_to_v5_adds_disabled_providers() {
        let _env = TempEnv::new();

        let path = Config::config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");

        let v4_json = r#"{
            "config_version": 4,
            "is_system_active": true,
            "border_size": 3,
            "active_anim_file": "",
            "active_border_file": "",
            "active_shader_file": "",
            "auto_start": false,
            "auto_minimize_enabled": true,
            "minimize_seconds": 5,
            "language": "",
            "tiling_mode": false,
            "theme": "system",
            "last_applied_theme": "",
            "keybinds_enabled": false
        }"#;
        std::fs::write(&path, v4_json).expect("write v4 config");

        let cfg = Config::load();
        assert_eq!(
            cfg.config_version, CONFIG_VERSION,
            "v4 config should be migrated to v{CONFIG_VERSION}"
        );
        assert!(cfg.disabled_providers.is_empty(), "disabled_providers should default to empty after migration");
    }

    // ── migration from v5 → v6 (border_radius) ───────────────────────

    #[test]
    fn test_migration_from_v5_to_v6_adds_border_radius() {
        let _env = TempEnv::new();

        let path = Config::config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");

        let v5_json = r#"{
            "config_version": 5,
            "is_system_active": true,
            "border_size": 3,
            "active_anim_file": "",
            "active_border_file": "",
            "active_shader_file": "",
            "auto_start": false,
            "auto_minimize_enabled": true,
            "minimize_seconds": 5,
            "language": "",
            "tiling_mode": false,
            "theme": "system",
            "last_applied_theme": "",
            "keybinds_enabled": false,
            "disabled_providers": []
        }"#;
        std::fs::write(&path, v5_json).expect("write v5 config");

        let cfg = Config::load();
        assert_eq!(
            cfg.config_version, CONFIG_VERSION,
            "v5 config should be migrated to v{CONFIG_VERSION}"
        );
        assert_eq!(
            cfg.border_radius, 32,
            "border_radius should default to 32 after migration"
        );
    }

    // ── migration from v6 → v7 (gaps_in / gaps_out) ──────────────────

    #[test]
    fn test_migration_from_v6_to_v7_adds_gaps() {
        let _env = TempEnv::new();

        let path = Config::config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");

        let v6_json = r#"{
            "config_version": 6,
            "is_system_active": true,
            "border_size": 3,
            "border_radius": 20,
            "active_anim_file": "",
            "active_border_file": "",
            "active_shader_file": "",
            "auto_start": false,
            "auto_minimize_enabled": true,
            "minimize_seconds": 5,
            "language": "",
            "tiling_mode": false,
            "theme": "system",
            "last_applied_theme": "",
            "keybinds_enabled": false,
            "disabled_providers": []
        }"#;
        std::fs::write(&path, v6_json).expect("write v6 config");

        let cfg = Config::load();
        assert_eq!(
            cfg.config_version, CONFIG_VERSION,
            "v6 config should be migrated to v{CONFIG_VERSION}"
        );
        assert_eq!(
            cfg.border_radius, 20,
            "border_radius should be preserved after migration"
        );
        assert_eq!(cfg.gaps_in, 5, "gaps_in should default to 5 after migration");
        assert_eq!(cfg.gaps_out, 5, "gaps_out should default to 5 after migration");
    }

    // ── old config (pre-v6) loads with border_radius default ─────────

    #[test]
    fn test_old_config_without_border_radius_and_gaps_loads() {
        let _env = TempEnv::new();

        let path = Config::config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");

        // A v5 config serialized WITHOUT border_radius/gaps (they have serde defaults)
        let v5_json = r#"{
            "config_version": 5,
            "is_system_active": true,
            "border_size": 3,
            "active_anim_file": "",
            "active_border_file": "",
            "active_shader_file": "",
            "auto_start": false,
            "auto_minimize_enabled": true,
            "minimize_seconds": 5,
            "language": "",
            "tiling_mode": false,
            "theme": "system",
            "last_applied_theme": "",
            "keybinds_enabled": false,
            "disabled_providers": []
        }"#;
        std::fs::write(&path, v5_json).expect("write v5 config");

        let cfg = Config::load();
        // serde defaults fill the missing fields without failing the parse
        assert_eq!(cfg.border_radius, 32, "border_radius serde default");
        assert_eq!(cfg.gaps_in, 5, "gaps_in serde default");
        assert_eq!(cfg.gaps_out, 5, "gaps_out serde default");
        assert_eq!(cfg.border_size, 3, "existing field preserved");
    }
}
