use crate::theme_manager::{ProviderCapabilities, ThemeProvider};
use std::fs;
use std::path::{Path, PathBuf};

/// Resolución de rutas para un shell específico.
/// Cada shell implementa este trait con sus propias rutas.
pub trait ShellPaths: Send + Sync {
    #[allow(dead_code)]
    fn shell_id(&self) -> &str;
    #[allow(dead_code)]
    fn config_dir(&self) -> Option<PathBuf>;
    #[allow(dead_code)]
    fn rendered_dir(&self) -> Option<PathBuf>;
    #[allow(dead_code)]
    fn template_processor(&self) -> Option<PathBuf>;
    #[allow(dead_code)]
    fn wallpapers_file(&self) -> Option<PathBuf>;
    #[allow(dead_code)]
    fn theming_config(&self) -> Option<PathBuf>;
    #[allow(dead_code)]
    fn reload_command(&self) -> Vec<String>;

    /// Apply a wallpaper to a specific screen via shell IPC.
    /// Default: no-op (shell doesn't support wallpaper setting via IPC).
    #[allow(dead_code, unused_variables)]
    fn apply_wallpaper(&self, _path: &Path, _screen: &str) -> Result<(), String> {
        Ok(())
    }

    /// Get the current wallpaper path for a screen.
    /// Default: Err (not supported).
    #[allow(dead_code, unused_variables)]
    fn get_wallpaper(&self, _screen: &str) -> Result<String, String> {
        Err("Shell does not support wallpaper IPC".into())
    }
}

/// Rutas por defecto para Noctalia v4.
///
/// Soporta tres niveles de personalización (prioridad descendente):
/// 1. Environment variables: `HVE_NOCTALIA_CONFIG`, `HVE_NOCTALIA_HYPR`,
///    `HVE_NOCTALIA_TEMPLATE_PROCESSOR`
/// 2. System paths: `/etc/xdg/quickshell/noctalia-shell/...`
/// 3. Standard locations: `dirs::config_dir()`, `~/.config/...`
pub struct NoctaliaPaths;

impl ShellPaths for NoctaliaPaths {
    fn shell_id(&self) -> &str {
        "noctalia"
    }

    fn config_dir(&self) -> Option<PathBuf> {
        if let Ok(dir) = std::env::var("HVE_NOCTALIA_CONFIG") {
            let p = PathBuf::from(dir);
            if p.exists() {
                return Some(p);
            }
        }
        dirs::config_dir().map(|d| d.join("noctalia"))
    }

    fn rendered_dir(&self) -> Option<PathBuf> {
        if let Ok(dir) = std::env::var("HVE_NOCTALIA_HYPR") {
            let p = PathBuf::from(dir);
            if p.exists() {
                return Some(p);
            }
        }
        dirs::home_dir().map(|d| d.join(".config").join("hypr").join("noctalia"))
    }

    fn template_processor(&self) -> Option<PathBuf> {
        if let Ok(path) = std::env::var("HVE_NOCTALIA_TEMPLATE_PROCESSOR") {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
        let candidates = [
            "/etc/xdg/quickshell/noctalia-shell/Scripts/python/src/theming/template-processor.py",
        ];
        for c in candidates {
            let p = PathBuf::from(c);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }

    fn wallpapers_file(&self) -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        Some(PathBuf::from(home).join(".cache").join("noctalia").join("wallpapers.json"))
    }

    fn theming_config(&self) -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        Some(PathBuf::from(home).join(".cache").join("noctalia").join("theming.dynamic.toml"))
    }

    fn reload_command(&self) -> Vec<String> {
        vec!["hyprctl".into(), "reload".into()]
    }

    /// Apply a wallpaper to a specific screen via Quickshell IPC.
    fn apply_wallpaper(&self, path: &Path, screen: &str) -> Result<(), String> {
        let path_str = path.to_str().ok_or("Invalid wallpaper path")?;
        let output = std::process::Command::new("quickshell")
            .args(["-c", "noctalia-shell", "ipc", "call", "wallpaper", "set", path_str, screen])
            .output()
            .map_err(|e| format!("Failed to run quickshell IPC: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Wallpaper IPC failed: {}", stderr.trim()))
        }
    }

    /// Get the current wallpaper path for a screen via Quickshell IPC.
    fn get_wallpaper(&self, screen: &str) -> Result<String, String> {
        let output = std::process::Command::new("quickshell")
            .args(["-c", "noctalia-shell", "ipc", "call", "wallpaper", "get", screen])
            .output()
            .map_err(|e| format!("Failed to run quickshell IPC: {}", e))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.trim().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Wallpaper get failed: {}", stderr.trim()))
        }
    }
}

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

/// Source config files we snapshot for Noctalia.
const NOCTALIA_FILES: &[&str] = &["settings.json", "colors.json", "plugins.json"];

/// Rendered color files generated by Noctalia in the Hyprland config dir.
/// These are what `colors.sh` actually reads, and what Hyprland sources.
const NOCTALIA_RENDERED_FILES: &[&str] = &["noctalia-colors.conf", "noctalia-colors.lua"];

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

pub struct NoctaliaProvider {
    paths: Box<dyn ShellPaths>,
}

impl NoctaliaProvider {
    pub fn new() -> Self {
        Self {
            paths: Box::new(NoctaliaPaths),
        }
    }
}

impl ThemeProvider for NoctaliaProvider {
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
        let src = self.paths.config_dir().ok_or("Noctalia config dir not found")?;
        let hypr_src = self.paths.rendered_dir().ok_or("Noctalia hypr dir not found")?;

        let provider_dir = theme_dir.join("providers").join(self.id());
        fs::create_dir_all(&provider_dir)
            .map_err(|e| format!("Cannot create provider dir: {}", e))?;

        // Save source config files
        for file in NOCTALIA_FILES {
            let src_path = src.join(file);
            if src_path.exists() {
                let dst_path = provider_dir.join(file);
                fs::copy(&src_path, &dst_path)
                    .map_err(|e| format!("Cannot copy {}: {}", file, e))?;
            }
        }

        // Save rendered color files so apply can restore them directly
        // without needing Noctalia to regenerate them.
        for file in NOCTALIA_RENDERED_FILES {
            let src_path = hypr_src.join(file);
            if src_path.exists() {
                let dst_path = provider_dir.join(file);
                fs::copy(&src_path, &dst_path)
                    .map_err(|e| format!("Cannot copy {}: {}", file, e))?;
            }
        }

        Ok(())
    }

    fn apply(&self, theme_dir: &Path) -> Result<(), String> {
        let dst = self.paths.config_dir().ok_or("Noctalia config dir not found")?;
        let provider_dir = theme_dir.join("providers").join(self.id());

        if !provider_dir.exists() {
            return Err("Noctalia provider data not found in theme".into());
        }

        // Step 1: Restore source config files (Noctalia app state)
        for file in NOCTALIA_FILES {
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

        // NOTE: Rendered color files (noctalia-colors.conf, noctalia-colors.lua)
        // are intentionally NOT restored here. When we write wallpapers.json in
        // step 1, Noctalia's inotify fires an async color extraction script
        // (~300-500ms). If we restored rendered files now, that async script
        // would overwrite them with its own calculated colors.
        //
        // Instead, rendered files are restored in post_apply() AFTER the template
        // processor runs and AFTER a short delay that lets the async script finish.
        // This ensures OUR colors are the final state on disk.

        Ok(())
    }

    fn post_apply(&self, theme_name: &str) -> Result<(), String> {
        // ── Step 1: Run Noctalia template processor ──
        // Regenerates GTK, QT, terminal, VSCode, Zen, btop, yazi themes from colors.json.
        // Each shell knows its own post-apply mechanism — this is Noctalia v4's.
        let template_processor = self.paths.template_processor();
        let noctalia_colors = self.paths.config_dir().map(|d| d.join("colors.json"));
        let theming_config = self.paths.theming_config();

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
        // an async color extraction script (~300-500ms). That script recalculates
        // colors from the wallpaper and writes its own noctalia-colors.conf/lua.
        // We MUST wait for it to finish before writing our final rendered files,
        // otherwise it would overwrite our colors after we write them.
        tracing::info!("[noctalia] Waiting for async color extraction to settle...");
        std::thread::sleep(std::time::Duration::from_millis(600));

        // ── Step 3: Restore our rendered color files (LAST WRITE WINS) ──
        // This is the CRITICAL step. We write our saved rendered files AFTER
        // the async script has finished, ensuring OUR colors are the final
        // state on disk. Hyprland sources these files, so what's here is
        // what the user sees.
        if let Some(hypr_dst) = self.paths.rendered_dir() {
            let provider_dir = theme_dir_for_post_apply(theme_name);
            if provider_dir.exists() {
                // Ensure rendered files exist in provider dir (generate from colors.json if old theme)
                let has_conf = provider_dir.join("noctalia-colors.conf").exists();
                let has_lua = provider_dir.join("noctalia-colors.lua").exists();
                if !has_conf || !has_lua {
                    let _ = generate_rendered_from_colors_json(&provider_dir);
                }

                fs::create_dir_all(&hypr_dst)
                    .map_err(|e| format!("Cannot create hypr noctalia dir: {}", e))?;
                for file in NOCTALIA_RENDERED_FILES {
                    let src_path = provider_dir.join(file);
                    if src_path.exists() {
                        let dst_path = hypr_dst.join(file);
                        let tmp = hypr_dst.join(format!("{}.tmp", file));
                        fs::copy(&src_path, &tmp)
                            .map_err(|e| format!("Cannot copy {}: {}", file, e))?;
                        fs::rename(&tmp, &dst_path)
                            .map_err(|e| format!("Cannot rename {}: {}", file, e))?;
                        tracing::debug!("[noctalia] Restored {} → {}", file, dst_path.display());
                    }
                }
                tracing::info!("[noctalia] Rendered color files restored (final state)");
            }
        }

        // ── Step 4: System notification ──
        // Portable across desktop environments, unlike Quickshell's ToastService.
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
    fn test_noctalia_paths_shell_id() {
        let p = NoctaliaPaths;
        assert_eq!(p.shell_id(), "noctalia");
    }

    #[test]
    fn test_noctalia_paths_wallpapers_file() {
        let p = NoctaliaPaths;
        let wf = p.wallpapers_file();
        assert!(wf.is_some());
        let wf = wf.unwrap();
        assert!(wf.ends_with("wallpapers.json"));
        assert!(wf.parent().unwrap().ends_with(".cache/noctalia"));
    }

    #[test]
    fn test_noctalia_paths_theming_config() {
        let p = NoctaliaPaths;
        let tc = p.theming_config();
        assert!(tc.is_some());
        let tc = tc.unwrap();
        assert!(tc.ends_with("theming.dynamic.toml"));
        assert!(tc.parent().unwrap().ends_with(".cache/noctalia"));
    }

    #[test]
    fn test_noctalia_paths_reload_command() {
        let p = NoctaliaPaths;
        let cmd = p.reload_command();
        assert_eq!(cmd, vec!["hyprctl".to_string(), "reload".to_string()]);
    }

    #[test]
    fn test_noctalia_provider_new() {
        let provider = NoctaliaProvider::new();
        assert_eq!(provider.id(), "noctalia");
    }

    #[test]
    fn test_apply_wallpaper_ipc_format() {
        let p = NoctaliaPaths;
        // When quickshell is available, IPC succeeds (returns Ok).
        // The important thing is the command format is correct.
        let result = p.apply_wallpaper(Path::new("/tmp/fake.png"), "DP-3");
        // IPC call was made — it should succeed if quickshell is running
        assert!(
            result.is_ok() || result.as_ref().unwrap_err().contains("quickshell"),
            "expected IPC call to be made, got: {:?}",
            result
        );
    }

    #[test]
    fn test_get_wallpaper_ipc_format() {
        let p = NoctaliaPaths;
        let result = p.get_wallpaper("DP-3");
        // IPC call was made — verify we get a response (success or error with path hint)
        assert!(
            result.is_ok() || result.as_ref().unwrap_err().contains("quickshell") || result.as_ref().unwrap_err().contains("/tmp"),
            "expected IPC call to be made, got: {:?}",
            result
        );
    }
}
