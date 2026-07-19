use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Current config version.
/// Bump this when making backward-incompatible changes and add a migration step.
pub const CONFIG_VERSION: u32 = 4;

// ── Panel config types ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum UiMode {
    #[default]
    Tray,
    Panel,
    Both,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PanelEdge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Default for PanelEdge {
    fn default() -> Self {
        Self::Right
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PanelWidth {
    pub collapsed: u32,
    pub expanded: u32,
}

impl Default for PanelWidth {
    fn default() -> Self {
        Self {
            collapsed: 48,
            expanded: 300,
        }
    }
}

fn default_config_version() -> u32 {
    0 // pre-versioning configs are treated as v0 and migrated forward
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default = "Config::default")]
pub struct Config {
    #[serde(default = "default_config_version")]
    pub config_version: u32,
    pub is_system_active: bool,
    pub border_size: i32,
    pub active_anim_file: String,
    pub active_border_file: String,
    pub active_shader_file: String,
    pub auto_start: bool,
    pub auto_minimize_enabled: bool,
    pub minimize_seconds: i32,
    pub language: String,
    pub tiling_mode: bool,
    pub theme: String,
    pub keybinds_enabled: bool,
    #[serde(default)]
    pub ui_mode: UiMode,
    #[serde(default)]
    pub panel_edge: PanelEdge,
    #[serde(default)]
    pub panel_width: PanelWidth,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            is_system_active: false,
            border_size: 2,
            active_anim_file: String::new(),
            active_border_file: String::new(),
            active_shader_file: String::new(),
            auto_start: false,
            auto_minimize_enabled: true,
            minimize_seconds: 5,
            language: String::new(),
            tiling_mode: false,
            theme: "system".to_string(),
            keybinds_enabled: false,
            ui_mode: UiMode::Tray,
            panel_edge: PanelEdge::Right,
            panel_width: PanelWidth::default(),
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

    // v3 → v4: add panel config fields
    if cfg.config_version < 4 {
        cfg.ui_mode = UiMode::Tray;
        cfg.panel_edge = PanelEdge::Right;
        cfg.panel_width = PanelWidth::default();
        cfg.config_version = 4;
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
                tracing::info!("[config] Loaded config: version={}, theme={:?}, tiling={}, keybinds={}, active={}",
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
            "[config] Saved config: version={}, theme={:?}, tiling={}, keybinds={}, active={}",
            self.config_version, self.theme, self.tiling_mode, self.keybinds_enabled, self.is_system_active
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests that manipulate the global `HOME` env var so they
    /// do not race with each other when running in parallel.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard: sets `HOME` to a temporary directory for the duration of
    /// the test, then restores the original value and cleans up the temp dir.
    struct TempEnv {
        old_home: Option<String>,
        tmp: std::path::PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TempEnv {
        fn new() -> Self {
            let guard = ENV_LOCK.lock().unwrap();
            let tmp = std::env::temp_dir().join(format!("hve_cfg_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp).unwrap();
            let old_home = std::env::var("HOME").ok();
            std::env::set_var("HOME", &tmp);
            Self {
                old_home,
                tmp,
                _guard: guard,
            }
        }
    }

    impl Drop for TempEnv {
        fn drop(&mut self) {
            match &self.old_home {
                Some(h) => std::env::set_var("HOME", h),
                None => std::env::remove_var("HOME"),
            }
            let _ = std::fs::remove_dir_all(&self.tmp);
        }
    }

    // ── Config::default() ────────────────────────────────────────────

    #[test]
    fn test_default_config_values() {
        let cfg = Config::default();
        assert!(!cfg.is_system_active, "system_active should default to false");
        assert_eq!(cfg.border_size, 2, "border_size should default to 2");
        assert!(!cfg.auto_start, "auto_start should default to false");
        assert!(cfg.auto_minimize_enabled, "auto_minimize_enabled should default to true");
        assert_eq!(cfg.minimize_seconds, 5, "minimize_seconds should default to 5");
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
        assert!(matches!(cfg.ui_mode, UiMode::Tray));
        assert!(matches!(cfg.panel_edge, PanelEdge::Right));
        assert_eq!(cfg.panel_width.collapsed, 48);
        assert_eq!(cfg.panel_width.expanded, 300);
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
            ui_mode: UiMode::Panel,
            panel_edge: PanelEdge::Left,
            panel_width: PanelWidth { collapsed: 60, expanded: 320 },
        };

        cfg.save().expect("save should succeed");

        let loaded = Config::load();
        assert!(loaded.is_system_active);
        assert_eq!(loaded.border_size, 6);
        assert!(loaded.auto_start);
        assert!(!loaded.auto_minimize_enabled);
        assert_eq!(loaded.minimize_seconds, 10);
        assert_eq!(loaded.language, "es");
        assert!(loaded.tiling_mode);
        assert_eq!(loaded.theme, "light");
        assert!(loaded.keybinds_enabled);
        assert!(matches!(loaded.ui_mode, UiMode::Panel), "panel roundtrip: ui_mode");
        assert!(matches!(loaded.panel_edge, PanelEdge::Left), "panel roundtrip: panel_edge");
        assert_eq!(loaded.panel_width.collapsed, 60, "panel roundtrip: collapsed");
        assert_eq!(loaded.panel_width.expanded, 320, "panel roundtrip: expanded");
        assert_eq!(loaded.active_anim_file, "glow.json");
        assert_eq!(loaded.active_border_file, "sharp.json");
        assert_eq!(loaded.active_shader_file, "rgb.json");
        assert_eq!(loaded.config_version, CONFIG_VERSION);
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

    // ── migration from v3 → v4 (panel fields) ────────────────────────

    #[test]
    fn test_migration_from_v3_to_v4_adds_panel_fields() {
        let _env = TempEnv::new();

        let path = Config::config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");

        // A v3 config exists without panel fields
        let v3_json = r#"{
            "config_version": 3,
            "is_system_active": true,
            "border_size": 2,
            "active_anim_file": "",
            "active_border_file": "",
            "active_shader_file": "",
            "auto_start": false,
            "auto_minimize_enabled": true,
            "minimize_seconds": 5,
            "language": "",
            "tiling_mode": false,
            "theme": "system",
            "keybinds_enabled": false
        }"#;
        std::fs::write(&path, v3_json).expect("write v3 config");

        let cfg = Config::load();
        assert_eq!(
            cfg.config_version, CONFIG_VERSION,
            "v3 config should be migrated to v{CONFIG_VERSION}"
        );
        // Check panel defaults after migration
        assert!(matches!(cfg.ui_mode, UiMode::Tray));
        assert!(matches!(cfg.panel_edge, PanelEdge::Right));
        assert_eq!(cfg.panel_width.collapsed, 48);
        assert_eq!(cfg.panel_width.expanded, 300);
        // v3 fields preserved
        assert!(cfg.is_system_active);
    }

    #[test]
    fn test_v4_config_roundtrip_preserves_panel_fields() {
        let _env = TempEnv::new();

        let cfg = Config {
            config_version: CONFIG_VERSION,
            is_system_active: false,
            border_size: 2,
            active_anim_file: String::new(),
            active_border_file: String::new(),
            active_shader_file: String::new(),
            auto_start: false,
            auto_minimize_enabled: true,
            minimize_seconds: 5,
            language: String::new(),
            tiling_mode: false,
            theme: "system".to_string(),
            keybinds_enabled: false,
            ui_mode: UiMode::Panel,
            panel_edge: PanelEdge::Left,
            panel_width: PanelWidth { collapsed: 64, expanded: 350 },
        };

        cfg.save().expect("save should succeed");
        let loaded = Config::load();
        assert!(matches!(loaded.ui_mode, UiMode::Panel));
        assert!(matches!(loaded.panel_edge, PanelEdge::Left));
        assert_eq!(loaded.panel_width.collapsed, 64);
        assert_eq!(loaded.panel_width.expanded, 350);
        assert_eq!(loaded.config_version, CONFIG_VERSION);
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
}
