mod callbacks;
mod config;
mod countdown;
mod engine;
mod hypr_ipc;
mod ipc;
mod presets;
mod providers;
mod theme;
mod theme_manager;
mod tr;
mod tray;
mod watcher;

use clap::Parser;
use config::Config;
use engine::Engine;
use fs2::FileExt;
use slint::{Color, ComponentHandle, ModelRc, SharedString, VecModel};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;
use tr::Tr;

slint::include_modules!();

#[derive(Parser)]
#[command(name = "hve", version, about = "Hyprland Visual Editor")]
struct Cli {
    /// Run in tray-only mode (start minimized to system tray)
    #[arg(long)]
    tray: bool,

    /// Enable verbose logging (-v for DEBUG, -vv for TRACE)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

fn project_dir() -> PathBuf {
    // Runtime project root (works for `cargo run` and installed binary)
    // Check: current dir, parent of exe, CARGO_MANIFEST_DIR fallback
    let candidates = [
        std::env::current_dir().ok(),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())),
        std::env::current_exe().ok().and_then(|_| {
            // For NixOS/macOS where the binary is in a store path,
            // try the original source tree
            None
        }),
    ];

    for dir in candidates.iter().flatten() {
        if dir.join("assets").join("scripts").exists() {
            return dir.clone();
        }
    }

    // Last resort: fallback to compiled manifest dir (works in dev builds)
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Create or update the autostart desktop entry at
/// `~/.config/autostart/hve.desktop`.
/// Detect whether HVE is in lua or conf mode by reading the format cache.
fn hve_format() -> &'static str {
    use std::sync::OnceLock;
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE.get_or_init(|| {
        let format_path = hve_cache_dir().join("hve_format");
        match std::fs::read_to_string(&format_path) {
            Ok(content) if content.trim() == "lua" => "lua".to_string(),
            _ => "conf".to_string(),
        }
    })
}

/// HVE cache directory: ~/.cache/hve/
fn hve_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".cache")
        })
        .join("hve")
}

/// Path to the HVE settings file (hve-settings.lua or .conf).
/// This file controls HVE's window rules and keyboard shortcuts.
/// Replaces the old hve-windowrules.{lua,conf} naming.
/// Migrates the old file to the new name on first call if it exists.
fn hve_settings_path() -> PathBuf {
    let ext = if hve_format() == "lua" { "lua" } else { "conf" };
    let new_path = hve_cache_dir().join(format!("hve-settings.{}", ext));
    let old_path = hve_cache_dir().join(format!("hve-windowrules.{}", ext));

    // Migrate old hve-windowrules file to hve-settings if it exists and new one doesn't
    if old_path.exists() && !new_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&old_path) {
            if std::fs::write(&new_path, &content).is_ok() {
                let _ = std::fs::remove_file(&old_path);
                tracing::info!(
                    "[settings] Migrated {} → {}",
                    old_path.display(),
                    new_path.display()
                );
            }
        }
    }

    new_path
}

/// Ensure the settings file exists with a default float rule and empty keybinds section.
/// Called once at startup — does NOT overwrite an existing file.
fn ensure_settings_file() {
    let path = hve_settings_path();
    if path.exists() {
        return;
    }

    let format = hve_format();
    let wr_marker_start = if format == "lua" {
        "-- >>> HVE WINDOW RULES <<<"
    } else {
        "# >>> HVE WINDOW RULES <<<"
    };
    let wr_marker_end = if format == "lua" {
        "-- >>> HVE WINDOW RULES END <<<"
    } else {
        "# >>> HVE WINDOW RULES END <<<"
    };
    let kb_marker_start = if format == "lua" {
        "-- >>> HVE KEYBINDS <<<"
    } else {
        "# >>> HVE KEYBINDS <<<"
    };
    let kb_marker_end = if format == "lua" {
        "-- >>> HVE KEYBINDS END <<<"
    } else {
        "# >>> HVE KEYBINDS END <<<"
    };
    let as_marker_start = if format == "lua" {
        "-- >>> HVE AUTOSTART <<<"
    } else {
        "# >>> HVE AUTOSTART <<<"
    };
    let as_marker_end = if format == "lua" {
        "-- >>> HVE AUTOSTART END <<<"
    } else {
        "# >>> HVE AUTOSTART END <<<"
    };
    let _ = std::fs::create_dir_all(hve_cache_dir());

    let content = if format == "lua" {
        format!(
            r#"{wr_marker_start}
hl.window_rule({{
  name  = "hve-floating",
  match = {{ title = "^Hyprland Visual Editor$" }},
  float = true,
  size  = {{ "95%", "95%" }},
  move  = {{ "center", "center" }},
}})
{wr_marker_end}
{kb_marker_start}
{kb_marker_end}
{as_marker_start}
{as_marker_end}
"#,
        )
    } else {
        format!(
            r#"{wr_marker_start}
windowrulev2 = float, title:^(Hyprland Visual Editor)$
windowrulev2 = center, title:^(Hyprland Visual Editor)$
windowrulev2 = size 95% 95%, title:^(Hyprland Visual Editor)$
{wr_marker_end}
{kb_marker_start}
{kb_marker_end}
{as_marker_start}
{as_marker_end}
"#,
        )
    };

    match std::fs::write(&path, content) {
        Ok(_) => tracing::info!("[settings] Created at {}", path.display()),
        Err(e) => tracing::error!("[settings] Failed to create: {}", e),
    }
}

/// Toggle HVE window rules between float (default) and tile.
///
/// Operates on hve-settings.lua/.conf — a dedicated file separate from
/// the overlay (which is managed by assemble.sh). This file is loaded AFTER
/// the user's windowrules.lua so its rule wins.
///
/// When `tiling` is false (default): `float = true` → window floats
/// When `tiling` is true:           `tile  = true` → window tiles
fn set_tiling_window_rules(tiling: bool) {
    let path = hve_settings_path();
    if !path.exists() {
        tracing::warn!("[windowrules] File not found — creating default");
        ensure_settings_file();
    }

    let format = hve_format();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[windowrules] Failed to read: {}", e);
            return;
        }
    };

    let marker_start = if format == "lua" {
        "-- >>> HVE WINDOW RULES <<<"
    } else {
        "# >>> HVE WINDOW RULES <<<"
    };
    let marker_end = if format == "lua" {
        "-- >>> HVE WINDOW RULES END <<<"
    } else {
        "# >>> HVE WINDOW RULES END <<<"
    };

    // Build the replacement block
    let rules_block: String = if tiling {
        // Tiling ON → tile rule (overrides float from user's windowrules.lua)
        if format == "lua" {
            format!(
                r#"{marker_start}
hl.window_rule({{
  name  = "hve-floating",
  match = {{ title = "^Hyprland Visual Editor$" }},
  tile  = true,
}})
{marker_end}"#,
            )
        } else {
            format!(
                r#"{marker_start}
windowrulev2 = tile, title:^(Hyprland Visual Editor)$
{marker_end}"#,
            )
        }
    } else {
        // Tiling OFF → float rule (default)
        if format == "lua" {
            format!(
                r#"{marker_start}
hl.window_rule({{
  name  = "hve-floating",
  match = {{ title = "^Hyprland Visual Editor$" }},
  float = true,
  size  = {{ "95%", "95%" }},
  move  = {{ "center", "center" }},
}})
{marker_end}"#,
            )
        } else {
            format!(
                r#"{marker_start}
windowrulev2 = float, title:^(Hyprland Visual Editor)$
windowrulev2 = center, title:^(Hyprland Visual Editor)$
windowrulev2 = size 95% 95%, title:^(Hyprland Visual Editor)$
{marker_end}"#,
            )
        }
    };

    // Replace content between markers
    let new_content = if content.contains(marker_start) {
        let mut result = String::new();
        let mut in_block = false;
        let mut replaced = false;
        for line in content.lines() {
            if line.trim() == marker_start {
                in_block = true;
                if !replaced {
                    result.push_str(&rules_block);
                    result.push('\n');
                    replaced = true;
                }
                continue;
            }
            if line.trim() == marker_end {
                in_block = false;
                continue;
            }
            if !in_block {
                result.push_str(line);
                result.push('\n');
            }
        }
        result
    } else {
        // No markers yet → append the block
        let mut result = content;
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(&rules_block);
        result.push('\n');
        result
    };

    match std::fs::write(&path, new_content) {
        Ok(_) => {
            tracing::info!(
                "[windowrules] {} mode applied (tile={})",
                if tiling { "TILING" } else { "FLOATING" },
                tiling,
            );
            if let Err(e) = std::process::Command::new("hyprctl")
                .arg("reload")
                .output()
                .map(|_| ())
            {
                tracing::warn!("[hve] hyprctl reload failed: {}", e);
            }
        }
        Err(e) => {
            tracing::error!("[windowrules] Failed to write: {}", e);
        }
    }
}

/// Write or remove the 5 HVE keybinds between `>>> HVE KEYBINDS <<<` markers
/// in the hve-settings file. Supports both Lua and conf formats.
///
/// When `enabled` is true: writes the 5 IPC keybind entries between markers.
/// When `enabled` is false: removes the markers and their content entirely.
/// Calls `hyprctl reload` after a successful write.
fn set_keybinds(enabled: bool) {
    let path = hve_settings_path();
    if !path.exists() {
        tracing::warn!("[keybinds] File not found — creating default");
        ensure_settings_file();
    }

    let format = hve_format();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[keybinds] Failed to read: {}", e);
            return;
        }
    };

    let marker_start = if format == "lua" {
        "-- >>> HVE KEYBINDS <<<"
    } else {
        "# >>> HVE KEYBINDS <<<"
    };
    let marker_end = if format == "lua" {
        "-- >>> HVE KEYBINDS END <<<"
    } else {
        "# >>> HVE KEYBINDS END <<<"
    };

    // Build the replacement block when enabled
    let keybinds_block: String = if enabled {
        if format == "lua" {
            format!(
                r#"{marker_start}
hl.bind("SUPER + H", hl.dsp.exec_cmd("hve-ipc toggle-tray"))
hl.bind("SUPER + ALT + Q", hl.dsp.exec_cmd("hve-ipc pause-restart"))
hl.bind("SUPER + ALT + N", hl.dsp.exec_cmd("hve-ipc next-anim"))
hl.bind("SUPER + ALT + B", hl.dsp.exec_cmd("hve-ipc next-border"))
hl.bind("SUPER + ALT + S", hl.dsp.exec_cmd("hve-ipc next-shader"))
{marker_end}"#,
            )
        } else {
            format!(
                r#"{marker_start}
bind = SUPER, H, exec, hve-ipc toggle-tray
bind = SUPER ALT, Q, exec, hve-ipc pause-restart
bind = SUPER ALT, N, exec, hve-ipc next-anim
bind = SUPER ALT, B, exec, hve-ipc next-border
bind = SUPER ALT, S, exec, hve-ipc next-shader
{marker_end}"#,
            )
        }
    } else {
        String::new()
    };

    // Replace or remove content between markers
    let new_content = if content.contains(marker_start) {
        let mut result = String::new();
        let mut in_block = false;
        for line in content.lines() {
            if line.trim() == marker_start {
                in_block = true;
                if enabled {
                    result.push_str(&keybinds_block);
                    result.push('\n');
                }
                continue;
            }
            if line.trim() == marker_end {
                in_block = false;
                continue;
            }
            if !in_block {
                result.push_str(line);
                result.push('\n');
            }
        }
        result
    } else if enabled {
        // No markers yet → append the block
        let mut result = content;
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(&keybinds_block);
        result.push('\n');
        result
    } else {
        // Already no markers and disabled → nothing to do
        content
    };

    match std::fs::write(&path, new_content) {
        Ok(_) => {
            tracing::info!(
                "[keybinds] {}",
                if enabled { "WRITTEN" } else { "REMOVED" }
            );
            if let Err(e) = std::process::Command::new("hyprctl")
                .arg("reload")
                .output()
                .map(|_| ())
            {
                tracing::warn!("[hve] hyprctl reload failed: {}", e);
            }
        }
        Err(e) => {
            tracing::error!("[keybinds] Failed to write: {}", e);
        }
    }
}

/// Toggle HVE autostart in hve-settings.lua.
///
/// When enabled, writes `hl.on("hyprland.start", ...)` with `hl.exec_cmd("hve --tray")`
/// between markers. When disabled, removes the block entirely (markers stay in the
/// template for next enable).
///
/// This avoids touching the user's exec.lua — everything lives in our managed file,
/// which is dofile'd from hyprland.lua. Clean uninstall: delete hve-settings.lua
/// and remove the dofile line from hyprland.lua.
fn set_autostart(enabled: bool) {
    let path = hve_settings_path();
    if !path.exists() {
        tracing::warn!("[autostart] hve-settings not found — creating default");
        ensure_settings_file();
    }

    let format = hve_format();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[autostart] Failed to read hve-settings: {}", e);
            return;
        }
    };

    let marker_start = if format == "lua" {
        "-- >>> HVE AUTOSTART <<<"
    } else {
        "# >>> HVE AUTOSTART <<<"
    };
    let marker_end = if format == "lua" {
        "-- >>> HVE AUTOSTART END <<<"
    } else {
        "# >>> HVE AUTOSTART END <<<"
    };

    let exe = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("hve"))
        .display()
        .to_string();

    let autostart_block: String = if enabled {
        if format == "lua" {
            format!(
                r#"{marker_start}
hl.on("hyprland.start", function()
    hl.exec_cmd("{} --tray")
end)
{marker_end}"#,
                exe
            )
        } else {
            format!(
                r#"{marker_start}
exec-once = {} --tray
{marker_end}"#,
                exe
            )
        }
    } else {
        String::new()
    };

    // Replace or remove content between markers
    let new_content = if content.contains(marker_start) {
        let mut result = String::new();
        let mut in_block = false;
        for line in content.lines() {
            if line.trim() == marker_start {
                in_block = true;
                if enabled {
                    result.push_str(&autostart_block);
                    result.push('\n');
                }
                continue;
            }
            if line.trim() == marker_end {
                in_block = false;
                continue;
            }
            if !in_block {
                result.push_str(line);
                result.push('\n');
            }
        }
        result
    } else if enabled {
        // No markers yet → append the block
        let mut result = content;
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(&autostart_block);
        result.push('\n');
        result
    } else {
        // Disabled with no markers → nothing to do
        content
    };

    match std::fs::write(&path, new_content) {
        Ok(_) => {
            tracing::info!(
                "[autostart] {} in hve-settings.lua",
                if enabled { "Enabled" } else { "Disabled" }
            );
            if let Err(e) = std::process::Command::new("hyprctl")
                .arg("reload")
                .output()
                .map(|_| ())
            {
                tracing::warn!("[hve] hyprctl reload failed: {}", e);
            }
        }
        Err(e) => tracing::error!("[autostart] Failed to write hve-settings: {}", e),
    }
}

/// Refresh the Slint theme list UI from the ThemeManager state.
fn refresh_theme_list(
    window: &crate::MainWindow,
    tm: &crate::theme_manager::ThemeManager,
) {
    use slint::{ModelRc, SharedString};
    let themes = tm.list().unwrap_or_default();

    let names: Vec<SharedString> = themes.iter().map(|t| SharedString::from(&t.name)).collect();
    let saved_ats: Vec<SharedString> = themes.iter().map(|t| SharedString::from(&t.saved_at)).collect();
    let is_actives: Vec<bool> = themes.iter().map(|t| t.is_active).collect();

    window.set_theme_names(ModelRc::from(names.as_slice()));
    window.set_theme_saved_ats(ModelRc::from(saved_ats.as_slice()));
    window.set_theme_is_actives(ModelRc::from(is_actives.as_slice()));

    // Update active theme index
    let active_idx = themes.iter().position(|t| t.is_active).map(|i| i as i32).unwrap_or(-1);
    window.set_active_theme_index(active_idx);
}

/// After applying a theme, sync the GUI preset indices (anim, border, shader, border-size)
/// so they reflect what the theme restored, not the stale values from before apply.
fn sync_preset_indices(
    window: &crate::MainWindow,
    cfg: &Config,
) {
    use slint::Model;

    let find = |files: &slint::ModelRc<slint::SharedString>, target: &str| -> i32 {
        if target.is_empty() {
            return -1;
        }
        for i in 0..files.row_count() {
            if files.row_data(i).as_ref().map(|s| s.as_str()) == Some(target) {
                return i as i32;
            }
        }
        -1
    };

    window.set_active_anim_index(find(&window.get_anim_files(), &cfg.active_anim_file));
    window.set_active_border_index(find(&window.get_border_files(), &cfg.active_border_file));
    window.set_active_shader_index(find(&window.get_shader_files(), &cfg.active_shader_file));
    window.set_border_size(cfg.border_size);
}

fn main() -> Result<(), slint::PlatformError> {
    let cli = Cli::parse();

    // ── Structured logging ──
    let filter = match cli.verbose {
        0 => EnvFilter::new("info"),
        1 => EnvFilter::new("debug"),
        _ => EnvFilter::new("trace"),
    }
    .add_directive("calloop=off".parse().unwrap());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    // ── Multi-instance protection ──
    // Use Arc<Mutex<Option>> so restart callback can release the lock before spawning
    let lock_dir = dirs::cache_dir()
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".cache")
        })
        .join("hve");
    let _ = std::fs::create_dir_all(&lock_dir);
    let lock_file = std::fs::File::create(lock_dir.join("hve.lock"))
        .unwrap_or_else(|e| {
            tracing::error!("Failed to create lock file: {}", e);
            std::process::exit(1);
        });
    if let Err(e) = lock_file.try_lock_exclusive() {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            tracing::error!("Another instance of HVE is already running.");
        } else {
            tracing::error!("Failed to acquire exclusive lock: {}", e);
        }
        std::process::exit(1);
    }
    let lock = Arc::new(std::sync::Mutex::new(Some(lock_file)));

    let tray_mode = cli.tray;

    let proj = project_dir();
    let engine = Engine::new(&proj);
    let cfg = Config::load();
    // Use saved language, or auto-detect from system locale
    let tr_lang = if cfg.language.is_empty() {
        tr::detect_language()
    } else {
        cfg.language.clone()
    };
    let tr = Arc::new(Tr::with_lang(&tr_lang));

    tracing::info!("Locale: {}", tr.lang);

    // ── Ensure hve-settings exists with default window rules and keybinds section ──
    ensure_settings_file();

    let window = MainWindow::new()?;

    // ── Load initial state ──
    window.set_system_active(cfg.is_system_active);
    window.set_border_size(cfg.border_size);
    window.set_auto_minimize(cfg.auto_minimize_enabled);
    window.set_minimize_seconds(cfg.minimize_seconds);
    window.set_language(tr_lang.clone().into());
    window.set_tiling_mode(cfg.tiling_mode);
    window.set_autostart(cfg.auto_start);
    window.set_theme(cfg.theme.clone().into());
    window.set_restart_required(false);

    // ── Scan + translate presets ──
    presets::populate_presets(&window, &engine, &cfg, &tr);

    // ── Theme Manager init ──
    let config_dir = dirs::config_dir()
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp/hve-config"));
    let mut theme_manager = crate::theme_manager::ThemeManager::new(&config_dir);
    theme_manager.register_provider(Box::new(crate::providers::noctalia::NoctaliaProvider));
    theme_manager.register_provider(Box::new(crate::providers::hve_presets::HvePresetsProvider::new(engine.clone())));
    // Restore last applied theme from config
    if !cfg!(test) {
        theme_manager.last_applied = cfg.last_applied_theme.clone();
    }
    let theme_manager = Arc::new(std::sync::Mutex::new(theme_manager));

    // Refresh UI theme list
    {
        let tm = theme_manager.lock().unwrap();
        refresh_theme_list(&window, &tm);
    }

    // ── i18n: static UI strings ──
    window.set_sidebar_subtitle(tr.tr_shared("panel.header_title", "Hyprland Visual Editor"));
    window.set_home_header_title(tr.tr_shared("panel.tabs.home", "Home"));
    window.set_home_header_subtitle(tr.tr_shared("panel.header_subtitle", "Aesthetic Control Center"));
    window.set_activation_title(tr.tr_shared("welcome.activation_title", "System Activation"));
    window.set_activation_active_text(tr.tr_shared("welcome.toast.enabled", "Visual Editor Enabled"));
    window.set_activation_inactive_text(tr.tr_shared("welcome.toast.disabled", "Visual Editor Disabled"));
    window.set_feature_title(tr.tr_shared("welcome.features.title", "Features & Benefits"));
    window.set_feature_desc(tr.tr_shared("welcome.features.description", "Fluid animations • Smart borders • Real-time shaders • Non-destructive"));
    window.set_how_title(tr.tr_shared("welcome.docs.title", "Architecture & Documentation"));
    window.set_how_desc(tr.tr_shared("welcome.docs.summary", "HVE uses a Fragments & Assembly system..."));
    window.set_anim_header_title(tr.tr_shared("animations.header_title", "Motion Library"));
    window.set_anim_header_subtitle(tr.tr_shared("animations.header_subtitle", "Select the animation style for your desktop"));
    window.set_border_header_title(tr.tr_shared("borders.header_title", "Visual Styles"));
    window.set_border_header_subtitle(tr.tr_shared("borders.header_subtitle", "Define your windows' personality"));
    window.set_border_geometry_label(tr.tr_shared("borders.geometry.title", "Border Thickness"));
    window.set_shader_header_title(tr.tr_shared("shaders.header_title", "Screen Filters"));
    window.set_shader_header_subtitle(tr.tr_shared("shaders.header_subtitle", "Real-time image post-processing"));

    // ── i18n: Home module strings ──
    window.set_home_status_active(tr.tr_shared("home.status_active", "System Active"));
    window.set_home_status_inactive(tr.tr_shared("home.status_inactive", "System Stopped"));
    window.set_home_nav_animations(tr.tr_shared("home.nav_animations", "Animations"));
    window.set_home_nav_borders(tr.tr_shared("home.nav_borders", "Borders"));
    window.set_home_nav_shaders(tr.tr_shared("home.nav_shaders", "Shaders"));
    window.set_home_count_styles(tr.tr_shared("home.count_styles", " styles"));
    window.set_home_count_filters(tr.tr_shared("home.count_filters", " filters"));
    window.set_home_active_config_title(tr.tr_shared("home.active_config", "Active Configuration"));
    window.set_home_active_anim_label(tr.tr_shared("home.active_anim", "Animation:"));
    window.set_home_active_border_label(tr.tr_shared("home.active_border", "Border:"));
    window.set_home_active_shader_label(tr.tr_shared("home.active_shader", "Shader:"));
    window.set_home_none_anim(tr.tr_shared("home.none_anim", "None"));
    window.set_home_none_border(tr.tr_shared("home.none_border", "None"));
    window.set_home_none_shader(tr.tr_shared("home.none_shader", "None"));
    window.set_home_active_theme_label(tr.tr_shared("home.active_theme", "Theme:"));
    window.set_home_none_theme(tr.tr_shared("home.none_theme", "None"));
    window.set_home_active_theme_name(cfg.last_applied_theme.clone().into());
    window.set_home_about_title(tr.tr_shared("home.about_title", "About HVE"));
    window.set_home_about_short(tr.tr_shared("home.about_short", "Hyprland Visual Editor makes your desktop truly yours."));
    window.set_home_about_full(tr.tr_shared("home.about_full", "HVE customizes Hyprland animations, borders, and shaders from a visual interface, in real time.\n\nTo apply changes, HVE adds a block at the end of your hyprland.lua (or hyprland.conf), delimited by markers. That block only includes (dofile / source) our overlay files in ~/.cache/hve/ — it never rewrites your personal config.\n\nWhen you disable the system, the markers are removed. If you uninstall, a watchdog cleans them on the next Hyprland start. Your original config always stays intact."));
    let about_items: Vec<AboutItem> = vec![
        AboutItem {
            text: tr.tr_shared("home.about_line_1", "Live preview — tweak and see it instantly").into(),
            color: Color::from_rgb_u8(251, 191, 36),
        },
        AboutItem {
            text: tr.tr_shared("home.about_line_2", "Safe assembly — zero risk to your configs").into(),
            color: Color::from_rgb_u8(16, 185, 129),
        },
        AboutItem {
            text: tr.tr_shared("home.about_line_3", "Hot reload — apply without restarting Hyprland").into(),
            color: Color::from_rgb_u8(56, 189, 248),
        },
        AboutItem {
            text: tr.tr_shared("home.about_line_4", "Fragments — composable pieces, not one huge file").into(),
            color: Color::from_rgb_u8(192, 132, 252),
        },
    ];
    window.set_home_about_items(ModelRc::new(VecModel::from(about_items)));

    // ── i18n: Settings strings ──
    window.set_settings_restart_banner(tr.tr_shared("settings.restart_banner", "⚠ Restart required"));
    window.set_settings_restart_button(tr.tr_shared("settings.restart_button", "Restart"));
    window.set_settings_title(tr.tr_shared("settings.title", "Settings"));
    window.set_auto_minimize_label(tr.tr_shared("settings.auto_minimize", "Auto-minimize"));
    window.set_timer_label(tr.tr_shared("settings.timer", "Timer:"));
    window.set_language_label(tr.tr_shared("settings.language", "Language"));
    window.set_tiling_label(tr.tr_shared("settings.tiling_mode", "Tiling mode"));
    window.set_autostart_label(tr.tr_shared("settings.autostart", "Autostart"));
    window.set_theme_label(tr.tr_shared("settings.theme", "Theme"));
    window.set_reset_label(tr.tr_shared("settings.reset_presets", "Reset presets"));
    window.set_keybinds_label(tr.tr_shared("settings.keybinds", "Keyboard shortcuts"));
    window.set_keybinds_mode(cfg.keybinds_enabled);

    // ── i18n: Theme strings ──
    window.set_theme_header_title(tr.tr_shared("themes.header_title", "Theme Manager"));
    window.set_theme_header_subtitle(tr.tr_shared("themes.header_subtitle", "Save and apply full desktop themes"));
    window.set_theme_save_placeholder(tr.tr_shared("themes.save_placeholder", "Theme name..."));
    window.set_theme_save_button(tr.tr_shared("themes.save_button", "Save"));
    window.set_theme_search_placeholder(tr.tr_shared("themes.search_placeholder", "Search themes..."));
    window.set_theme_empty_text(tr.tr_shared("themes.empty", "No themes yet. Save your current setup as a theme."));
    window.set_theme_apply_text(tr.tr_shared("themes.apply", "Apply"));
    window.set_theme_rename_text(tr.tr_shared("themes.rename", "Rename"));
    window.set_theme_delete_text(tr.tr_shared("themes.delete", "Delete"));
    window.set_theme_cancel_text(tr.tr_shared("common.cancel", "Cancel"));
    window.set_theme_confirm_delete(tr.tr_shared("themes.confirm_delete", "Delete theme"));
    window.set_theme_confirm_delete_msg(tr.tr_shared("themes.confirm_delete_msg", "Are you sure you want to delete"));
    window.set_theme_rename_title(tr.tr_shared("themes.rename_title", "Rename theme"));
    window.set_theme_include_label(tr.tr_shared("themes.include", "Include:"));
    window.set_theme_provider_noctalia(tr.tr_shared("themes.provider.noctalia", "Noctalia"));
    window.set_theme_provider_hve_presets(tr.tr_shared("themes.provider.hve-presets", "HVE Presets"));
    window.set_theme_action_overwrite(tr.tr_shared("themes.action_overwrite", "Overwrite"));

    // ── Dynamic theme + logo + tray icon ──
    let tray_handle = match engine.get_colors() {
        Ok(colors) => {
            let resolved = theme::resolve_scheme(&colors, &cfg.theme);
            theme::apply_theme(&window, &resolved);
            let surface_lowest = theme::parse_hex(&resolved.surface_lowest);
            let secondary = theme::parse_hex(&resolved.secondary);
            let tertiary = theme::parse_hex(&resolved.tertiary);
            let accent = theme::parse_hex(&resolved.accent);
            // Sidebar logo (multi-color)
            let logo = theme::render_logo_image(&surface_lowest, &secondary, &tertiary, &accent);
            window.set_logo_image(logo);
            // Tray icon (48x48 square, centered — tray scales down)
            // Lighten accent so the tray icon is visible on dark panels
                    let tray_color = theme::lighten(&accent, 0.6);
            let icon = theme::render_logo_square_mono(&tray_color, 48)
                .unwrap_or_default();
            tracing::info!("Theme loaded: {} (pref={})", colors.primary, cfg.theme);
            tray::start_tray(window.as_weak(), tr.clone(), icon, 48, 48)
        }
        Err(_) => tray::start_tray(window.as_weak(), tr.clone(), Vec::new(), 48, 48),
    };
    let tray_system_active = tray_handle.system_active.clone();
    tray::init_global(tray_handle);

    // ── Shared config for all callbacks (prevents stale clones from overwriting each other) ──
    let cfg = Arc::new(std::sync::Mutex::new(cfg));

    callbacks::setup_callbacks(&window, &cfg, proj.clone(), tray_system_active);

    // ── Nav modules (data-driven sidebar, translated) ──
    let nav_modules = Vec::from([
        crate::NavModule {
            label: SharedString::from(tr.tr_or("panel.tabs.home", "Home")),
            icon: SharedString::from("⌂"),
            accent: theme::parse_hex("#38bdf8"),
            tab_index: 0,
        },
        crate::NavModule {
            label: SharedString::from(tr.tr_or("panel.tabs.animations", "Animations")),
            icon: SharedString::from("▶"),
            accent: theme::parse_hex("#fbbf24"),
            tab_index: 1,
        },
        crate::NavModule {
            label: SharedString::from(tr.tr_or("panel.tabs.borders", "Borders")),
            icon: SharedString::from("◻"),
            accent: theme::parse_hex("#10b981"),
            tab_index: 2,
        },
        crate::NavModule {
            label: SharedString::from(tr.tr_or("panel.tabs.effects", "Effects")),
            icon: SharedString::from("◆"),
            accent: theme::parse_hex("#c084fc"),
            tab_index: 3,
        },
        crate::NavModule {
            label: SharedString::from(tr.tr_or("panel.tabs.themes", "Themes")),
            icon: SharedString::from("✦"),
            accent: theme::parse_hex("#f472b6"),
            tab_index: 4,
        },
    ]);
    window.set_nav_modules(ModelRc::new(VecModel::from(nav_modules)));

    // ── Start Hyprland IPC listener (handle kept alive so threads
    //     don't outlive the app on quit/restart) ──
    let _ipc_listener = hypr_ipc::start_listener(&window, proj.clone());

    // ── Countdown auto-minimize on focus loss ──
    let _focus_listener = countdown::setup_countdown(window.as_weak());

    // ── Close button callback ──
    {
        let weak = window.as_weak();
        window.on_close_button_clicked(move || {
            countdown::minimize_now(weak.clone());
        });
    }

    // ── Settings callbacks ──
    // We use clone-then-save since all Slint callbacks run on the same thread
    {
        let window_weak = window.as_weak();
        window.on_toggle_settings(move || {
            let w = window_weak.upgrade().unwrap();
            w.set_settings_open(!w.get_settings_open());
        });
    }

    {
        let settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_toggle_auto_minimize(move |enabled| {
            {
                let mut c = settings_cfg.lock().unwrap();
                c.auto_minimize_enabled = enabled;
                let _ = c.save();
            }
            if let Some(w) = weak.upgrade() {
                w.set_auto_minimize(enabled);
            }
        });
    }

    {
        let settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_change_minimize_seconds(move |secs| {
            {
                let mut c = settings_cfg.lock().unwrap();
                c.minimize_seconds = secs;
                let _ = c.save();
            }
            if let Some(w) = weak.upgrade() {
                w.set_minimize_seconds(secs);
            }
        });
    }

    {
        let settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_change_language(move |lang| {
            let lang_str = lang.to_string();
            {
                let mut c = settings_cfg.lock().unwrap();
                c.language = lang_str.clone();
                let _ = c.save();
            }
            if let Some(w) = weak.upgrade() {
                w.set_language(lang);
                w.set_restart_required(true);
            }
            tracing::info!("Language changed to {}. Restart to apply fully.", lang_str);
        });
    }

    {
        let settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_toggle_tiling_mode(move |enabled| {
            {
                let mut c = settings_cfg.lock().unwrap();
                c.tiling_mode = enabled;
                let _ = c.save();
            }
            if let Some(w) = weak.upgrade() {
                w.set_tiling_mode(enabled);
                w.set_restart_required(false);
            }

            // Toggle window rules in hyprland.conf — this persists across restarts
            set_tiling_window_rules(enabled);

            // Try to toggle the CURRENT window immediately
            let _ = std::process::Command::new("hyprctl")
                .args(["dispatch", "togglefloating", "title:Hyprland Visual Editor"])
                .output();

            // Show restart banner explaining that full effect requires restart
            if let Some(w) = weak.upgrade() {
                w.set_restart_required(true);
            }
            tracing::info!(
                "[settings] Tiling mode {} — config updated + togglefloating dispatched",
                if enabled { "ON" } else { "OFF" }
            );
        });
    }

    {
        let settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_toggle_keybinds(move |enabled| {
            {
                let mut c = settings_cfg.lock().unwrap();
                c.keybinds_enabled = enabled;
                let _ = c.save();
            }
            if let Some(w) = weak.upgrade() {
                w.set_keybinds_mode(enabled);
            }
            set_keybinds(enabled);
            tracing::info!("[settings] Keyboard shortcuts {}", if enabled { "ON" } else { "OFF" });
        });
    }

    {
        let settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_toggle_autostart(move |enabled| {
            {
                let mut c = settings_cfg.lock().unwrap();
                c.auto_start = enabled;
                let _ = c.save();
            }
            if let Some(w) = weak.upgrade() {
                w.set_autostart(enabled);
            }
            set_autostart(enabled);
        });
    }

    {
        let settings_cfg = cfg.clone();
        let weak = window.as_weak();
        let eng = engine.clone();
        window.on_change_theme(move |theme| {
            let theme_str = theme.to_string();
            {
                let mut c = settings_cfg.lock().unwrap();
                c.theme = theme_str.clone();
                let _ = c.save();
            }
            if let Some(w) = weak.upgrade() {
                // Apply new palette immediately (no restart needed)
                if let Ok(colors) = eng.get_colors() {
                    let resolved = theme::resolve_scheme(&colors, &theme_str);
                    theme::apply_theme(&w, &resolved);
                    // Regenerate logo with new theme colors (multi-color)
                    let surface_lowest = theme::parse_hex(&resolved.surface_lowest);
                    let secondary = theme::parse_hex(&resolved.secondary);
                    let tertiary = theme::parse_hex(&resolved.tertiary);
                    let accent = theme::parse_hex(&resolved.accent);
                    let logo = theme::render_logo_image(&surface_lowest, &secondary, &tertiary, &accent);
                    w.set_logo_image(logo);
                    // Update tray icon too
            let tray_color = theme::lighten(&accent, 0.6);
                    if let Some(icon) = theme::render_logo_square_mono(&tray_color, 48) {
                        tray::update_global_icon(icon);
                    }
                }
                w.set_theme(theme);
            }
            tracing::info!("Theme changed to {}", theme_str);
        });
    }

    {
        fn resolve_exe() -> PathBuf {
            if let Ok(path) = std::env::current_exe() {
                if path.is_file() {
                    return path;
                }
            }
            if let Some(arg0) = std::env::args().next() {
                let p = PathBuf::from(&arg0);
                if p.is_absolute() {
                    if p.is_file() {
                        return p;
                    }
                } else if let Ok(paths) = std::env::var("PATH") {
                    for dir in std::env::split_paths(&paths) {
                        let candidate = dir.join(&arg0);
                        if candidate.is_file() {
                            return candidate;
                        }
                    }
                }
            }
            PathBuf::from("hve")
        }

        let restart_lock = lock.clone();
        window.on_restart_app(move || {
            tracing::info!("[settings] Restarting app...");
            // 1. Release the exclusive lock so the new instance can start
            if let Ok(mut guard) = restart_lock.lock() {
                drop(guard.take());
            }
            // 2. Spawn the new instance
            let exe = resolve_exe();
            match std::process::Command::new(&exe)
                .args(std::env::args().skip(1))
                .spawn()
            {
                Ok(child) => {
                    tracing::info!(
                        "[settings] New instance spawned (PID: {}), quitting event loop",
                        child.id()
                    );
                }
                Err(e) => {
                    tracing::error!("[settings] Failed to restart: {}", e);
                    return;
                }
            }
            // 3. Gracefully quit the event loop — the main function continues past
            //    run_event_loop_until_quit() and returns Ok(()), letting the process
            //    die naturally. The new instance already has the lock released.
            let _ = slint::quit_event_loop();
        });
    }

    {
        let settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_reset_presets(move || {
            tracing::info!("[settings] Resetting presets...");
            if let Some(w) = weak.upgrade() {
                w.set_active_anim_index(-1);
                w.set_active_border_index(-1);
                w.set_active_shader_index(-1);
            }
            {
                let mut c = settings_cfg.lock().unwrap();
                c.active_anim_file = String::new();
                c.active_border_file = String::new();
                c.active_shader_file = String::new();
                let _ = c.save();
            }
            tracing::info!("[settings] Presets reset complete.");
        });
    }

    // ── Theme callbacks ──
    {
        let tm = theme_manager.clone();
        let weak = window.as_weak();
        window.on_save_theme(move |name| {
            let name_str = name.to_string();
            let provider_ids = {
                let w = weak.upgrade();
                w.map(|win| {
                    let mut ids: Vec<String> = Vec::new();
                    if win.get_include_noctalia() { ids.push("noctalia".into()); }
                    if win.get_include_hve_presets() { ids.push("hve-presets".into()); }
                    ids
                }).unwrap_or_default()
            };

            let result = tm.lock().unwrap().save(&name_str, &provider_ids);
            match result {
                Ok(_) => {
                    tracing::info!("[themes] Saved theme: {}", name_str);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_busy(true);
                        let tm = tm.lock().unwrap();
                        refresh_theme_list(&w, &tm);
                        w.set_theme_busy(false);
                    }
                }
                Err(e) => {
                    tracing::error!("[themes] Save failed: {}", e);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_error_text(e.into());
                    }
                }
            }
        });
    }

    {
        let tm = theme_manager.clone();
        let cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_apply_theme(move |name| {
            let name_str = name.to_string();
            let result = tm.lock().unwrap().apply(&name_str);
            match result {
                Ok(_) => {
                    tracing::info!("[themes] Applied theme: {}", name_str);
                    // Reload config from disk — the provider may have updated it
                    let updated_cfg = Config::load();
                    // Update in-memory config and persist
                    if let Ok(mut c) = cfg.lock() {
                        *c = updated_cfg.clone();
                        c.last_applied_theme = name_str.clone();
                        let _ = c.save();
                    }
                    if let Some(w) = weak.upgrade() {
                        sync_preset_indices(&w, &updated_cfg);
                        let tm = tm.lock().unwrap();
                        refresh_theme_list(&w, &tm);
                        w.set_home_active_theme_name(name_str.clone().into());
                    }
                }
                Err(e) => {
                    tracing::error!("[themes] Apply failed: {}", e);
                }
            }
        });
    }

    {
        let tm = theme_manager.clone();
        let weak = window.as_weak();
        window.on_delete_theme(move |name| {
            let name_str = name.to_string();
            match tm.lock().unwrap().delete(&name_str) {
                Ok(_) => {
                    tracing::info!("[themes] Deleted theme: {}", name_str);
                    if let Some(w) = weak.upgrade() {
                        let tm = tm.lock().unwrap();
                        refresh_theme_list(&w, &tm);
                    }
                }
                Err(e) => {
                    tracing::error!("[themes] Delete failed: {}", e);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_error_text(e.into());
                    }
                }
            }
        });
    }

    {
        let tm = theme_manager.clone();
        let weak = window.as_weak();
        window.on_rename_theme(move |old, new| {
            let old_str = old.to_string();
            let new_str = new.to_string();
            match tm.lock().unwrap().rename(&old_str, &new_str) {
                Ok(_) => {
                    tracing::info!("[themes] Renamed: {} -> {}", old_str, new_str);
                    if let Some(w) = weak.upgrade() {
                        let tm = tm.lock().unwrap();
                        refresh_theme_list(&w, &tm);
                        w.set_home_active_theme_name((&tm.last_applied).clone().into());
                    }
                }
                Err(e) => {
                    tracing::error!("[themes] Rename failed: {}", e);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_error_text(e.into());
                    }
                }
            }
        });
    }

    {
        let tm = theme_manager.clone();
        let weak = window.as_weak();
        window.on_overwrite_theme(move |name| {
            let name_str = name.to_string();
            let provider_ids = {
                let w = weak.upgrade();
                w.map(|win| {
                    let mut ids: Vec<String> = Vec::new();
                    if win.get_include_noctalia() { ids.push("noctalia".into()); }
                    if win.get_include_hve_presets() { ids.push("hve-presets".into()); }
                    ids
                }).unwrap_or_default()
            };

            let result = tm.lock().unwrap().save(&name_str, &provider_ids);
            match result {
                Ok(_) => {
                    tracing::info!("[themes] Overwritten theme: {}", name_str);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_busy(true);
                        let tm = tm.lock().unwrap();
                        refresh_theme_list(&w, &tm);
                        w.set_theme_busy(false);
                    }
                }
                Err(e) => {
                    tracing::error!("[themes] Overwrite failed: {}", e);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_error_text(e.into());
                    }
                }
            }
        });
    }

    {
        let tm = theme_manager.clone();
        let weak = window.as_weak();
        window.on_refresh_themes(move || {
                    if let Some(w) = weak.upgrade() {
                        let tm = tm.lock().unwrap();
                        refresh_theme_list(&w, &tm);
                        w.set_home_active_theme_name((&tm.last_applied).clone().into());
                    }
        });
    }

    // ── Start color watcher (bash inotify) ──
    // The bash-based watcher uses inotify for efficient file monitoring.
    let _color_watcher = watcher::spawn_color_watcher(&proj);

    // ── IPC server (Unix socket) ──
    ipc::start_ipc_server(window.as_weak(), proj.clone());

    // ── Intercept close events — always hide instead of destroying
    //     the window so the global event loop keeps running. ──
    window.window().on_close_requested(|| {
        // Keep the tray menu in sync: the window is now hidden
        ipc::WINDOW_HIDDEN.store(true, std::sync::atomic::Ordering::Relaxed);
        slint::CloseRequestResponse::HideWindow
    });

    // ── Startup: sync keybinds + autostart from saved config ──
    let cfg_guard = cfg.lock().unwrap();
    if cfg_guard.keybinds_enabled {
        set_keybinds(true);
    }
    window.set_keybinds_mode(cfg_guard.keybinds_enabled);
    set_autostart(cfg_guard.auto_start);
    drop(cfg_guard);

    // ── Visibilidad inicial según el modo ──
    if tray_mode {
        // Lazy load: no llamamos a show() hasta que el usuario pulse SUPER+H.
        // El primer toggle via IPC creará el xdg_toplevel con un debounce
        // de 400ms para evitar que el roundtrip de Wayland sea saboteado
        // por una pulsación accidental repetida.
        //
        // Esto evita por completo el problema de set_minimized() en Wayland
        // (no existe un-minimize programático) y es 100% agnóstico al WM.
        ipc::WINDOW_HIDDEN.store(true, std::sync::atomic::Ordering::Relaxed);
        ipc::TRAY_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
        tracing::info!("Starting in tray mode (lazy load — first toggle shows window)");
    } else {
        window.show()?;
        ipc::WINDOW_HIDDEN.store(false, std::sync::atomic::Ordering::Relaxed);
        ipc::TRAY_MODE.store(false, std::sync::atomic::Ordering::Relaxed);

        // ── Pre-warm tab layouts ──
        // Slint no calcula el layout de tabs inactivos (width: 0%)
        // hasta que se renderizan por primera vez. Esto causa un delay
        // en el primer click. Solución: mostrar cada tab brevemente
        // durante el startup para que Slint cachem los layouts.
        fn prewarm_tabs(weak: slint::Weak<crate::MainWindow>, step: u8) {
            if step > 3 {
                if let Some(win) = weak.upgrade() {
                    win.set_active_tab(0);
                }
                return;
            }
            if let Some(win) = weak.upgrade() {
                win.set_active_tab(step as i32);
            }
            slint::Timer::single_shot(std::time::Duration::from_millis(16), move || {
                prewarm_tabs(weak, step + 1);
            });
        }
        prewarm_tabs(window.as_weak(), 1);

        // En Wayland/Hyprland, show() no garantiza foco automático.
        // Forzamos foco via hyprctl para evitar el doble-click inicial.
        let startup_tiling = cfg.lock().unwrap().tiling_mode;
        let startup_weak = window.as_weak();
        // First timer: focus window + sync config rules
        slint::Timer::single_shot(std::time::Duration::from_millis(200), move || {
            let _ = std::process::Command::new("hyprctl")
                .args(["dispatch", "focuswindow", "title:Hyprland Visual Editor"])
                .output();

            // ── Ensure hyprland.conf rules match saved config ──
            set_tiling_window_rules(startup_tiling);

            // ── Second timer: toggle current window if tiling mode is ON ──
            // The config file change only affects NEW windows. For the already-mapped
            // window, we dispatch togglefloating after a short delay to let the
            // config reload complete.
            if startup_tiling {
                let weak2 = startup_weak.clone();
                slint::Timer::single_shot(std::time::Duration::from_millis(600), move || {
                    let _w = weak2.upgrade();
                    let _ = std::process::Command::new("hyprctl")
                        .args(["dispatch", "togglefloating", "title:Hyprland Visual Editor"])
                        .output();
                    tracing::info!("[startup] Tiling mode ON: togglefloating dispatched (delayed)");
                });
            }
        });
    }

    // ── Global event loop (decoupled from window lifecycle) ──
    slint::run_event_loop_until_quit()?;

    // Clean up IPC socket after the event loop stops
    ipc::cleanup();

    drop(_color_watcher);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct TempEnv {
        old_home: Option<String>,
        old_cache_home: Option<String>,
        tmp: std::path::PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TempEnv {
        fn new() -> Self {
            let guard = ENV_LOCK.lock().unwrap();
            let tmp = std::env::temp_dir().join(format!("hve_test_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp).unwrap();
            let old_home = std::env::var("HOME").ok();
            let old_cache_home = std::env::var("XDG_CACHE_HOME").ok();
            std::env::set_var("HOME", &tmp);
            // Unset XDG_CACHE_HOME so dirs::cache_dir falls through to HOME/.cache
            std::env::remove_var("XDG_CACHE_HOME");
            Self {
                old_home,
                old_cache_home,
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
            match &self.old_cache_home {
                Some(c) => std::env::set_var("XDG_CACHE_HOME", c),
                None => std::env::remove_var("XDG_CACHE_HOME"),
            }
            let _ = std::fs::remove_dir_all(&self.tmp);
        }
    }

    // ── set_keybinds ─────────────────────────────────────────────────

    #[test]
    fn test_set_keybinds_writes_and_removes() {
        let _env = TempEnv::new();

        // Ensure settings file exists (default conf format since hve_format is absent)
        ensure_settings_file();
        assert!(hve_settings_path().exists(), "settings file should exist");

        // Enable keybinds
        set_keybinds(true);
        let content = std::fs::read_to_string(hve_settings_path()).unwrap();
        assert!(
            content.contains(">>> HVE KEYBINDS <<<"),
            "should have keybinds start marker"
        );
        assert!(
            content.contains(">>> HVE KEYBINDS END <<<"),
            "should have keybinds end marker"
        );
        assert!(content.contains("hve-ipc"), "should have hve-ipc commands");
        assert!(
            content.contains("toggle-tray"),
            "should have toggle-tray bind"
        );
        assert!(
            content.contains("pause-restart"),
            "should have pause-restart bind"
        );
        assert!(content.contains("next-anim"), "should have next-anim bind");
        assert!(
            content.contains("next-border"),
            "should have next-border bind"
        );
        assert!(
            content.contains("next-shader"),
            "should have next-shader bind"
        );

        // Disable keybinds
        set_keybinds(false);
        let content = std::fs::read_to_string(hve_settings_path()).unwrap();
        assert!(
            !content.contains(">>> HVE KEYBINDS <<<"),
            "should NOT have keybinds start marker"
        );
        assert!(
            !content.contains(">>> HVE KEYBINDS END <<<"),
            "should NOT have keybinds end marker"
        );
        assert!(
            !content.contains("hve-ipc"),
            "should NOT have hve-ipc commands"
        );
    }

    #[test]
    fn test_set_keybinds_noop_when_disabled_and_markers_absent() {
        let _env = TempEnv::new();

        // Create settings file manually WITHOUT keybinds markers
        let path = hve_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let no_kb_content = "# >>> HVE WINDOW RULES <<<\nwindowrulev2 = float, title:^(Hyprland Visual Editor)$\n# >>> HVE WINDOW RULES END <<<\n";
        std::fs::write(&path, no_kb_content).unwrap();

        let before = std::fs::read_to_string(&path).unwrap();
        assert!(
            !before.contains("HVE KEYBINDS"),
            "test file should not have keybinds markers"
        );

        // Disable when no markers exist → should be no-op
        set_keybinds(false);
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(before, after, "disabling when markers absent should be no-op");
    }

}
