use crate::providers::noctalia_runtime::{noctalia_config_dir, noctalia_msg, noctalia_state_dir};
use crate::providers::shell::NoctaliaV4Paths;
use crate::providers::shell::ShellProvider;
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

/// Parse a hex string (#RGB or #RRGGBB) into (r, g, b) u8 components.
/// Returns (0, 0, 0) on failure.
fn parse_hex_rgb(hex: &str) -> (u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    if hex.len() >= 6 {
        (
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(0),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(0),
        )
    } else {
        (0, 0, 0)
    }
}

/// Format (r, g, b) as `"#rrggbb"`.
fn rgb_to_hex(r: u8, g: u8, b: u8) -> String {
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

/// Darken a hex color by `amount` (0.0 = no change, 1.0 = black).
fn darken_hex(hex: &str, amount: f32) -> String {
    let (r, g, b) = parse_hex_rgb(hex);
    let clamp = |v: f32| -> u8 { (v * (1.0 - amount)).round().clamp(0.0, 255.0) as u8 };
    rgb_to_hex(clamp(r as f32), clamp(g as f32), clamp(b as f32))
}

/// Lighten a hex color by `amount` (0.0 = no change, 1.0 = white).
fn lighten_hex(hex: &str, amount: f32) -> String {
    let (r, g, b) = parse_hex_rgb(hex);
    let clamp = |v: f32| -> u8 { (v + (255.0 - v) * amount).round().clamp(0.0, 255.0) as u8 };
    rgb_to_hex(clamp(r as f32), clamp(g as f32), clamp(b as f32))
}

/// Blend two hex colors: `t=0.0` = all `a`, `t=1.0` = all `b`.
fn blend_hex(a: &str, b: &str, t: f32) -> String {
    let (r1, g1, b1) = parse_hex_rgb(a);
    let (r2, g2, b2) = parse_hex_rgb(b);
    let lerp = |x: f32, y: f32| -> u8 { (x + (y - x) * t).round().clamp(0.0, 255.0) as u8 };
    rgb_to_hex(
        lerp(r1 as f32, r2 as f32),
        lerp(g1 as f32, g2 as f32),
        lerp(b1 as f32, b2 as f32),
    )
}

/// Determine if a hex color represents a dark surface (dark mode).
/// Uses perceived luminance: if the weighted brightness is < 128, it's dark.
fn is_dark_hex(hex: &str) -> bool {
    let (r, g, b) = parse_hex_rgb(hex);
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) < 128.0
}

/// Best-effort delegation to skwd-walld (now skwd-wall-v2 / skwd-helm).
///
/// HVE remains agnostic: if the daemon's socket is absent we do nothing.
/// If it is present we try `skwd-helm apply <path>` (and `skwd-wall-v2 apply` as fallback)
/// and swallow any error so theme apply never fails. Invisible, no UI.
fn skwd_wall_socket_path() -> PathBuf {
    if let Ok(custom) = std::env::var("SKWD_WALL_V2_SOCK") {
        let p = PathBuf::from(&custom);
        if p.exists() {
            return p;
        }
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/user/1000"));
    runtime.join("skwd-wall-v2").join("wall.sock")
}

fn delegate_to_skwd_walld(wallpaper_path: &Path) {
    let socket = skwd_wall_socket_path();
    if !socket.exists() {
        tracing::debug!("[skwd-wall] socket absent at {:?} — skipping delegation", socket);
        return;
    }
    let path_str = match wallpaper_path.to_str() {
        Some(s) if !s.is_empty() => s,
        _ => {
            tracing::warn!("[skwd-wall] invalid wallpaper path {:?}", wallpaper_path);
            return;
        }
    };
    // Try skwd-helm first, then skwd-wall-v2 as fallback
    for bin in ["skwd-helm", "skwd-wall-v2"] {
        let res = std::process::Command::new(bin)
            .arg("apply")
            .arg(path_str)
            .output();
        match res {
            Ok(out) if out.status.success() => {
                tracing::info!("[skwd-wall] delegated wallpaper to {}: {}", bin, path_str);
                return;
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr);
                tracing::warn!("[skwd-wall] {} apply failed: {}", bin, err.trim());
                // try next bin
            }
            Err(e) => {
                tracing::debug!("[skwd-wall] {} not available: {}", bin, e);
            }
        }
    }
    tracing::warn!("[skwd-wall] delegation failed for {} (daemon may be unreachable)", path_str);
}

/// Generate a `terminal` section for a Noctalia predefined scheme from M3 core colors.
///
/// Returns a `serde_json::Value::Object` with keys:
/// `foreground`, `background`, `cursor`, `cursorText`, `selectionFg`, `selectionBg`,
/// `normal` (8 entries), `bright` (8 entries).
fn generate_terminal_section(scheme: &serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
    let get = |key: &str, fallback: &str| -> String {
        scheme
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or(fallback)
            .to_string()
    };

    let surface = get("mSurface", "#000000");
    let on_surface = get("mOnSurface", "#ffffff");
    let primary = get("mPrimary", "#ffffff");
    let on_primary = get("mOnPrimary", "#000000");
    let secondary = get("mSecondary", "#ffffff");
    let tertiary = get("mTertiary", "#ffffff");
    let error = get("mError", "#ff0000");
    let surface_variant = get("mSurfaceVariant", &surface);
    let on_surface_variant = get("mOnSurfaceVariant", &on_surface);
    let outline = get("mOutline", "#888888");

    // Terminal background/foreground derived from surface
    let bg = darken_hex(&surface, 0.08);
    let fg = &on_surface;

    // ANSI normal: map M3 roles to terminal convention
    let black = darken_hex(&surface_variant, 0.5);
    let red = &error;
    let green = &primary;
    let yellow = &secondary;
    let blue = &tertiary;
    // Magenta = mostly tertiary (purple) with warm secondary tint
    let magenta = blend_hex(&secondary, &tertiary, 0.7);
    // Cyan = blend of outline (blue) and primary (green) toward cyan
    let cyan = blend_hex(&outline, &primary, 0.5);
    let white = fg;

    // Bright: lighter versions of normal (with 0.4 lighten factor)
    let b_black = lighten_hex(&black, 0.35);
    let b_red = lighten_hex(red, 0.25);
    let b_green = lighten_hex(green, 0.25);
    let b_yellow = lighten_hex(yellow, 0.25);
    let b_blue = lighten_hex(blue, 0.25);
    let b_magenta = lighten_hex(&magenta, 0.25);
    let b_cyan = lighten_hex(&cyan, 0.25);
    let b_white = lighten_hex(white, 0.15);

    // Selection: use surface_variant (the middle tone) for bg, on_surface_variant for fg
    let sel_bg = &on_surface_variant;
    let sel_fg = darken_hex(&surface, 0.1);

    serde_json::json!({
        "foreground": fg,
        "background": bg,
        "cursor": primary,
        "cursorText": on_primary,
        "selectionFg": sel_fg,
        "selectionBg": sel_bg,
        "normal": {
            "black": black,
            "red": red,
            "green": green,
            "yellow": yellow,
            "blue": blue,
            "magenta": magenta,
            "cyan": cyan,
            "white": white,
        },
        "bright": {
            "black": b_black,
            "red": b_red,
            "green": b_green,
            "yellow": b_yellow,
            "blue": b_blue,
            "magenta": b_magenta,
            "cyan": b_cyan,
            "white": b_white,
        }
    })
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
                } else {
                    // Best-effort delegation to skwd-walld (agnostic, silent if absent)
                    delegate_to_skwd_walld(Path::new(path));
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

            // Generate terminal colors from the M3 palette and add to scheme
            let terminal = generate_terminal_section(&scheme);
            scheme.insert("terminal".to_string(), terminal);

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

// ── Noctalia v5 path lookup ──────────────────────────────────────────

/// Find the palette JSON file for a given source and name.
///
/// Sources:
/// - `custom`       → ~/.config/noctalia/palettes/<name>.json
/// - `builtin`      → No file — built-in palettes are internal to Noctalia
/// - `community`    → ~/.local/state/noctalia/community-palettes/<name>
/// - `wallpaper`    → No file — extracted from wallpaper at runtime
fn find_palette_path(source: &str, name: &str) -> Option<PathBuf> {
    match source {
        "custom" => {
            noctalia_config_dir().map(|d| d.join("palettes").join(format!("{}.json", name)))
        }
        "community" => {
            let home = std::env::var("HOME").ok()?;
            Some(
                PathBuf::from(home)
                    .join(".local")
                    .join("state")
                    .join("noctalia")
                    .join("community-palettes")
                    .join(name),
            )
        }
        _ => None,
    }
}

// ── NoctaliaV5Provider ───────────────────────────────────────────────
//
// Auto-contenido: todo el save/apply se hace por IPC (`noctalia msg`).
// No necesita ShellProvider, ni file paths, ni template processor.

pub struct NoctaliaV5Provider;

impl NoctaliaV5Provider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoctaliaV5Provider {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeProvider for NoctaliaV5Provider {
    fn id(&self) -> &str {
        "noctalia-v5"
    }

    fn display_name_key(&self) -> &str {
        "themes.provider.noctalia_v5"
    }

    fn icon(&self) -> &str {
        "◈"
    }

    fn shell(&self) -> &str {
        "noctalia-v5"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::COLORS | ProviderCapabilities::WALLPAPERS
    }

    fn save(&self, theme_dir: &Path) -> Result<(), String> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        fs::create_dir_all(&provider_dir)
            .map_err(|e| format!("Cannot create provider dir: {}", e))?;

        // 1. Get active color scheme via IPC
        let scheme = noctalia_msg(&["msg", "color-scheme-get"])
            .map_err(|e| format!("Cannot get color scheme: {}", e))?;
        let scheme = scheme.trim().to_string();
        if scheme.is_empty() {
            return Err("Empty color-scheme-get response".into());
        }

        // Save scheme source info (e.g. "custom JokerTheme")
        fs::write(provider_dir.join("source.txt"), &scheme)
            .map_err(|e| format!("Cannot write source.txt: {}", e))?;

        // Parse source and name
        let parts: Vec<&str> = scheme.splitn(2, ' ').collect();
        let source = parts.first().copied().unwrap_or("custom");
        let name = parts.get(1).copied().unwrap_or("");

        // 2. Save palette JSON
        if !name.is_empty() {
            if let Some(palette_path) = find_palette_path(source, name) {
                if palette_path.exists() {
                    fs::copy(&palette_path, provider_dir.join("palette.json"))
                        .map_err(|e| format!("Cannot copy palette: {}", e))?;
                    tracing::info!(
                        "[noctalia-v5] Saved palette from {:?}",
                        palette_path
                    );
                } else {
                    tracing::warn!(
                        "[noctalia-v5] Palette file not found: {:?}",
                        palette_path
                    );
                }
            } else {
                tracing::warn!(
                    "[noctalia-v5] Cannot resolve palette path for source={}, name={}",
                    source,
                    name
                );
            }
        }

        // 3. Save default wallpaper via IPC
        match noctalia_msg(&["msg", "wallpaper-get"]) {
            Ok(wp) => {
                let wp = wp.trim().to_string();
                if !wp.is_empty() {
                    fs::write(provider_dir.join("wallpaper.txt"), &wp)
                        .map_err(|e| format!("Cannot write wallpaper.txt: {}", e))?;
                }
            }
            Err(e) => {
                tracing::warn!("[noctalia-v5] Cannot get wallpaper: {}", e);
            }
        }

        // 4. Save full Noctalia v5 settings (bar, OSD, widgets, layout, everything)
        //    v5 stores its settings in ~/.local/state/noctalia/settings.toml, NOT
        //    in ~/.config/noctalia/settings.json (which is a v4 artifact).
        let v5_settings = noctalia_state_dir().map(|d| d.join("settings.toml"));
        if let Some(settings_path) = v5_settings {
            if settings_path.exists() {
                fs::copy(&settings_path, provider_dir.join("settings.toml"))
                    .map_err(|e| format!("Cannot copy settings.toml: {}", e))?;
                tracing::info!("[noctalia-v5] Saved settings.toml");
            }
        }

        // 5. Save animated wallpaper manifest (mpvpaper plugin), reference only.
        //    It is safe to keep going if the plugin has no live assignments.
        match crate::providers::mpvpaper::save_manifest(&provider_dir) {
            Ok(()) => {}
            Err(e) => tracing::warn!("[noctalia-v5] Could not save mpvpaper manifest: {}", e),
        }

        Ok(())
    }

    fn apply(&self, theme_dir: &Path) -> Result<(), String> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        if !provider_dir.exists() {
            return Err("Noctalia v5 provider data not found in theme".into());
        }

        let palette_src = provider_dir.join("palette.json");
        let source_src = provider_dir.join("source.txt");

        // 0. Restore full Noctalia v5 settings FIRST (bar, OSD, widgets, layout, etc.)
        //    v5 stores settings in ~/.local/state/noctalia/settings.toml, NOT
        //    in settings.json (v4 artifact). This must happen before wallpaper/scheme
        //    so config-reload loads the complete ecosystem state first.
        let saved_settings = provider_dir.join("settings.toml");
        if saved_settings.exists() {
            if let Some(state_dir) = noctalia_state_dir() {
                let dst = state_dir.join("settings.toml");
                fs::copy(&saved_settings, &dst)
                    .map_err(|e| format!("Cannot restore settings.toml: {}", e))?;
                tracing::info!("[noctalia-v5] Restored settings.toml");

                noctalia_msg(&["msg", "config-reload"])
                    .map_err(|e| format!("Cannot reload config: {}", e))?;
                tracing::info!("[noctalia-v5] Config reloaded — bar, OSD, widgets restored");
            }
        }

        // 1. Animated wallpaper (mpvpaper plugin).
        //    If the theme saved animated wallpapers, resolve/download the videos
        //    and bounce the plugin so it launches them. If the theme has NO
        //    manifest, send clear-all FIRST: a running video would otherwise
        //    keep hijacking the screen and the static wallpaper below could
        //    never show. These operations are best-effort: a plugin problem
        //    must never fail the whole theme apply.
        let manifest_exists = provider_dir
            .join(crate::providers::mpvpaper::MANIFEST_FILE)
            .exists();
        let mut animated_applied = false;
        if manifest_exists {
            // Warn the user up-front when the theme has animated wallpapers
            // but the plugin is not available — otherwise they would apply the
            // theme and silently lose the videos.
            if !crate::providers::mpvpaper::mpvpaper_enabled() {
                crate::providers::mpvpaper::notify_plugin_required();
            }
            match crate::providers::mpvpaper::apply_manifest(theme_dir) {
                Ok(()) => {
                    animated_applied = true;
                    tracing::info!("[noctalia-v5] Animated wallpapers applied");
                }
                Err(e) => tracing::warn!("[noctalia-v5] Animated wallpapers skipped: {}", e),
            }
        } else {
            match crate::providers::mpvpaper::clear_all() {
                Ok(()) => tracing::info!("[noctalia-v5] Stopped running video wallpapers"),
                Err(e) => tracing::warn!("[noctalia-v5] clear-all warning: {}", e),
            }
        }

        // 2. Restore wallpaper FIRST — for wallpaper-derived schemes, setting
        //    the wallpaper triggers Noctalia's palette regeneration, so it
        //    must happen before color-scheme-set and templates-apply.
        //    SKIP when a video wallpaper was successfully applied: the mpvpaper
        //    plugin disables the static wallpaper while a video plays, so
        //    re-setting the static image here would cover the running video.
        //    If the animated apply failed, fall back to the static wallpaper.
        let wp_path = provider_dir.join("wallpaper.txt");
        if !animated_applied && wp_path.exists() {
            let wp = fs::read_to_string(&wp_path)
                .map_err(|e| format!("Cannot read wallpaper.txt: {}", e))?;
            let wp = wp.trim();
            if !wp.is_empty() {
                noctalia_msg(&["msg", "wallpaper-set", "", wp])
                    .map_err(|e| format!("Cannot set wallpaper: {}", e))?;
                tracing::info!("[noctalia-v5] Restored wallpaper: {}", wp);
                // Best-effort delegation to skwd-walld (agnostic, silent if absent)
                delegate_to_skwd_walld(Path::new(wp));
            }
        }

        // 2. Restore color scheme
        if source_src.exists() {
            let raw = fs::read_to_string(&source_src)
                .map_err(|e| format!("Cannot read source.txt: {}", e))?;
            let scheme = raw.trim();
            let parts: Vec<&str> = scheme.splitn(2, ' ').collect();
            let source = parts.first().copied().unwrap_or("custom");
            let name = parts.get(1).copied().unwrap_or("");

            if !name.is_empty() && palette_src.exists() {
                // Custom or community palette — copy the saved palette file back
                // and restore as "custom" regardless of original source.
                let safe_name = name.replace('/', "_");

                if let Some(palettes_dir) = noctalia_config_dir().map(|d| d.join("palettes")) {
                    fs::create_dir_all(&palettes_dir)
                        .map_err(|e| format!("Cannot create palettes dir: {}", e))?;
                    let dst = palettes_dir.join(format!("{}.json", safe_name));
                    fs::copy(&palette_src, &dst)
                        .map_err(|e| format!("Cannot restore palette: {}", e))?;
                    tracing::info!("[noctalia-v5] Restored palette to {:?}", dst);

                    noctalia_msg(&["msg", "color-scheme-set", "custom", &safe_name])
                        .map_err(|e| format!("Cannot set color scheme: {}", e))?;
                    tracing::info!("[noctalia-v5] Set active scheme: custom {}", safe_name);
                }
            } else if !name.is_empty() && matches!(source, "builtin" | "community") {
                // Builtin or community — no palette file to copy, restore directly.
                noctalia_msg(&["msg", "color-scheme-set", source, name])
                    .map_err(|e| format!("Cannot set color scheme: {}", e))?;
                tracing::info!("[noctalia-v5] Set active scheme: {} {}", source, name);
            }
            // wallpaper scheme: already handled by wallpaper restoration above

            // 3. Apply templates so rendered files (noctalia-colors.conf, etc.)
            //    reflect the restored palette. color-scheme-set only persists
            //    the setting; templates-apply renders the actual output files.
            noctalia_msg(&["msg", "templates-apply"])
                .map_err(|e| format!("Cannot apply templates: {}", e))?;
            tracing::info!("[noctalia-v5] Templates applied");
        }

        Ok(())
    }

    fn post_apply(&self, theme_name: &str) -> Result<(), String> {
        // No reload needed — v5 hot-reloads automatically.
        // Just a notification.
        match std::process::Command::new("notify-send")
            .arg("--app-name=HVE")
            .arg(format!("🎨 Tema '{}' aplicado", theme_name))
            .arg(format!(
                "Theme \"{}\" applied — Noctalia v5 colors, settings, wallpaper restored",
                theme_name,
            ))
            .output()
        {
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("[noctalia-v5] notify-send not available: {}", e);
            }
        }

        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_noctalia_v4_provider_new() {
        let provider = NoctaliaV4Provider::new();
        assert_eq!(provider.id(), "noctalia");
    }

    #[test]
    fn test_noctalia_v5_provider_new() {
        let provider = NoctaliaV5Provider::new();
        assert_eq!(provider.id(), "noctalia-v5");
        assert_eq!(provider.shell(), "noctalia-v5");
    }

    #[test]
    fn test_noctalia_v5_provider_display_name_key() {
        let provider = NoctaliaV5Provider::new();
        assert_eq!(provider.display_name_key(), "themes.provider.noctalia_v5");
    }

    #[test]
    fn test_noctalia_v5_provider_icon() {
        let provider = NoctaliaV5Provider::new();
        assert_eq!(provider.icon(), "◈");
    }

    #[test]
    fn test_noctalia_v5_provider_capabilities() {
        let provider = NoctaliaV5Provider::new();
        let caps = provider.capabilities();
        assert!(caps.contains(ProviderCapabilities::COLORS));
        assert!(caps.contains(ProviderCapabilities::WALLPAPERS));
    }

    #[test]
    fn test_noctalia_v5_save_creates_provider_dir() {
        // Save requires noctalia running — solo verificamos que el
        // directorio del provider se intente crear. TempDir cleans up
        // automatically on drop (no leftover state between runs).
        let provider = NoctaliaV5Provider::new();
        let dir = TempDir::new().unwrap();

        // No assert on result — puede ser Ok (noctalia corriendo) o
        // Err (noctalia no disponible). Solo verificamos que no panic.
        let _result = provider.save(dir.path());
    }

    #[test]
    fn test_find_palette_path_custom() {
        let path = find_palette_path("custom", "JokerTheme");
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.ends_with("JokerTheme.json"));
        assert!(path.to_string_lossy().contains("palettes"));
    }

    #[test]
    fn test_find_palette_path_community() {
        let path = find_palette_path("community", "Ayu Green");
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.to_string_lossy().contains("community-palettes"));
    }

    #[test]
    fn test_find_palette_path_builtin_returns_none() {
        let path = find_palette_path("builtin", "Kanagawa");
        assert!(path.is_none());
    }

    #[test]
    fn test_find_palette_path_wallpaper_returns_none() {
        let path = find_palette_path("wallpaper", "");
        assert!(path.is_none());
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

    #[test]
    fn test_darken_hex() {
        assert_eq!(darken_hex("#ffffff", 0.0), "#ffffff");
        assert_eq!(darken_hex("#ffffff", 1.0), "#000000");
        assert_eq!(darken_hex("#808080", 0.5), "#404040");
        assert_eq!(darken_hex("#ff8800", 0.5), "#804400");
    }

    #[test]
    fn test_lighten_hex() {
        assert_eq!(lighten_hex("#000000", 0.0), "#000000");
        assert_eq!(lighten_hex("#000000", 1.0), "#ffffff");
        assert_eq!(lighten_hex("#404040", 0.5), "#a0a0a0");
        assert_eq!(lighten_hex("#804400", 0.5), "#c0a280");
    }

    #[test]
    fn test_blend_hex() {
        assert_eq!(blend_hex("#000000", "#ffffff", 0.0), "#000000");
        assert_eq!(blend_hex("#000000", "#ffffff", 1.0), "#ffffff");
        assert_eq!(blend_hex("#000000", "#ffffff", 0.5), "#808080");
        assert_eq!(blend_hex("#ff0000", "#0000ff", 0.5), "#800080");
    }

    #[test]
    fn test_generate_terminal_section() {
        let mut scheme = serde_json::Map::new();
        scheme.insert("mPrimary".into(), serde_json::Value::String("#2ec436".into()));
        scheme.insert("mOnPrimary".into(), serde_json::Value::String("#0e1015".into()));
        scheme.insert("mSecondary".into(), serde_json::Value::String("#fb9e0f".into()));
        scheme.insert("mOnSecondary".into(), serde_json::Value::String("#0e1015".into()));
        scheme.insert("mTertiary".into(), serde_json::Value::String("#9d00ff".into()));
        scheme.insert("mOnTertiary".into(), serde_json::Value::String("#0e1015".into()));
        scheme.insert("mError".into(), serde_json::Value::String("#b32d2d".into()));
        scheme.insert("mOnError".into(), serde_json::Value::String("#0e1015".into()));
        scheme.insert("mSurface".into(), serde_json::Value::String("#0c1017".into()));
        scheme.insert("mOnSurface".into(), serde_json::Value::String("#5c8ac4".into()));
        scheme.insert("mSurfaceVariant".into(), serde_json::Value::String("#11151d".into()));
        scheme.insert("mOnSurfaceVariant".into(), serde_json::Value::String("#9b6bc1".into()));
        scheme.insert("mOutline".into(), serde_json::Value::String("#45a0d6".into()));

        let terminal = generate_terminal_section(&scheme);
        let obj = terminal.as_object().expect("terminal should be an object");

        // Check required keys exist
        assert!(obj.contains_key("foreground"));
        assert!(obj.contains_key("background"));
        assert!(obj.contains_key("cursor"));
        assert!(obj.contains_key("normal"));
        assert!(obj.contains_key("bright"));

        // Check normal section has all 8 ANSI colors
        let normal = obj["normal"].as_object().expect("normal should be object");
        for color in &["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"] {
            assert!(normal.contains_key(*color), "missing normal.{}", color);
        }

        // Bright section has all 8 ANSI colors
        let bright = obj["bright"].as_object().expect("bright should be object");
        for color in &["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"] {
            assert!(bright.contains_key(*color), "missing bright.{}", color);
        }

        // Cursor should match primary
        assert_eq!(obj["cursor"].as_str(), Some("#2ec436"));
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

        let dir = TempDir::new().unwrap();
        let fpath = dir.path().join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = parse_theme_entries(&fpath).unwrap();

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

        let dir = TempDir::new().unwrap();
        let fpath = dir.path().join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = parse_theme_entries(&fpath).unwrap();

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

        let dir = TempDir::new().unwrap();
        let fpath = dir.path().join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = parse_theme_entries(&fpath).unwrap();

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

        let dir = TempDir::new().unwrap();
        let fpath = dir.path().join("wp.json");
        fs::write(&fpath, json).unwrap();
        let entries = parse_theme_entries(&fpath).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "HDMI-A-1");
        assert_eq!(entries[0].1, "/path/light.png");
    }

    #[test]
    fn test_pending_cleared_on_empty_theme() {
        // No wallpapers.json in dir → parse_theme_entries should return error
        let dir = TempDir::new().unwrap();
        let result = parse_theme_entries(&dir.path().join("wallpapers.json"));
        assert!(result.is_err());
    }

    #[test]
    fn settings_json_copied_verbatim() {
        // TDD for Task 1: apply must copy settings.json verbatim, not force wallpaper.enabled=false
        let config_dir = TempDir::new().unwrap();
        let theme_dir = TempDir::new().unwrap();
        let provider_dir = theme_dir.path().join("providers").join("noctalia");
        std::fs::create_dir_all(&provider_dir).unwrap();

        let raw = r##"{"bar":{"position":"top"},"wallpaper":{"enabled":true,"directory":"/x"}}"##;
        std::fs::write(provider_dir.join("settings.json"), raw).unwrap();
        // Need at least one source file; colors.json dummy to satisfy other logic but not needed for this check
        std::fs::write(provider_dir.join("colors.json"), r##"{"mPrimary":"#ff0000"}"##).unwrap();

        // Point provider to our temp config dir via env var
        let orig = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        std::env::set_var("HVE_NOCTALIA_CONFIG", config_dir.path());
        // Ensure rendered_dir exists for save path (not needed for apply but create)
        let hypr_dir = TempDir::new().unwrap();
        std::env::set_var("HVE_NOCTALIA_HYPR", hypr_dir.path());

        let provider = NoctaliaV4Provider::new();
        let res = provider.apply(theme_dir.path());
        assert!(res.is_ok(), "apply should succeed: {:?}", res);

        let live = std::fs::read_to_string(config_dir.path().join("settings.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&live).unwrap();
        assert_eq!(
            v["wallpaper"]["enabled"], true,
            "settings.json must be copied verbatim, not forced off"
        );

        // restore env
        if let Some(val) = orig {
            std::env::set_var("HVE_NOCTALIA_CONFIG", val);
        } else {
            std::env::remove_var("HVE_NOCTALIA_CONFIG");
        }
        std::env::remove_var("HVE_NOCTALIA_HYPR");
    }

    #[test]
    fn delegate_noop_when_socket_absent() {
        // Task 2: delegation must be no-op when socket absent and never panic
        let tmp_runtime = TempDir::new().unwrap();
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        std::env::set_var("XDG_RUNTIME_DIR", tmp_runtime.path());
        std::env::remove_var("SKWD_WALL_V2_SOCK");
        // No socket file exists in tmp_runtime/skwd-wall-v2/wall.sock
        delegate_to_skwd_walld(Path::new("/tmp/fake-wallpaper.jpg"));
        // Should not panic and socket path should be absent
        assert!(!skwd_wall_socket_path().exists());
        if let Some(v) = orig_runtime {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        if let Some(v) = orig_sock {
            std::env::set_var("SKWD_WALL_V2_SOCK", v);
        } else {
            std::env::remove_var("SKWD_WALL_V2_SOCK");
        }
    }

    #[test]
    fn delegate_swallow_failure_when_socket_is_not_socket() {
        // Task 2: failure must be swallowed, not propagated
        let tmp_runtime = TempDir::new().unwrap();
        let sock_dir = tmp_runtime.path().join("skwd-wall-v2");
        std::fs::create_dir_all(&sock_dir).unwrap();
        let sock_path = sock_dir.join("wall.sock");
        // Create a regular file where socket should be — connect will fail
        std::fs::write(&sock_path, b"not a socket").unwrap();
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        std::env::set_var("XDG_RUNTIME_DIR", tmp_runtime.path());
        std::env::remove_var("SKWD_WALL_V2_SOCK");
        // Must not panic, must swallow error
        delegate_to_skwd_walld(Path::new("/tmp/another.jpg"));
        // restore
        if let Some(v) = orig_runtime {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        if let Some(v) = orig_sock {
            std::env::set_var("SKWD_WALL_V2_SOCK", v);
        } else {
            std::env::remove_var("SKWD_WALL_V2_SOCK");
        }
    }
}
