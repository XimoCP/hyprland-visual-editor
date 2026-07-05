mod callbacks;
mod config;
mod countdown;
mod engine;
mod hypr_ipc;
mod ipc;
mod tr;
mod presets;
mod theme;
mod tray;
mod watcher;

use clap::Parser;
use config::Config;
use engine::Engine;
use fs2::FileExt;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
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
fn setup_autostart() {
    let autostart_dir = dirs::config_dir()
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".config")
        })
        .join("autostart");

    let _ = std::fs::create_dir_all(&autostart_dir);
    let desktop_path = autostart_dir.join("hve.desktop");

    let exe_path =
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hve"));

    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=HVE\n\
         Comment=Hyprland Visual Editor\n\
         Exec={} --tray\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exe_path.display()
    );

    match std::fs::write(&desktop_path, content) {
        Ok(_) => tracing::info!("Autostart entry created at {}", desktop_path.display()),
        Err(e) => tracing::error!("Failed to create autostart entry: {}", e),
    }
}

/// Detect whether HVE is in lua or conf mode by reading the format cache.
fn hve_format() -> &'static str {
    let format_path = hve_cache_dir().join("hve_format");
    if let Ok(content) = std::fs::read_to_string(&format_path) {
        if content.trim() == "lua" {
            return "lua";
        }
    }
    "conf"
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
            let _ = std::process::Command::new("hyprctl")
                .arg("reload")
                .output();
        }
        Err(e) => {
            tracing::error!("[windowrules] Failed to write: {}", e);
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let cli = Cli::parse();

    // ── Structured logging ──
    let filter = match cli.verbose {
        0 => EnvFilter::new("info"),
        1 => EnvFilter::new("debug"),
        _ => EnvFilter::new("trace"),
    };
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

    // ── Autostart ──
    if cfg.auto_start {
        setup_autostart();
    }

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
    window.set_home_about_title(tr.tr_shared("home.about_title", "About HVE"));
    window.set_home_about_short(tr.tr_shared("home.about_short", "Hyprland Visual Editor makes your desktop truly yours."));
    window.set_home_about_full(tr.tr_shared("home.about_full", "HVE uses a real-time Fragments and Assembly system. It never touches your main configuration. Everything is safely generated in an isolated overlay.conf master file inside ~/.cache/hve/.\n\n① Environment detection\n② Atomic assembly\n③ Hot Reload\n④ Guardian Shield"));

    // ── i18n: Settings strings ──
    window.set_settings_restart_banner(tr.tr_shared("settings.restart_banner", "⚠ Restart required"));
    window.set_settings_restart_button(tr.tr_shared("settings.restart_button", "Restart"));

    // ── Dynamic theme ──
    match engine.get_colors() {
        Ok(colors) => {
            let resolved = theme::resolve_scheme(&colors, &cfg.theme);
            theme::apply_theme(&window, &resolved);
            tracing::info!("Theme loaded: {} (pref={})", colors.primary, cfg.theme);
        }
        Err(e) => tracing::error!("Could not load theme: {}", e),
    }

    // ── System tray (start before callbacks so the shared atomic exists) ──
    let tray_active = tray::start_tray(window.as_weak(), tr.clone());

    // ── Callbacks ──
    callbacks::setup_callbacks(&window, &cfg, proj.clone(), tray_active);

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
    ]);
    window.set_nav_modules(ModelRc::new(VecModel::from(nav_modules)));

    // ── Start Hyprland IPC listener ──
    hypr_ipc::start_listener(&window, proj.clone());

    // ── Countdown auto-minimize on focus loss ──
    countdown::setup_countdown(window.as_weak());

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
        let mut settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_toggle_auto_minimize(move |enabled| {
            settings_cfg.auto_minimize_enabled = enabled;
            let _ = settings_cfg.save();
            if let Some(w) = weak.upgrade() {
                w.set_auto_minimize(enabled);
            }
        });
    }

    {
        let mut settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_change_minimize_seconds(move |secs| {
            settings_cfg.minimize_seconds = secs;
            let _ = settings_cfg.save();
            if let Some(w) = weak.upgrade() {
                w.set_minimize_seconds(secs);
            }
        });
    }

    {
        let mut settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_change_language(move |lang| {
            let lang_str = lang.to_string();
            settings_cfg.language = lang_str.clone();
            let _ = settings_cfg.save();
            if let Some(w) = weak.upgrade() {
                w.set_language(lang);
                w.set_restart_required(true);
            }
            tracing::info!("Language changed to {}. Restart to apply fully.", lang_str);
        });
    }

    {
        let mut settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_toggle_tiling_mode(move |enabled| {
            settings_cfg.tiling_mode = enabled;
            let _ = settings_cfg.save();
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
        let mut settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_toggle_autostart(move |enabled| {
            settings_cfg.auto_start = enabled;
            let _ = settings_cfg.save();
            if let Some(w) = weak.upgrade() {
                w.set_autostart(enabled);
            }
            if enabled {
                crate::setup_autostart();
            } else {
                // Remove autostart desktop file
                let autostart_path = dirs::config_dir()
                    .unwrap_or_else(|| {
                        let home = std::env::var("HOME").unwrap_or_default();
                        PathBuf::from(home).join(".config")
                    })
                    .join("autostart")
                    .join("hve.desktop");
                let _ = std::fs::remove_file(autostart_path);
            }
        });
    }

    {
        let mut settings_cfg = cfg.clone();
        let weak = window.as_weak();
        let eng = engine.clone();
        window.on_change_theme(move |theme| {
            let theme_str = theme.to_string();
            settings_cfg.theme = theme_str.clone();
            let _ = settings_cfg.save();
            if let Some(w) = weak.upgrade() {
                // Apply new palette immediately (no restart needed)
                if let Ok(colors) = eng.get_colors() {
                    let resolved = theme::resolve_scheme(&colors, &theme_str);
                    theme::apply_theme(&w, &resolved);
                }
                w.set_theme(theme);
            }
            tracing::info!("Theme changed to {}", theme_str);
        });
    }

    {
        let restart_lock = lock.clone();
        window.on_restart_app(move || {
            tracing::info!("[settings] Restarting app...");
            // 1. Release the exclusive lock so the new instance can start
            if let Ok(mut guard) = restart_lock.lock() {
                drop(guard.take());
            }
            // 2. Spawn the new instance
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hve"));
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
        let mut settings_cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_reset_presets(move || {
            tracing::info!("[settings] Resetting presets...");
            if let Some(w) = weak.upgrade() {
                w.set_active_anim_index(-1);
                w.set_active_border_index(-1);
                w.set_active_shader_index(-1);
            }
            settings_cfg.active_anim_file = String::new();
            settings_cfg.active_border_file = String::new();
            settings_cfg.active_shader_file = String::new();
            let _ = settings_cfg.save();
            tracing::info!("[settings] Presets reset complete.");
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

    // ── Visibilidad inicial según el modo ──
    if tray_mode {
        ipc::WINDOW_HIDDEN.store(true, std::sync::atomic::Ordering::Relaxed);
        ipc::TRAY_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
        tracing::info!("Starting in tray mode (window hidden)");
    } else {
        window.show()?;
        ipc::WINDOW_HIDDEN.store(false, std::sync::atomic::Ordering::Relaxed);
        ipc::TRAY_MODE.store(false, std::sync::atomic::Ordering::Relaxed);

        // En Wayland/Hyprland, show() no garantiza foco automático.
        // Forzamos foco via hyprctl para evitar el doble-click inicial.
        let startup_tiling = cfg.tiling_mode;
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
