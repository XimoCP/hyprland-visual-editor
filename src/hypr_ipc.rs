use crate::config::Config;
use crate::engine::Engine;
use crate::theme;
use slint::ComponentHandle;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct HyprIpc {
    socket_path: PathBuf,
}

/// Opaque handle to a background listener thread.
/// Dropping the handle sets an atomic shutdown flag so the loop exits cleanly
/// on its next reconnect iteration.
pub struct ListenerHandle {
    shutdown: std::sync::Arc<AtomicBool>,
    _thread: JoinHandle<()>,
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl HyprIpc {
    pub fn new() -> Option<Self> {
        let instance = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| "/run/user/1000".to_string());
        let socket = PathBuf::from(format!(
            "{}/hypr/{}/.socket2.sock",
            runtime_dir, instance
        ));
        if socket.exists() {
            Some(Self { socket_path: socket })
        } else {
            None
        }
    }

    /// Spawn a thread that listens for Hyprland IPC events.
    /// Returns a `ListenerHandle` that, when dropped, signals the thread to
    /// stop on its next reconnect attempt.
    ///
    /// Calls `on_config_reload` when `configreloaded` event fires.
    pub fn spawn_listener<F>(self, on_config_reload: F) -> ListenerHandle
    where
        F: Fn() + Send + Sync + 'static,
    {
        let callback = std::sync::Arc::new(on_config_reload);
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));

        let shutdown_clone = shutdown.clone();
        let handle = thread::spawn(move || {
            loop {
                // Check shutdown flag before every reconnect attempt
                if shutdown_clone.load(Ordering::Relaxed) {
                    break;
                }
                match self.connect_and_listen(&callback) {
                    Ok(_) => break, // Clean exit from Hyprland
                    Err(e) => {
                        eprintln!("[HVE] IPC listener error: {}, reconnecting in 2s...", e);
                        // Respect shutdown during the sleep too
                        for _ in 0..20 {
                            if shutdown_clone.load(Ordering::Relaxed) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            }
        });

        ListenerHandle {
            shutdown,
            _thread: handle,
        }
    }

    fn connect_and_listen<F>(&self, on_config_reload: &std::sync::Arc<F>) -> Result<(), String>
    where
        F: Fn() + Send + Sync,
    {
        let stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| format!("Cannot connect to Hyprland IPC: {}", e))?;

        // Set read timeout to detect disconnects
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .map_err(|e| format!("Cannot set timeout: {}", e))?;

        // socket2.sock auto-subscribes — no handshake needed

        // Throttle config reload — assemble.sh triggers another reload event
        let last_reload: Mutex<Option<Instant>> = Mutex::new(None);

        let reader = BufReader::new(stream);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    // Events come as: "eventname>>data"
                    if line.starts_with("configreloaded") {
                        let mut last = last_reload.lock().unwrap();
                        let now = Instant::now();
                        if last.is_none_or(|t| now.duration_since(t) > Duration::from_secs(3)) {
                            *last = Some(now);
                            println!("[HVE] Config reloaded, regenerating overlay...");
                            on_config_reload();
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // Timeout is normal — just continue listening
                    continue;
                }
                Err(e) => {
                    return Err(format!("Read error: {}", e));
                }
            }
        }

        Ok(())
    }
}

/// Start the IPC listener that auto-refreshes colors on config reload.
/// Returns a handle so the caller can keep it alive for the app lifetime.
pub fn start_listener(window: &crate::MainWindow, proj: PathBuf) -> ListenerHandle {
    let weak = window.as_weak();
    let proj_for_ipc = proj;
    if let Some(ipc) = HyprIpc::new() {
        let handle = ipc.spawn_listener(move || {
            // 1. Re-run assemble.sh when config reloads
            let script = proj_for_ipc
                .join("assets")
                .join("scripts")
                .join("assemble.sh");
            if script.exists() {
                let _ = std::process::Command::new("bash")
                    .arg(script)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }

            // 2. Refresh UI colors from the current color source
            let eng = Engine::new(&proj_for_ipc);
            if let Ok(colors) = eng.get_colors() {
                let w = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(window) = w.upgrade() {
                        let cfg = Config::load();
                        let resolved = theme::resolve_scheme(&colors, &cfg.theme);
                        theme::apply_theme(&window, &resolved);
                    }
                });
            }
        });
        println!("[HVE] Hyprland IPC listener started");
        handle
    } else {
        eprintln!("[HVE] Could not find Hyprland IPC socket");
        // Return a dummy handle that does nothing on drop (no listener to stop)
        let shutdown = std::sync::Arc::new(AtomicBool::new(true));
        ListenerHandle {
            shutdown,
            _thread: thread::spawn(move || {}),
        }
    }
}

// ─── Focus listener ────────────────────────────────────────────────
// Escucha el evento `activewindow` de Hyprland. Llama a on_focus_lost
// cuando la ventana activa deja de ser "Hyprland Visual Editor".
const HVE_WINDOW_TITLE: &str = "Hyprland Visual Editor";

/// Spawn a thread that listens for Hyprland `activewindow` events.
/// Returns a `ListenerHandle` that, when dropped, signals the thread to
/// stop on its next reconnect attempt.
///
/// Calls `on_focus_lost` when the active window is NOT HVE.
/// Calls `on_focus_gained` when the active window IS HVE.
pub fn spawn_focus_listener<F, G>(on_focus_lost: F, on_focus_gained: G) -> ListenerHandle
where
    F: Fn() + Send + Sync + 'static,
    G: Fn() + Send + Sync + 'static,
{
    let ipc = match HyprIpc::new() {
        Some(i) => i,
        None => {
            eprintln!("[HVE] Could not find Hyprland IPC socket (focus listener skipped)");
            // Return a dummy handle
            let shutdown = std::sync::Arc::new(AtomicBool::new(true));
            return ListenerHandle {
                shutdown,
                _thread: thread::spawn(move || {}),
            };
        }
    };

    tracing::info!("[focus] Focus listener connecting to {:?}", ipc.socket_path);

    let lost = std::sync::Arc::new(on_focus_lost);
    let gained = std::sync::Arc::new(on_focus_gained);
    let shutdown = std::sync::Arc::new(AtomicBool::new(false));

    let shutdown_clone = shutdown.clone();
    let handle = thread::spawn(move || {
        loop {
            // Check shutdown flag before every reconnect attempt
            if shutdown_clone.load(Ordering::Relaxed) {
                break;
            }
            match connect_and_listen_focus(&ipc, &lost, &gained) {
                Ok(_) => break,
                Err(e) => {
                    eprintln!("[HVE] Focus listener error: {}, reconnecting in 2s...", e);
                    // Respect shutdown during the sleep too
                    for _ in 0..20 {
                        if shutdown_clone.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        }
    });

    println!("[HVE] Hyprland focus listener started");
    ListenerHandle {
        shutdown,
        _thread: handle,
    }
}

fn connect_and_listen_focus<F, G>(
    ipc: &HyprIpc,
    on_focus_lost: &std::sync::Arc<F>,
    on_focus_gained: &std::sync::Arc<G>,
) -> Result<(), String>
where
    F: Fn() + Send + Sync,
    G: Fn() + Send + Sync,
{
    let stream = UnixStream::connect(&ipc.socket_path)
        .map_err(|e| format!("Cannot connect to Hyprland IPC: {}", e))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|e| format!("Cannot set timeout: {}", e))?;

    // socket2.sock auto-subscribes — no handshake needed

    tracing::info!("[focus] Connected and subscribed to Hyprland events");

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        match line {
            Ok(line) => {
                // `activewindow` event → "activewindow>>CLASS,TITLE"
                if line.starts_with("activewindow>>") {
                    tracing::debug!("[focus] activewindow event: {}", line);
                    let title = line.trim_start_matches("activewindow>>");
                    if title.contains(HVE_WINDOW_TITLE) {
                        tracing::debug!("[focus] Foco GANADO (HVE)");
                        on_focus_gained();
                    } else if !title.is_empty() {
                        tracing::debug!("[focus] Foco PERDIDO -> '{}'", title);
                        on_focus_lost();
                    }
                    // Si title está vacío, no actuar (transición entre escritorios)
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => {
                return Err(format!("Read error: {}", e));
            }
        }
    }
    Ok(())
}
