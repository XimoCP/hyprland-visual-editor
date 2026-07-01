use crate::engine::Engine;
use crate::theme;
use slint::ComponentHandle;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread;

pub struct HyprIpc {
    socket_path: PathBuf,
}

impl HyprIpc {
    pub fn new() -> Option<Self> {
        let instance = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| "/run/user/1000".to_string());
        let socket = PathBuf::from(format!(
            "{}/hypr/{}/.socket.sock",
            runtime_dir, instance
        ));
        if socket.exists() {
            Some(Self { socket_path: socket })
        } else {
            None
        }
    }

    /// Spawn a thread that listens for Hyprland IPC events.
    /// Calls `on_config_reload` when `configreloaded` event fires.
    pub fn spawn_listener<F>(self, on_config_reload: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let callback = std::sync::Arc::new(on_config_reload);
        thread::spawn(move || {
            loop {
                match self.connect_and_listen(&callback) {
                    Ok(_) => break, // Clean exit
                    Err(e) => {
                        eprintln!("[HVE] IPC listener error: {}, reconnecting in 2s...", e);
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                }
            }
        });
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

        let mut writer = stream
            .try_clone()
            .map_err(|e| format!("Cannot clone stream: {}", e))?;

        // Subscribe to events
        writer
            .write_all(b"subscribe\n")
            .map_err(|e| format!("Cannot subscribe: {}", e))?;

        let reader = BufReader::new(stream);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    // Events come as: "eventname>>data"
                    if line.starts_with("configreloaded") {
                        println!("[HVE] Config reloaded, regenerating overlay...");
                        on_config_reload();
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
pub fn start_listener(window: &crate::MainWindow, proj: PathBuf) {
    let weak = window.as_weak();
    let proj_for_ipc = proj;
    if let Some(ipc) = HyprIpc::new() {
        ipc.spawn_listener(move || {
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
                        theme::apply_theme(&window, &colors);
                    }
                });
            }
        });
        println!("[HVE] Hyprland IPC listener started");
    } else {
        eprintln!("[HVE] Could not find Hyprland IPC socket");
    }
}

// ─── Focus listener ────────────────────────────────────────────────
// Escucha el evento `activewindow` de Hyprland. Llama a on_focus_lost
// cuando la ventana activa deja de ser "Hyprland Visual Editor".
const HVE_WINDOW_TITLE: &str = "Hyprland Visual Editor";

/// Spawn a thread that listens for Hyprland `activewindow` events.
/// Calls `on_focus_lost` when the active window is NOT HVE.
/// Calls `on_focus_gained` when the active window IS HVE.
pub fn spawn_focus_listener<F, G>(on_focus_lost: F, on_focus_gained: G)
where
    F: Fn() + Send + Sync + 'static,
    G: Fn() + Send + Sync + 'static,
{
    let ipc = match HyprIpc::new() {
        Some(i) => i,
        None => {
            eprintln!("[HVE] Could not find Hyprland IPC socket (focus listener skipped)");
            return;
        }
    };

    let lost = std::sync::Arc::new(on_focus_lost);
    let gained = std::sync::Arc::new(on_focus_gained);

    thread::spawn(move || {
        loop {
            match connect_and_listen_focus(&ipc, &lost, &gained) {
                Ok(_) => break,
                Err(e) => {
                    eprintln!("[HVE] Focus listener error: {}, reconnecting in 2s...", e);
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }
    });
    println!("[HVE] Hyprland focus listener started");
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

    let mut writer = stream
        .try_clone()
        .map_err(|e| format!("Cannot clone stream: {}", e))?;
    writer
        .write_all(b"subscribe\n")
        .map_err(|e| format!("Cannot subscribe: {}", e))?;

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        match line {
            Ok(line) => {
                // `activewindow` event → "activewindow>>window_title"
                if line.starts_with("activewindow>>") {
                    let title = line.trim_start_matches("activewindow>>");
                    if title.contains(HVE_WINDOW_TITLE) {
                        on_focus_gained();
                    } else if !title.is_empty() {
                        // Cualquier otra ventana activa → perdimos foco
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
