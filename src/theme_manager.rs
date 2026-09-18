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
    }
}

/// A provider knows how to capture and restore one slice of desktop state.
///
/// Each provider is identified by a stable `id` (e.g. `"noctalia"`,
/// `"hve-presets"`) and stores its data in a subdirectory
/// `{theme_dir}/providers/{id}/`.
pub trait ThemeProvider: Send + Sync {
    fn id(&self) -> &str;
    // The four methods below are exercised only by the test suite (noctalia.rs
    // tests); production code uses `id`, `save`, `apply` and `post_apply`.
    // The allows are required: rustc's dead_code lint does not count
    // `#[cfg(test)]` usage during a plain `cargo check`.
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

/// Provider ids that have been removed from the product itself.
///
/// A theme saved before a removal still lists the retired id in its
/// `meta.json`. `ThemeManager::list()` hides every theme whose providers are
/// not all registered — correct for a genuine capability mismatch (e.g.
/// `mpvpaper` unavailable on this machine), wrong for a product removal like
/// `hyprland-settings`. `migrate_removed_provider_ids` strips exactly these
/// ids so a product removal is never mistaken for an unavailable provider.
pub const REMOVED_PROVIDER_IDS: &[&str] = &["hyprland-settings"];

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
    pub providers: Vec<String>,
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
        let themes_dir = config_dir.join("hve").join("themes");
        // Migration lives in the constructor on purpose: every code path that
        // can observe themes goes through `ThemeManager` (`list()` is a method
        // on it), so running here guarantees the stale metas are fixed before
        // any observer reads them. `main.rs` builds a second manager for the
        // gallery; running the migration once per instance is harmless because
        // it rewrites a `meta.json` only when a removed id is present.
        Self::migrate_removed_provider_ids(&themes_dir);
        Self {
            themes_dir,
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

    // ─── Helpers ─────────────────────────────────────────────────────

    /// Strip retired provider ids from every `meta.json` under `themes_dir`.
    ///
    /// Idempotent: a `meta.json` is rewritten only when it actually lists a
    /// removed id, so callers can run it unconditionally.
    ///
    /// The rewrite operates on the raw `serde_json::Value` instead of the
    /// typed `ThemeMeta` so any field this code does not know about — written
    /// by a newer HVE or by hand — survives the round-trip untouched. Only the
    /// removed ids are dropped from `providers`; `saved_at`, `description` and
    /// the relative order of the remaining ids are preserved because the value
    /// is edited in place.
    ///
    /// The matching directory filter mirrors `list()`: names starting with `.`
    /// (hidden) or `_` (backup) are not themes and are left alone.
    ///
    /// Writes are atomic (temp file + `rename` in the same directory) because
    /// this runs automatically at startup over every user meta: a crash or a
    /// full disk must never truncate a `meta.json`.
    fn migrate_removed_provider_ids(themes_dir: &Path) {
        let Ok(entries) = fs::read_dir(themes_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Same filter as `list()`: hidden / backup dirs are not themes.
            if name.starts_with('.') || name.starts_with('_') {
                continue;
            }
            if !entry.path().is_dir() {
                continue;
            }
            let meta_path = entry.path().join("meta.json");
            let Ok(raw) = fs::read_to_string(&meta_path) else {
                continue;
            };
            let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let Some(providers) = value.get_mut("providers").and_then(|p| p.as_array_mut())
            else {
                continue;
            };
            let before = providers.len();
            providers.retain(|id| {
                !id.as_str()
                    .map(|s| REMOVED_PROVIDER_IDS.contains(&s))
                    .unwrap_or(false)
            });
            if providers.len() == before {
                continue;
            }
            let Ok(json) = serde_json::to_string_pretty(&value) else {
                continue;
            };
            match Self::write_atomic(&meta_path, &json) {
                Ok(()) => tracing::info!(
                    "[themes] Migrated '{name}': removed retired provider id(s) from meta.json"
                ),
                Err(e) => {
                    tracing::warn!("[themes] Cannot migrate '{name}/meta.json': {e}")
                }
            }
        }
    }

    /// Atomically replace `path` with `contents`.
    ///
    /// Writes a sibling temp file and `rename`s it over the target, so a
    /// concurrent reader either sees the old bytes or the new bytes — never a
    /// truncated file. `rename` is atomic only within one filesystem, which is
    /// why the temp file lives in the target's own directory.
    fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "meta.json".to_string());
        let tmp_path = dir.join(format!(".{file_name}.tmp-{}", std::process::id()));
        fs::write(&tmp_path, contents)?;
        match fs::rename(&tmp_path, path) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Best-effort cleanup; the original file is still intact.
                let _ = fs::remove_file(&tmp_path);
                Err(e)
            }
        }
    }

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
            themes.push(ThemeInfo {
                is_active: name == self.last_applied,
                saved_at: meta.as_ref().map(|m| m.saved_at.clone()).unwrap_or_default(),
                name,
                providers,
            });
        }

        // Filter: only show themes whose providers are all currently registered.
        // This ensures shell-specific themes (e.g. noctalia v4, v5, or future shells)
        // are hidden when their shell is not active. Shell-agnostic themes
        // (e.g. presets-only) always show because their providers are always registered.
        let registered = self.provider_ids();
        themes.retain(|t| {
            t.providers.iter().all(|pid| registered.contains(pid))
        });

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Minimal provider used to control which ids `list()` sees as registered.
    struct StubProvider {
        id: &'static str,
    }

    impl ThemeProvider for StubProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn display_name_key(&self) -> &str {
            self.id
        }
        fn icon(&self) -> &str {
            self.id
        }
        fn save(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn apply(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::empty()
        }
    }

    /// Seed `{themes_dir}/{name}/meta.json` and return its path.
    fn seed_meta(
        themes_dir: &Path,
        name: &str,
        saved_at: &str,
        description: &str,
        ids: &[&str],
    ) -> PathBuf {
        let theme_dir = themes_dir.join(name);
        fs::create_dir_all(&theme_dir).unwrap();
        let meta = ThemeMeta {
            saved_at: saved_at.to_string(),
            description: description.to_string(),
            providers: ids.iter().map(|s| s.to_string()).collect(),
        };
        let meta_path = theme_dir.join("meta.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap()).unwrap();
        meta_path
    }

    /// Regression: a theme saved before the `hyprland-settings` provider was
    /// removed from the product must reappear in the gallery. The migration
    /// strips the retired id, so the `list()` capability filter no longer
    /// mistakes a product removal for an unavailable provider.
    #[test]
    fn migration_lists_a_theme_that_lists_a_removed_provider() {
        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        let meta_path = seed_meta(
            &themes_dir,
            "RedTest",
            "2026-09-06T22:24:00.000Z",
            "keeper theme",
            &["noctalia-v5", "hve-presets", "hyprland-settings"],
        );

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(StubProvider { id: "noctalia-v5" }));
        tm.register_provider(Box::new(StubProvider { id: "hve-presets" }));

        let themes = tm.list().unwrap();
        let theme = themes
            .iter()
            .find(|t| t.name == "RedTest")
            .expect("a theme listing a removed provider id must still be listed");
        assert_eq!(theme.saved_at, "2026-09-06T22:24:00.000Z");
        assert_eq!(theme.providers, vec!["noctalia-v5", "hve-presets"]);

        // The on-disk meta was migrated, preserving saved_at/description and
        // the relative order of the remaining ids.
        let meta: ThemeMeta =
            serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(meta.saved_at, "2026-09-06T22:24:00.000Z");
        assert_eq!(meta.description, "keeper theme");
        assert_eq!(meta.providers, vec!["noctalia-v5", "hve-presets"]);
    }

    /// Invariant: the `list()` filter still protects against a genuinely
    /// unavailable provider. The migration must not touch such a meta.
    #[test]
    fn theme_with_unavailable_provider_outside_the_removed_list_stays_hidden() {
        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        let meta_path = seed_meta(
            &themes_dir,
            "WallpaperOnly",
            "2026-09-06T22:24:00.000Z",
            "",
            &["mpvpaper"],
        );
        let before = fs::read_to_string(&meta_path).unwrap();
        let mtime_before = fs::metadata(&meta_path).unwrap().modified().unwrap();

        let mut tm = ThemeManager::new(config_dir.path());
        tm.register_provider(Box::new(StubProvider { id: "hve-presets" }));

        assert!(
            tm.list().unwrap().is_empty(),
            "a theme for an unavailable provider must stay hidden"
        );
        assert_eq!(
            fs::read_to_string(&meta_path).unwrap(),
            before,
            "the migration must not rewrite a meta without a removed id"
        );
        // Regression guard: skipping the rewrite must mean *no write at all*.
        // A rewrite that happens to produce identical bytes would still churn
        // the mtime and is exactly what this catches.
        assert_eq!(
            fs::metadata(&meta_path).unwrap().modified().unwrap(),
            mtime_before,
            "the migration must not touch a meta without a removed id (mtime churn)"
        );
    }

    /// Hardening: the migration runs at every startup over every user meta, so
    /// a field written by a newer HVE (or by hand) must not be lost. The old
    /// typed `ThemeMeta` round-trip silently dropped unknown keys.
    #[test]
    fn migration_preserves_unknown_meta_fields() {
        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        let theme_dir = themes_dir.join("Future");
        fs::create_dir_all(&theme_dir).unwrap();
        let meta_path = theme_dir.join("meta.json");
        fs::write(
            &meta_path,
            r#"{
                "saved_at": "2026-09-06T22:24:00.000Z",
                "description": "hand edited",
                "providers": ["hve-presets", "hyprland-settings", "noctalia-v5"],
                "future_field": { "nested": [1, 2, 3] }
            }"#,
        )
        .unwrap();

        let _tm = ThemeManager::new(config_dir.path());

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(
            value["future_field"]["nested"],
            serde_json::json!([1, 2, 3]),
            "an unknown field must survive the migration intact"
        );
        assert_eq!(value["saved_at"], "2026-09-06T22:24:00.000Z");
        assert_eq!(value["description"], "hand edited");
        assert_eq!(
            value["providers"],
            serde_json::json!(["hve-presets", "noctalia-v5"])
        );
    }

    /// Hardening: the migration must scan exactly what `list()` scans. Hidden
    /// (`.`-prefixed) and backup (`_`-prefixed) directories are not themes, so
    /// even one listing a removed id must be left byte-identical.
    #[test]
    fn migration_ignores_hidden_and_backup_directories() {
        let config_dir = TempDir::new().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        for name in [".Hidden", "_Backup"] {
            let meta_path = seed_meta(
                &themes_dir,
                name,
                "2026-09-06T22:24:00.000Z",
                "",
                &["hyprland-settings"],
            );
            let before = fs::read_to_string(&meta_path).unwrap();

            let _tm = ThemeManager::new(config_dir.path());

            assert_eq!(
                fs::read_to_string(&meta_path).unwrap(),
                before,
                "'{name}' must not be migrated: it is not a listed theme"
            );
        }
    }
}
