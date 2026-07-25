use crate::providers::noctalia::ShellPaths;
use crate::theme_manager::ThemeProvider;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Wallpaper provider state after parsing wallpapers.json.
#[derive(Debug, Clone, Deserialize)]
struct WallpaperEntry {
    dark: Option<String>,
    light: Option<String>,
}

/// Internal representation of the saved "nested object" format.
#[derive(Debug, Deserialize)]
struct NestedWallpapersFormat {
    #[serde(rename = "wallpapers")]
    wallpapers: Option<std::collections::HashMap<String, WallpaperEntry>>,
}

/// Internal representation of the "screens array" format.
#[derive(Debug, Deserialize)]
struct ScreenWallpaperFormat {
    #[serde(rename = "screens")]
    screens: Option<Vec<ScreenEntry>>,
}

#[derive(Debug, Deserialize)]
struct ScreenEntry {
    name: Option<String>,
    #[serde(default)]
    dark: Option<String>,
    #[serde(default)]
    light: Option<String>,
}

/// WallpaperProvider saves/restore wallpaper assignments independently from NoctaliaProvider.
///
/// Wallpaper data lives in `~/.cache/noctalia/wallpapers.json` (the canonical location
/// that Noctalia v4 reads/writes). This provider copies that file into and out of
/// theme directories so wallpaper assignments travel with themes.
///
/// IPC ORDER (critical):
///   1. Named screens FIRST (DP-3, HDMI-A-1, etc.)
///   2. Empty/default screen "" LAST
///   FALLBACK is NEVER sent via IPC — the file copy to cache is sufficient.
///
/// The pending field stores entries parsed from the THEME's saved file during apply(),
/// so post_apply() uses the CORRECT data regardless of what Noctalia's inotify may
/// have written to the cache file in between.
pub struct WallpaperProvider {
    shell: Box<dyn ShellPaths>,
    pending: Mutex<Vec<(String, String)>>,
}

impl WallpaperProvider {
    pub fn new(shell: Box<dyn ShellPaths>) -> Self {
        Self { shell, pending: Mutex::new(Vec::new()) }
    }

    /// Parse wallpapers.json from the THEME's saved file (not the cache).
    /// Returns entries sorted: named screens first, empty string last, FALLBACK excluded.
    fn parse_theme_entries(&self, saved_path: &Path) -> Result<Vec<(String, String)>, String> {
        let raw = fs::read_to_string(saved_path)
            .map_err(|e| format!("Cannot read saved wallpapers.json: {}", e))?;

        let mut entries: Vec<(String, String)> = Vec::new();

        // Try "screens" array format first (Noctalia plugin format)
        if let Ok(screen_format) = serde_json::from_str::<ScreenWallpaperFormat>(&raw) {
            if let Some(screens) = screen_format.screens {
                for s in screens {
                    let name = s.name.unwrap_or_default();
                    if let Some(path) = s.light.or(s.dark) {
                        entries.push((name, path));
                    }
                }
            }
        }

        // Try nested "wallpapers" object format (cache format)
        if entries.is_empty() {
            if let Ok(nested_format) = serde_json::from_str::<NestedWallpapersFormat>(&raw) {
                if let Some(wallpapers) = nested_format.wallpapers {
                    for (name, entry) in wallpapers {
                        // Skip FALLBACK — setting it via IPC overrides all named screens
                        if name == "FALLBACK" || name == "usedRandomWallpapers" {
                            continue;
                        }
                        if let Some(path) = entry.light.or(entry.dark) {
                            entries.push((name, path));
                        }
                    }
                }
            }
        }

        if entries.is_empty() {
            return Err("No wallpaper entries found in saved file".into());
        }

        // Sort: named screens first, empty/default screen last
        // This prevents the default screen IPC from overriding named screens
        entries.sort_by(|a, b| {
            let a_is_empty = a.0.is_empty();
            let b_is_empty = b.0.is_empty();
            if a_is_empty && !b_is_empty {
                std::cmp::Ordering::Greater
            } else if !a_is_empty && b_is_empty {
                std::cmp::Ordering::Less
            } else {
                a.0.cmp(&b.0)
            }
        });

        Ok(entries)
    }
}

impl ThemeProvider for WallpaperProvider {
    fn id(&self) -> &str {
        "wallpaper"
    }

    fn display_name_key(&self) -> &str {
        "themes.provider.wallpaper"
    }

    fn icon(&self) -> &str {
        "🖼"
    }

    fn shell(&self) -> &str {
        "system"
    }

    fn capabilities(&self) -> crate::theme_manager::ProviderCapabilities {
        crate::theme_manager::ProviderCapabilities::WALLPAPERS
    }

    fn save(&self, theme_dir: &Path) -> Result<(), String> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        fs::create_dir_all(&provider_dir)
            .map_err(|e| format!("Cannot create provider dir: {}", e))?;

        let home = std::env::var("HOME").unwrap_or_default();
        let wallpaper_src = PathBuf::from(home)
            .join(".cache").join("noctalia").join("wallpapers.json");

        if wallpaper_src.exists() {
            fs::copy(&wallpaper_src, provider_dir.join("wallpapers.json"))
                .map_err(|e| format!("Cannot copy wallpapers.json: {}", e))?;
        }
        Ok(())
    }

    fn apply(&self, theme_dir: &Path) -> Result<(), String> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        let saved_file = provider_dir.join("wallpapers.json");

        if !saved_file.exists() {
            return Ok(()); // Old theme without wallpaper data
        }

        // Step 1: Write to cache file so Noctalia's inotify can read it
        let home = std::env::var("HOME").unwrap_or_default();
        let wallpaper_dst = PathBuf::from(home)
            .join(".cache").join("noctalia").join("wallpapers.json");

        if let Some(parent) = wallpaper_dst.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let tmp = wallpaper_dst.with_extension("json.wallpaper-tmp");
        fs::copy(&saved_file, &tmp)
            .map_err(|e| format!("Cannot copy wallpapers.json: {}", e))?;
        fs::rename(&tmp, &wallpaper_dst)
            .map_err(|e| format!("Cannot rename wallpapers.json: {}", e))?;

        // Step 2: Parse entries from the SAVED file and store them for post_apply.
        // We read from the saved file (not the cache) because Noctalia's inotify
        // may have already modified the cache by the time we reach post_apply().
        match self.parse_theme_entries(&saved_file) {
            Ok(entries) => {
                tracing::debug!(
                    "[wallpaper] Stored {} entries for IPC apply: {:?}",
                    entries.len(),
                    entries.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>()
                );
                *self.pending.lock().unwrap() = entries;
            }
            Err(e) => {
                tracing::warn!("[wallpaper] Could not parse saved wallpapers.json: {}", e);
            }
        }

        Ok(())
    }

    fn post_apply(&self, _theme_name: &str) -> Result<(), String> {
        let entries = self.pending.lock().unwrap();
        if entries.is_empty() {
            return Ok(());
        }

        // Apply IPC with deterministic order (already sorted by parse_theme_entries):
        // named screens first, empty/default screen last.
        for (screen, path) in entries.iter() {
            tracing::debug!(
                "[wallpaper] IPC apply: screen='{}' path='{}'",
                screen,
                path
            );
            if let Err(e) = self.shell.apply_wallpaper(Path::new(path), screen) {
                tracing::warn!(
                    "[wallpaper] IPC apply for screen '{}': {}",
                    screen,
                    e
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::noctalia::NoctaliaPaths;

    #[test]
    fn test_wallpaper_provider_new() {
        let provider = WallpaperProvider::new(Box::new(NoctaliaPaths));
        assert_eq!(provider.id(), "wallpaper");
    }

    #[test]
    fn test_parse_nested_wallpapers_format() {
        let json = r#"{
            "wallpapers": {
                "": { "dark": "/home/user/wall/dark.png", "light": "/home/user/wall/light.png" },
                "DP-3": { "dark": "/home/user/wall/dp3.png", "light": "/home/user/wall/dp3-light.png" }
            }
        }"#;

        let p = WallpaperProvider::new(Box::new(NoctaliaPaths));
        let dir = std::env::temp_dir().join("wallpaper-test-nested");
        let _ = fs::create_dir_all(&dir);
        let fpath = dir.join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = p.parse_theme_entries(&fpath).unwrap();
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(entries.len(), 2);
        // DP-3 should be first (non-empty name)
        assert_eq!(entries[0].0, "DP-3");
        assert_eq!(entries[0].1, "/home/user/wall/dp3-light.png");
        // empty string should be last
        assert_eq!(entries[1].0, "");
        assert_eq!(entries[1].1, "/home/user/wall/light.png");
    }

    #[test]
    fn test_parse_skips_fallback() {
        let json = r#"{
            "defaultWallpaper": "/etc/default.png",
            "wallpapers": {
                "": { "dark": "/home/user/wall/dark.png", "light": "/home/user/wall/light.png" },
                "DP-3": { "dark": "/home/user/wall/dp3.png", "light": "/home/user/wall/dp3-light.png" },
                "FALLBACK": { "dark": "/home/user/wall/fallback.png", "light": "/home/user/wall/fallback.png" }
            }
        }"#;

        let p = WallpaperProvider::new(Box::new(NoctaliaPaths));
        let dir = std::env::temp_dir().join("wallpaper-test-fallback");
        let _ = fs::create_dir_all(&dir);
        let fpath = dir.join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = p.parse_theme_entries(&fpath).unwrap();
        let _ = fs::remove_dir_all(&dir);

        // Should have 2 entries: DP-3 and "", NOT FALLBACK
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "DP-3");
        assert_eq!(entries[1].0, "");
    }

    #[test]
    fn test_parse_screens_array_format() {
        let json = r#"{
            "screens": [
                { "name": "", "light": "/home/user/wall/screen0.png", "dark": "/home/user/wall/screen0-dark.png" },
                { "name": "DP-3", "light": "/home/user/wall/screen1.png", "dark": "/home/user/wall/screen1-dark.png" }
            ]
        }"#;

        let p = WallpaperProvider::new(Box::new(NoctaliaPaths));
        let dir = std::env::temp_dir().join("wallpaper-test-screens");
        let _ = fs::create_dir_all(&dir);
        let fpath = dir.join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = p.parse_theme_entries(&fpath).unwrap();
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "DP-3");
        assert_eq!(entries[0].1, "/home/user/wall/screen1.png");
        assert_eq!(entries[1].0, "");
        assert_eq!(entries[1].1, "/home/user/wall/screen0.png");
    }

    #[test]
    fn test_parse_screens_object_format() {
        let json = r#"{
            "screens": [
                { "name": "HDMI-A-1", "dark": "/path/dark.png", "light": "/path/light.png" }
            ]
        }"#;

        let p = WallpaperProvider::new(Box::new(NoctaliaPaths));
        let dir = std::env::temp_dir().join("wallpaper-test-single");
        let _ = fs::create_dir_all(&dir);
        let fpath = dir.join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = p.parse_theme_entries(&fpath).unwrap();
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "HDMI-A-1");
        assert_eq!(entries[0].1, "/path/light.png");
    }

    #[test]
    fn test_pending_cleared_on_empty_theme() {
        let p = WallpaperProvider::new(Box::new(NoctaliaPaths));
        // No saved file in a temp dir → apply() returns Ok(()) without parsing
        let dir = std::env::temp_dir().join("wallpaper-test-empty");
        let _ = fs::create_dir_all(&dir);
        let result = p.apply(&dir);
        let _ = fs::remove_dir_all(&dir);
        assert!(result.is_ok());
        // pending should be empty because no file was found
        assert!(p.pending.lock().unwrap().is_empty());
    }
}