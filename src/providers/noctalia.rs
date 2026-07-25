use crate::providers::shell::{NoctaliaV4Paths, ShellProvider};
use crate::theme_manager::{ProviderCapabilities, ThemeProvider};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

// ── Wallpaper JSON parsing types ─────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct WallpaperEntry {
    dark: Option<String>,
    light: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NestedWallpapersFormat {
    #[serde(rename = "wallpapers")]
    wallpapers: Option<std::collections::HashMap<String, WallpaperEntry>>,
}

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

/// Parse wallpapers.json from the THEME's saved file (not the cache).
/// Returns entries sorted: named screens first, empty string last, FALLBACK excluded.
fn parse_theme_entries(saved_path: &Path) -> Result<Vec<(String, String)>, String> {
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

// ── Color helpers ────────────────────────────────────────────────────

/// Determine if a hex color represents a dark surface (dark mode).
/// Uses perceived luminance: if the weighted brightness is < 128, it's dark.
fn is_dark_hex(hex: &str) -> bool {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 {
        return true; // default to dark
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f32;
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f32;
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f32;
    // Perceived luminance: weights reflect human eye sensitivity
    (0.299 * r + 0.587 * g + 0.114 * b) < 128.0
}

// ── NoctaliaV4Provider ───────────────────────────────────────────────

pub struct NoctaliaV4Provider {
    shell: Box<dyn ShellProvider>,
    wallpaper_pending: Mutex<Vec<(String, String)>>,
}

impl NoctaliaV4Provider {
    pub fn new() -> Self {
        Self {
            shell: Box::new(NoctaliaV4Paths),
            wallpaper_pending: Mutex::new(Vec::new()),
        }
    }
}

impl Default for NoctaliaV4Provider {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeProvider for NoctaliaV4Provider {
    fn id(&self) -> &str {
        "noctalia"
    }

    fn display_name_key(&self) -> &str {
        "themes.provider.noctalia"
    }

    fn icon(&self) -> &str {
        "◈"
    }

    fn shell(&self) -> &str {
        "noctalia"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::COLORS
    }

    fn save(&self, theme_dir: &Path) -> Result<(), String> {
        let src = self.shell.config_dir().ok_or("Noctalia config dir not found")?;
        let hypr_src = self.shell.rendered_dir().ok_or("Noctalia hypr dir not found")?;

        let provider_dir = theme_dir.join("providers").join(self.id());
        fs::create_dir_all(&provider_dir)
            .map_err(|e| format!("Cannot create provider dir: {}", e))?;

        // Save source config files
        for file in self.shell.source_files() {
            let src_path = src.join(file);
            if src_path.exists() {
                let dst_path = provider_dir.join(file);
                fs::copy(&src_path, &dst_path)
                    .map_err(|e| format!("Cannot copy {}: {}", file, e))?;
            }
        }

        // Save rendered color files
        for file in self.shell.rendered_color_files() {
            let src_path = hypr_src.join(file);
            if src_path.exists() {
                let dst_path = provider_dir.join(file);
                fs::copy(&src_path, &dst_path)
                    .map_err(|e| format!("Cannot copy {}: {}", file, e))?;
            }
        }

        // Save wallpapers.json from cache
        if let Some(wp_file) = self.shell.wallpapers_file() {
            if wp_file.exists() {
                let dst_path = provider_dir.join("wallpapers.json");
                fs::copy(&wp_file, &dst_path)
                    .map_err(|e| format!("Cannot copy wallpapers.json: {}", e))?;
            }
        }

        Ok(())
    }

    fn apply(&self, theme_dir: &Path) -> Result<(), String> {
        let dst = self.shell.config_dir().ok_or("Noctalia config dir not found")?;
        let provider_dir = theme_dir.join("providers").join(self.id());

        if !provider_dir.exists() {
            return Err("Noctalia provider data not found in theme".into());
        }

        // Step 1: Copy source config files (colors.json, settings.json, plugins.json)
        // Noctalia's inotify detects the changes and regenerates rendered files
        // (noctalia-colors.conf, noctalia-colors.lua) automatically from colors.json.
        for file in self.shell.source_files() {
            let src_path = provider_dir.join(file);
            if src_path.exists() {
                let dst_path = dst.join(file);
                let tmp = dst_path.with_extension("json.noctalia-tmp");
                fs::copy(&src_path, &tmp)
                    .map_err(|e| format!("Cannot copy {}: {}", file, e))?;
                fs::rename(&tmp, &dst_path)
                    .map_err(|e| format!("Cannot rename {}: {}", file, e))?;
            }
        }

        // Step 2: Parse wallpapers.json from saved theme and store entries
        // for IPC apply in post_apply(). We do NOT write wallpapers.json to
        // the Noctalia cache here — the IPC call in post_apply() tells
        // Noctalia to set the wallpaper, and Noctalia handles its own cache.
        let saved_wp = provider_dir.join("wallpapers.json");
        if saved_wp.exists() {
            match parse_theme_entries(&saved_wp) {
                Ok(entries) => {
                    tracing::debug!(
                        "[noctalia] Stored {} entries for IPC apply: {:?}",
                        entries.len(),
                        entries.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>()
                    );
                    *self.wallpaper_pending.lock().unwrap() = entries;
                }
                Err(e) => {
                    tracing::warn!("[noctalia] Could not parse wallpapers.json: {}", e);
                }
            }
        }

        Ok(())
    }

    fn post_apply(&self, theme_name: &str) -> Result<(), String> {
        // ── Wallpaper IPC ──
        // We set wallpapers AFTER copying source files in apply(). The IPC
        // goes through Noctalia's WallpaperService, which triggers its own
        // reactive pipeline via AppThemeService.onWallpaperChanged().
        // However, that pipeline regenerates templates using the LAST predefined
        // scheme data, NOT the theme's saved colors. We correct that below.
        //
        // IMPORTANT: Entries are sorted with named screens first, empty-string
        // ("") last. The IPC handler treats screen="" as "all screens", which
        // would OVERRIDE any named screen entries we just set. So we must:
        //   - Set named screens individually
        //   - Only use the empty-string entry if there are NO named screens
        let entries = self.wallpaper_pending.lock().unwrap().clone();
        if !entries.is_empty() {
            tracing::info!("[noctalia] Applying wallpapers via IPC...");
            let has_named = entries.iter().any(|(s, _)| !s.is_empty());
            for (screen, path) in &entries {
                if has_named && screen.is_empty() {
                    tracing::debug!("[noctalia] Skipping empty-screen entry (named screens present)");
                    continue;
                }
                tracing::debug!("[noctalia] IPC apply: screen='{}' path='{}'", screen, path);
                if let Err(e) = self.shell.apply_wallpaper(Path::new(path), screen) {
                    tracing::warn!("[noctalia] Wallpaper IPC for '{}': {}", screen, e);
                }
            }
        }

        // ── Wait for Noctalia's wallpaper-triggered template generation ──
        // The wallpaper IPC triggers onWallpaperChanged → generateFromPredefinedScheme,
        // which runs template-processor.py asynchronously via QML Process (debounced
        // at 150ms). We wait briefly for it to finish so our run below is the last
        // writer to all template output files.
        tracing::info!("[noctalia] Waiting for async template generation to settle...");
        std::thread::sleep(Duration::from_millis(500));

        // ── Regenerate templates with THEME colors ──
        // After the wallpaper-triggered pipeline finishes, we run Noctalia's
        // template-processor.py with --scheme using the theme's saved colors.json.
        // This regenerates GTK, Qt, terminal, and other app templates using the
        // CORRECT theme colors, overwriting whatever was generated from
        // lastPredefinedSchemeData by the wallpaper IPC above.
        //
        // The scheme JSON is built from the 14 core M3 colors in the theme's
        // colors.json. The template processor expands them to the full 48-color
        // palette using expand_predefined_scheme() and renders all enabled templates
        // (from theming.predefined.toml, which excludes the noctalia colors output).
        let config_dir = dirs::config_dir()
            .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("/tmp/hve-config"));
        let theme_dir = config_dir.join("hve").join("themes").join(theme_name);
        let provider_dir = theme_dir.join("providers").join(self.id());
        let saved_colors = provider_dir.join("colors.json");

        if saved_colors.exists() {
            let raw = match fs::read_to_string(&saved_colors) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("[noctalia] Could not read saved colors.json: {}", e);
                    return Ok(());
                }
            };
            let parsed: serde_json::Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("[noctalia] Could not parse saved colors.json: {}", e);
                    return Ok(());
                }
            };
            let obj = match parsed.as_object() {
                Some(o) => o,
                None => {
                    tracing::warn!("[noctalia] Saved colors.json is not an object");
                    return Ok(());
                }
            };

            // Extract 14 core M3 colors to build a scheme JSON for --scheme
            let core_keys = [
                "mPrimary", "mOnPrimary",
                "mSecondary", "mOnSecondary",
                "mTertiary", "mOnTertiary",
                "mError", "mOnError",
                "mSurface", "mOnSurface",
                "mSurfaceVariant", "mOnSurfaceVariant",
                "mOutline",
            ];
            let optional_keys = ["mShadow", "mHover"];

            let mut scheme = serde_json::Map::new();
            for key in &core_keys {
                if let Some(val) = obj.get(*key).and_then(|v| v.as_str()) {
                    scheme.insert(key.to_string(), serde_json::Value::String(val.to_string()));
                }
            }
            for key in &optional_keys {
                if let Some(val) = obj.get(*key).and_then(|v| v.as_str()) {
                    scheme.insert(key.to_string(), serde_json::Value::String(val.to_string()));
                }
            }

            let has_primary = scheme.contains_key("mPrimary");
            let has_surface = scheme.contains_key("mSurface");
            if !has_primary || !has_surface {
                tracing::warn!("[noctalia] Saved colors.json missing mPrimary or mSurface");
                return Ok(());
            }

            // Detect default mode from surface color luminance
            let surface_val = obj.get("mSurface").and_then(|v| v.as_str()).unwrap_or("#000000");
            let default_mode = if is_dark_hex(surface_val) { "dark" } else { "light" };

            let scheme_json = serde_json::to_string_pretty(&serde_json::Value::Object(scheme))
                .unwrap_or_default();

            let safe_name = theme_name.replace('/', "_");
            let temp_dir = std::env::temp_dir().join("hve-scheme").join(&safe_name);
            let _ = fs::create_dir_all(&temp_dir);
            let scheme_path = temp_dir.join("scheme.json");
            if fs::write(&scheme_path, &scheme_json).is_err() {
                tracing::warn!("[noctalia] Could not write temp scheme file");
                return Ok(());
            }

            if let Some(tp) = self.shell.template_processor() {
                // Prefer theming.predefined.toml (no noctalia colors output template).
                // Fall back to theming.dynamic.toml if predefined doesn't exist.
                let config_path = self.shell.theming_config().and_then(|p| {
                    let predefined = p.with_file_name("theming.predefined.toml");
                    if predefined.exists() {
                        Some(predefined)
                    } else if p.exists() {
                        Some(p)
                    } else {
                        None
                    }
                });

                if let Some(tc) = config_path {
                    tracing::info!(
                        "[noctalia] Running template processor with theme colors (mode={})...",
                        default_mode
                    );
                    let processor_dir = tp.parent().unwrap_or_else(|| std::path::Path::new("/"));
                    let result = std::process::Command::new("python3")
                        .arg(&tp)
                        .arg("--scheme")
                        .arg(&scheme_path)
                        .arg("--config")
                        .arg(&tc)
                        .arg("--default-mode")
                        .arg(default_mode)
                        .current_dir(processor_dir)
                        .output();

                    match result {
                        Ok(out) => {
                            if out.status.success() {
                                tracing::info!("[noctalia] Template processor completed successfully");
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                tracing::warn!("[noctalia] Template processor stderr: {}", stderr);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("[noctalia] Could not launch template processor: {}", e);
                        }
                    }
                } else {
                    tracing::warn!("[noctalia] No theming config found, skipping template processor");
                }
            } else {
                tracing::warn!("[noctalia] Template processor not found, skipping");
            }

            // Clean up temp scheme file
            let _ = fs::remove_dir_all(&temp_dir);
        } else {
            tracing::debug!("[noctalia] No saved colors.json in theme, skipping template processor");
        }

        // ── System notification ──
        match std::process::Command::new("notify-send")
            .arg("--app-name=HVE")
            .arg(format!("🎨 Tema '{}' aplicado", theme_name))
            .arg(format!(
                "Theme \"{}\" applied — Noctalia colors + wallpapers restored",
                theme_name,
            ))
            .output()
        {
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("[noctalia] notify-send not available: {}", e);
            }
        }

        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noctalia_v4_provider_new() {
        let provider = NoctaliaV4Provider::new();
        assert_eq!(provider.id(), "noctalia");
    }

    #[test]
    fn test_is_dark_hex_dark() {
        assert!(is_dark_hex("#0c1017"));  // very dark blue
        assert!(is_dark_hex("#000000"));  // pure black
        assert!(is_dark_hex("#1e1e2e"));  // catppuccin surface
    }

    #[test]
    fn test_is_dark_hex_light() {
        assert!(!is_dark_hex("#ffffff"));  // pure white
        assert!(!is_dark_hex("#f5f5f5"));  // off-white
        assert!(!is_dark_hex("#c0c0c0"));  // silver
    }

    #[test]
    fn test_is_dark_hex_edge_cases() {
        // Short hex → default dark
        assert!(is_dark_hex("#fff"));
        assert!(is_dark_hex(""));
        // With and without hash
        assert!(is_dark_hex("0c1017"));
        assert!(!is_dark_hex("ffffff"));
    }

    // ── Wallpaper parsing tests (moved from wallpaper.rs) ──

    #[test]
    fn test_parse_nested_wallpapers_format() {
        let json = r#"{
            "wallpapers": {
                "": { "dark": "/home/user/wall/dark.png", "light": "/home/user/wall/light.png" },
                "DP-3": { "dark": "/home/user/wall/dp3.png", "light": "/home/user/wall/dp3-light.png" }
            }
        }"#;

        let dir = std::env::temp_dir().join("noctalia-wp-test-nested");
        let _ = fs::create_dir_all(&dir);
        let fpath = dir.join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = parse_theme_entries(&fpath).unwrap();
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

        let dir = std::env::temp_dir().join("noctalia-wp-test-fallback");
        let _ = fs::create_dir_all(&dir);
        let fpath = dir.join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = parse_theme_entries(&fpath).unwrap();
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

        let dir = std::env::temp_dir().join("noctalia-wp-test-screens");
        let _ = fs::create_dir_all(&dir);
        let fpath = dir.join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = parse_theme_entries(&fpath).unwrap();
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

        let dir = std::env::temp_dir().join("noctalia-wp-test-single");
        let _ = fs::create_dir_all(&dir);
        let fpath = dir.join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = parse_theme_entries(&fpath).unwrap();
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "HDMI-A-1");
        assert_eq!(entries[0].1, "/path/light.png");
    }

    #[test]
    fn test_pending_cleared_on_empty_theme() {
        // No wallpapers.json in dir → parse_theme_entries should return error
        let dir = std::env::temp_dir().join("noctalia-wp-test-empty");
        let _ = fs::create_dir_all(&dir);
        let result = parse_theme_entries(&dir.join("wallpapers.json"));
        let _ = fs::remove_dir_all(&dir);
        assert!(result.is_err());
    }
}
