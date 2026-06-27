mod callbacks;
mod config;
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
    let _lock = lock_file;

    let tray_mode = cli.tray;

    let proj = project_dir();
    let engine = Engine::new(&proj);
    let cfg = Config::load();
    let tr = Tr::new();

    tracing::info!("Locale: {}", tr.lang);

    let window = MainWindow::new()?;

    // ── Autostart ──
    if cfg.auto_start {
        setup_autostart();
    }

    // ── Load initial state ──
    window.set_system_active(cfg.is_system_active);
    window.set_border_size(cfg.border_size);

    // ── Scan + translate presets ──
    presets::populate_presets(&window, &engine, &cfg, &tr);

    // ── i18n: static UI strings ──
    window.set_sidebar_subtitle(tr.tr_shared("panel.header_title", "Hyprland Visual"));
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

    // ── Dynamic theme ──
    match engine.get_colors() {
        Ok(colors) => {
            theme::apply_theme(&window, &colors);
            tracing::info!("Theme loaded: primary={}", colors.primary);
        }
        Err(e) => tracing::error!("Could not load theme: {}", e),
    }

    // ── System tray (start before callbacks so the shared atomic exists) ──
    let tray_active = tray::start_tray(window.as_weak());

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
        // En modo tray, la ventana ya nace oculta, pero actualizamos los estados atómicos
        ipc::WINDOW_HIDDEN.store(true, std::sync::atomic::Ordering::Relaxed);
        ipc::TRAY_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
        tracing::info!("Starting in tray mode (window hidden)");
    } else {
        // En modo normal, DEBEMOS mostrar la ventana explícitamente
        window.show()?;
        ipc::WINDOW_HIDDEN.store(false, std::sync::atomic::Ordering::Relaxed);
        ipc::TRAY_MODE.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    // ── Global event loop (decoupled from window lifecycle) ──
    slint::run_event_loop_until_quit()?;

    // Clean up IPC socket after the event loop stops
    ipc::cleanup();

    drop(_color_watcher);
    drop(_lock);
    Ok(())
}
