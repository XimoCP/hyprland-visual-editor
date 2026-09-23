use crate::providers::noctalia_runtime::{noctalia_config_dir, noctalia_msg, noctalia_state_dir};
use crate::providers::wallpaper_authority::{self, SavePlan, WallpaperKind};
use crate::providers::shell::NoctaliaV4Paths;
use crate::providers::shell::ShellProvider;
use crate::theme_manager::{ProviderCapabilities, ThemeProvider};
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

/// Best-effort delegation to skwd-walld (now skwd-wall-v2 / skwd-helm).
///
/// HVE remains agnostic: if the daemon's socket is absent we do nothing.
/// If it is present we try `skwd-helm apply <path>` (and `skwd-wall-v2 apply` as fallback).
///
/// D1: returns whether the wallpaper was actually applied. Only a real
/// success reports success — every other outcome (absent socket, invalid
/// path, failing or unreachable daemon) returns false so the caller keeps
/// the fallback routes (manifest, static restore) instead of claiming a
/// background that was never painted. Failures are logged (warn), never
/// silent and never fatal to the theme apply.
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

fn delegate_to_skwd_walld(wallpaper_path: &Path) -> bool {
    let socket = skwd_wall_socket_path();
    if !socket.exists() {
        tracing::debug!("[skwd-wall] socket absent at {:?} — skipping delegation", socket);
        return false;
    }
    let path_str = match wallpaper_path.to_str() {
        Some(s) if !s.is_empty() => s,
        _ => {
            tracing::warn!("[skwd-wall] invalid wallpaper path {:?}", wallpaper_path);
            return false;
        }
    };
    // Try skwd-helm first, then skwd-wall-v2 as fallback
    for bin in ["skwd-helm", "skwd-wall-v2"] {
        match run_delegate_apply(bin, path_str) {
            Ok(()) => {
                tracing::info!("[skwd-wall] delegated wallpaper to {}: {}", bin, path_str);
                return true;
            }
            Err(reason) => {
                tracing::warn!("[skwd-wall] {} apply failed: {}", bin, reason);
                // try next bin
            }
        }
    }
    tracing::warn!("[skwd-wall] delegation failed for {} (daemon may be unreachable)", path_str);
    false
}

/// Bounded wait for one delegation `apply` call, so a hung daemon helper
/// can never freeze the theme apply (≤500 ms per binary; never held across
/// I/O locks).
const DELEGATE_TIMEOUT: Duration = Duration::from_millis(500);

fn run_delegate_apply(bin: &str, path_str: &str) -> Result<(), String> {
    let mut child = std::process::Command::new(bin)
        .arg("apply")
        .arg(path_str)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("not available: {}", e))?;
    let start = Instant::now();
    loop {
        match child
            .try_wait()
            .map_err(|e| format!("wait failed: {}", e))?
        {
            Some(_) => {
                let out = child
                    .wait_with_output()
                    .map_err(|e| format!("output read failed: {}", e))?;
                if out.status.success() {
                    return Ok(());
                }
                return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
            }
            None => {
                if start.elapsed() >= DELEGATE_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("timed out after {:?}", DELEGATE_TIMEOUT));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
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
    // failure is "unknown", never "disabled".
    match noctalia_msg(&["msg", "plugins", "list"]) {
        Ok(out) => Some(
            out.lines().any(|l| {
                l.trim_start().starts_with("noctalia/mpvpaper")
                    && l.trim_end().ends_with("enabled")
            }),
        ),
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

// ── skwd-wall color-authority detection + single-shot re-assert ─────
//
// WHY this exists: the keeper runs the skwd-wall wallpaper engine as the
// color AUTHORITY (its config sets theme.policy == "wallpaper" and/or
// noctalia.themeMode == "follow"). HVE hands that engine the wallpaper
// during apply, so the engine regenerates its wallpaper-derived palette
// asynchronously AFTER HVE's synchronous steps and re-imposes
// `custom skwd-wall` — silently discarding the theme's custom palette for
// over a minute. HVE's palette copy is correct; it just loses the race
// without noticing. This section detects the ownership and re-asserts the
// theme's palette once, bounded, off the UI thread, with honest logging.

/// Path to the keeper's skwd-wall engine config.
///
/// No path helper existed for this file, so this follows the codebase's
/// established seam style: `SKWD_WALL_V2_CONFIG` overrides outright (tests
/// point it at a temp file, including a nonexistent path for the missing
/// case); otherwise the platform config dir
/// (`$XDG_CONFIG_HOME/skwd-wall-v2/config.json`, i.e. `~/.config/...`),
/// which the `TempEnv` test sandbox already redirects via HOME.
///
/// Single-resolver rule: the path logic lives ONLY in
/// [`crate::providers::skwd_policy::config_path`] (the W1 yield/restore
/// module needs the exact same seam); this accessor delegates to it so a
/// future change cannot fork the two copies.
fn skwd_wall_config_path() -> PathBuf {
    crate::providers::skwd_policy::config_path()
}

/// True when the skwd-wall engine owns the color scheme and will
/// asynchronously override whatever HVE sets: `theme.policy == "wallpaper"`
/// (the engine derives the palette from the wallpaper it was just handed)
/// OR `noctalia.themeMode == "follow"` (it tracks Noctalia instead of
/// leaving the scheme alone). Missing or unparsable config is NOT
/// ownership — HVE then behaves exactly as before. Never panics.
fn skwd_wall_owns_color_scheme() -> bool {
    let raw = match fs::read_to_string(skwd_wall_config_path()) {
        Ok(raw) => raw,
        Err(_) => return false,
    };
    let parsed: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(_) => return false,
    };
    let policy_wallpaper = parsed
        .get("theme")
        .and_then(|theme| theme.get("policy"))
        .and_then(|policy| policy.as_str())
        == Some("wallpaper");
    let follow_mode = parsed
        .get("noctalia")
        .and_then(|noctalia| noctalia.get("themeMode"))
        .and_then(|mode| mode.as_str())
        == Some("follow");
    policy_wallpaper || follow_mode
}

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
fn spawn_custom_scheme_reassert(palette_name: String) {
    std::thread::spawn(move || {
        let (settle, max_sets, delay, cap) = color_reassert_timing();
        reassert_custom_scheme_blocking(&palette_name, settle, max_sets, delay, cap);
    });
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
            match crate::providers::mpvpaper::save_manifest(&provider_dir) {
                Ok(()) => {}
                Err(e) => tracing::warn!("[noctalia-v5] Could not save mpvpaper manifest: {}", e),
            }
        } else {
            // Authority decided exactly (static, or video with a painter
            // record): drop any stale manifest (re-save does not wipe the
            // dir, and a leftover would hijack apply).
            remove_stale_artifact(&provider_dir.join(crate::providers::mpvpaper::MANIFEST_FILE));
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
        if painter_record.exists() {
            let recorded: Option<PathBuf> = fs::read_to_string(&painter_record)
                .ok()
                .and_then(|text| wallpaper_authority::parse_painter_video_record(&text));
            match recorded {
                Some(path) if path.exists() => {
                    if delegate_to_skwd_walld(&path) {
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
            remove_stale_artifact(&provider_dir.join(crate::providers::mpvpaper::MANIFEST_FILE));
        }
        // 1b. Legacy animated wallpaper (mpvpaper plugin manifest).
        //    Only when no exact painter video was restored. Refuses (never
        //    enables) while the plugin is disabled — see mpvpaper.rs.
        let manifest_exists = provider_dir
            .join(crate::providers::mpvpaper::MANIFEST_FILE)
            .exists();
        if !animated_applied {
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
                    custom_restored = Some(safe_name);
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

            // 4. The keeper's wallpaper engine may own the color scheme (see
            //    `skwd_wall_owns_color_scheme`): it then regenerates its
            //    wallpaper palette asynchronously AFTER these synchronous
            //    steps and re-imposes `custom skwd-wall`. Only a restored
            //    CUSTOM palette qualifies for the re-assert — builtin,
            //    community and wallpaper schemes keep today's behaviour
            //    untouched. Off the UI thread, single-shot, bounded.
            if let Some(restored) = custom_restored {
                if skwd_wall_owns_color_scheme() {
                    tracing::info!(
                        "[noctalia-v5] Wallpaper engine owns the color scheme; re-asserting '{}' off-thread",
                        restored
                    );
                    spawn_custom_scheme_reassert(restored);
                }
            }
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

    #[test]
    #[serial]
    fn delegate_noop_when_socket_absent() {
        let _env = crate::test_utils::env_guard();
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
    #[serial]
    fn delegate_swallow_failure_when_socket_is_not_socket() {
        let _env = crate::test_utils::env_guard();
        // Task 2: failure must be swallowed, not propagated. D1: it must
        // also REPORT the failure. Stub binaries on PATH (both fail) keep
        // this hermetic: no real `skwd-helm apply` may run during tests.
        let tmp_runtime = TempDir::new().unwrap();
        let sock_dir = tmp_runtime.path().join("skwd-wall-v2");
        std::fs::create_dir_all(&sock_dir).unwrap();
        let sock_path = sock_dir.join("wall.sock");
        // Create a regular file where socket should be — connect will fail
        std::fs::write(&sock_path, b"not a socket").unwrap();
        let bin_dir = tmp_runtime.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        for bin in ["skwd-helm", "skwd-wall-v2"] {
            let stub = bin_dir.join(bin);
            std::fs::write(&stub, "#!/bin/sh\nexit 1\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        let orig_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("XDG_RUNTIME_DIR", tmp_runtime.path());
        std::env::remove_var("SKWD_WALL_V2_SOCK");
        std::env::set_var(
            "PATH",
            format!("{}:{}", bin_dir.display(), orig_path),
        );
        // Must not panic, must swallow the error — and must report failure
        // so the caller keeps its fallback routes.
        assert!(!delegate_to_skwd_walld(Path::new("/tmp/another.jpg")));
        // restore
        std::env::set_var("PATH", &orig_path);
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

    /// D1: the delegation must report whether it actually applied. An
    /// absent socket (daemon unreachable) is a failed delegation, never a
    /// silent success — the caller gates the fallback routes on this.
    #[test]
    #[serial]
    fn delegate_reports_failure_when_socket_absent() {
        let _env = crate::test_utils::env_guard();
        let tmp_runtime = TempDir::new().unwrap();
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        std::env::set_var("XDG_RUNTIME_DIR", tmp_runtime.path());
        std::env::remove_var("SKWD_WALL_V2_SOCK");
        let applied = delegate_to_skwd_walld(Path::new("/tmp/fake-wallpaper.jpg"));
        assert!(
            !applied,
            "absent socket must report failure so the caller keeps the fallback routes"
        );
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

    /// D1: a failed delegation must not claim the video is back. The apply
    /// path may only treat the painter video as restored on a real success;
    /// otherwise the manifest must survive and the static restore must run.
    /// Comment lines are stripped first so a commented-out gate cannot
    /// satisfy this. Needles are built with concat() so this test's own
    /// source — it lives in the file it inspects — can never satisfy them.
    #[test]
    fn v5_apply_claims_video_only_on_real_delegation_success_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let gated = ["if delegate_to_skwd", "_walld"].concat();
        assert!(
            code.contains(&gated),
            "apply must gate the painter-video success on the delegation result"
        );
        let unconditional = ["delegate_to_skwd_walld(&path);\n", "                    animated_applied = true"].concat();
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
    /// (`skwd-helm apply`, i.e. the existing delegation helper), never by
    /// bouncing the mpvpaper plugin. Comment lines are stripped first so a
    /// commented-out call cannot satisfy this. Needles are built with
    /// concat() so this test's own source — it lives in the file it
    /// inspects — can never satisfy them.
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
        let delegator = ["delegate_to_skwd", "_walld"].concat();
        assert!(
            code.contains(&video_record),
            "noctalia.rs apply must restore the painter-identified video record"
        );
        assert!(
            code.contains(&delegator),
            "noctalia.rs apply must route the recorded video through skwd delegation"
        );
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
    fn skwd_wall_config_path_matches_the_shared_skwd_policy_resolver() {
        // Single-resolver rule: the seam lives in exactly one place
        // (skwd_policy::config_path); this accessor delegates to it. Both
        // seam states must agree: env override set, and env override unset
        // (platform config dir under the TempEnv-redirected HOME).
        use crate::providers::skwd_policy::config_path;
        let _env = crate::test_utils::TempEnv::new();
        let saved = std::env::var("SKWD_WALL_V2_CONFIG").ok();
        std::env::remove_var("SKWD_WALL_V2_CONFIG");
        assert_eq!(
            skwd_wall_config_path(),
            config_path(),
            "both resolvers must agree on the config-dir fallback"
        );
        let override_path = std::env::temp_dir().join("hve-skwd-delegation-check.json");
        std::env::set_var("SKWD_WALL_V2_CONFIG", &override_path);
        assert_eq!(
            skwd_wall_config_path(),
            config_path(),
            "both resolvers must agree on the env override"
        );
        assert_eq!(config_path(), override_path);
        match &saved {
            Some(v) => std::env::set_var("SKWD_WALL_V2_CONFIG", v),
            None => std::env::remove_var("SKWD_WALL_V2_CONFIG"),
        }
    }

    #[test]
    #[serial]
    fn skwd_config_missing_or_broken_never_owns_scheme() {
        // No file at the configured path: HVE behaves exactly as today.
        let stub = ColorStub::new("custom skwd-wall", 99, None, true);
        assert!(
            !skwd_wall_owns_color_scheme(),
            "missing keeper config must not count as ownership"
        );
        // Garbage content: never guess ownership from what we cannot read.
        std::fs::write(&stub.skwd_cfg, b"{{{ not json").unwrap();
        assert!(
            !skwd_wall_owns_color_scheme(),
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
            skwd_wall_owns_color_scheme(),
            "theme.policy == wallpaper must count as ownership"
        );
        std::fs::write(
            &stub.skwd_cfg,
            r#"{"theme": {"policy": "manual"}, "noctalia": {"themeMode": "follow"}}"#,
        )
        .unwrap();
        assert!(
            skwd_wall_owns_color_scheme(),
            "noctalia.themeMode == follow must count as ownership"
        );
        std::fs::write(&stub.skwd_cfg, OWNER_JSON).unwrap();
        assert!(skwd_wall_owns_color_scheme());
        // Neither signal: not an owner, behaviour byte-identical to today.
        std::fs::write(
            &stub.skwd_cfg,
            r#"{"theme": {"policy": "manual"}, "noctalia": {"themeMode": "manual"}}"#,
        )
        .unwrap();
        assert!(
            !skwd_wall_owns_color_scheme(),
            "no ownership signal must behave exactly as today"
        );
        std::fs::write(&stub.skwd_cfg, r#"{}"#).unwrap();
        assert!(!skwd_wall_owns_color_scheme());
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
        assert_eq!(stub.templates(), 2, "templates re-rendered per set");
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
}
