mod callbacks;
mod config;
mod engine;
mod hypr_ipc;
mod ipc;
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

    let window = MainWindow::new()?;

    // ── Autostart ──
    if cfg.auto_start {
        setup_autostart();
    }

    // ── Load initial state ──
    window.set_system_active(cfg.is_system_active);
    window.set_border_size(cfg.border_size);

    // ── Scan presets ──
    presets::populate_presets(&window, &engine, &cfg);

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

    // ── Nav modules (data-driven sidebar) ──
    let nav_modules = Vec::from([
        crate::NavModule {
            label: SharedString::from("Home"),
            icon: SharedString::from("⌂"),
            accent: theme::parse_hex("#38bdf8"),
            tab_index: 0,
        },
        crate::NavModule {
            label: SharedString::from("Animations"),
            icon: SharedString::from("▶"),
            accent: theme::parse_hex("#fbbf24"),
            tab_index: 1,
        },
        crate::NavModule {
            label: SharedString::from("Borders"),
            icon: SharedString::from("◻"),
            accent: theme::parse_hex("#10b981"),
            tab_index: 2,
        },
        crate::NavModule {
            label: SharedString::from("Effects"),
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

    // ── Hide window immediately when running in tray-only mode ──
    if tray_mode {
        // Hide BEFORE run() — the event loop keeps running so the tray
        // and IPC server continue to function.
        let _ = window.window().hide();
    }

    // ── Run UI (blocks until the event loop exits or window closes) ──
    let result = window.run();

    // Clean up IPC socket after the event loop stops
    ipc::cleanup();

    drop(_color_watcher);
    drop(_lock);
    result
}
