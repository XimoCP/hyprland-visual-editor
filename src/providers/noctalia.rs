use crate::providers::shell::{NoctaliaV4Paths, ShellProvider};
use crate::theme_manager::{ProviderCapabilities, ThemeProvider};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Wait time for Noctalia inotify-driven async color extraction.
/// The extraction takes ~300-500ms; 600ms provides margin.
/// Post-wallpaper-IPC we always restore rendered files AGAIN, so
/// even if sleep is slightly short, the second restore wins.
const POST_APPLY_SLEEP_MS: u64 = 600;

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

// ── Path / theme helpers ─────────────────────────────────────────────

/// Resolve the theme directory from a theme name.
/// Theme dirs live at `~/.config/hve/themes/{name}/`.
fn theme_dir_for_post_apply(theme_name: &str) -> PathBuf {
    let config_dir = dirs::config_dir()
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from("/tmp/hve-config"));
    config_dir.join("hve").join("themes").join(theme_name)
}

/// Restore rendered color files (noctalia-colors.conf, noctalia-colors.lua)
/// from the saved theme into the shell's rendered directory.
///
/// If the theme is old and doesn't have rendered files, try to generate
/// them from colors.json as fallback.
fn restore_rendered_files(shell: &dyn ShellProvider, theme_name: &str) {
    let Some(hypr_dst) = shell.rendered_dir() else {
        return;
    };
    let provider_dir = theme_dir_for_post_apply(theme_name);
    if !provider_dir.exists() {
        return;
    }

    // Ensure rendered files exist in provider dir (generate from colors.json if old theme)
    let has_conf = provider_dir.join("noctalia-colors.conf").exists();
    let has_lua = provider_dir.join("noctalia-colors.lua").exists();
    if !has_conf || !has_lua {
        let _ = generate_rendered_from_colors_json(&provider_dir);
    }

    if let Err(e) = fs::create_dir_all(&hypr_dst) {
        tracing::warn!("[noctalia] Cannot create rendered dir: {}", e);
        return;
    }
    for file in shell.rendered_color_files() {
        let src_path = provider_dir.join(file);
        if src_path.exists() {
            let dst_path = hypr_dst.join(file);
            let tmp = hypr_dst.join(format!("{}.tmp", file));
            if let Err(e) = fs::copy(&src_path, &tmp) {
                tracing::warn!("[noctalia] Cannot copy {}: {}", file, e);
                continue;
            }
            if let Err(e) = fs::rename(&tmp, &dst_path) {
                tracing::warn!("[noctalia] Cannot rename {}: {}", file, e);
            }
            tracing::debug!("[noctalia] Restored {} → {}", file, dst_path.display());
        }
    }
}

/// Re-copy wallpapers.json from the saved theme back to the cache.
/// This ensures the final state on disk is OUR file, not anything
/// Noctalia may have modified during the post-apply async script.
fn re_copy_wallpapers(shell: &dyn ShellProvider, theme_name: &str) {
    let provider_dir = theme_dir_for_post_apply(theme_name);
    let saved_wp = provider_dir.join("wallpapers.json");
    if !saved_wp.exists() {
        return;
    }
    let Some(wp_cache) = shell.wallpapers_file() else {
        return;
    };
    if let Some(parent) = wp_cache.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = wp_cache.with_extension("json.wallpaper-tmp");
    if let Err(e) = fs::copy(&saved_wp, &tmp) {
        tracing::warn!("[noctalia] Cannot re-copy wallpapers.json: {}", e);
        return;
    }
    if let Err(e) = fs::rename(&tmp, &wp_cache) {
        tracing::warn!("[noctalia] Cannot rename wallpapers.json: {}", e);
    }
}

// ── Color helpers ────────────────────────────────────────────────────

/// Strip leading `#` from a hex color if present, returning the raw hex digits.
fn strip_hash(hex: &str) -> &str {
    hex.trim_start_matches('#')
}

/// Convert `#RRGGBB` to `rgb(RRGGBB)` format (no hash).
fn to_rgb_func(hex: &str) -> String {
    format!("rgb({})", strip_hash(hex))
}

/// Darken a hex color by reducing luminance (simple per-channel scaling).
fn darken_hex(hex: &str, factor: f32) -> String {
    let h = strip_hash(hex);
    if h.len() != 6 {
        return hex.to_string();
    }
    let r = (u8::from_str_radix(&h[0..2], 16).unwrap_or(0) as f32 * (1.0 - factor)) as u8;
    let g = (u8::from_str_radix(&h[2..4], 16).unwrap_or(0) as f32 * (1.0 - factor)) as u8;
    let b = (u8::from_str_radix(&h[4..6], 16).unwrap_or(0) as f32 * (1.0 - factor)) as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

/// Generate `noctalia-colors.conf` content from a colors.json-style value map.
fn generate_conf_content(colors: &serde_json::Map<String, serde_json::Value>) -> String {
    let primary = colors.get("mPrimary").and_then(|v| v.as_str()).unwrap_or("#cba6f7");
    let secondary = colors.get("mSecondary").and_then(|v| v.as_str()).unwrap_or("#89b4fa");
    let tertiary = colors.get("mTertiary").and_then(|v| v.as_str()).unwrap_or("#89b4fa");
    let surface = colors.get("mSurface").and_then(|v| v.as_str()).unwrap_or("#1e1e2e");
    let error = colors.get("mError").and_then(|v| v.as_str()).unwrap_or("#f7768e");
    // surface_lowest: prefer mShadow if it exists and is different from mSurface, else darken surface
    let shadow = colors.get("mShadow").and_then(|v| v.as_str());
    let surface_lowest = shadow
        .filter(|s| *s != surface)
        .map(|s| s.to_string())
        .unwrap_or_else(|| darken_hex(surface, 0.08));

    format!(
        concat!(
            "$primary = {}\n",
            "$surface = {}\n",
            "$secondary = {}\n",
            "$error = {}\n",
            "$tertiary = {}\n",
            "$surface_lowest = {}\n",
        ),
        to_rgb_func(primary),
        to_rgb_func(surface),
        to_rgb_func(secondary),
        to_rgb_func(error),
        to_rgb_func(tertiary),
        to_rgb_func(&surface_lowest),
    )
}

/// Generate `noctalia-colors.lua` content from a colors.json-style value map.
fn generate_lua_content(colors: &serde_json::Map<String, serde_json::Value>) -> String {
    let primary = colors.get("mPrimary").and_then(|v| v.as_str()).unwrap_or("#cba6f7");
    let secondary = colors.get("mSecondary").and_then(|v| v.as_str()).unwrap_or("#89b4fa");
    let tertiary = colors.get("mTertiary").and_then(|v| v.as_str()).unwrap_or("#89b4fa");
    let surface = colors.get("mSurface").and_then(|v| v.as_str()).unwrap_or("#1e1e2e");
    let error = colors.get("mError").and_then(|v| v.as_str()).unwrap_or("#f7768e");
    let shadow = colors.get("mShadow").and_then(|v| v.as_str());
    let surface_lowest = shadow
        .filter(|s| *s != surface)
        .map(|s| s.to_string())
        .unwrap_or_else(|| darken_hex(surface, 0.08));

    format!(
        concat!(
            "primary = \"{}\"\n",
            "surface = \"{}\"\n",
            "secondary = \"{}\"\n",
            "error = \"{}\"\n",
            "tertiary = \"{}\"\n",
            "surface_lowest = \"{}\"\n",
        ),
        to_rgb_func(primary),
        to_rgb_func(surface),
        to_rgb_func(secondary),
        to_rgb_func(error),
        to_rgb_func(tertiary),
        to_rgb_func(&surface_lowest),
    )
}

/// Fallback: read `colors.json` and generate the rendered files on the fly.
/// This handles themes saved before we started snapshotting rendered files.
fn generate_rendered_from_colors_json(provider_dir: &Path) -> Result<(), String> {
    let colors_path = provider_dir.join("colors.json");
    if !colors_path.exists() {
        return Err("No colors.json in theme to generate rendered files".into());
    }

    let raw = fs::read_to_string(&colors_path)
        .map_err(|e| format!("Cannot read colors.json: {}", e))?;
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Cannot parse colors.json: {}", e))?;

    let map = match parsed.as_object() {
        Some(m) => m,
        None => return Err("colors.json is not a JSON object".into()),
    };

    let conf = generate_conf_content(map);
    let lua = generate_lua_content(map);

    fs::write(provider_dir.join("noctalia-colors.conf"), &conf)
        .map_err(|e| format!("Cannot write noctalia-colors.conf: {}", e))?;
    fs::write(provider_dir.join("noctalia-colors.lua"), &lua)
        .map_err(|e| format!("Cannot write noctalia-colors.lua: {}", e))?;

    Ok(())
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

        // Step 1: Restore source config files (Noctalia app state)
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

        // Step 2: Restore wallpapers.json to cache + parse for IPC
        let saved_wp = provider_dir.join("wallpapers.json");
        if saved_wp.exists() {
            if let Some(wp_cache) = self.shell.wallpapers_file() {
                if let Some(parent) = wp_cache.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("Cannot create cache dir: {}", e))?;
                }
                let tmp = wp_cache.with_extension("json.wallpaper-tmp");
                fs::copy(&saved_wp, &tmp)
                    .map_err(|e| format!("Cannot copy wallpapers.json: {}", e))?;
                fs::rename(&tmp, &wp_cache)
                    .map_err(|e| format!("Cannot rename wallpapers.json: {}", e))?;
            }

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

        // NOTE: Rendered color files (noctalia-colors.conf, noctalia-colors.lua)
        // are intentionally NOT restored here. When we write wallpapers.json
        // above, Noctalia's inotify fires an async color extraction script
        // (~300-500ms). If we restored rendered files now, that async script
        // would overwrite them with its own calculated colors.
        //
        // Instead, rendered files are restored in post_apply() AFTER the
        // template processor runs and AFTER a short delay that lets the async
        // script finish. This ensures OUR colors are the final state on disk.

        Ok(())
    }

    fn post_apply(&self, theme_name: &str) -> Result<(), String> {
        // ── Step 1: Run Noctalia template processor ──
        // Regenerates GTK, QT, terminal, VSCode, Zen, btop, yazi themes from colors.json.
        let template_processor = self.shell.template_processor();
        let noctalia_colors = self.shell.config_dir().map(|d| d.join("colors.json"));
        let theming_config = self.shell.theming_config();

        if let (Some(tp), Some(colors), Some(tc)) =
            (template_processor.as_ref(), noctalia_colors.as_ref(), theming_config.as_ref())
        {
            if tp.exists() && colors.exists() && tc.exists() {
                tracing::info!("[noctalia] Running template processor to refresh system themes...");
                let processor_dir = tp.parent().unwrap_or_else(|| Path::new("/"));
                match std::process::Command::new("python3")
                    .arg(tp)
                    .arg(colors)
                    .arg("--scheme")
                    .arg(colors)
                    .arg("-c")
                    .arg(tc)
                    .current_dir(processor_dir)
                    .output()
                {
                    Ok(output) => {
                        if !output.status.success() {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            tracing::warn!("[noctalia] Template processor stderr: {}", stderr);
                        } else {
                            tracing::info!("[noctalia] Template processor completed successfully");
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[noctalia] Could not launch template processor: {}", e);
                    }
                }
            }
        }

        // ── Step 2: Wait for Noctalia's async color extraction to finish ──
        // When we wrote wallpapers.json in apply(), Noctalia's inotify triggered
        // an async color extraction script (~300-500ms).
        tracing::info!("[noctalia] Waiting for async color extraction to settle...");
        std::thread::sleep(Duration::from_millis(POST_APPLY_SLEEP_MS));

        // ── Step 3: First restore rendered color files ──
        tracing::info!("[noctalia] First restore of rendered color files...");
        restore_rendered_files(&*self.shell, theme_name);

        // ── Step 4: Wallpaper IPC ──
        let entries = self.wallpaper_pending.lock().unwrap().clone();
        if !entries.is_empty() {
            tracing::info!("[noctalia] Applying wallpapers via IPC...");
            for (screen, path) in &entries {
                tracing::debug!("[noctalia] IPC apply: screen='{}' path='{}'", screen, path);
                if let Err(e) = self.shell.apply_wallpaper(Path::new(path), screen) {
                    tracing::warn!("[noctalia] Wallpaper IPC for '{}': {}", screen, e);
                }
            }
        }

        // ── Step 5: Wait for wallpaper-triggered inotify ──
        // The wallpaper IPC triggers Noctalia's color extraction AGAIN.
        // We wait for it to finish before restoring our files.
        tracing::info!("[noctalia] Waiting for wallpaper-triggered color extraction...");
        std::thread::sleep(Duration::from_millis(POST_APPLY_SLEEP_MS));

        // ── Step 6: Second restore rendered color files (LAST WRITE WINS) ──
        tracing::info!("[noctalia] Second restore of rendered color files (last write wins)...");
        restore_rendered_files(&*self.shell, theme_name);

        // ── Step 7: Re-copy wallpapers.json from theme to cache ──
        // Override any modifications Noctalia may have written during extraction.
        re_copy_wallpapers(&*self.shell, theme_name);

        // ── Step 8: Reload command ──
        let cmd = self.shell.reload_command();
        if !cmd.is_empty() {
            tracing::info!("[noctalia] Running reload command: {:?}", cmd);
            let _ = std::process::Command::new(&cmd[0])
                .args(&cmd[1..])
                .output()
                .map_err(|e| tracing::warn!("[noctalia] Reload command failed: {}", e));
        }

        // ── Step 9: System notification ──
        match std::process::Command::new("notify-send")
            .arg("--app-name=HVE")
            .arg(format!("🎨 Tema '{}' aplicado", theme_name))
            .arg(format!(
                "Theme \"{}\" applied — colors synced to GTK, QT, terminal, and apps",
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
    fn test_generate_conf_content() {
        let mut map = serde_json::Map::new();
        map.insert("mPrimary".into(), serde_json::Value::String("#2ec436".into()));
        map.insert("mSecondary".into(), serde_json::Value::String("#fb9e0f".into()));
        map.insert("mTertiary".into(), serde_json::Value::String("#9d00ff".into()));
        map.insert("mSurface".into(), serde_json::Value::String("#0c1017".into()));
        map.insert("mError".into(), serde_json::Value::String("#b32d2d".into()));
        map.insert("mShadow".into(), serde_json::Value::String("#090d13".into()));

        let conf = generate_conf_content(&map);
        assert!(conf.contains("$primary = rgb(2ec436)"));
        assert!(conf.contains("$surface = rgb(0c1017)"));
        assert!(conf.contains("$secondary = rgb(fb9e0f)"));
        assert!(conf.contains("$error = rgb(b32d2d)"));
        assert!(conf.contains("$tertiary = rgb(9d00ff)"));
        assert!(conf.contains("$surface_lowest = rgb(090d13)"));
    }

    #[test]
    fn test_generate_lua_content() {
        let mut map = serde_json::Map::new();
        map.insert("mPrimary".into(), serde_json::Value::String("#2ec436".into()));
        map.insert("mSecondary".into(), serde_json::Value::String("#fb9e0f".into()));
        map.insert("mTertiary".into(), serde_json::Value::String("#9d00ff".into()));
        map.insert("mSurface".into(), serde_json::Value::String("#0c1017".into()));
        map.insert("mError".into(), serde_json::Value::String("#b32d2d".into()));
        map.insert("mShadow".into(), serde_json::Value::String("#090d13".into()));

        let lua = generate_lua_content(&map);
        assert!(lua.contains("primary = \"rgb(2ec436)\""));
        assert!(lua.contains("surface = \"rgb(0c1017)\""));
        assert!(lua.contains("secondary = \"rgb(fb9e0f)\""));
        assert!(lua.contains("error = \"rgb(b32d2d)\""));
        assert!(lua.contains("tertiary = \"rgb(9d00ff)\""));
        assert!(lua.contains("surface_lowest = \"rgb(090d13)\""));
    }

    #[test]
    fn test_generate_fallback_no_shadow() {
        let mut map = serde_json::Map::new();
        map.insert("mPrimary".into(), serde_json::Value::String("#2ec436".into()));
        map.insert("mSurface".into(), serde_json::Value::String("#0c1017".into()));
        // No mShadow — should darken surface as fallback

        let conf = generate_conf_content(&map);
        assert!(conf.contains("$surface_lowest"));
        // Darkened #0c1017 by 0.08 should be #0b0e15 (approx)
        assert!(conf.contains("rgb(0b0e15)") || conf.contains("rgb(0b0f15)") || conf.contains("rgb(0b0e14)") || conf.contains("rgb(0b0f14)"));
    }

    #[test]
    fn test_to_rgb_func() {
        assert_eq!(to_rgb_func("#2ec436"), "rgb(2ec436)");
        assert_eq!(to_rgb_func("2ec436"), "rgb(2ec436)");
    }

    #[test]
    fn test_darken_hex() {
        let darkened = darken_hex("#0c1017", 0.08);
        assert_ne!(darkened, "#0c1017");
        assert!(darkened.starts_with('#'));
        assert_eq!(darkened.len(), 7);
    }

    #[test]
    fn test_noctalia_v4_provider_new() {
        let provider = NoctaliaV4Provider::new();
        assert_eq!(provider.id(), "noctalia");
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
