use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ProviderCapabilities: u32 {
        const WALLPAPERS   = 0b0000_0001;
        const COLORS       = 0b0000_0010;
        const ANIMATIONS   = 0b0000_0100;
        const BORDERS      = 0b0000_1000;
        const SHADERS      = 0b0001_0000;
        const KEYBINDS     = 0b0010_0000;
        const AUTOSTART    = 0b0100_0000;
        const WINDOW_RULES = 0b1000_0000;
    }
}

/// A provider knows how to capture and restore one slice of desktop state.
///
/// Each provider is identified by a stable `id` (e.g. `"noctalia"`,
/// `"hve-presets"`) and stores its data in a subdirectory
/// `{theme_dir}/providers/{id}/`.
pub trait ThemeProvider: Send + Sync {
    fn id(&self) -> &str;
    #[allow(dead_code)]
    fn display_name_key(&self) -> &str;
    #[allow(dead_code)]
    fn icon(&self) -> &str;

    #[allow(dead_code)]
    /// Return the desktop shell this provider targets (e.g. "hyprland", "noctalia").
    fn shell(&self) -> &str {
        "unknown"
    }

    /// Capabilities this provider manages.
    #[allow(dead_code)]
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::empty()
    }

    /// Capture current state into `{theme_dir}/providers/{id}/`.
    fn save(&self, theme_dir: &Path) -> Result<(), String>;

    /// Restore state from `{theme_dir}/providers/{id}/`.
    fn apply(&self, theme_dir: &Path) -> Result<(), String>;

    /// Optional hook called after all providers have applied successfully.
    ///
    /// Each provider defines post-apply actions specific to its shell:
    /// - Noctalia v4: runs the template-processor to regenerate GTK/QT/terminal themes
    /// - Noctalia v5: sends IPC to the daemon
    /// - Other shells: their own refresh mechanism
    ///
    /// Default implementation is no-op. Errors are logged but do NOT fail the apply.
    fn post_apply(&self, theme_name: &str) -> Result<(), String> {
        let _ = theme_name;
        Ok(())
    }
}

/// Metadata persisted inside each theme directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeMeta {
    pub saved_at: String,
    pub description: String,
    pub providers: Vec<String>,
}

/// Info about a saved theme, used by the Slint UI.
#[derive(Debug, Clone)]
pub struct ThemeInfo {
    pub name: String,
    pub saved_at: String,
    pub is_active: bool,
    #[allow(dead_code)]
    pub providers: Vec<String>,
    pub has_shell: bool,
    pub has_compositor: bool,
    pub has_presets: bool,
    pub has_wallpaper: bool,
}

/// Manages themes: list, save, apply, delete, rename.
pub struct ThemeManager {
    themes_dir: PathBuf,
    /// Ordered list of registered providers.
    providers: Vec<Box<dyn ThemeProvider>>,
    /// Last applied theme name (empty = none).
    pub last_applied: String,
}

impl ThemeManager {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            themes_dir: config_dir.join("hve").join("themes"),
            providers: Vec::new(),
            last_applied: String::new(),
        }
    }

    pub fn register_provider(&mut self, provider: Box<dyn ThemeProvider>) {
        self.providers.push(provider);
    }

    /// Returns the IDs of all registered providers.
    pub fn provider_ids(&self) -> Vec<String> {
        self.providers.iter().map(|p| p.id().to_string()).collect()
    }

    #[allow(dead_code)]
    pub fn providers(&self) -> &[Box<dyn ThemeProvider>] {
        &self.providers
    }

    #[allow(dead_code)]
    pub fn themes_dir(&self) -> &Path {
        &self.themes_dir
    }

    #[allow(dead_code)]
    fn provider_dir(theme_dir: &Path, provider_id: &str) -> PathBuf {
        theme_dir.join("providers").join(provider_id)
    }

    // ─── Helpers ─────────────────────────────────────────────────────

    fn timestamp() -> String {
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs();
        let millis = dur.subsec_millis();
        // Compute YMD HMS from epoch seconds (no chrono dep)
        let (y, mo, d, h, mi, s) = Self::epoch_to_utc(secs);
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{millis:03}Z")
    }

    /// Convert epoch seconds to UTC date/time components.
    fn epoch_to_utc(epoch: u64) -> (u64, u64, u64, u64, u64, u64) {
        // Days since epoch
        let days = epoch / 86400;
        let time_secs = epoch % 86400;
        let h = time_secs / 3600;
        let mi = (time_secs % 3600) / 60;
        let s = time_secs % 60;

        // Civil date from days since 1970-01-01 (Rata Die algorithm)
        let z = days + 719468;
        let era = z / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mo = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if mo <= 2 { y + 1 } else { y };
        (y, mo, d, h, mi, s)
    }

    fn validate_name(name: &str) -> Result<String, String> {
        let t = name.trim();
        if t.is_empty() {
            return Err("name-empty".into());
        }
        if t.len() > 64 {
            return Err("name-too-long".into());
        }
        if t.contains('/') || t.contains('\\') || t.contains('\0') {
            return Err("name-invalid".into());
        }
        Ok(t.to_string())
    }

    // ─── Public API ──────────────────────────────────────────────────

    pub fn list(&self) -> Result<Vec<ThemeInfo>, String> {
        let dir = &self.themes_dir;
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries: Vec<_> = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read themes dir: {}", e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter(|e| {
                let name = e.file_name();
                let s = name.to_string_lossy();
                !s.starts_with('.') && !s.starts_with('_')
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());

        let mut themes = Vec::new();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta_path = entry.path().join("meta.json");
            let meta = fs::read_to_string(&meta_path)
                .ok()
                .and_then(|s| serde_json::from_str::<ThemeMeta>(&s).ok());
            let providers = meta.as_ref().map(|m| m.providers.clone()).unwrap_or_default();
            let has_shell = providers.iter().any(|p| p == "noctalia");
            let has_compositor = providers.iter().any(|p| p == "hyprland-settings");
            let has_presets = providers.iter().any(|p| p == "hve-presets");
            let has_wallpaper = providers.iter().any(|p| p == "wallpaper");
            themes.push(ThemeInfo {
                is_active: name == self.last_applied,
                saved_at: meta.as_ref().map(|m| m.saved_at.clone()).unwrap_or_default(),
                name,
                providers,
                has_shell,
                has_compositor,
                has_presets,
                has_wallpaper,
            });
        }

        Ok(themes)
    }

    pub fn save(&mut self, name: &str, provider_ids: &[String]) -> Result<(), String> {
        let name = Self::validate_name(name)?;
        let theme_dir = self.themes_dir.join(&name);

        // Create directory structure
        fs::create_dir_all(&theme_dir)
            .map_err(|e| format!("Cannot create theme dir: {}", e))?;

        // Run each registered provider if selected
        let mut enabled = Vec::new();
        for p in &self.providers {
            if provider_ids.is_empty() || provider_ids.iter().any(|id| id == p.id()) {
                p.save(&theme_dir)?;
                enabled.push(p.id().to_string());
            }
        }

        // Write meta.json
        let meta = ThemeMeta {
            saved_at: Self::timestamp(),
            description: String::new(),
            providers: enabled,
        };
        let meta_json = serde_json::to_string_pretty(&meta)
            .map_err(|e| format!("Meta serialization error: {}", e))?;
        fs::write(theme_dir.join("meta.json"), &meta_json)
            .map_err(|e| format!("Cannot write meta.json: {}", e))?;

        Ok(())
    }

    pub fn apply(
        &mut self,
        name: &str,
        reload_after_post_apply: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let name = name.trim().to_string();
        let theme_dir = self.themes_dir.join(&name);

        if !theme_dir.exists() {
            return Err(format!("Theme '{}' not found", name));
        }

        // Read meta to know which providers were saved
        let meta_path = theme_dir.join("meta.json");
        let meta: ThemeMeta = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(ThemeMeta {
                saved_at: String::new(),
                description: String::new(),
                providers: self.providers.iter().map(|p| p.id().to_string()).collect(),
            });

        // ── Pass 1: All providers write files to disk ──
        for p in &self.providers {
            if meta.providers.iter().any(|id| id == p.id()) {
                p.apply(&theme_dir)?;
            }
        }

        // ── Pass 2: All providers run post-apply hooks (template processors, etc.) ──
        for p in &self.providers {
            if meta.providers.iter().any(|id| id == p.id()) {
                if let Err(e) = p.post_apply(&name) {
                    tracing::warn!(
                        "[themes] Provider '{}' post_apply warning: {}",
                        p.id(),
                        e
                    );
                }
            }
        }

        // ── Final reload AFTER all post_apply (including wallpaper IPC) ──
        if let Err(e) = reload_after_post_apply() {
            tracing::warn!("[themes] reload_after_post_apply warning: {e}");
        }

        self.last_applied = name;
        Ok(())
    }

    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        let theme_dir = self.themes_dir.join(name);
        if !theme_dir.exists() {
            return Err(format!("Theme '{}' not found", name));
        }
        fs::remove_dir_all(&theme_dir)
            .map_err(|e| format!("Failed to delete theme '{}': {}", name, e))?;
        if self.last_applied == name {
            self.last_applied.clear();
        }
        Ok(())
    }

    pub fn rename(&mut self, old_name: &str, new_name: &str) -> Result<(), String> {
        let old_name = old_name.trim();
        let new_name = Self::validate_name(new_name)?;
        if old_name == new_name {
            return Ok(());
        }
        let old_path = self.themes_dir.join(old_name);
        let new_path = self.themes_dir.join(&new_name);
        if !old_path.exists() {
            return Err(format!("Theme '{}' not found", old_name));
        }
        if new_path.exists() {
            return Err("name-exists".into());
        }
        fs::rename(&old_path, &new_path)
            .map_err(|e| format!("Failed to rename theme: {}", e))?;
        if self.last_applied == old_name {
            self.last_applied = new_name;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn name_for_index(&self, index: i32) -> Option<String> {
        let themes = self.list().ok()?;
        themes.into_iter().nth(index as usize).map(|t| t.name)
    }
}
