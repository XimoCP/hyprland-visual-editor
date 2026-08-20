mod app_state;
mod callbacks;
mod composer;
mod config;
mod countdown;
mod engine;
mod hypr_ipc;
mod ipc;
mod presets;
mod providers;
mod settings;
mod shell;
mod theme;
mod theme_manager;
mod tr;
mod tray;
mod utils;
mod watcher;
#[cfg(test)]
mod test_utils;

use app_state::AppState;
use clap::Parser;
use config::Config;
use engine::{ColorScheme, Engine};
use settings::{ensure_settings_file, set_autostart, set_keybinds, set_tiling_window_rules};
use slint::{Color, ComponentHandle, ModelRc, SharedString, VecModel};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;
use tr::Tr;
use crate::providers::shell::ShellDetector;

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
        std::env::current_exe().ok().and({
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

/// Refresh the Slint theme list UI from the ThemeManager state.
pub(crate) fn refresh_theme_list(
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

/// Pre-warm tab layouts so the first user interaction after a show()
/// does not hit cold (unmeasured) layouts.
///
/// Slint does not compute the layout of inactive tabs (width: 0%) until
/// they are rendered. This briefly cycles through tabs during startup /
/// after each show() so Slint caches the layouts, avoiding first-keypress
/// lag and skipped animations. In tray mode this must be called again
/// after every show() because hide() discards the rendered state.
pub(crate) fn prewarm_tabs(weak: slint::Weak<crate::MainWindow>, step: u8) {
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

/// Block keyboard navigation for a short warmup window after the window is
/// shown, so Slint's animation clock starts ticking before the first key
/// press. Without this, the very first navigation step after show() skips
/// its animation (Slint issue #1255) and the auto-repeat timer makes it
/// look like a double tab-skip.
pub(crate) fn warmup_navigation(win: &crate::MainWindow) {
    win.set_nav_ready(false);
    let weak = win.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(250), move || {
        if let Some(w) = weak.upgrade() {
            w.set_nav_ready(true);
        }
    });
}

/// Re-read the current palette from the system and refresh every window
/// visual that depends on it: resolve the color scheme, apply it to the
/// window, regenerate the sidebar logo, and update the tray icon.
///
/// Convenience wrapper that combines [`fetch_visual_colors`] and
/// [`apply_visual_state`]. Callers that must keep the UI thread free
/// (e.g. the IPC server) should call the two halves separately instead.
///
/// Returns `(tray icon RGBA, primary color)` so callers that START the
/// tray (instead of updating an already-registered one) can reuse the
/// freshly rendered icon and log the loaded palette.
pub(crate) fn refresh_visual_state(
    window: &MainWindow,
    engine: &Engine,
    theme_pref: &str,
) -> Option<(Vec<u8>, String)> {
    apply_visual_state(window, &fetch_visual_colors(engine)?, theme_pref)
}

/// Fetch the current palette from the active color source. This spawns the
/// `get_colors.sh` subprocess, so it is blocking and must be called off
/// the UI thread.
pub(crate) fn fetch_visual_colors(engine: &Engine) -> Option<ColorScheme> {
    engine.get_colors().ok()
}

/// Apply a fetched palette to every window visual that depends on it:
/// resolve the color scheme, apply it to the window, regenerate the
/// sidebar logo, and update the tray icon. Pure UI work — safe to call
/// on the event-loop thread.
pub(crate) fn apply_visual_state(
    window: &MainWindow,
    colors: &ColorScheme,
    theme_pref: &str,
) -> Option<(Vec<u8>, String)> {
    let resolved = theme::resolve_scheme(colors, theme_pref);
    theme::apply_theme(window, &resolved);
    let secondary = theme::parse_hex(&resolved.secondary);
    let tertiary = theme::parse_hex(&resolved.tertiary);
    let accent = theme::parse_hex(&resolved.accent);
    // Sidebar logo (multi-color)
    let logo = theme::render_logo_image(&secondary, &tertiary, &accent);
    window.set_logo_image(logo);
    // Tray icon (48x48 square, centered — tray scales down)
    // Lighten accent so the tray icon is visible on dark panels
    let tray_color = theme::lighten(&accent, 0.6);
    let icon = theme::render_logo_square_mono(&tray_color, 48);
    if let Some(icon) = &icon {
        tray::update_global_icon(icon.clone());
    }
    Some((icon?, colors.primary.clone()))
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
    if let Err(e) = rustix::fs::flock(&lock_file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
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
    let engine = Arc::new(Engine::new(&proj));
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

    // ── HVE 2 shell init (delivery 4/5) ──
    // The Shell owns navigation state, slot registry, and the stepped
    // size animator. It lives in Rc<RefCell<>> because slint::Timer is
    // !Send + !Sync.
    let shell = shell::Shell::new(window.as_weak());
    // Register production stub slots for Gallery and Workshop. Real
    // slot views land in modules 2 and 4.
    shell::Shell::register_slot(&shell, Box::new(shell::slots::StubSlot::new(shell::nav::Screen::Gallery)));
    shell::Shell::register_slot(&shell, Box::new(shell::slots::StubSlot::new(shell::nav::Screen::Workshop)));
    // Set shell i18n strings (nav-shell spec R5).
    window.set_shell_brand_text(tr.tr_shared("shell.brand", "HVE"));
    window.set_shell_brand_subtitle(tr.tr_shared("shell.subtitle", "Hyprland Visual Editor"));
    window.set_shell_status_text(tr.tr_shared("shell.status_ready", "Ready"));
    window.set_shell_back_hint(tr.tr_shared("shell.back_hint", "Esc collapses"));
    window.set_shell_home_hint(tr.tr_shared("shell.home.hint", "Home — theme cards land in module 2"));
    window.set_shell_gallery_hint(tr.tr_shared("shell.gallery.hint", "Gallery slot — module 2"));
    window.set_shell_workshop_hint(tr.tr_shared("shell.workshop.hint", "Workshop slot — module 4"));

    // ── Load initial state ──
    window.set_system_active(cfg.is_system_active);
    window.set_border_size(cfg.border_size);
    window.set_corner_radius(cfg.border_radius);
    window.set_gap_in(cfg.gaps_in);
    window.set_gap_out(cfg.gaps_out);
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
    // Detectar qué versión de Noctalia está activa y registrar el provider correspondiente
    let noctalia_v4 = crate::providers::shell::NoctaliaV4Paths;
    let noctalia_v5 = crate::providers::shell::NoctaliaV5Paths;

    if noctalia_v5.is_active() {
        theme_manager.register_provider(Box::new(crate::providers::noctalia::NoctaliaV5Provider::new()));
        tracing::info!("[shell] Noctalia v5 detectado — registrando provider v5");
    } else if noctalia_v4.is_active() {
        theme_manager.register_provider(Box::new(crate::providers::noctalia::NoctaliaV4Provider::new()));
        tracing::info!("[shell] Noctalia v4 detectado — registrando provider v4");
    } else {
        // Fallback: registrar v4 por defecto
        theme_manager.register_provider(Box::new(crate::providers::noctalia::NoctaliaV4Provider::new()));
        tracing::warn!("[shell] Noctalia no detectado — registrando provider v4 por defecto");
    }
    theme_manager.register_provider(Box::new(crate::providers::hve_presets::HvePresetsProvider::new((*engine).clone())));
    theme_manager.register_provider(Box::new(crate::providers::hyprland_settings::HyprlandSettingsProvider::new()));
    // Restore last applied theme from config
    if !cfg!(test) {
        theme_manager.last_applied = cfg.last_applied_theme.clone();
    }

    // Refresh UI theme list
    refresh_theme_list(&window, &theme_manager);

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
    window.set_border_radius_label(tr.tr_shared("borders.radius.title", "Corner Radius"));
    window.set_gap_in_label(tr.tr_shared("borders.gaps.in.title", "Inner Gap"));
    window.set_gap_out_label(tr.tr_shared("borders.gaps.out.title", "Outer Gap"));
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
    window.set_home_about_full(tr.tr_shared("home.about_full", "HVE is a graphical app to visually manage your Hyprland desktop aesthetics: animations, borders, rounded corners, window gaps, and visual effects — all with live preview.\n\nThe Themes tab lets you save, apply, rename, and delete full configurations, including static and animated (mpvpaper) wallpapers depending on the active provider (Noctalia v5, HVE presets, and wallpapers).\n\nIt also includes tiling mode, auto-minimize on focus loss, autostart with your session, full keyboard navigation, Spanish/English languages, and system tray control.\n\nEverything applies safely: HVE assembles fragments and never rewrites your personal config. When you disable the system or uninstall, a watchdog cleans up and your original config always stays intact."));
    window.set_home_about_tree_label(tr.tr_shared("home.about_tree_label", "Project structure"));
    let tree_paths: Vec<SharedString> = tr.tr_array("home.about_tree_paths");
    let tree_descs: Vec<SharedString> = tr.tr_array("home.about_tree_descs");
    window.set_home_about_tree_paths(ModelRc::from(tree_paths.as_slice()));
    window.set_home_about_tree_descs(ModelRc::from(tree_descs.as_slice()));
    window.set_home_about_tree_path_max(
        tree_paths
            .iter()
            .max_by_key(|p| p.len())
            .cloned()
            .unwrap_or_default(),
    );
    window.set_home_about_docs_label(tr.tr_shared("home.about_docs", "View documentation"));
    let about_items: Vec<AboutItem> = vec![
        AboutItem {
            text: tr.tr_shared("home.about_line_1", "Live preview — tweak and see it instantly"),
            color: Color::from_rgb_u8(251, 191, 36),
        },
        AboutItem {
            text: tr.tr_shared("home.about_line_2", "Safe assembly — zero risk to your configs"),
            color: Color::from_rgb_u8(16, 185, 129),
        },
        AboutItem {
            text: tr.tr_shared("home.about_line_3", "Hot reload — apply without restarting Hyprland"),
            color: Color::from_rgb_u8(56, 189, 248),
        },
        AboutItem {
            text: tr.tr_shared("home.about_line_4", "Fragments — composable pieces, not one huge file"),
            color: Color::from_rgb_u8(192, 132, 252),
        },
        AboutItem {
            text: tr.tr_shared("home.about_line_5", "Themes — save and apply complete configurations"),
            color: Color::from_rgb_u8(56, 189, 248),
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
    window.set_theme_confirm_refresh(tr.tr_shared("themes.confirm_refresh", "Refresh theme"));
    window.set_theme_confirm_refresh_msg(tr.tr_shared("themes.confirm_refresh_msg", "This will reload the theme with the current configuration. Continue?"));
    window.set_theme_refresh_text(tr.tr_shared("themes.refresh", "Refresh"));
    window.set_theme_rename_title(tr.tr_shared("themes.rename_title", "Rename theme"));
    window.set_theme_save_section_text(tr.tr_shared("themes.save_section", "Save Current State"));
    window.set_theme_list_section_text(tr.tr_shared("themes.list_section", "My Themes"));
    window.set_app_version_label(slint::SharedString::from(format!(
        "v{} — Standalone",
        env!("CARGO_PKG_VERSION")
    )));

    // ── Dynamic theme + logo + tray icon ──
    let tray_handle = match refresh_visual_state(&window, &engine, &cfg.theme) {
        Some((icon, primary)) => {
            tracing::info!("Theme loaded: {} (pref={})", primary, cfg.theme);
            tray::start_tray(window.as_weak(), tr.clone(), icon, 48, 48)
        }
        None => tray::start_tray(window.as_weak(), tr.clone(), Vec::new(), 48, 48),
    };
    let tray_system_active = tray_handle.system_active.clone();
    tray::init_global(tray_handle);

    // ── Central view-model: Config + Engine + ThemeManager behind one lock ──
    // The engine now lives inside AppState; the providers registered above
    // keep their own engine clones, so the outer Arc can be dropped.
    let state = Arc::new(std::sync::Mutex::new(AppState::new(
        cfg,
        (*engine).clone(),
        theme_manager,
    )));
    drop(engine);

    callbacks::setup_callbacks(
        &window,
        &state,
        proj.clone(),
        tray_system_active,
        &lock,
        &shell,
    );

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

    // ── Start color watcher (bash inotify) ──
    // The bash-based watcher uses inotify for efficient file monitoring.
    let _color_watcher = watcher::spawn_color_watcher(&proj);

    // ── Composer controller (composition root) ──
    let composer_driver = Box::new(composer::HyprlandComposer::new());
    let controller = composer::Controller::new(composer_driver);
    composer::init_global(controller);

    // ── IPC server (Unix socket) ──
    ipc::start_ipc_server(window.as_weak(), proj.clone());

    // ── Startup: sync keybinds + autostart from saved config ──
    {
        let state_guard = state.lock().unwrap_or_else(|e| e.into_inner());
        if state_guard.cfg().keybinds_enabled {
            set_keybinds(true);
        }
        window.set_keybinds_mode(state_guard.cfg().keybinds_enabled);
        set_autostart(state_guard.cfg().auto_start);
    }

    // ── Visibilidad inicial según el modo ──
    if tray_mode {
        // Lazy load: no llamamos a show() hasta que el usuario pulse SUPER+H.
        // El primer toggle via IPC creará el xdg_toplevel con un debounce
        // de 400ms para evitar que el roundtrip de Wayland sea saboteado
        // por una pulsación accidental repetida.
        //
        // Esto evita por completo el problema de set_minimized() en Wayland
        // (no existe un-minimize programático) y es 100% agnóstico al WM.
        if let Some(mut ctrl) = composer::global_controller() {
            ctrl.set_window_hidden(true);
            ctrl.set_tray_mode(true);
        }
        tracing::info!("Starting in tray mode (lazy load — first toggle shows window)");
    } else {
        window.show()?;
        if let Some(mut ctrl) = composer::global_controller() {
            ctrl.set_window_hidden(false);
            ctrl.set_tray_mode(false);
        }

        // ── Pre-warm tab layouts ──
        // Slint no calcula el layout de tabs inactivos (width: 0%)
        // hasta que se renderizan por primera vez. Esto causa un delay
        // en el primer click. Solución: mostrar cada tab brevemente
        // durante el startup para que Slint cachem los layouts.
        prewarm_tabs(window.as_weak(), 1);

        // ── Warm up keyboard navigation ──
        // Block nav for 250ms after show so the animation clock ticks
        // before the first key press.
        warmup_navigation(&window);

        // En Wayland/Hyprland, show() no garantiza foco automático.
        // Forzamos foco via Composer para evitar el doble-click inicial.
        let startup_tiling = state.lock().unwrap_or_else(|e| e.into_inner()).cfg().tiling_mode;
        // First timer: focus window + sync config rules
        slint::Timer::single_shot(std::time::Duration::from_millis(200), move || {
            if let Some(ctrl) = composer::global_controller() {
                ctrl.composer().focus();
            }

            // ── Ensure hyprland.conf rules match saved config ──
            set_tiling_window_rules(startup_tiling);

            // ── Second timer: toggle current window if tiling mode is ON ──
            // The config file change only affects NEW windows. For the already-mapped
            // window, we dispatch togglefloating after a short delay to let the
            // config reload complete.
            if startup_tiling {
                slint::Timer::single_shot(std::time::Duration::from_millis(600), move || {
                    if let Some(ctrl) = composer::global_controller() {
                        ctrl.composer().toggle_float();
                    }
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

    // ── Keyboard navigation logic (backend-testing) ──────────────────
    // Ignored by default: initializes the Slint testing backend which can
    // only run once per process. Run with:
    //   cargo test -- --ignored nav_logic --test-threads=1
    //
    // Regression test: a plain hold of the arrow key (<600ms, no OS
    // auto-repeat yet) must NOT jump two tabs. The repeat Timer only starts
    // after the OS confirms the hold with its first repeat (event.repeat).
    #[test]
    #[ignore]
    fn nav_logic_single_keypress_single_step() {
        i_slint_backend_testing::init_integration_test_with_mock_time();
        let win = MainWindow::new().unwrap();
        win.show().unwrap();
        let w = win.window();

        // 1. Tap: un KeyPressed Down desde Inicio (tab 0) → exactamente UNA tab
        w.dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::DownArrow.into(),
        });
        assert_eq!(win.get_active_tab(), 1, "un key-pressed (tap) debe mover exactamente una tab");

        // 2. Hold de 500ms SIN repeat del OS: el Timer NO está activo
        //    (arranca solo tras event.repeat) → NO debe saltar otra tab.
        //    ESTO era el bug: el Timer viejo disparaba a los 400ms.
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(500));
        assert_eq!(win.get_active_tab(), 1, "hold de 500ms sin repeat del OS no debe saltar dos tabs");

        // 3. El OS confirma el hold con su primer repeat → activa el Timer, sin step
        w.dispatch_event(slint::platform::WindowEvent::KeyPressRepeated {
            text: slint::platform::Key::DownArrow.into(),
        });
        assert_eq!(win.get_active_tab(), 1, "el repeat del OS activa el Timer pero no hace step");

        // 4. El Timer toma el relevo del repeat. Nota: en el backend de
        //    testing, el Timer se registra recién en el siguiente tick (el
        //    dispatch del evento no avanza el reloj). En la app real el event
        //    loop de winit actualiza los timers continuamente.
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(150));

        // 5. Primer step del Timer (intervalo 150ms dentro de un módulo)
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(150));
        assert_eq!(win.get_active_tab(), 2, "Timer activo tras repeat del OS: step cada 150ms");

        // 6. El Timer se re-registra en el tick siguiente y luego dispara el
        //    2º step (mismo patrón de registro diferido del backend testing)
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(150));
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(150));
        assert_eq!(win.get_active_tab(), 3, "segundo intervalo del Timer: otro step");

        // 7. Release → todo se resetea
        w.dispatch_event(slint::platform::WindowEvent::KeyReleased {
            text: slint::platform::Key::DownArrow.into(),
        });

        // 8. Nuevo tap tras release → exactamente una tab más
        w.dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::DownArrow.into(),
        });
        assert_eq!(win.get_active_tab(), 4, "nuevo press tras release mueve una tab");
    }

    // ── Composer wiring verification ─────────────────────────────────

    /// Verify that the Controller is constructed with correct defaults
    /// and that the Composer trait is properly implemented.
    /// This test does NOT call init_global() (OnceLock can only be set once
    /// per process), but it verifies the Controller + Composer integration
    /// that main.rs uses at the composition root.
    #[test]
    fn test_controller_wiring_defaults() {
        let (fake, _calls) = composer::tests::FakeComposer::new();
        let controller = composer::Controller::new(Box::new(fake));

        // Verify the Controller's initial state matches what main.rs expects
        assert!(!controller.window_hidden(), "window should not be hidden at startup");
        assert!(!controller.tray_mode(), "tray_mode should be false at startup");
        assert!(controller.prev_workspace().is_none(), "no prev workspace at startup");

        // Verify the composer reference works
        let _composer = controller.composer();
    }

}
