use crate::providers::background;
use crate::providers::noctalia_runtime::{
    noctalia_config_dir, noctalia_msg, noctalia_state_dir, plugin_enabled_probe, MPVPAPER_PLUGIN_ID,
};
use crate::providers::wallpaper_authority::{self, SavePlan, WallpaperKind};
use crate::providers::shell::NoctaliaV4Paths;
use crate::providers::shell::ShellProvider;
use crate::theme_manager::{DeletableArtifact, ProviderCapabilities, ThemeProvider};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

/// Remove a stale background artifact, logging failures instead of
/// swallowing them: a silent failure leaves the wrong background record
/// behind and the next apply resurrects it. A missing file is the normal
/// case (nothing stale) and stays quiet.
fn remove_stale_artifact(path: &Path) {
    if let Err(e) = fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "[noctalia-v5] Could not remove stale {}: {}",
                path.display(),
                e
            );
        }
    }
}

/// Read the `noctalia/mpvpaper` enabled state from a settings.toml text.
///
/// D4: applying a theme restores settings.toml verbatim and runs
/// `config-reload`, which re-enables a plugin the user disabled (every
/// saved theme carries it enabled while the keeper runs disabled). The
/// `[plugins] enabled = [...]` list is machine state, not theme content,
/// so only that section counts: other sections, commented lines and
/// trailing comments never match, and a multi-line array is followed to
/// its closing bracket. `false` for anything unparseable — the caller
/// then treats the state as unknown, never as enabled.
fn mpvpaper_enabled_in_settings(raw: &str) -> bool {
    let mut in_plugins = false;
    let mut collecting = false;
    let mut depth: i32 = 0;
    let mut buf = String::new();
    let mut found = false;
    for line in raw.lines() {
        let stripped = line.trim();
        if stripped.starts_with('#') || stripped.is_empty() {
            continue;
        }
        // Machine-written file: no '#' inside quoted values, so a '#'
        // always starts a trailing comment.
        let code = stripped.split('#').next().unwrap_or("").trim();
        if let Some(header) = code.strip_prefix('[') {
            let header = header.split(']').next().unwrap_or("").trim();
            in_plugins = header == "plugins";
            collecting = false;
            continue;
        }
        if !in_plugins {
            continue;
        }
        let piece = if !collecting {
            match code.strip_prefix("enabled") {
                Some(rest) => {
                    let rest = rest.trim_start();
                    match rest.strip_prefix('=') {
                        Some(value) => value,
                        None => continue,
                    }
                }
                None => continue,
            }
        } else {
            code
        };
        if !collecting {
            buf.clear();
            collecting = true;
        }
        buf.push_str(piece);
        depth += piece.chars().filter(|c| *c == '[').count() as i32
            - piece.chars().filter(|c| *c == ']').count() as i32;
        if depth <= 0 {
            collecting = false;
            if buf.contains("\"noctalia/mpvpaper\"") || buf.contains("'noctalia/mpvpaper'") {
                found = true;
            }
            buf.clear();
        }
    }
    found
}

/// What apply must do to `noctalia/mpvpaper` after `config-reload` so the
/// theme cannot change whether the plugin is enabled (D4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginReassert {
    Disable,
    Enable,
    Leave,
}

/// Map the pre-apply snapshot to the post-reload action: a disabled plugin
/// stays disabled (a theme carrying it enabled must not leave it
/// enabled), an enabled one stays enabled, unknown stays untouched.
fn plugin_reassert_action(was_enabled: Option<bool>) -> PluginReassert {
    match was_enabled {
        Some(false) => PluginReassert::Disable,
        Some(true) => PluginReassert::Enable,
        None => PluginReassert::Leave,
    }
}

/// Snapshot whether `noctalia/mpvpaper` is enabled RIGHT NOW, before apply
/// overwrites the live settings. Primary signal is the live settings.toml
/// (fast, local, no IPC); fallback is the `plugins list` IPC. `None` when
/// neither answers — the caller then leaves the reloaded plugin state
/// alone and says so, instead of changing what it could not read.
fn live_mpvpaper_plugin_enabled() -> Option<bool> {
    if let Some(state_dir) = noctalia_state_dir() {
        match fs::read_to_string(state_dir.join("settings.toml")) {
            Ok(raw) => return Some(mpvpaper_enabled_in_settings(&raw)),
            Err(e) => tracing::debug!(
                "[noctalia-v5] Cannot read live settings.toml for plugin snapshot: {}",
                e
            ),
        }
    }
    // Same predicate as mpvpaper::mpvpaper_enabled, but tri-state: an IPC
    // failure is "unknown", never "disabled" — the seam's probe carries the
    // failure back verbatim, so this warning keeps its cause.
    match plugin_enabled_probe(MPVPAPER_PLUGIN_ID) {
        Ok(enabled) => Some(enabled),
        Err(e) => {
            tracing::warn!(
                "[noctalia-v5] Cannot snapshot plugin state ({}); leaving reloaded state alone",
                e
            );
            None
        }
    }
}

// ── NoctaliaV4Provider ───────────────────────────────────────────────

// `deletable-artefacts`: none, so this provider inherits the trait's
// no-op declaration. v4 only copies LIVE config files (colors.json,
// settings.json, plugins.json, rendered outputs) that belong to the
// running shell and are rewritten by every apply — a deleted v4 theme
// owns no artifact outside its own directory (and the pre-Phase-3 core
// cleanup only ever read the v5 record, so the no-op preserves it).

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
                    background::hand_off_path(Path::new(path));
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

/// Parse `custom <PaletteName>` from a `source.txt` body. Splits on ANY
/// whitespace (space, tab, …): HVE's own Save UI accepts names the old
/// space-only split misread, and a misread referrer is a palette deleted
/// while a surviving theme still needs it. `None` for non-custom records.
/// (Moved here from the theme manager with the `deletable-artefacts`
/// declaration: the record format is this backend's own knowledge.)
fn parse_custom_palette_name(text: &str) -> Option<String> {
    let mut parts = text.split_whitespace();
    if parts.next()? != "custom" {
        return None;
    }
    let name: String = parts.collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return None;
    }
    // Apply sanitizes `/` so the on-disk file can never contain one.
    Some(name.replace('/', "_"))
}

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

// ── Wallpaper-engine colour authority: detection + single-shot re-assert
//
// WHY this exists: the keeper runs the skwd-wall wallpaper engine as the
// color AUTHORITY (its config sets theme.policy == "wallpaper" and/or
// noctalia.themeMode == "follow"). HVE hands that engine the wallpaper
// during apply, so the engine regenerates its wallpaper-derived palette
// asynchronously AFTER HVE's synchronous steps and re-imposes
// `custom skwd-wall` — silently discarding the theme's custom palette for
// over a minute. HVE's palette copy is correct; it just loses the race
// without noticing. This section re-asserts the theme's palette once,
// bounded, off the UI thread, with honest logging.
//
// WHO owns what (capability routing): the engine's config file is the
// engine's business, so both the ownership READ and the yield/restore of
// the colour authority are reached through the `apply-background` router
// (`background::engine_owns_color_scheme`, `background::hold_color_authority`
// / `release_color_authority`, `background::color_authority_held`) — this
// provider never opens, flips or restores another backend's configuration.

/// Bounds for the single-shot re-assert: a short settle so the engine's
/// async regen can land first, at most 3 re-assert sets with a fixed delay
/// between checks, and a hard cap on the whole worker. Worst case the
/// worker lives settle + cap ≈ 10 s, then exits by itself — it can never
/// become a "keep watching" thread or a cross-engine war.
const COLOR_REASSERT_SETTLE: Duration = Duration::from_secs(2);
const COLOR_REASSERT_DELAY: Duration = Duration::from_secs(2);
const COLOR_REASSERT_MAX_SETS: u32 = 3;
const COLOR_REASSERT_CAP: Duration = Duration::from_secs(8);

/// Timing source: production constants, unless the `fast` test profile is
/// selected (same env-seam style as the rest of the codebase) so tests can
/// observe the full race end to end without waiting out the real delays.
fn color_reassert_timing() -> (Duration, u32, Duration, Duration) {
    if std::env::var("HVE_REASSERT_PROFILE").as_deref() == Ok("fast") {
        (
            Duration::from_millis(5),
            COLOR_REASSERT_MAX_SETS,
            Duration::from_millis(15),
            Duration::from_millis(1500),
        )
    } else {
        (
            COLOR_REASSERT_SETTLE,
            COLOR_REASSERT_MAX_SETS,
            COLOR_REASSERT_DELAY,
            COLOR_REASSERT_CAP,
        )
    }
}

/// Verify the custom palette is live and re-assert it when it is not.
///
/// Runs to completion on the calling thread: the apply path calls this via
/// [`spawn_custom_scheme_reassert`] so the UI thread never blocks. Each
/// check runs `color-scheme-get`; only a check that did NOT stick re-issues
/// `color-scheme-set custom <name>` + `templates-apply`. Returns true when
/// the scheme is verified live at the end. Never fails, never panics: every
/// IPC error is logged and swallowed, and a missing `noctalia` binary means
/// "do nothing" (the get fails, we log at debug and return).
fn reassert_custom_scheme_blocking(
    palette_name: &str,
    settle: Duration,
    max_sets: u32,
    delay: Duration,
    cap: Duration,
) -> bool {
    std::thread::sleep(settle);
    let expected = format!("custom {}", palette_name);
    let deadline = Instant::now() + cap;
    let mut sets = 0u32;
    loop {
        if Instant::now() >= deadline {
            tracing::warn!(
                "[noctalia-v5] custom scheme '{}' unverified at the cap: the background engine owns the color scheme — set skwd-wall-v2/config.json theme.policy away from \"wallpaper\" / noctalia.themeMode away from \"follow\", or the theme's saved palette will not stick",
                palette_name
            );
            return false;
        }
        match noctalia_msg(&["msg", "color-scheme-get"]) {
            Err(e) => {
                tracing::debug!(
                    "[noctalia-v5] color re-assert: cannot query scheme ({}); leaving it alone",
                    e
                );
                return false;
            }
            Ok(live) if live.trim() == expected => {
                if sets == 0 {
                    tracing::info!(
                        "[noctalia-v5] custom scheme '{}' stuck on first check — no re-assert needed",
                        palette_name
                    );
                } else {
                    tracing::info!(
                        "[noctalia-v5] custom scheme '{}' re-asserted and verified",
                        palette_name
                    );
                }
                return true;
            }
            Ok(live) => {
                if sets >= max_sets {
                    tracing::warn!(
                        "[noctalia-v5] custom scheme '{}' still overridden (live: '{}'): the background engine owns the color scheme — set skwd-wall-v2/config.json theme.policy away from \"wallpaper\" / noctalia.themeMode away from \"follow\", or the theme's saved palette will not stick",
                        palette_name,
                        live.trim()
                    );
                    return false;
                }
                if let Err(e) = noctalia_msg(&["msg", "color-scheme-set", "custom", palette_name])
                {
                    tracing::warn!(
                        "[noctalia-v5] color re-assert: cannot set scheme '{}': {}",
                        palette_name,
                        e
                    );
                } else if let Err(e) = noctalia_msg(&["msg", "templates-apply"]) {
                    tracing::warn!("[noctalia-v5] color re-assert: cannot apply templates: {}", e);
                }
                sets += 1;
                // Never sleep past the deadline: the cap is a hard wall time.
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .unwrap_or(Duration::ZERO);
                std::thread::sleep(delay.min(remaining));
            }
        }
    }
}

/// Fire-and-forget the single-shot re-assert after an apply restored a
/// custom palette while the engine owns the scheme.
///
/// Detached by design: the engine's regen lands after apply returns, so
/// joining here would freeze the UI thread for up to ~10 s. The worker
/// never touches Slint state — it only runs `noctalia msg` and logs — so
/// detaching is safe, and it always exits by itself at the attempt cap.
///
/// `previous_value`: when the apply yielded the engine's colour authority
/// (W2), the worker restores it once the palette is verified (or at the
/// bounded cap); `None` keeps today's behaviour byte-for-byte.
fn spawn_custom_scheme_reassert(palette_name: String, previous_value: Option<String>) {
    std::thread::spawn(move || {
        let (settle, max_sets, delay, cap) = color_reassert_timing();
        reassert_custom_scheme_blocking(&palette_name, settle, max_sets, delay, cap);
        // The palette is now verified live (or the bounded cap was hit, or
        // the IPC hardened): put the engine's colour authority back EXACTLY
        // as it was. Runs on every worker exit path, so the restore is
        // never lost — the guard disarmed itself in favour of this thread.
        if let Some(previous) = previous_value {
            match background::release_color_authority(&previous) {
                Ok(wrote) => tracing::info!(
                    "[noctalia-v5] engine colour authority restored (theme.policy -> \"{}\", wrote={})",
                    previous,
                    wrote
                ),
                Err(e) => tracing::warn!(
                    "[noctalia-v5] could not restore engine colour authority: {}",
                    e
                ),
            }
        }
    });
}

/// Whether the live Noctalia colour state still matches the theme's saved
/// palette snapshot (unit 1c2 of `odd/tasks/hve-capability-routing.md`).
///
/// Pure: no I/O, no `Command` — `reassert_colours` reads the inputs and this
/// function decides, so the decision is pinned by unit tests.
///
/// `None` live scheme (noctalia not answering / no scheme readable) answers
/// `true`: the re-assert is idempotent (copy + set + templates converge on
/// the snapshot), so an unreadable scheme cannot start a fight; a dead
/// daemon surfaces as `Err` from the CLI step instead. An empty snapshot or
/// an empty name owns nothing and answers `false`.
fn palette_needs_reassert(
    live_scheme: Option<&str>,
    live_palette: Option<&[u8]>,
    snapshot: &[u8],
    palette_name: &str,
) -> bool {
    if snapshot.is_empty() || palette_name.is_empty() {
        return false;
    }
    if live_scheme.is_none() {
        return true;
    }
    let expected = format!("custom {}", palette_name);
    if live_scheme.map(|s| s.trim()) != Some(expected.as_str()) {
        return true;
    }
    !matches!(live_palette, Some(live) if live == snapshot)
}

// ── NoctaliaV5Provider ───────────────────────────────────────────────
//
// Self-contained: the whole save/apply happens over IPC (`noctalia msg`).
// It needs no ShellProvider, no file paths, no template processor.

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

        // 3. Ask the compositor which background is actually on top (the
        //    daemon is a secondary signal only), and capture ONLY that one.
        //    Static -> wallpaper.txt only (drop any stale video record);
        //    video with a resolvable painter path -> the painter record
        //    (video.txt) only (drop the stale mpvpaper manifest, which still
        //    names the previous video). Anything unverifiable — no signal,
        //    or video with no resolvable path — keeps the legacy
        //    capture-everything behaviour so a missing/unreachable query
        //    never loses data silently, and says so.
        //
        //    Two-tier decision from one snapshot: the cheap kind-only
        //    decision sets the plan; the richer background decision (kind +
        //    path + painter) runs only when a video is on top and its path
        //    is needed. Both read the same captured inputs, so the kind
        //    cannot flip between them; a video decision without a
        //    resolvable path still vetoes honestly instead of recording a
        //    video that may no longer be showing.
        let authority = wallpaper_authority::query_authority_snapshot();
        let plan = match wallpaper_authority::snapshot_wallpaper_kind(&authority) {
            Ok(kind) => wallpaper_authority::plan_from_kind(Some(kind)),
            Err(reason) => {
                tracing::warn!(
                    "[noctalia-v5] No wallpaper signal ({}); capturing available backgrounds",
                    reason
                );
                SavePlan::Unknown
            }
        };
        // A video kind is exact only when the painter's path resolves:
        // without it there is nothing truthful to record, so veto honestly
        // (legacy capture) instead of inventing a path.
        let video_bg: Option<wallpaper_authority::ActiveBackground> =
            if matches!(plan, SavePlan::VideoOnly) {
                match wallpaper_authority::snapshot_background(&authority) {
                    Ok(bg)
                        if bg.kind == WallpaperKind::Video && bg.video_path.is_some() =>
                    {
                        Some(bg)
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "[noctalia-v5] Video on top but painter identity unavailable ({:?}); capturing available backgrounds",
                            other.kind
                        );
                        None
                    }
                    Err(reason) => {
                        tracing::warn!(
                            "[noctalia-v5] Video on top but painter identity unavailable ({}); capturing available backgrounds",
                            reason
                        );
                        None
                    }
                }
            } else {
                None
            };
        let exact_video = video_bg.is_some();
        let vetoed_video = matches!(plan, SavePlan::VideoOnly) && !exact_video;
        let capture_static =
            matches!(plan, SavePlan::StaticOnly | SavePlan::Unknown) || vetoed_video;
        let capture_video_manifest = matches!(plan, SavePlan::Unknown) || vetoed_video;

        // 4. Save default wallpaper (static only, unless authority unknown).
        //
        // WHY the daemon override: the suite that paints the layer keeps
        // its own per-output truth (`skwd-helm outputs --json`), while
        // `wallpaper-get` is a third party value that can lag it (seen
        // live: Noctalia reported car3.jpg while the suite painted
        // joker1.png). When the compositor proves the skwd suite paints a
        // static scene, the recorded path is the daemon's `current` for
        // the lexicographically-first painted output (see
        // `resolve_static_path` for the rule); any other painter, an
        // unreachable compositor, or an unusable daemon keeps today's
        // `wallpaper-get` value byte-for-byte, with a warning naming the
        // output when the suite paints but the daemon cannot complete it.
        if capture_static {
            let legacy: Option<String> = match noctalia_msg(&["msg", "wallpaper-get"]) {
                Ok(wp) => {
                    let wp = wp.trim().to_string();
                    if wp.is_empty() {
                        None
                    } else {
                        Some(wp)
                    }
                }
                Err(e) => {
                    tracing::warn!("[noctalia-v5] Cannot get wallpaper: {}", e);
                    None
                }
            };
            let recorded: Option<String> = match wallpaper_authority::snapshot_static_path(
                &authority,
            ) {
                Ok(Some(current)) => {
                    let current = current.trim().to_string();
                    if current.is_empty() {
                        legacy
                    } else {
                        if legacy.as_deref() != Some(current.as_str()) {
                            tracing::info!(
                                "[noctalia-v5] Recording skwd daemon current as wallpaper (the suite paints it): {}",
                                current
                            );
                        }
                        Some(current)
                    }
                }
                Ok(None) => legacy,
                Err(reason) => {
                    tracing::warn!(
                        "[noctalia-v5] {} — recording wallpaper-get value instead",
                        reason
                    );
                    legacy
                }
            };
            if let Some(wp) = recorded {
                fs::write(provider_dir.join("wallpaper.txt"), &wp)
                    .map_err(|e| format!("Cannot write wallpaper.txt: {}", e))?;
            }
        } else {
            // Authority says video: a stale static record would resurrect the
            // wrong background, so remove it (re-save does not wipe the dir).
            remove_stale_artifact(&provider_dir.join("wallpaper.txt"));
        }

        // 5. Save full Noctalia v5 settings (bar, OSD, widgets, layout, everything)
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

        // 6. Painter-identified video record (video.txt): kind + path + who
        //    painted it, so apply can restore it exactly through the manager
        //    in charge. Written only for an exact video; every other arm
        //    drops a stale record (re-save does not wipe the dir, and a
        //    leftover would hijack apply with a video no longer on top).
        //    A video KIND with no resolvable path records nothing — the
        //    legacy capture below stays honest instead of inventing one.
        match video_bg.as_ref() {
            Some(bg) => match bg.video_path.as_ref() {
                Some(path) => {
                    let record = wallpaper_authority::format_painter_video_record(
                        path,
                        bg.painter_proc.as_deref(),
                        &bg.top_namespace,
                        bg.top_pid,
                    );
                    match fs::write(
                        provider_dir.join(wallpaper_authority::PAINTER_VIDEO_FILE),
                        &record,
                    ) {
                        Ok(()) => tracing::info!(
                            "[noctalia-v5] Saved painter video record (video.txt): {}",
                            path.display()
                        ),
                        Err(e) => {
                            tracing::warn!("[noctalia-v5] Cannot write video.txt: {}", e)
                        }
                    }
                }
                // Unreachable by construction (video_bg only holds resolved
                // paths), but honesty first: never write a record without one.
                None => tracing::warn!(
                    "[noctalia-v5] Exact video lost its identity; no video.txt written"
                ),
            },
            None => {
                remove_stale_artifact(&provider_dir.join(wallpaper_authority::PAINTER_VIDEO_FILE));
            }
        }

        // 7. Save animated wallpaper manifest (mpvpaper plugin), reference only.
        //    Only for the honest legacy capture (unreachable authority, or a
        //    vetoed video with no painter path): the manifest comes from
        //    mpvpaper's state file, which still names the previous video, so
        //    an exact painter record must never share a theme with it — apply
        //    would otherwise have two videos claiming the screen.
        if capture_video_manifest {
            match crate::providers::bg_info::save_manifest(&provider_dir) {
                Ok(()) => {}
                Err(e) => tracing::warn!("[noctalia-v5] Could not save mpvpaper manifest: {}", e),
            }
        } else {
            // Authority decided exactly (static, or video with a painter
            // record): drop any stale manifest (re-save does not wipe the
            // dir, and a leftover would hijack apply).
            remove_stale_artifact(&provider_dir.join(crate::providers::bg_info::MANIFEST_FILE));
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
        //
        //    D4: the saved settings.toml carries its own `[plugins] enabled`
        //    list (every saved theme has `noctalia/mpvpaper` enabled while
        //    the keeper runs disabled), so restoring it verbatim plus
        //    `config-reload` would re-enable a plugin the user turned off —
        //    and its boot `applyAll()` would relaunch the stale assignments
        //    video. Snapshot the pre-apply state first and re-assert it
        //    after the reload; the rest of the settings restore is
        //    untouched.
        let plugin_was_enabled = live_mpvpaper_plugin_enabled();
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

                match plugin_reassert_action(plugin_was_enabled) {
                    PluginReassert::Disable => {
                        if let Err(e) =
                            noctalia_msg(&["msg", "plugins", "disable", "noctalia/mpvpaper"])
                        {
                            tracing::warn!(
                                "[noctalia-v5] Could not keep noctalia/mpvpaper disabled: {}",
                                e
                            );
                        } else {
                            tracing::info!(
                                "[noctalia-v5] Kept noctalia/mpvpaper disabled (pre-apply state)"
                            );
                        }
                    }
                    PluginReassert::Enable => {
                        if let Err(e) =
                            noctalia_msg(&["msg", "plugins", "enable", "noctalia/mpvpaper"])
                        {
                            tracing::warn!(
                                "[noctalia-v5] Could not keep noctalia/mpvpaper enabled: {}",
                                e
                            );
                        }
                    }
                    PluginReassert::Leave => tracing::warn!(
                        "[noctalia-v5] Unknown pre-apply plugin state; leaving reloaded plugins as-is"
                    ),
                }
            }
        }

        // 1. Painter-identified video (video.txt): the exact record of what
        //    was on top at save time (kind + path + who painted it).
        //    Restored through the manager in charge (`skwd-helm apply` via
        //    the delegation helper) — the mpvpaper plugin is never touched,
        //    so a plugin the user disabled stays disabled (G3). When the
        //    record is missing or unresolvable, fall through to the legacy
        //    manifest path below; a theme without any video record still
        //    sends clear-all first so a running video cannot hijack the
        //    screen. Best-effort throughout: a video problem must never fail
        //    the whole theme apply.
        let mut animated_applied = false;
        let painter_record = provider_dir.join(wallpaper_authority::PAINTER_VIDEO_FILE);
        // ── Colour-authority yield (W2 + W4) ────────────────────────────
        // The keeper's wallpaper engine owns the colour scheme
        // (`theme.policy == "wallpaper"`): if a wallpaper is handed over
        // with the policy still on, the engine regenerates its wallpaper
        // palette asynchronously AFTER these synchronous steps and
        // re-imposes it — the ~4 s flash the keeper measured. Flip the
        // policy to `off` BEFORE the first hand-off, give the engine's
        // config reload a bounded moment to observe the flip, and hold the
        // authority off until the theme palette is verified. The previous
        // value comes back EXACTLY as it was, and a failed write never
        // aborts the apply. The whole mechanism lives behind the
        // `apply-background` router (`background::hold_color_authority`),
        // because this provider never configures another backend.
        //
        // W2 wired the yield into the STATIC branch only; W4 hoists it
        // before the WHOLE wallpaper-restoration phase so the animated
        // hand-off (video.txt -> skwd-helm apply, below) is covered too.
        // One yield per apply — whichever branch hands the wallpaper over
        // first (the video delegation when the animated branch runs, the
        // static `wallpaper-set` otherwise), the flip already happened and
        // the observe wait already elapsed. The hold returns its elapsed
        // time so the keeper's live test can tune the router's bound.
        //
        // Yield only when a wallpaper is actually going to change hands: a
        // painter video record exists (the delegation below) or a static
        // wallpaper.txt names a non-empty path (the restore further down).
        // The static read is LENIENT here — the authoritative read with
        // its honest error stays in the static branch — so an unreadable
        // wallpaper.txt cannot change the error behaviour of today. A
        // theme with neither asset hands nothing over: nothing to yield,
        // the engine config stays byte-identical (today's behaviour).
        //
        // Ownership decision BEFORE the flip: after the flip the config
        // reads `off`, and `background::engine_owns_color_scheme()` would
        // answer FALSE — denying the very re-assert that must run (the W2
        // trap). This snapshot powers the spawn decision below, and the
        // yielded value rides along to the worker explicitly so the
        // restore is never lost.
        let wp_path = provider_dir.join("wallpaper.txt");
        // Same lenient read as before (an unreadable wallpaper.txt must not
        // change the yield behaviour): the non-empty saved path, kept as a
        // fact for the `apply-background` request built below.
        let static_wallpaper = fs::read_to_string(&wp_path)
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .map(PathBuf::from);
        let static_planned = static_wallpaper.is_some();
        let owner_snapshot: Option<bool>;
        let mut authority_guard: Option<background::ColourAuthorityHold> = None;
        if painter_record.exists() || static_planned {
            owner_snapshot = Some(background::engine_owns_color_scheme());
            match background::hold_color_authority() {
                Ok(Some(hold)) => {
                    tracing::info!(
                        "[noctalia-v5] engine colour authority yielded for the apply \
                         (theme.policy -> off); flip observed in {:.0} ms",
                        hold.observed_in().as_millis()
                    );
                    authority_guard = Some(hold);
                }
                Ok(None) => {
                    tracing::debug!(
                        "[noctalia-v5] no engine colour authority to yield; behaving \
                         exactly as today"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "[noctalia-v5] cannot yield engine colour authority ({}); \
                         proceeding as today",
                        e
                    );
                }
            }
        } else {
            owner_snapshot = None;
        }
        if painter_record.exists() {
            let recorded: Option<PathBuf> = fs::read_to_string(&painter_record)
                .ok()
                .and_then(|text| wallpaper_authority::parse_painter_video_record(&text));
            match recorded {
                Some(path) if path.exists() => {
                    if background::hand_off_path(&path) {
                        animated_applied = true;
                        tracing::info!(
                            "[noctalia-v5] Restored painter video (video.txt) via skwd: {}",
                            path.display()
                        );
                    } else {
                        // D1: the video is NOT back — keep every fallback
                        // route (manifest below, static restore after it)
                        // instead of claiming success and deleting them.
                        tracing::warn!(
                            "[noctalia-v5] Painter video delegation failed ({}); keeping fallback routes",
                            path.display()
                        );
                    }
                }
                Some(path) => {
                    tracing::warn!(
                        "[noctalia-v5] Painter video no longer on disk ({}); falling back",
                        path.display()
                    );
                }
                None => {
                    tracing::warn!(
                        "[noctalia-v5] Cannot parse video.txt; falling back to manifest"
                    );
                }
            }
        }
        if animated_applied {
            // The exact video is back with its own manager: a stale mpvpaper
            // manifest must not bounce the plugin over it.
            remove_stale_artifact(&provider_dir.join(crate::providers::bg_info::MANIFEST_FILE));
        }
        // 1b. Legacy animated wallpaper (mpvpaper plugin manifest).
        //    Only when no exact painter video was restored. Refuses (never
        //    enables) while the plugin is disabled — see mpvpaper.rs.
        //    The hand-off to the OTHER backends goes through the
        //    `apply-background` router: this path names no backend. The
        //    up-front notification for a missing/disabled plugin happens
        //    inside the router, before the manifest leg runs, exactly as
        //    it did here.
        let mut bg_request = background::ApplyBackgroundRequest {
            theme_dir: theme_dir.to_path_buf(),
            provider_dir,
            animated_applied,
            static_wallpaper,
        };
        let outcome = background::apply_animated(&bg_request);
        animated_applied = outcome.animated_applied;
        match outcome.leg {
            background::RoutedLeg::AlreadyApplied => {}
            background::RoutedLeg::ManifestApplied => {
                tracing::info!("[noctalia-v5] Animated wallpapers applied");
            }
            background::RoutedLeg::ManifestRefused(reason) => {
                tracing::warn!("[noctalia-v5] Animated wallpapers skipped: {}", reason);
            }
            background::RoutedLeg::Cleared => {
                tracing::info!("[noctalia-v5] Stopped running video wallpapers");
            }
            background::RoutedLeg::ClearFailed(reason) => {
                tracing::warn!("[noctalia-v5] clear-all warning: {}", reason);
            }
        }

        // 2. Restore wallpaper FIRST — for wallpaper-derived schemes, setting
        //    the wallpaper triggers Noctalia's palette regeneration, so it
        //    must happen before color-scheme-set and templates-apply.
        //    SKIP when a video wallpaper was successfully applied: the mpvpaper
        //    plugin disables the static wallpaper while a video plays, so
        //    re-setting the static image here would cover the running video.
        //    If the animated apply failed, fall back to the static wallpaper.
        //
        //    The colour-authority yield already happened BEFORE the animated
        //    branch above (W4): one flip per apply, the engine held off
        //    through every hand-off in this phase. The guard — armed with
        //    the previous value — restores on every exit path unless the
        //    re-assert worker takes ownership.
        if !animated_applied && wp_path.exists() {
            let wp = fs::read_to_string(&wp_path)
                .map_err(|e| format!("Cannot read wallpaper.txt: {}", e))?;
            let wp = wp.trim();
            if !wp.is_empty() {
                noctalia_msg(&["msg", "wallpaper-set", "", wp])
                    .map_err(|e| format!("Cannot set wallpaper: {}", e))?;
                tracing::info!("[noctalia-v5] Restored wallpaper: {}", wp);
                // Best-effort delegation to skwd-walld (agnostic, silent if absent)
                // — reached through the `apply-background` router. The
                // authoritative read above is what Noctalia just set: state it
                // on the request so the router hands the engine the SAME path.
                bg_request.static_wallpaper = Some(PathBuf::from(wp));
                background::hand_off_static(&bg_request);
            }
        }

        // 2. Restore color scheme
        // Tracks the custom palette actually restored below, so the
        // ownership re-assert at the end fires only for what was set.
        let mut custom_restored: Option<String> = None;
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
                    // The applied theme owns the colours: declare it (backend,
                    // theme, saved snapshot, palette name) so the central
                    // colour pipeline can follow the snapshot instead of the
                    // live palette. Best-effort: a failed write warns and
                    // never fails the apply; nothing reads it yet.
                    let theme_name = theme_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    let authority = crate::color_authority::ColorAuthority {
                        backend: self.id().to_string(),
                        theme: theme_name,
                        palette_file: palette_src.display().to_string(),
                        palette_name: safe_name.clone(),
                    };
                    if let Err(e) = crate::color_authority::write_descriptor(&authority) {
                        tracing::warn!(
                            "[noctalia-v5] Cannot write colour-authority descriptor: {}",
                            e
                        );
                    }
                    custom_restored = Some(safe_name);
                }
            } else if !name.is_empty() && matches!(source, "builtin" | "community") {
                // Builtin or community — no palette file to copy, restore directly.
                noctalia_msg(&["msg", "color-scheme-set", source, name])
                    .map_err(|e| format!("Cannot set color scheme: {}", e))?;
                tracing::info!("[noctalia-v5] Set active scheme: {} {}", source, name);
            } else {
                // Wallpaper scheme (or no named scheme): the theme carries no
                // palette snapshot, so it claims no colour authority — drop a
                // stale descriptor instead of writing one.
                if let Err(e) = crate::color_authority::clear_descriptor() {
                    tracing::warn!(
                        "[noctalia-v5] Cannot clear colour-authority descriptor: {}",
                        e
                    );
                }
            }
            // wallpaper scheme: already handled by wallpaper restoration above

            // 3. Apply templates so rendered files (noctalia-colors.conf, etc.)
            //    reflect the restored palette. color-scheme-set only persists
            //    the setting; templates-apply renders the actual output files.
            noctalia_msg(&["msg", "templates-apply"])
                .map_err(|e| format!("Cannot apply templates: {}", e))?;
            tracing::info!("[noctalia-v5] Templates applied");

            // 4. The keeper's wallpaper engine may own the color scheme (see
            //    `background::engine_owns_color_scheme`): it then regenerates its
            //    wallpaper palette asynchronously AFTER these synchronous
            //    steps and re-imposes `custom skwd-wall`. Only a restored
            //    CUSTOM palette qualifies for the re-assert — builtin,
            //    community and wallpaper schemes keep today's behaviour
            //    untouched. Off the UI thread, single-shot, bounded.
            //
            //    The ownership decision comes from the PRE-yield snapshot:
            //    the flip set theme.policy to "off", so re-evaluating here
            //    would deny the re-assert (the W2 trap). When the authority
            //    was yielded, the worker restores the previous value once
            //    the palette is verified (or at the bounded cap); the guard
            //    is disarmed ONLY when the worker takes ownership, so every
            //    other exit path (early error, no custom palette) still
            //    restores.
            if let Some(restored) = custom_restored {
                let owned = owner_snapshot.unwrap_or_else(background::engine_owns_color_scheme);
                if owned {
                    tracing::info!(
                        "[noctalia-v5] Wallpaper engine owns the color scheme; re-asserting '{}' off-thread",
                        restored
                    );
                    let previous = authority_guard.as_mut().and_then(|guard| guard.disarm());
                    spawn_custom_scheme_reassert(restored, previous);
                }
            }
        }

        Ok(())
    }

    /// Re-assert this backend's colours for an applied theme (unit 1c2 of
    /// `odd/tasks/hve-capability-routing.md`): when the theme carries our
    /// colour snapshot and the live state drifted, put it back — copy +
    /// `color-scheme-set` + `templates-apply`, reusing the apply path's own
    /// steps. Read-only until the decision: anything unowned or already
    /// matching is `Ok(())` with no CLI write at all.
    fn reassert_colours(&self, theme_dir: &Path) -> Result<(), String> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        let snapshot = match fs::read(provider_dir.join("palette.json")) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            _ => return Ok(()),
        };
        let source = match fs::read_to_string(provider_dir.join("source.txt")) {
            Ok(text) => text,
            Err(_) => return Ok(()),
        };
        // Same parse as the apply path: `custom <name>` owns a snapshot,
        // anything else (builtin/community/wallpaper) carries no palette
        // file to put back.
        let mut parts = source.trim().splitn(2, ' ');
        let origin = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("").trim();
        if origin != "custom" || name.is_empty() {
            return Ok(());
        }
        // Same sanitization the apply path applies before the name touches
        // a file name or a CLI argument (`Command` args never see a shell).
        let safe_name = name.replace('/', "_");
        // While an apply holds the engine's colour authority off, the hold
        // is in flight and the re-assert worker owns the palette — a
        // watcher-driven re-assert now would fight it. Same held state the
        // apply path opens and clears, asked of the router; quiet by
        // design, nothing alarming.
        if background::color_authority_held() {
            tracing::debug!(
                "[noctalia-v5] colour authority yielded to the engine; skipping colour re-assert"
            );
            return Ok(());
        }
        let live_path = find_palette_path("custom", &safe_name);
        let live_palette = live_path
            .as_ref()
            .and_then(|path| fs::read(path).ok());
        let live_scheme = noctalia_msg(&["msg", "color-scheme-get"])
            .ok()
            .map(|out| out.trim().to_string());
        if !palette_needs_reassert(
            live_scheme.as_deref(),
            live_palette.as_deref(),
            &snapshot,
            &safe_name,
        ) {
            return Ok(());
        }
        let live_path =
            live_path.ok_or_else(|| "Cannot resolve live palette path".to_string())?;
        if let Some(parent) = live_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create palettes dir: {}", e))?;
        }
        fs::write(&live_path, &snapshot)
            .map_err(|e| format!("Cannot restore palette: {}", e))?;
        noctalia_msg(&["msg", "color-scheme-set", "custom", &safe_name])
            .map_err(|e| format!("Cannot set color scheme: {}", e))?;
        noctalia_msg(&["msg", "templates-apply"])
            .map_err(|e| format!("Cannot apply templates: {}", e))?;
        tracing::info!("[noctalia-v5] Colours re-asserted: custom {}", safe_name);
        Ok(())
    }

    /// Rebuild the colour-authority descriptor from this theme's OWN saved
    /// state (capability-routing R2, the upgrade window: a theme applied
    /// before the descriptor existed otherwise declares no authority until
    /// the next apply). Same parse and same construction as the apply path
    /// above — but purely declarative: nothing is copied into a live
    /// palette, no CLI runs, and `None` comes back whenever the state that
    /// would have to be declared is missing.
    fn derive_colour_authority(
        &self,
        theme_dir: &Path,
    ) -> Option<crate::color_authority::ColorAuthority> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        let palette_src = provider_dir.join("palette.json");
        let raw = fs::read_to_string(provider_dir.join("source.txt")).ok()?;
        let mut parts = raw.trim().splitn(2, ' ');
        let origin = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("").trim();
        // Same rule as the apply path: only a custom palette carries a
        // snapshot we can point at; builtin/community/wallpaper schemes
        // claim no colour authority.
        if origin != "custom" || name.is_empty() {
            return None;
        }
        // An empty snapshot is no snapshot — the same guard
        // `reassert_colours` applies before it would write bytes back.
        match fs::read(&palette_src) {
            Ok(bytes) if !bytes.is_empty() => {}
            _ => return None,
        }
        let theme_name = theme_dir.file_name()?.to_str()?.to_string();
        Some(crate::color_authority::ColorAuthority {
            backend: self.id().to_string(),
            theme: theme_name,
            palette_file: palette_src.display().to_string(),
            // Same sanitization the apply path applies before the name
            // touches a file name or a CLI argument.
            palette_name: name.replace('/', "_"),
        })
    }

    /// The backend contract's `deletable-artefacts` (capability-routing
    /// Phase 3): a custom palette restored outside the theme dir lives in
    /// this backend's own `palettes/` directory and is SHARED by every
    /// theme recording the same name — so on delete the core may remove it
    /// only while no surviving theme still declares it. The provider
    /// supplies its own knowledge only (its record file, its parse, its
    /// live path); the reference decision, the fail-open scan and the
    /// "applied theme keeps its live palette" rule stay the core's.
    /// Nothing declared — and therefore nothing deleted — when the record
    /// is missing or unreadable, the scheme is not `custom`, or the config
    /// dir cannot be resolved.
    fn deletable_artifacts(&self, theme_dir: &Path) -> Vec<DeletableArtifact> {
        let provider_dir = theme_dir.join("providers").join(self.id());
        let Ok(text) = fs::read_to_string(provider_dir.join("source.txt")) else {
            return Vec::new();
        };
        let Some(safe_name) = parse_custom_palette_name(&text) else {
            return Vec::new();
        };
        let Some(config_dir) = noctalia_config_dir() else {
            return Vec::new();
        };
        vec![DeletableArtifact {
            path: config_dir.join("palettes").join(format!("{safe_name}.json")),
            reference_key: safe_name,
        }]
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
    use serial_test::serial;
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

    /// The `deletable-artefacts` declaration (capability-routing Phase 3):
    /// the v5 provider declares the custom palette file it restores OUTSIDE
    /// the theme dir — its own `palettes/` layout, its own record parse.
    /// A missing record or a non-custom scheme declares nothing, so the
    /// core deletes nothing; `/` is sanitized exactly as the apply path
    /// sanitizes it before the name touches a file name.
    #[test]
    fn deletable_artifacts_declares_the_custom_palette_only() {
        let _env = crate::test_utils::TempEnv::new();
        let old_noctalia = std::env::var("HVE_NOCTALIA_CONFIG").ok();
        let noct_home = TempDir::new().unwrap();
        std::env::set_var("HVE_NOCTALIA_CONFIG", noct_home.path());

        let provider = NoctaliaV5Provider::new();
        let theme = TempDir::new().unwrap();
        let prov_dir = theme.path().join("providers").join("noctalia-v5");
        std::fs::create_dir_all(&prov_dir).unwrap();

        // Missing record: nothing declared, therefore nothing deleted.
        assert!(provider.deletable_artifacts(theme.path()).is_empty());

        // Non-custom scheme: the backend owns no palette file for it.
        std::fs::write(prov_dir.join("source.txt"), "builtin Ocean\n").unwrap();
        assert!(provider.deletable_artifacts(theme.path()).is_empty());

        // Custom scheme: the live shared-palette path + the reference key.
        std::fs::write(prov_dir.join("source.txt"), "custom Joker/Theme\n").unwrap();
        assert_eq!(
            provider.deletable_artifacts(theme.path()),
            vec![DeletableArtifact {
                path: noct_home.path().join("palettes").join("Joker_Theme.json"),
                reference_key: "Joker_Theme".to_string(),
            }]
        );

        match old_noctalia {
            Some(v) => std::env::set_var("HVE_NOCTALIA_CONFIG", v),
            None => std::env::remove_var("HVE_NOCTALIA_CONFIG"),
        }
    }

    /// Stale-artifact deletes must not swallow failures: silencing
    /// remove_file hides a read-only dir or I/O error and the save looks
    /// clean while the wrong background record survives. Comment lines are
    /// stripped first so a commented-out delete cannot satisfy this
    /// (block/trailing comments are not stripped). Needles are built with
    /// concat() so this test's own source — it lives in the file it
    /// inspects — can never satisfy them.
    #[test]
    fn stale_artifact_deletes_log_failures_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let silent_delete = ["let _", " = fs::remove", "_file"].concat();
        assert!(
            !code.contains(&silent_delete),
            "stale deletes must log their failure instead of silencing it"
        );
        let logged_wallpaper =
            ["remove_stale_artifact", "(&provider_dir.join(\"wallpaper"].concat();
        let logged_manifest =
            ["remove_stale_artifact", "(&provider_dir.join(crate"].concat();
        let warns = ["Could not remove ", "stale"].concat();
        assert!(
            code.contains(&logged_wallpaper) && code.contains(&logged_manifest),
            "both stale deletes (wallpaper.txt, mpvpaper manifest) must go through the logging helper"
        );
        assert!(
            code.contains(&warns),
            "stale deletes must warn with the path and reason"
        );
    }

    #[test]
    fn test_noctalia_v5_save_creates_provider_dir() {
        // Save requires noctalia running — we only check that creating the
        // provider directory is attempted. TempDir cleans up
        // automatically on drop (no leftover state between runs).
        let provider = NoctaliaV5Provider::new();
        let dir = TempDir::new().unwrap();

        // No assert on result — it can be Ok (noctalia running) or
        // Err (noctalia not available). We only check that it does not panic.
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

    // WHY #[serial]: this test mutates process-global env vars
    // (HVE_NOCTALIA_CONFIG / HVE_NOCTALIA_HYPR) without a TempEnv sandbox,
    // like the stub-harness tests do under #[serial]. Unserialized, a
    // parallel suite-mate mutating the same keys corrupts it — seen once as
    // 883 passed / 1 failed while green alone and under --test-threads=1.
    #[test]
    #[serial]
    fn settings_json_copied_verbatim() {
        let _env = crate::test_utils::env_guard();
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

    /// D1: a failed delegation must not claim the video is back. The apply
    /// path may only treat the painter video as restored on a real success;
    /// otherwise the manifest must survive and the static restore must run.
    /// The hand-off itself moved behind the `apply-background` router, so
    /// the gate is asserted on the routed call. Comment lines are stripped
    /// first so a commented-out gate cannot satisfy this. Needles are
    /// built with concat() so this test's own source — it lives in the
    /// file it inspects — can never satisfy them.
    #[test]
    fn v5_apply_claims_video_only_on_real_delegation_success_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let gated = ["if background::hand_off", "_path(&path)"].concat();
        assert!(
            code.contains(&gated),
            "apply must gate the painter-video success on the routed hand-off result"
        );
        let unconditional = ["hand_off_path(&path);\n", "                        animated_applied = true"].concat();
        assert!(
            !code.contains(&unconditional),
            "apply must not set animated_applied unconditionally after delegating"
        );
    }

    /// D4: the `[plugins] enabled` list in settings.toml is the machine's
    /// plugin state. The keeper's live file has the plugin disabled while
    /// every saved theme carries it enabled — the parser must tell both
    /// apart, including multi-line arrays and commented lines.
    #[test]
    fn settings_toml_parser_detects_mpvpaper_state() {
        let enabled = "[plugins]\nenabled = [ \"kenn/keybind-cheatsheet\", \"noctalia/mpvpaper\", \"yuuto/arch-updater\" ]\n";
        assert!(mpvpaper_enabled_in_settings(enabled));
        let disabled = "[plugins]\nenabled = [ \"yuuto/arch-updater\" ]\n";
        assert!(!mpvpaper_enabled_in_settings(disabled));
        let multiline = "[plugins]\nenabled = [\n  \"kenn/keybind-cheatsheet\",\n  \"noctalia/mpvpaper\",\n]\n";
        assert!(mpvpaper_enabled_in_settings(multiline));
        let commented = "[plugins]\n# enabled = [ \"noctalia/mpvpaper\" ]\nenabled = [ \"yuuto/arch-updater\" ]\n";
        assert!(!mpvpaper_enabled_in_settings(commented));
        let other_section = "[other]\nenabled = [ \"noctalia/mpvpaper\" ]\n[plugins]\nenabled = [ \"yuuto/arch-updater\" ]\n";
        assert!(!mpvpaper_enabled_in_settings(other_section));
        assert!(!mpvpaper_enabled_in_settings(""));
    }

    /// D4: the re-assert mapping is the proof a theme carrying the plugin
    /// enabled cannot leave it enabled — a disabled pre-apply state maps
    /// to disabling it again after the reload.
    #[test]
    fn plugin_reassert_keeps_pre_apply_state() {
        assert_eq!(plugin_reassert_action(Some(false)), PluginReassert::Disable);
        assert_eq!(plugin_reassert_action(Some(true)), PluginReassert::Enable);
        assert_eq!(plugin_reassert_action(None), PluginReassert::Leave);
    }

    /// What the stub answers the `plugins list` probe with.
    enum ProbeAnswer {
        /// The probe prints this line and exits 0.
        Line(&'static str),
        /// The probe succeeds but prints no matching line.
        Empty,
        /// The probe logs the call, then the CLI fails outright.
        Fail,
    }

    /// Hermetic `noctalia msg plugins list` sandbox — the SAME mechanism as
    /// `ColorStub` above and `ProbeSandbox` in `background.rs`: `TempEnv`
    /// takes the shared environment lock and redirects `HOME` (so the live
    /// `settings.toml` the primary signal reads lands inside the
    /// sandbox), the stub bin dir is prepended to `PATH`, and `Drop`
    /// restores the original `PATH` while that lock is still held. The
    /// stub logs ONLY `plugins list` invocations, so a concurrent
    /// suite-mate's unrelated IPC (the shell wallpaper test spawns
    /// `noctalia` without the env lock) cannot pollute the probe log.
    struct PluginProbeStub {
        /// Restored first: `Drop` puts PATH back before any field goes.
        old_path: Option<String>,
        /// Every `msg plugins list` the probe sent, in order.
        log: PathBuf,
        /// Sandbox HOME: the primary signal reads
        /// `<home>/.local/state/noctalia/settings.toml`.
        home: PathBuf,
        /// Lives on PATH until `old_path` is restored above.
        _bin: TempDir,
        /// Shared env lock, released last.
        _env: crate::test_utils::TempEnv,
    }

    impl PluginProbeStub {
        fn new(answer: ProbeAnswer) -> Self {
            let env = crate::test_utils::TempEnv::new();
            let bin = tempfile::tempdir().expect("stub bin dir");
            let log = bin.path().join("probes.log");
            let logging = format!("printf 'msg plugins list\\n' >> '{}'\n", log.display());
            let script = match answer {
                ProbeAnswer::Line(line) => format!(
                    "#!/bin/sh\n\
                     if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"plugins\" ] && [ \"$3\" = \"list\" ]; then\n\
                     {logging}printf '%s' '{line}'\n\
                     fi\nexit 0\n"
                ),
                ProbeAnswer::Empty => format!(
                    "#!/bin/sh\n\
                     if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"plugins\" ] && [ \"$3\" = \"list\" ]; then\n\
                     {logging}fi\nexit 0\n"
                ),
                ProbeAnswer::Fail => format!(
                    "#!/bin/sh\n\
                     if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"plugins\" ] && [ \"$3\" = \"list\" ]; then\n\
                     {logging}echo 'no daemon' >&2\nexit 4\n\
                     fi\nexit 0\n"
                ),
            };
            let stub = bin.path().join("noctalia");
            std::fs::write(&stub, script).expect("stub script");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
                    .expect("stub must be executable");
            }
            let home = PathBuf::from(std::env::var("HOME").expect("TempEnv must set HOME"));
            let old_path = std::env::var("PATH").ok();
            let new_path = match &old_path {
                Some(prev) => format!("{}:{}", bin.path().display(), prev),
                None => bin.path().display().to_string(),
            };
            std::env::set_var("PATH", &new_path);
            Self {
                old_path,
                log,
                home,
                _bin: bin,
                _env: env,
            }
        }

        /// Write the live `settings.toml` under the sandbox HOME so the
        /// primary signal of `live_mpvpaper_plugin_enabled` answers.
        fn write_settings(&self, body: &str) {
            let dir = self.home.join(".local").join("state").join("noctalia");
            std::fs::create_dir_all(&dir).expect("live state dir");
            std::fs::write(dir.join("settings.toml"), body).expect("live settings.toml");
        }

        /// The recorded `msg plugins list` invocations, in order.
        fn probes(&self) -> Vec<String> {
            std::fs::read_to_string(&self.log)
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    impl Drop for PluginProbeStub {
        fn drop(&mut self) {
            match &self.old_path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
        }
    }

    /// D4: with no readable live settings.toml the snapshot falls back to
    /// the `plugins list` IPC — a matching `enabled` line reads
    /// `Some(true)`, a successful answer without one reads `Some(false)`,
    /// and each answers with exactly ONE probe.
    #[test]
    #[serial]
    fn live_snapshot_falls_back_to_the_plugins_list_ipc() {
        let stub = PluginProbeStub::new(ProbeAnswer::Line("noctalia/mpvpaper enabled"));
        assert_eq!(
            live_mpvpaper_plugin_enabled(),
            Some(true),
            "an `enabled` line for the plugin must snapshot as enabled"
        );
        assert_eq!(
            stub.probes(),
            vec!["msg plugins list"],
            "the fallback must send exactly one probe"
        );
        drop(stub);

        let stub = PluginProbeStub::new(ProbeAnswer::Empty);
        assert_eq!(
            live_mpvpaper_plugin_enabled(),
            Some(false),
            "a successful probe without a matching line must snapshot as disabled"
        );
        assert_eq!(stub.probes(), vec!["msg plugins list"]);
    }

    /// D4: a failing IPC is "unknown", never "disabled" — the caller then
    /// leaves the reloaded state alone instead of disabling a plugin it
    /// could not read.
    #[test]
    #[serial]
    fn live_snapshot_is_none_when_the_ipc_fails() {
        let stub = PluginProbeStub::new(ProbeAnswer::Fail);
        assert_eq!(
            live_mpvpaper_plugin_enabled(),
            None,
            "an IPC failure must read as unknown, never as disabled"
        );
        assert_eq!(stub.probes(), vec!["msg plugins list"]);
    }

    /// D4: the live settings.toml is the PRIMARY signal — it wins over an
    /// IPC answer that says the opposite, and it short-circuits the probe
    /// entirely (no IPC at all), even when that IPC would fail.
    #[test]
    #[serial]
    fn live_snapshot_prefers_the_live_settings_toml() {
        let stub = PluginProbeStub::new(ProbeAnswer::Line("noctalia/mpvpaper enabled"));
        stub.write_settings("[plugins]\nenabled = [ \"yuuto/arch-updater\" ]\n");
        assert_eq!(
            live_mpvpaper_plugin_enabled(),
            Some(false),
            "the live settings.toml must win over a contradicting IPC answer"
        );
        assert!(
            stub.probes().is_empty(),
            "a readable settings.toml must not trigger any IPC"
        );
        drop(stub);

        let stub = PluginProbeStub::new(ProbeAnswer::Fail);
        stub.write_settings("[plugins]\nenabled = [ \"noctalia/mpvpaper\" ]\n");
        assert_eq!(
            live_mpvpaper_plugin_enabled(),
            Some(true),
            "the primary signal must win even over a failing IPC"
        );
        assert!(stub.probes().is_empty(), "no IPC while the primary answers");
    }

    /// The duplication this unit breaks: the apply-time snapshot must
    /// reach `plugins list` ONLY through the neutral seam's tri-state
    /// probe — a second parse here would be a second implementation to
    /// keep in sync. Comment lines are stripped first so a commented-out
    /// parse cannot satisfy it. Needles are built with concat() so this
    /// test's own source — it lives in the file it inspects — can never
    /// contain them.
    #[test]
    fn live_snapshot_probes_through_the_seam_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let parse = ["plugins\", \"list"].concat();
        assert!(
            !code.contains(&parse),
            "noctalia.rs must not parse `plugins list` itself — the probe belongs \
             to the neutral seam (found: {parse})"
        );
        let start = code
            .find("fn live_mpvpaper_plugin_enabled(")
            .expect("the snapshot function must exist");
        let brace_at = start + code[start..].find('{').expect("it must have a body");
        let rest = &code[brace_at..];
        let body = &rest[..rest.find("\n}").expect("the body must close")];
        assert!(
            body.contains("plugin_enabled_probe(") && body.contains("MPVPAPER_PLUGIN_ID"),
            "the fallback must call the seam probe with the seam's plugin id; body was:\n{body}"
        );
        assert!(
            body.contains("leaving reloaded state alone"),
            "the unknown-state warning must keep its wording; body was:\n{body}"
        );
    }

    /// D4: applying a theme restores settings.toml verbatim and runs
    /// config-reload, which would re-enable a plugin the user disabled.
    /// Apply must therefore snapshot the pre-apply enabled state BEFORE
    /// overwriting the live settings and re-assert it AFTER the reload —
    /// without touching the rest of the settings restore. Comment lines
    /// are stripped first so a commented-out snapshot cannot satisfy this.
    /// Needles are built with concat() so this test's own source — it
    /// lives in the file it inspects — can never satisfy them.
    #[test]
    fn v5_apply_preserves_mpvpaper_enabled_state_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let snapshot = ["plugin_", "was_enabled"].concat();
        assert!(
            code.contains(&snapshot),
            "apply must snapshot the pre-apply mpvpaper enabled state"
        );
        let snapshot_at = code.find(&snapshot).expect("snapshot must exist");
        let restore_at = code
            .find("Cannot restore settings.toml")
            .expect("apply must still restore settings.toml");
        assert!(
            snapshot_at < restore_at,
            "the snapshot must happen before the live settings are overwritten"
        );
        let reload_at = code
            .find("config-reload")
            .expect("apply must still reload the config");
        let reassert = ["plugins\", \"dis", "able\""].concat();
        let reassert_at = code[reload_at..]
            .find(&reassert)
            .expect("apply must re-assert the plugin state after the reload");
        let _ = reassert_at;
    }

    /// G2/G3: a painter-identified video must be recorded exactly (kind +
    /// path + who painted it) and restored through the manager in charge
    /// (`skwd-helm apply`, reached through the `apply-background` router),
    /// never by bouncing the mpvpaper plugin. Comment lines are stripped
    /// first so a commented-out call cannot satisfy this. Needles are
    /// built with concat() so this test's own source — it lives in the
    /// file it inspects — can never satisfy them.
    #[test]
    fn v5_apply_restores_painter_video_through_skwd_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let video_record = ["vid", "eo.txt"].concat();
        let router = ["background::hand_off", "_path(&path)"].concat();
        assert!(
            code.contains(&video_record),
            "noctalia.rs apply must restore the painter-identified video record"
        );
        assert!(
            code.contains(&router),
            "noctalia.rs apply must route the recorded video through the \
             apply-background router to the skwd engine"
        );
    }

    /// The chain this unit breaks: `noctalia.rs` must reach the skwd
    /// engine and the mpvpaper plugin ONLY through the `apply-background`
    /// router — never by naming their helpers or APIs. The provider's own
    /// knowledge stays (settings parser, colour-authority seams, the
    /// third-party plugin id as data). Comment lines are stripped first
    /// so a commented-out call cannot satisfy this. Needles are built
    /// with concat() so this test's own source — it lives in the file it
    /// inspects — can never contain them.
    #[test]
    fn v5_apply_routes_backgrounds_through_the_router_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            ["delegate_to_", "skwd_walld"].concat(),
            ["run_delegate_", "apply"].concat(),
            ["skwd_wall_", "socket_path"].concat(),
            ["is_video_", "path"].concat(),
            ["providers::", "mpvpaper::"].concat(),
        ] {
            assert!(
                !code.contains(&needle),
                "noctalia.rs must not name `{}` — the hand-off belongs to the \
                 apply-background router (capability routing: a backend never \
                 calls another backend)",
                needle
            );
        }
        let routed_manifest = ["background::apply_", "animated"].concat();
        assert!(
            code.contains(&routed_manifest),
            "noctalia.rs apply must reach the manifest leg through the router"
        );
        let routed_path = ["background::hand_off", "_path("].concat();
        assert!(
            code.contains(&routed_path),
            "noctalia.rs must reach the path hand-off through the router"
        );
    }

    /// The chain THIS unit breaks: the colour-authority yield (W2/W4) —
    /// flipping the engine's `theme.policy` off around one apply and
    /// putting it back — configures the skwd engine's own config file. A
    /// backend never configures another backend, so `noctalia.rs` must
    /// name NO part of that mechanism: not `skwd_policy`, not the config
    /// resolver, not the yield, not the restore, not the marker, not the
    /// guard. The capability is reached through the `apply-background`
    /// router instead. The scan is cut at the trailing test module AND has
    /// comment lines stripped first, so neither this test's own source nor
    /// a commented-out call can satisfy (or fail) an assertion.
    #[test]
    fn v5_apply_reaches_the_colour_authority_yield_through_the_router() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let production: String = src
            .split("mod tests")
            .next()
            .expect("the production body comes first")
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // Old, forbidden coupling: the provider configuring the engine.
        for needle in [
            "skwd_policy",
            "YieldGuard",
            "yield_color_authority",
            "restore_color_authority",
            "marker_path",
        ] {
            assert!(
                !production.contains(needle),
                "noctalia.rs must not name `{}` — the colour-authority yield \
                 configures the skwd engine, and a backend never configures \
                 another backend: hold/release is a capability the \
                 apply-background router owns (found in the production body)",
                needle
            );
        }
        // New, required route: the router owns hold, release, the held
        // query and the ownership read.
        for needle in [
            "background::hold_color_authority(",
            "background::ColourAuthorityHold",
            "background::release_color_authority(",
            "background::color_authority_held()",
            "background::engine_owns_color_scheme()",
        ] {
            assert!(
                production.contains(needle),
                "noctalia.rs must reach the colour-authority capability through \
                 the apply-background router: `{}` is missing from the \
                 production body",
                needle
            );
        }
    }

    // ── skwd color-authority re-assert tests ──
    //
    // WHY these exist: when the keeper's wallpaper engine owns the color
    // scheme (skwd-wall-v2 config: theme.policy == "wallpaper" and/or
    // noctalia.themeMode == "follow"), it regenerates the wallpaper palette
    // asynchronously AFTER HVE's synchronous apply and re-imposes
    // `custom skwd-wall`, silently discarding the theme's custom palette.
    // HVE must detect that ownership and re-assert once, bounded, off the
    // UI thread — never a watcher war.

    const OWNER_JSON: &str =
        r#"{"theme": {"policy": "wallpaper"}, "noctalia": {"themeMode": "follow"}}"#;

    /// Variant WITHOUT `noctalia.themeMode == "follow"`: the only thing
    /// that made ownership true was `theme.policy == "wallpaper"`, so after
    /// the yield flip the config reads `off` and a POST-yield ownership
    /// re-evaluation answers FALSE — the exact W2 trap. The re-assert must
    /// be decided from the PRE-yield snapshot alone, or the palette is
    /// never re-asserted (and the flash returns, just later).
    const OWNER_TRAP_JSON: &str =
        r#"{"theme": {"policy": "wallpaper"}, "noctalia": {"themeMode": "manual"}}"#;

    /// Hermetic harness: a stub `noctalia` shadowed onto PATH plus a
    /// stub skwd-wall config path.
    ///
    /// One deliberate wrinkle: PATH shadowing is process-global, so a
    /// concurrent suite-mate that runs `noctalia msg color-scheme-get`
    /// (today: the v5 save test, which is not `#[serial]`) can slip one
    /// extra get line into this log. Nothing else in the suite issues
    /// `color-scheme-set` or `templates-apply`, so set/template counts
    /// stay EXACT while get counts are asserted as LOWER BOUNDS.
    ///
    /// The stub records every invocation in `<state>/log` and answers
    /// `color-scheme-get` from `<state>/scheme`. `NOCTALIA_YIELD_AFTER`
    /// controls how many `color-scheme-set` calls the keeper daemon
    /// "overrides" (ignores) before a set sticks — the async race, bottled.
    struct ColorStub {
        _env: crate::test_utils::TempEnv,
        _tmp: TempDir,
        state_dir: PathBuf,
        skwd_cfg: PathBuf,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl ColorStub {
        fn new(scheme: &str, yield_after: u32, skwd_config: Option<&str>, fast: bool) -> Self {
            let env = crate::test_utils::TempEnv::new();
            let tmp = TempDir::new().unwrap();
            let keys = [
                "PATH",
                "SKWD_WALL_V2_CONFIG",
                "HVE_NOCTALIA_CONFIG",
                "HVE_REASSERT_PROFILE",
                "NOCTALIA_STUB_STATE",
                "NOCTALIA_YIELD_AFTER",
                "XDG_RUNTIME_DIR",
                "SKWD_WALL_V2_SOCK",
            ];
            let saved: Vec<(&'static str, Option<String>)> = keys
                .iter()
                .map(|k| (*k, std::env::var(k).ok()))
                .collect();
            let state_dir = tmp.path().join("state");
            let bin_dir = tmp.path().join("bin");
            std::fs::create_dir_all(&state_dir).unwrap();
            std::fs::create_dir_all(&bin_dir).unwrap();
            std::fs::write(state_dir.join("scheme"), scheme).unwrap();
            std::fs::write(state_dir.join("log"), b"").unwrap();
            let stub = r#"#!/bin/sh
STATE="${NOCTALIA_STUB_STATE:?missing stub state}"
YIELD_AFTER="${NOCTALIA_YIELD_AFTER:-0}"
if [ "$1" = "msg" ] && [ "$2" = "color-scheme-get" ]; then
  printf 'color-scheme-get\n' >> "$STATE/log"
  cat "$STATE/scheme"
  exit 0
fi
if [ "$1" = "msg" ] && [ "$2" = "color-scheme-set" ]; then
  printf 'color-scheme-set %s %s\n' "$3" "$4" >> "$STATE/log"
  n=$(grep -c '^color-scheme-set' "$STATE/log")
  if [ "$n" -gt "$YIELD_AFTER" ]; then
    printf '%s %s\n' "$3" "$4" > "$STATE/scheme"
  fi
  exit 0
fi
printf '%s\n' "$*" >> "$STATE/log"
exit 0
"#;
            let noctalia = bin_dir.join("noctalia");
            std::fs::write(&noctalia, stub).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&noctalia, std::fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
            let orig_path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var(
                "PATH",
                format!("{}:{}", bin_dir.display(), orig_path),
            );
            std::env::set_var("NOCTALIA_STUB_STATE", &state_dir);
            std::env::set_var("NOCTALIA_YIELD_AFTER", yield_after.to_string());
            let skwd_cfg = tmp.path().join("skwd-config.json");
            if let Some(content) = skwd_config {
                std::fs::write(&skwd_cfg, content).unwrap();
            }
            std::env::set_var("SKWD_WALL_V2_CONFIG", &skwd_cfg);
            std::env::remove_var("HVE_NOCTALIA_CONFIG");
            if fast {
                std::env::set_var("HVE_REASSERT_PROFILE", "fast");
            } else {
                std::env::remove_var("HVE_REASSERT_PROFILE");
            }
            // No daemon socket in tests: delegation stays a silent no-op.
            std::env::set_var("XDG_RUNTIME_DIR", tmp.path());
            std::env::remove_var("SKWD_WALL_V2_SOCK");
            Self {
                _env: env,
                _tmp: tmp,
                state_dir,
                skwd_cfg,
                saved,
            }
        }

        /// Theme carrying (or not) a custom palette; points the palette
        /// restore at a temp noctalia home.
        fn custom_theme(&self, source: &str, with_palette: bool) -> TempDir {
            let theme = TempDir::new().unwrap();
            let dir = theme.path().join("providers").join("noctalia-v5");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("source.txt"), source).unwrap();
            if with_palette {
                std::fs::write(dir.join("palette.json"), r#"{"palette":"joker"}"#).unwrap();
            }
            let noct_home = self._tmp.path().join("noct-home");
            std::fs::create_dir_all(&noct_home).unwrap();
            std::env::set_var("HVE_NOCTALIA_CONFIG", &noct_home);
            theme
        }

        fn count(&self, needle: &str) -> usize {
            // Line shapes in the stub log: `color-scheme-set <src> <name>`
            // (set branch), bare `color-scheme-get` (get branch), and the
            // full argv `msg templates-apply` (generic fall-through branch).
            std::fs::read_to_string(self.state_dir.join("log"))
                .unwrap_or_default()
                .lines()
                .filter(|l| match needle {
                    "color-scheme-set" => l.starts_with("color-scheme-set "),
                    "templates-apply" => l.trim() == "msg templates-apply",
                    other => l.trim() == other,
                })
                .count()
        }

        fn sets(&self) -> usize {
            self.count("color-scheme-set")
        }

        fn gets(&self) -> usize {
            self.count("color-scheme-get")
        }

        fn templates(&self) -> usize {
            self.count("templates-apply")
        }

        fn poll(&self, needle: &str, want: usize, timeout: Duration) -> usize {
            let start = Instant::now();
            loop {
                let n = self.count(needle);
                if n >= want || start.elapsed() >= timeout {
                    return n;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    impl Drop for ColorStub {
        fn drop(&mut self) {
            for (k, v) in &self.saved {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    /// In-memory tracing sink so the cap test can assert the honest
    /// warning without a global subscriber (thread-local default only).
    #[derive(Clone, Default)]
    struct LogCapture {
        buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl std::io::Write for LogCapture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.buf.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
        type Writer = LogCapture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    #[serial]
    fn skwd_config_missing_or_broken_never_owns_scheme() {
        // No file at the configured path: HVE behaves exactly as today.
        let stub = ColorStub::new("custom skwd-wall", 99, None, true);
        assert!(
            !background::engine_owns_color_scheme(),
            "missing keeper config must not count as ownership"
        );
        // Garbage content: never guess ownership from what we cannot read.
        std::fs::write(&stub.skwd_cfg, b"{{{ not json").unwrap();
        assert!(
            !background::engine_owns_color_scheme(),
            "unparsable keeper config must not count as ownership"
        );
    }

    #[test]
    #[serial]
    fn skwd_config_owner_when_policy_wallpaper_or_follow_mode() {
        let stub = ColorStub::new("custom skwd-wall", 99, None, true);
        // Either signal alone makes the engine the color authority.
        std::fs::write(
            &stub.skwd_cfg,
            r#"{"theme": {"policy": "wallpaper"}, "noctalia": {"themeMode": "manual"}}"#,
        )
        .unwrap();
        assert!(
            background::engine_owns_color_scheme(),
            "theme.policy == wallpaper must count as ownership"
        );
        std::fs::write(
            &stub.skwd_cfg,
            r#"{"theme": {"policy": "manual"}, "noctalia": {"themeMode": "follow"}}"#,
        )
        .unwrap();
        assert!(
            background::engine_owns_color_scheme(),
            "noctalia.themeMode == follow must count as ownership"
        );
        std::fs::write(&stub.skwd_cfg, OWNER_JSON).unwrap();
        assert!(background::engine_owns_color_scheme());
        // Neither signal: not an owner, behaviour byte-identical to today.
        std::fs::write(
            &stub.skwd_cfg,
            r#"{"theme": {"policy": "manual"}, "noctalia": {"themeMode": "manual"}}"#,
        )
        .unwrap();
        assert!(
            !background::engine_owns_color_scheme(),
            "no ownership signal must behave exactly as today"
        );
        std::fs::write(&stub.skwd_cfg, r#"{}"#).unwrap();
        assert!(!background::engine_owns_color_scheme());
    }

    #[test]
    #[serial]
    fn custom_scheme_reassert_holds_first_check_without_action() {
        // Scheme already stuck: zero extra commands, just the honest log.
        let stub = ColorStub::new("custom JokerTheme", 0, Some(OWNER_JSON), true);
        let held = reassert_custom_scheme_blocking(
            "JokerTheme",
            Duration::from_millis(0),
            3,
            Duration::from_millis(10),
            Duration::from_secs(2),
        );
        assert!(held, "an already-stuck scheme must report held");
        assert_eq!(stub.sets(), 0, "held on first check must not re-issue set");
        assert!(stub.gets() >= 1, "held on first check needs its get");
    }

    #[test]
    #[serial]
    fn custom_scheme_reasserts_once_then_verifies() {
        // Daemon overrode the synchronous set, the re-assert sticks: exactly
        // one re-assert (one set + one templates-apply), then verified held.
        let stub = ColorStub::new("custom skwd-wall", 0, Some(OWNER_JSON), true);
        let held = reassert_custom_scheme_blocking(
            "JokerTheme",
            Duration::from_millis(0),
            3,
            Duration::from_millis(10),
            Duration::from_secs(2),
        );
        assert!(held, "scheme must be held after one re-assert");
        assert_eq!(stub.sets(), 1, "exactly one re-assert set expected");
        assert!(stub.gets() >= 2, "one check before and one verification after");
        assert_eq!(stub.templates(), 1, "each re-assert re-renders templates");
    }

    #[test]
    #[serial]
    fn custom_scheme_reassert_gives_up_bounded_when_owner_never_yields() {
        // The keeper never yields: attempts stay bounded and the warning
        // names the knob — no war, no silent loss.
        let stub = ColorStub::new("custom skwd-wall", 99, Some(OWNER_JSON), true);
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let start = Instant::now();
        let held = reassert_custom_scheme_blocking(
            "JokerTheme",
            Duration::from_millis(0),
            3,
            Duration::from_millis(20),
            Duration::from_secs(5),
        );
        let elapsed = start.elapsed();
        assert!(!held, "a never-yielding owner must report not-held");
        assert_eq!(stub.sets(), 3, "at most 3 re-assert sets");
        assert!(stub.gets() >= 4, "3 checks plus the final verdict get");
        assert!(
            elapsed < Duration::from_secs(2),
            "re-assert must be bounded, took {:?}",
            elapsed
        );
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("JokerTheme"),
            "warning must name the palette, got: {}",
            text
        );
        assert!(
            text.contains("theme.policy"),
            "warning must name the keeper knob, got: {}",
            text
        );
    }

    #[test]
    #[serial]
    fn v5_apply_custom_owner_reasserts_off_thread_without_blocking() {
        // Production timing: apply must return long before the settle
        // period ends, while the re-assert still lands afterwards.
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_JSON), false);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let start = Instant::now();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        let apply_elapsed = start.elapsed();
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            apply_elapsed < Duration::from_millis(1500),
            "apply must not block on the re-assert, took {:?}",
            apply_elapsed
        );
        // The re-asserted set lands AFTER apply returned (spawned thread).
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        assert_eq!(sets, 2, "one sync set plus exactly one re-assert");
        // `templates-apply` is a SEPARATE subprocess issued right after the set
        // (production: set then templates), so the second one may not have
        // landed when the set line appears. Poll for it, like the single-shot
        // sibling test does, instead of asserting on the instant — under a
        // loaded parallel suite that gap widens and the direct read raced.
        let templates = stub.poll("templates-apply", 2, Duration::from_secs(10));
        assert_eq!(templates, 2, "templates re-rendered per set");
        // Let the final verifying get finish so no thread outlives the test
        // (it would otherwise run against a removed stub PATH).
        std::thread::sleep(Duration::from_millis(2500));
        assert_eq!(stub.sets(), 2, "re-assert held: no further sets");
    }

    #[test]
    #[serial]
    fn v5_apply_custom_owner_reasserts_single_shot_fast() {
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(5));
        assert_eq!(sets, 2, "one sync set plus exactly one re-assert");
        let templates = stub.poll("templates-apply", 2, Duration::from_secs(5));
        assert_eq!(templates, 2, "templates re-rendered per set");
        // Single-shot: after the re-assert holds, nothing else may fire.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(stub.sets(), 2, "no second re-assert: single-shot per apply");
        assert!(stub.gets() >= 2, "one re-assert check plus its verification");
    }

    #[test]
    #[serial]
    fn v5_apply_without_owner_adds_no_scheme_commands() {
        // Custom palette but no keeper ownership: byte-identical to today —
        // exactly the one synchronous set, no spawned follow-up.
        let stub = ColorStub::new("custom skwd-wall", 99, None, true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        // Fast profile would have re-asserted within ms if one were spawned.
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 1, "only the synchronous set, zero extra");
        assert_eq!(stub.templates(), 1, "only the synchronous templates-apply");
    }

    #[test]
    #[serial]
    fn v5_apply_non_custom_palette_ignores_owner() {
        // `wallpaper vibrant` has no saved palette: the owner's policy
        // behaviour is untouched — zero scheme commands even with an owner.
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("wallpaper vibrant", false);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 0, "non-custom palette must issue no set");
        assert_eq!(stub.templates(), 1, "templates-apply still runs once");
    }

    #[test]
    #[serial]
    fn v5_apply_owner_never_yields_gives_up_bounded() {
        // Owner that never yields: 1 sync set + 3 re-asserts, then silence —
        // the give-up that proves this cannot become a war.
        let stub = ColorStub::new("custom skwd-wall", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let sets = stub.poll("color-scheme-set", 4, Duration::from_secs(5));
        assert_eq!(sets, 4, "one sync set plus three bounded re-asserts");
        let templates = stub.poll("templates-apply", 4, Duration::from_secs(5));
        assert_eq!(templates, 4, "templates re-rendered per set");
        // After the cap: no further attempts, ever.
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(stub.sets(), 4, "gave up at the cap: no war");
    }

    // ── W2: yield the engine's colour authority around the apply ──────
    //
    // WHY these exist: before W2, applying a theme showed the engine's
    // auto-derived colours for ~4 s — the keeper's engine owns the colour
    // scheme (`theme.policy == "wallpaper"`), regenerates the wallpaper
    // palette asynchronously AFTER HVE's synchronous apply steps, and
    // re-imposes it. W2 flips the policy to `off` BEFORE the wallpaper
    // hand-off, holds it through the apply, and restores the previous
    // value once the theme palette is verified (or at the bounded cap);
    // any exit path before the worker hand-off must still restore.

    #[test]
    #[serial]
    fn v5_apply_yields_before_handoff_and_restores_after_verification() {
        // (a) The ownership decision is taken BEFORE the flip: the fixture
        // is OWNER_TRAP_JSON (policy wallpaper, NO themeMode follow), so
        // after the flip the config reads `off` and re-evaluating ownership
        // there would answer FALSE — denying the very re-assert that must
        // run (the W2 trap). The wallet of proof: apply returns with the
        // authority still held off, the worker still re-asserts (2 sets),
        // and the value comes back to EXACTLY its original bytes only after
        // the palette is verified.
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_TRAP_JSON), false);
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let held = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert!(
            held.contains("\"policy\": \"off\""),
            "the authority must be held off through the apply, got: {}",
            held
        );
        // The worker verifies the palette (2 s settle), then restores.
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
            if cfg == OWNER_TRAP_JSON {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "colour authority must come back exactly, still: {}",
                cfg
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let final_cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(final_cfg, OWNER_TRAP_JSON, "restored byte-for-byte");
        assert!(
            !background::color_authority_held(),
            "the worker's restore must clear the marker"
        );
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        assert_eq!(sets, 2, "one sync set plus exactly one re-assert");
        // Let the worker's final verifying get and thread teardown finish so
        // no detached thread outlives this test into the next serial one.
        std::thread::sleep(Duration::from_millis(1500));
    }

    #[test]
    #[serial]
    fn v5_apply_early_error_after_yield_still_restores() {
        // (b) Early return: apply fails AFTER the yield (the palette target
        // dir cannot be created) — the guard's Drop must restore and log.
        let stub = ColorStub::new("custom skwd-wall", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        // Break the palette target: a FILE where the palettes dir must be
        // created inside. `noctalia_config_dir()` honours the env var only
        // when the path EXISTS, so the env var must name the file itself —
        // `create_dir_all(<file>/palettes)` then fails deterministically
        // AFTER the wallpaper hand-off.
        let broken = std::env::temp_dir().join(format!("hve-noct-broken-{}", std::process::id()));
        std::fs::write(&broken, b"file, not a dir").unwrap();
        std::env::set_var("HVE_NOCTALIA_CONFIG", &broken);
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _tracing = tracing::subscriber::set_default(subscriber);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_err(), "the broken palette target must fail apply");
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, OWNER_JSON,
            "an early error must still restore the authority: {}",
            cfg
        );
        assert!(
            !background::color_authority_held(),
            "restore clears the marker"
        );
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("yield guard"),
            "the guard's scope-exit restore must be logged, got: {}",
            text
        );
        let _ = std::fs::remove_file(&broken);
    }

    #[test]
    #[serial]
    fn v5_apply_without_custom_palette_restores_synchronously() {
        // (b) No-palette branch: a wallpaper is handed over (yield fires)
        // but there is no custom palette to verify — no worker is spawned,
        // so the guard must restore synchronously at the end of apply.
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("wallpaper vibrant", false);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _tracing = tracing::subscriber::set_default(subscriber);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, OWNER_JSON,
            "the no-palette branch must restore synchronously: {}",
            cfg
        );
        assert!(!background::color_authority_held());
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("yield guard"),
            "the guard's scope-exit restore must be logged, got: {}",
            text
        );
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 0, "non-custom palette must issue no set");
    }

    #[test]
    #[serial]
    fn v5_apply_without_ownership_writes_nothing_and_behaves_as_today() {
        // (c) Missing engine config: no yield, nothing written anywhere,
        // and the scheme commands stay exactly today's.
        let stub = ColorStub::new("custom skwd-wall", 99, None, true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            !stub.skwd_cfg.exists(),
            "no config file must mean no yield and no write"
        );
        assert!(
            !background::color_authority_held(),
            "no yield must leave no marker"
        );
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 1, "only the synchronous set, zero extra");
        assert_eq!(stub.templates(), 1, "only the synchronous templates-apply");
    }

    #[test]
    #[serial]
    fn v5_apply_fixed_policy_writes_nothing_and_behaves_as_today() {
        // (c) `fixed` policy: no ownership, no yield — the engine config
        // bytes must stay exactly as they were, marker absent, re-assert
        // behaviour unchanged (no worker: no spawn without ownership).
        let fixed = r#"{"theme": {"policy": "fixed"}, "noctalia": {"themeMode": "manual"}}"#;
        let stub = ColorStub::new("custom skwd-wall", 99, Some(fixed), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, fixed,
            "a fixed policy must leave the engine config byte-identical: {}",
            cfg
        );
        assert!(!background::color_authority_held());
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 1, "only the synchronous set, zero extra");
        assert_eq!(stub.templates(), 1, "only the synchronous templates-apply");
    }

    // ── W4: the yield covers the animated hand-off too ────────────────
    //
    // WHY these exist: W2 wired the colour-authority yield only into the
    // STATIC-wallpaper branch of apply. A theme's ANIMATED wallpaper
    // (video.txt -> skwd-helm apply) is handed over EARLIER in the same
    // function, before the yield point — so the engine still regenerated
    // its palette and the ~4 s flash remained (measured in the keeper's
    // live log: no yield line before the video hand-off, slow re-assert
    // after). W4 hoists the yield BEFORE the whole wallpaper-restoration
    // phase: one yield per apply, whichever branch hands the wallpaper
    // over first, with the observe wait between the flip and that first
    // hand-off.
    //
    // The ORDER is made observable, not just asserted: a stub `skwd-helm`
    // / `skwd-wall-v2` on PATH (with an existing socket file so the
    // delegation actually runs) records what the engine config said at
    // hand-off time — `delegation saw policy off` when the yield had
    // already flipped it, `delegation saw no yield` otherwise.

    /// Install stub `skwd-helm` / `skwd-wall-v2` binaries that record the
    /// engine-config state at hand-off time into the stub log and exit with
    /// the configured code (0 = the hand-off succeeded, 1 = it failed).
    fn install_delegation_stubs(bin_dir: &Path, exit_code: &str) {
        for bin in ["skwd-helm", "skwd-wall-v2"] {
            let script = format!(
                r#"#!/bin/sh
STATE="${{NOCTALIA_STUB_STATE:?missing stub state}}"
CFG="${{SKWD_WALL_V2_CONFIG:-}}"
if [ "$1" = "apply" ]; then
  printf '%s apply %s\n' "{bin}" "$2" >> "$STATE/log"
  if [ -n "$CFG" ] && [ -f "$CFG" ] && grep -q '"policy": "off"' "$CFG" 2>/dev/null; then
    printf 'delegation saw policy off\n' >> "$STATE/log"
  else
    printf 'delegation saw no yield\n' >> "$STATE/log"
  fi
  exit {exit_code}
fi
printf '%s\n' "$*" >> "$STATE/log"
exit 0
"#,
                bin = bin,
                exit_code = exit_code
            );
            let path = bin_dir.join(bin);
            std::fs::write(&path, script).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    }

    /// Make the delegation path live for a stub harness: an existing socket
    /// file at `SKWD_WALL_V2_SOCK` (the delegation gate) plus stub binaries
    /// on the harness PATH. `exit_code` decides whether the simulated
    /// hand-off succeeds or fails.
    fn enable_delegation(stub: &ColorStub, exit_code: &str) {
        let sock = stub._tmp.path().join("wall.sock");
        std::fs::write(&sock, b"x").unwrap();
        std::env::set_var("SKWD_WALL_V2_SOCK", &sock);
        install_delegation_stubs(&stub._tmp.path().join("bin"), exit_code);
    }

    /// Count stub-log lines STARTING with `prefix` (the existing `count`
    /// helper matches exact lines, but the delegation lines carry paths).
    fn count_prefix(stub: &ColorStub, prefix: &str) -> usize {
        std::fs::read_to_string(stub.state_dir.join("log"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.trim_start().starts_with(prefix))
            .count()
    }

    /// A theme whose provider dir carries an ANIMATED painter record
    /// (`video.txt`) naming a real temp video file, plus a static
    /// wallpaper.txt (the static branch stays available when the animated
    /// hand-off fails).
    fn animated_theme(stub: &ColorStub) -> TempDir {
        let theme = stub.custom_theme("custom JokerTheme", true);
        let video = stub._tmp.path().join("loop.mp4");
        std::fs::write(&video, b"fake video").unwrap();
        let dir = theme.path().join("providers").join("noctalia-v5");
        std::fs::write(
            dir.join("video.txt"),
            format!(
                "path={}\npainter=mpvpaper\nnamespace=dp-3\npid=1234\n",
                video.display()
            ),
        )
        .unwrap();
        std::fs::write(dir.join("wallpaper.txt"), "/tmp/joker3.png").unwrap();
        theme
    }

    #[test]
    #[serial]
    fn v5_apply_animated_yields_before_the_video_hand_off() {
        // (a) ORDERING, not mere co-presence: with the yield hoisted before
        // the whole wallpaper phase, the engine config must already read
        // `off` when the ANIMATED hand-off (`skwd-helm apply <video>`)
        // runs. Before W4 the yield sat inside the static branch — skipped
        // once the video applied — so the delegation probe reported
        // `delegation saw no yield` and the flash stayed.
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_TRAP_JSON), true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            count_prefix(&stub, "skwd-helm apply") >= 1,
            "the video must be handed to the engine"
        );
        assert!(
            count_prefix(&stub, "delegation saw policy off") >= 1,
            "the yield must precede the video hand-off (ordering)"
        );
        assert_eq!(
            count_prefix(&stub, "delegation saw no yield"),
            0,
            "no hand-off may run before the yield"
        );
        // The worker verified the palette, then restored the authority
        // byte-for-byte and cleared the marker.
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
            if cfg == OWNER_TRAP_JSON {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "colour authority must come back exactly, still: {}",
                cfg
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            !background::color_authority_held(),
            "the worker's restore must clear the marker"
        );
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        assert_eq!(sets, 2, "one sync set plus exactly one re-assert");
        std::thread::sleep(Duration::from_millis(500));
    }

    #[test]
    #[serial]
    fn v5_apply_static_yields_before_the_static_hand_off() {
        // (b) A static-only apply: W4 moves the yield earlier, it must not
        // alter the static path's guarantees — the yield still precedes the
        // static hand-off, the re-assert still fires, and the authority
        // comes back byte-for-byte.
        //
        // Engine order (2026-09-26): a STILL image now goes to the fast engine
        // first (`skwd-wall-v2` ~3ms vs `skwd-helm` ~616ms), the other staying
        // as fallback. The guarantee under test is untouched: whichever engine
        // does the hand-off, the yield must already have happened.
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_JSON), true);
        enable_delegation(&stub, "0");
        let theme = stub.custom_theme("custom JokerTheme", true);
        std::fs::write(
            theme.path().join("providers").join("noctalia-v5").join("wallpaper.txt"),
            "/tmp/joker3.png",
        )
        .unwrap();
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            count_prefix(&stub, "skwd-wall-v2 apply") >= 1,
            "a still image must be handed to the fast engine"
        );
        assert_eq!(
            count_prefix(&stub, "delegation saw no yield"),
            0,
            "the yield must precede the static hand-off too"
        );
        assert!(
            count_prefix(&stub, "delegation saw policy off") >= 1,
            "the static hand-off must see the flip"
        );
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
            if cfg == OWNER_JSON {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "colour authority must come back exactly, still: {}",
                cfg
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(!background::color_authority_held());
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        assert_eq!(sets, 2, "re-assert behaviour unchanged");
        std::thread::sleep(Duration::from_millis(500));
    }

    #[test]
    #[serial]
    fn v5_apply_both_branches_single_yield_single_restore() {
        // (c) One apply in which BOTH wallpaper branches run (the animated
        // hand-off fails, so the static restore follows): exactly ONE
        // yield, ONE marker, ONE restore. The yield covers the whole
        // phase, never per-branch. Mid-apply the authority is held off
        // (the video hand-off already saw the flip), the marker exists;
        // after the worker verifies, the value comes back byte-for-byte
        // and the marker is gone.
        let stub = ColorStub::new("custom skwd-wall", 1, Some(OWNER_TRAP_JSON), false);
        enable_delegation(&stub, "1"); // both delegations fail -> static runs too
        let theme = animated_theme(&stub);
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::INFO)
            .finish();
        let _tracing = tracing::subscriber::set_default(subscriber);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert_eq!(
            count_prefix(&stub, "delegation saw no yield"),
            0,
            "the video hand-off attempt must already see the flip"
        );
        // Mid-apply (production timing, worker settle 2 s): held off, marker
        // present — exactly one yield in flight.
        let held = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert!(
            held.contains("\"policy\": \"off\""),
            "the authority must be held off through the apply, got: {}",
            held
        );
        assert!(
            background::color_authority_held(),
            "one active yield -> one marker"
        );
        // The worker verifies, then restores; the guard was disarmed in its
        // favour, so the file and the marker change exactly once.
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
            if cfg == OWNER_TRAP_JSON {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "restore pending, still: {}",
                cfg
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            !background::color_authority_held(),
            "the single restore must clear the marker"
        );
        drop(_tracing);
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert_eq!(
            text.matches("engine colour authority yielded for the apply").count(),
            1,
            "exactly one yield per apply, got: {}",
            text
        );
        let sets = stub.poll("color-scheme-set", 2, Duration::from_secs(10));
        assert_eq!(sets, 2, "one sync set plus exactly one re-assert");
        std::thread::sleep(Duration::from_millis(1500));
    }

    #[test]
    #[serial]
    fn v5_apply_animated_early_error_still_restores() {
        // (d) Early return AFTER an animated-path yield: the video hand-off
        // succeeded, then the palette target cannot be created and apply
        // fails — the guard's Drop must restore and log.
        let stub = ColorStub::new("custom skwd-wall", 99, Some(OWNER_TRAP_JSON), true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        let broken =
            std::env::temp_dir().join(format!("hve-noct-broken-w4-{}", std::process::id()));
        std::fs::write(&broken, b"file, not a dir").unwrap();
        std::env::set_var("HVE_NOCTALIA_CONFIG", &broken);
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _tracing = tracing::subscriber::set_default(subscriber);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_err(), "the broken palette target must fail apply");
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, OWNER_TRAP_JSON,
            "an early error after an animated yield must still restore: {}",
            cfg
        );
        assert!(
            !background::color_authority_held(),
            "restore clears the marker"
        );
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("yield guard"),
            "the guard's scope-exit restore must be logged, got: {}",
            text
        );
        let _ = std::fs::remove_file(&broken);
    }

    #[test]
    #[serial]
    fn v5_apply_animated_no_palette_restores_synchronously() {
        // (d) No-palette branch: an animated hand-off happened (yield
        // fired) but there is no custom palette to verify — no worker is
        // spawned, so the guard must restore synchronously at the end of
        // apply.
        let stub = ColorStub::new("wallpaper vibrant", 99, Some(OWNER_JSON), true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        // Downgrade to a non-custom source with no palette.
        let dir = theme.path().join("providers").join("noctalia-v5");
        std::fs::write(dir.join("source.txt"), "wallpaper vibrant").unwrap();
        std::fs::remove_file(dir.join("palette.json")).unwrap();
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _tracing = tracing::subscriber::set_default(subscriber);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            count_prefix(&stub, "skwd-helm apply") >= 1,
            "the animated hand-off ran (yield fired)"
        );
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, OWNER_JSON,
            "the no-palette branch must restore synchronously: {}",
            cfg
        );
        assert!(!background::color_authority_held());
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("yield guard"),
            "the guard's scope-exit restore must be logged, got: {}",
            text
        );
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 0, "non-custom palette must issue no set");
    }

    #[test]
    #[serial]
    fn v5_apply_animated_without_ownership_writes_nothing() {
        // (e) Animated theme, NO engine config: nothing to yield, nothing
        // written anywhere, marker absent — and the video hand-off still
        // runs exactly as today.
        let stub = ColorStub::new("custom skwd-wall", 99, None, true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        assert!(
            !stub.skwd_cfg.exists(),
            "no config file must mean no yield and no write"
        );
        assert!(
            !background::color_authority_held(),
            "no yield must leave no marker"
        );
        assert!(
            count_prefix(&stub, "skwd-helm apply") >= 1,
            "the animated hand-off runs unchanged"
        );
        assert_eq!(
            count_prefix(&stub, "delegation saw policy off"),
            0,
            "no config -> the flip never happened"
        );
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 1, "only the synchronous set, zero extra");
        assert_eq!(stub.templates(), 1, "only the synchronous templates-apply");
    }

    #[test]
    #[serial]
    fn v5_apply_animated_fixed_policy_writes_nothing() {
        // (e) Animated theme, `fixed` policy: no ownership, no yield — the
        // engine config bytes must stay exactly as they were, marker
        // absent, hand-off behaviour untouched.
        let fixed = r#"{"theme": {"policy": "fixed"}, "noctalia": {"themeMode": "manual"}}"#;
        let stub = ColorStub::new("custom skwd-wall", 99, Some(fixed), true);
        enable_delegation(&stub, "0");
        let theme = animated_theme(&stub);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, fixed,
            "a fixed policy must leave the engine config byte-identical: {}",
            cfg
        );
        assert!(!background::color_authority_held());
        assert_eq!(
            count_prefix(&stub, "delegation saw policy off"),
            0,
            "nothing was ever flipped"
        );
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(stub.sets(), 1, "only the synchronous set, zero extra");
        assert_eq!(stub.templates(), 1, "only the synchronous templates-apply");
    }

    #[test]
    #[serial]
    fn v5_apply_no_wallpaper_assets_never_yields_even_when_owner() {
        // (e) A theme with NO wallpaper to hand over (no video.txt, no
        // wallpaper.txt) must not flip anything, even though the engine
        // owns colours: nothing is handed over, so nothing to yield — the
        // engine config stays byte-identical and the re-assert behaves as
        // today.
        let stub = ColorStub::new("custom skwd-wall", 99, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let res = NoctaliaV5Provider::new().apply(theme.path());
        assert!(res.is_ok(), "apply must succeed: {:?}", res);
        let cfg = std::fs::read_to_string(&stub.skwd_cfg).unwrap();
        assert_eq!(
            cfg, OWNER_JSON,
            "no wallpaper assets -> engine config untouched: {}",
            cfg
        );
        assert!(!background::color_authority_held());
        // The re-assert still runs, exactly as today: the owner never
        // yields, so the worker makes its bounded attempts (1 sync set plus
        // at most 3 re-asserts), never a war.
        let sets = stub.poll("color-scheme-set", 4, Duration::from_secs(5));
        assert_eq!(sets, 4, "owner re-assert keeps its bounded shape");
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(stub.sets(), 4, "gave up at the cap: no war");
    }

    // ── Save-side static background: record what the painter shows ──
    //
    // WHY these exist: the keeper changed colors + background, then
    // overwrote a theme. Save recorded `wallpaper-get` (Noctalia's stale
    // car3.jpg) while the skwd suite actually painted joker1.png, so the
    // gallery previewed the wrong image and a later apply would restore
    // it. The static arm must prefer the painter's own truth when the
    // compositor proves the skwd suite paints a static scene.
    //
    // Hermetic harness: stub `noctalia` / `hyprctl` / `skwd-helm` /
    // `skwd-wall-v2` are shadowed onto PATH. A real spawn would query the
    // keeper's live desktop (and Noctalia IPC can mutate it) — these tests
    // must never run the real binaries.

    /// Stub fleet for the v5 save path: answers `color-scheme-get`,
    /// `wallpaper-get`, `hyprctl -j layers` / `monitors` and the daemon's
    /// `outputs --json` from files under a private state dir.
    struct SaveStub {
        _env: crate::test_utils::TempEnv,
        _tmp: TempDir,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl SaveStub {
        /// `layers` is the `hyprctl -j layers` fixture; `monitors` `None`
        /// means the monitors query fails (the parser then stays
        /// permissive, logging the gap). `daemon_mode` `"ok"` serves
        /// `daemon_json`, `"garbage"` serves unparseable output, anything
        /// else makes both daemon binaries fail.
        fn new(
            layers: &str,
            monitors: Option<&str>,
            daemon_mode: &str,
            daemon_json: &str,
            wallpaper_get: &str,
        ) -> Self {
            let env = crate::test_utils::TempEnv::new();
            let tmp = TempDir::new().unwrap();
            let keys = [
                "PATH",
                "HVE_NOCTALIA_CONFIG",
                "NOCTALIA_STUB_STATE",
                "SKWD_STUB_MODE",
                "XDG_RUNTIME_DIR",
                "SKWD_WALL_V2_SOCK",
                "SKWD_WALL_V2_CONFIG",
            ];
            let saved: Vec<(&'static str, Option<String>)> = keys
                .iter()
                .map(|k| (*k, std::env::var(k).ok()))
                .collect();
            let state_dir = tmp.path().join("state");
            let bin_dir = tmp.path().join("bin");
            std::fs::create_dir_all(&state_dir).unwrap();
            std::fs::create_dir_all(&bin_dir).unwrap();
            std::fs::write(state_dir.join("layers.json"), layers).unwrap();
            if let Some(m) = monitors {
                std::fs::write(state_dir.join("monitors.json"), m).unwrap();
            }
            std::fs::write(state_dir.join("daemon.json"), daemon_json).unwrap();
            std::fs::write(state_dir.join("wallpaper-get"), wallpaper_get).unwrap();
            std::fs::write(state_dir.join("log"), b"").unwrap();
            let noctalia_stub = r#"#!/bin/sh
STATE="${NOCTALIA_STUB_STATE:?missing stub state}"
printf '%s\n' "$*" >> "$STATE/log"
if [ "$1" = "msg" ] && [ "$2" = "color-scheme-get" ]; then
  printf 'custom JokerTheme\n'
  exit 0
fi
if [ "$1" = "msg" ] && [ "$2" = "wallpaper-get" ]; then
  cat "$STATE/wallpaper-get"
  exit 0
fi
exit 0
"#;
            let hyprctl_stub = r#"#!/bin/sh
STATE="${NOCTALIA_STUB_STATE:?missing stub state}"
if [ "$1" = "-j" ] && [ "$2" = "layers" ]; then
  cat "$STATE/layers.json"
  exit 0
fi
if [ "$1" = "-j" ] && [ "$2" = "monitors" ]; then
  if [ -f "$STATE/monitors.json" ]; then
    cat "$STATE/monitors.json"
    exit 0
  fi
  exit 1
fi
exit 1
"#;
            let helm_stub = r#"#!/bin/sh
STATE="${NOCTALIA_STUB_STATE:?missing stub state}"
MODE="${SKWD_STUB_MODE:-ok}"
if [ "$1" = "outputs" ] && [ "$2" = "--json" ]; then
  case "$MODE" in
    ok) cat "$STATE/daemon.json"; exit 0;;
    garbage) printf 'not json at all'; exit 0;;
    *) exit 1;;
  esac
fi
exit 1
"#;
            for (name, content) in [
                ("noctalia", noctalia_stub),
                ("hyprctl", hyprctl_stub),
                ("skwd-helm", helm_stub),
                ("skwd-wall-v2", "#!/bin/sh\nexit 1\n"),
            ] {
                let p = bin_dir.join(name);
                std::fs::write(&p, content).unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))
                        .unwrap();
                }
            }
            let orig_path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var(
                "PATH",
                format!("{}:{}", bin_dir.display(), orig_path),
            );
            std::env::set_var("NOCTALIA_STUB_STATE", &state_dir);
            std::env::set_var("SKWD_STUB_MODE", daemon_mode);
            let noct_home = tmp.path().join("noct-home");
            std::fs::create_dir_all(&noct_home).unwrap();
            std::env::set_var("HVE_NOCTALIA_CONFIG", &noct_home);
            // No daemon socket in tests: delegation stays a silent no-op.
            std::env::set_var("XDG_RUNTIME_DIR", tmp.path());
            std::env::remove_var("SKWD_WALL_V2_SOCK");
            std::env::set_var(
                "SKWD_WALL_V2_CONFIG",
                tmp.path().join("no-skwd-config.json"),
            );
            Self {
                _env: env,
                _tmp: tmp,
                saved,
            }
        }

        fn provider_dir(&self, theme: &TempDir) -> PathBuf {
            theme.path().join("providers").join("noctalia-v5")
        }

        fn wallpaper_txt(&self, theme: &TempDir) -> Option<String> {
            std::fs::read_to_string(self.provider_dir(theme).join("wallpaper.txt"))
                .ok()
                .map(|s| s.trim().to_string())
        }
    }

    impl Drop for SaveStub {
        fn drop(&mut self) {
            for (k, v) in &self.saved {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    /// Two-output layers fixture with the given topmost namespaces,
    /// fullscreen opaque on both outputs.
    fn save_layers_doc(dp_ns: &str, hdmi_ns: &str) -> String {
        format!(
            r#"{{"DP-3":{{"levels":{{"0":[{{"address":"0x1","x":0,"y":0,"w":2560,"h":1440,"alpha":1,"namespace":"{dp_ns}","pid":439101}}],"1":[]}}}},"HDMI-A-1":{{"levels":{{"0":[{{"address":"0x2","x":0,"y":0,"w":1440,"h":900,"alpha":1,"namespace":"{hdmi_ns}","pid":439102}}],"1":[]}}}}}}"#,
        )
    }

    /// Two-output daemon fixture of one kind word with the given currents.
    fn save_daemon_doc(kind_word: &str, dp_current: &str, hdmi_current: &str) -> String {
        format!(
            r#"{{"outputs": [{{"name": "DP-3", "current": "{dp_current}", "path": "{dp_current}", "type": "{kind_word}", "connected": true}},{{"name": "HDMI-A-1", "current": "{hdmi_current}", "path": "{hdmi_current}", "type": "{kind_word}", "connected": true}}]}}"#,
        )
    }

    #[test]
    #[serial]
    fn v5_save_skwd_static_records_daemon_current_not_wallpaper_get() {
        // The keeper's case: Noctalia reports the stale car3.jpg while the
        // suite paints joker1.png on both outputs. Save must record what
        // the painter actually has.
        let stale = "/pictures/stale/car3.jpg";
        let live_path = "/pictures/live/joker1.png";
        let stub = SaveStub::new(
            &save_layers_doc("skwd-paper", "skwd-paper"),
            None,
            "ok",
            &save_daemon_doc("static", live_path, live_path),
            stale,
        );
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert_eq!(
            stub.wallpaper_txt(&theme).as_deref(),
            Some(live_path),
            "skwd paints a static scene: wallpaper.txt must hold the daemon current, not the stale wallpaper-get"
        );
    }

    #[test]
    #[serial]
    fn v5_save_partial_scene_records_proven_output_current() {
        // The keeper's live partial scene: DP-3 has NO attributable
        // background layer (only a small Noctalia widget at level 1, which
        // the monitors reference excludes as furniture) while HDMI-A-1 is
        // skwd-painted fullscreen static. The daemon reports static joker1
        // for both; wallpaper-get is the stale car3. Absence is not
        // disagreement: save must record the proven output's current.
        let live_path = "/pictures/live/joker1.png";
        let stale = "/pictures/stale/car3.jpg";
        let layers = r#"{"DP-3":{"levels":{"0":[],"1":[{"address":"0x1","x":0,"y":0,"w":272,"h":144,"alpha":1,"namespace":"noctalia-desktop-widget-weather-0000000000000001","pid":250289}]}},"HDMI-A-1":{"levels":{"0":[{"address":"0x2","x":0,"y":0,"w":1440,"h":900,"alpha":1,"namespace":"skwd-paper","pid":439102}],"1":[]}}}"#;
        let monitors = r#"[{"name":"DP-3","width":2560,"height":1440,"x":0,"y":0,"scale":1.0},{"name":"HDMI-A-1","width":1440,"height":900,"x":0,"y":0,"scale":1.0}]"#;
        let daemon = save_daemon_doc("static", live_path, live_path);
        let stub = SaveStub::new(layers, Some(monitors), "ok", &daemon, stale);
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert_eq!(
            stub.wallpaper_txt(&theme).as_deref(),
            Some(live_path),
            "partial scene (one output absent, one proven skwd static): wallpaper.txt must hold the daemon current, not the stale wallpaper-get"
        );
    }

    #[test]
    #[serial]
    fn v5_save_skwd_static_vs_video_conflict_keeps_wallpaper_get() {
        // A genuine conflict: HDMI-A-1 proven skwd-painted static but DP-3
        // proven skwd-painted video. The static record must not claim the
        // screen: with the video identity unresolvable (daemon names a file
        // that does not exist) the save vetoes honestly and keeps the
        // wallpaper-get value, writing no video record.
        let stale = "/pictures/stale/car3.jpg";
        let layers = save_layers_doc("skwd-wall-vk", "skwd-paper");
        let daemon = r#"{"outputs": [{"name": "DP-3", "current": "/nonexistent/slugcat.mp4", "path": "/nonexistent/slugcat.mp4", "type": "video", "connected": true},{"name": "HDMI-A-1", "current": "/pictures/live/joker1.png", "path": "/pictures/live/joker1.png", "type": "static", "connected": true}]}"#;
        let stub = SaveStub::new(&layers, None, "ok", daemon, stale);
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert_eq!(
            stub.wallpaper_txt(&theme).as_deref(),
            Some(stale),
            "static-vs-video conflict: wallpaper.txt must keep the wallpaper-get value"
        );
        assert!(
            !stub.provider_dir(&theme).join("video.txt").exists(),
            "unresolvable video: no painter video record may appear"
        );
    }

    /// The static save path must reuse the authority inputs the kind
    /// decision already captured (one bounded snapshot per save), never run
    /// the full compositor + daemon + monitors capture a second time.
    /// Comment lines are stripped first so a commented-out call cannot
    /// satisfy this. Needles are built with concat() so this test's own
    /// source — it lives in the file it inspects — can never satisfy them.
    #[test]
    fn v5_save_shares_one_authority_snapshot_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            code.contains("query_authority_snapshot"),
            "noctalia.rs save must capture the authority inputs once and share them"
        );
        for needle in [
            ["query_active_wall", "paper_kind("].concat(),
            ["query_active_back", "ground("].concat(),
            ["query_active_static", "_path("].concat(),
        ] {
            assert!(
                !code.contains(&needle),
                "save must reuse the snapshot instead of re-querying via {}",
                needle
            );
        }
    }

    #[test]
    #[serial]
    fn v5_save_non_skwd_painter_keeps_wallpaper_get() {
        // An unknown painter owns the layer: the suite has nothing to say,
        // so save keeps today's behaviour byte-for-byte.
        let stale = "/pictures/stale/car3.jpg";
        let stub = SaveStub::new(
            &save_layers_doc("mystery-layer", "mystery-layer"),
            None,
            "ok",
            &save_daemon_doc("static", "/pictures/live/joker1.png", "/pictures/live/joker1.png"),
            stale,
        );
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert_eq!(
            stub.wallpaper_txt(&theme).as_deref(),
            Some(stale),
            "non-skwd painter: wallpaper.txt must keep the wallpaper-get value"
        );
        assert!(
            !stub.provider_dir(&theme).join("video.txt").exists(),
            "no video on top: no painter video record may appear"
        );
    }

    #[test]
    #[serial]
    fn v5_save_skwd_daemon_unusable_falls_back_with_warning() {
        // The suite paints, but its daemon answers garbage: save must fall
        // back to wallpaper-get (never an empty or guessed path) and warn
        // naming the output and the cause.
        let stale = "/pictures/stale/car3.jpg";
        let stub = SaveStub::new(
            &save_layers_doc("skwd-paper", "skwd-paper"),
            None,
            "garbage",
            "",
            stale,
        );
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert_eq!(
            stub.wallpaper_txt(&theme).as_deref(),
            Some(stale),
            "unusable daemon: wallpaper.txt must fall back to the wallpaper-get value"
        );
        let text = String::from_utf8_lossy(&capture.buf.lock().unwrap()).to_string();
        assert!(
            text.contains("DP-3"),
            "the fallback warning must name the output, got: {}",
            text
        );
        assert!(
            text.to_lowercase().contains("daemon"),
            "the fallback warning must name the daemon as the cause, got: {}",
            text
        );
    }

    #[test]
    #[serial]
    fn v5_save_video_on_top_writes_no_static_record() {
        // A skwd native video on top with a resolvable daemon path is an
        // exact video today: video.txt only, no wallpaper.txt. The static
        // fix must not touch the video arm.
        let media_tmp = TempDir::new().unwrap();
        let video = media_tmp.path().join("slugcat.mp4");
        std::fs::write(&video, b"fake video").unwrap();
        let video_str = video.to_string_lossy().to_string();
        let stub = SaveStub::new(
            &save_layers_doc("skwd-wall-vk", "skwd-wall-vk"),
            None,
            "ok",
            &save_daemon_doc("video", &video_str, &video_str),
            "/pictures/stale/car3.jpg",
        );
        let theme = TempDir::new().unwrap();
        NoctaliaV5Provider::new()
            .save(theme.path())
            .expect("save must succeed");
        assert!(
            !stub.provider_dir(&theme).join("wallpaper.txt").exists(),
            "exact video on top: no static record may be written"
        );
        let record = std::fs::read_to_string(stub.provider_dir(&theme).join("video.txt"))
            .expect("exact video must write video.txt");
        assert!(
            record.contains(&video_str),
            "video.txt must hold the resolved video path, got: {}",
            record
        );
    }

    // ── Unit 1c2: pure re-assert decision ──
    //
    // WHY these exist: `reassert_colours` (the `set-colours` refresh of the
    // capability-routing contract) must decide from data alone — no I/O, no
    // `Command` — so the decision is pinned here and the method stays a thin
    // shell around it.

    #[test]
    fn palette_needs_reassert_match_returns_false() {
        let snapshot = br#"{"palette":"blue"}"#;
        assert!(
            !palette_needs_reassert(
                Some("custom theme-blue"),
                Some(snapshot),
                snapshot,
                "theme-blue"
            ),
            "live scheme and bytes both match the snapshot: nothing to do"
        );
    }

    #[test]
    fn palette_needs_reassert_engine_override_returns_true() {
        let snapshot = br#"{"palette":"wall"}"#;
        let live = br#"{"palette":"blue"}"#;
        assert!(
            palette_needs_reassert(Some("custom skwd-wall"), Some(live), snapshot, "theme-blue"),
            "another backend's scheme with different bytes must re-assert"
        );
    }

    #[test]
    fn palette_needs_reassert_same_scheme_changed_bytes_returns_true() {
        let snapshot = br#"{"palette":"blue-v2"}"#;
        let live = br#"{"palette":"blue-v1"}"#;
        assert!(
            palette_needs_reassert(Some("custom theme-blue"), Some(live), snapshot, "theme-blue"),
            "right scheme but drifted bytes must re-assert"
        );
    }

    #[test]
    fn palette_needs_reassert_unreadable_scheme_reasserts() {
        // `None` = noctalia not answering / no scheme readable. Pinned
        // `true`: the re-assert is idempotent (copy + set + templates
        // converge on the snapshot), so an unreadable scheme cannot start a
        // fight; a dead daemon surfaces as `Err` from the CLI step instead.
        let snapshot = br#"{"palette":"blue"}"#;
        assert!(
            palette_needs_reassert(None, Some(snapshot), snapshot, "theme-blue"),
            "an unreadable live scheme must re-assert, never silently keep"
        );
    }

    #[test]
    fn palette_needs_reassert_empty_snapshot_or_name_returns_false() {
        assert!(
            !palette_needs_reassert(Some("custom skwd-wall"), Some(b"x"), b"", "theme-blue"),
            "an empty snapshot owns nothing"
        );
        assert!(
            !palette_needs_reassert(
                Some("custom skwd-wall"),
                Some(br#"{"palette":"blue"}"#),
                br#"{"palette":"blue"}"#,
                ""
            ),
            "an empty name owns nothing"
        );
    }

    #[test]
    fn palette_needs_reassert_trims_scheme_whitespace() {
        // CLI output carries a trailing newline; the caller-facing decision
        // trims it so a matching scheme is not misread as drift.
        let snapshot = br#"{"palette":"blue"}"#;
        assert!(
            !palette_needs_reassert(
                Some("custom theme-blue\n"),
                Some(snapshot),
                snapshot,
                "theme-blue"
            ),
            "a matching scheme with trailing whitespace must not re-assert"
        );
    }

    #[test]
    fn palette_needs_reassert_missing_live_palette_returns_true() {
        // Scheme matches but the live palette file is gone/unreadable: the
        // bytes cannot be proven equal, and the copy recreates the file —
        // a genuine repair, not a fight.
        let snapshot = br#"{"palette":"blue"}"#;
        assert!(
            palette_needs_reassert(Some("custom theme-blue"), None, snapshot, "theme-blue"),
            "a missing live palette must re-assert even when the scheme matches"
        );
    }

    // ── Unit 1c2: `reassert_colours` through the stub harness ──
    //
    // The existing `ColorStub` seam (stub `noctalia` on PATH + invocation
    // log) observes CLI calls: set/template counts stay EXACT because
    // nothing else in the suite issues those commands.

    #[test]
    #[serial]
    fn reassert_colours_without_snapshot_is_noop_without_cli() {
        let stub = ColorStub::new("custom skwd-wall", 0, None, true);
        let theme = TempDir::new().unwrap();
        let dir = theme.path().join("providers").join("noctalia-v5");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("source.txt"), "custom JokerTheme").unwrap();
        let res = NoctaliaV5Provider::new().reassert_colours(theme.path());
        assert!(res.is_ok(), "a missing snapshot must be a quiet Ok: {:?}", res);
        assert_eq!(stub.sets(), 0, "no snapshot must issue no set");
        assert_eq!(stub.templates(), 0, "no snapshot must run no templates-apply");
    }

    #[test]
    #[serial]
    fn reassert_colours_non_custom_source_is_noop_without_cli() {
        let stub = ColorStub::new("wallpaper vibrant", 0, None, true);
        let theme = stub.custom_theme("builtin Kanagawa", true);
        let res = NoctaliaV5Provider::new().reassert_colours(theme.path());
        assert!(res.is_ok(), "a non-custom source must be a quiet Ok: {:?}", res);
        assert_eq!(stub.sets(), 0, "a non-custom source must issue no set");
        assert_eq!(stub.templates(), 0, "a non-custom source must run no templates-apply");
    }

    #[test]
    #[serial]
    fn reassert_colours_matching_live_state_writes_nothing() {
        let stub = ColorStub::new("custom JokerTheme", 0, None, true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let snapshot = std::fs::read(
            theme.path().join("providers").join("noctalia-v5").join("palette.json"),
        )
        .unwrap();
        let live_path = PathBuf::from(std::env::var("HVE_NOCTALIA_CONFIG").unwrap())
            .join("palettes")
            .join("JokerTheme.json");
        std::fs::create_dir_all(live_path.parent().unwrap()).unwrap();
        std::fs::write(&live_path, &snapshot).unwrap();
        let res = NoctaliaV5Provider::new().reassert_colours(theme.path());
        assert!(res.is_ok(), "matching state must be Ok: {:?}", res);
        assert_eq!(stub.sets(), 0, "matching state must issue no set");
        assert_eq!(stub.templates(), 0, "matching state must run no templates-apply");
    }

    #[test]
    #[serial]
    fn reassert_colours_yielded_authority_is_quiet_noop() {
        // A hold in flight — opened through the router exactly like the
        // apply path does, never fabricated: the apply's worker owns the
        // palette — Ok with zero writes even though the live state drifted.
        let stub = ColorStub::new("custom skwd-wall", 0, Some(OWNER_JSON), true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let hold = background::hold_color_authority()
            .expect("hold is an honest Result")
            .expect("the engine owns the scheme, so the hold must open");
        assert!(
            background::color_authority_held(),
            "precondition: a hold is in flight"
        );
        let res = NoctaliaV5Provider::new().reassert_colours(theme.path());
        // Armed: dropping restores the fake engine config and clears the
        // held state, so this test leaves nothing behind.
        drop(hold);
        assert!(res.is_ok(), "a yielded authority must be a quiet Ok: {:?}", res);
        assert_eq!(stub.sets(), 0, "a yield in flight must issue no set");
        assert_eq!(stub.templates(), 0, "a yield in flight must run no templates-apply");
        assert_eq!(
            std::fs::read_to_string(&stub.skwd_cfg).unwrap(),
            OWNER_JSON,
            "the dropped hold must restore the engine config byte-for-byte"
        );
        assert!(
            !background::color_authority_held(),
            "the dropped hold must clear the held state"
        );
    }

    #[test]
    #[serial]
    fn reassert_colours_through_dyn_provider_reasserts_drift() {
        // Reached through `dyn ThemeProvider` — proof this is the override,
        // not the default no-op (the default would write nothing): a drifted
        // live state converges back on the snapshot with one set + one
        // templates-apply.
        let stub = ColorStub::new("custom skwd-wall", 0, None, true);
        let theme = stub.custom_theme("custom JokerTheme", true);
        let live_path = PathBuf::from(std::env::var("HVE_NOCTALIA_CONFIG").unwrap())
            .join("palettes")
            .join("JokerTheme.json");
        std::fs::create_dir_all(live_path.parent().unwrap()).unwrap();
        std::fs::write(&live_path, br#"{"palette":"stale"}"#).unwrap();
        let provider: Box<dyn ThemeProvider> = Box::new(NoctaliaV5Provider::new());
        let res = provider.reassert_colours(theme.path());
        assert!(res.is_ok(), "the re-assert must succeed: {:?}", res);
        assert_eq!(stub.sets(), 1, "one set converges the drifted scheme");
        assert_eq!(stub.templates(), 1, "one templates-apply re-renders");
        let snapshot = std::fs::read(
            theme.path().join("providers").join("noctalia-v5").join("palette.json"),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(&live_path).unwrap(),
            snapshot,
            "the live palette must converge on the snapshot"
        );
        let scheme = std::fs::read_to_string(stub.state_dir.join("scheme")).unwrap();
        assert_eq!(scheme.trim(), "custom JokerTheme");
    }
}
