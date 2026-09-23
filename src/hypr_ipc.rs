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
        // Prefer the real XDG runtime dir (honors XDG_RUNTIME_DIR and the
        // per-UID /run/user/<uid> fallback) instead of a hardcoded /run/user/1000.
        let runtime_dir = dirs::runtime_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        let socket = runtime_dir
            .join("hypr")
            .join(&instance)
            .join(".socket2.sock");
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
                        tracing::warn!("[HVE] IPC listener error: {}, reconnecting in 2s...", e);
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
                        // Line-level handling, BEFORE the throttle: masks +
                        // fullscreen restore always run; the heavy reload at
                        // most once per 3s. See on_configreloaded_line.
                        handle_configreloaded_line(
                            &last_reload,
                            Instant::now(),
                            on_configreloaded_line,
                            || {
                                tracing::info!("[HVE] Config reloaded, regenerating overlay...");
                                on_config_reload();
                            },
                        );
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

/// One `configreloaded` line, headless-testable: the line-level actions
/// (masks + fullscreen restore) ALWAYS run; the heavy regenerating reload
/// runs at most once per 3s window. Returns whether the throttled callback
/// ran.
fn handle_configreloaded_line<F, G>(
    last_reload: &Mutex<Option<Instant>>,
    now: Instant,
    mut on_line: F,
    mut throttled: G,
) -> bool
where
    F: FnMut(),
    G: FnMut(),
{
    // Masks + fullscreen restore run BEFORE the throttle: every reload
    // (assemble, noctalia, watcher) wipes the runtime animations/blur
    // overrides mid-fade and re-floats HVE, dropping its immersive
    // fullscreen. Both are cheap and idempotent, and a reload that never
    // reaches the throttled callback (assemble re-trigger) still needs its
    // masks back and its fullscreen back.
    on_line();
    let mut last = last_reload.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_none_or(|t| now.duration_since(t) > Duration::from_secs(3)) {
        *last = Some(now);
        throttled();
        true
    } else {
        false
    }
}

/// Line-level `configreloaded` actions: re-apply the transition masks and
/// consult the fullscreen-restore decision. Runs on the IPC listener
/// thread, so the restore hops to the UI thread — its step-2 timer uses
/// `slint::Timer`, and timers are thread-local (a timer created on the
/// listener thread never fires; verified in i-slint-core 1.17.0
/// `timers.rs::single_shot`). Best-effort and bounded: before the event
/// loop runs the hop errors and is ignored — the masks still apply
/// directly and the restore waits for the next reload line. The failure is
/// logged honestly (never silently dropped; nothing is aborted).
fn on_configreloaded_line() {
    crate::reapply_theme_masks();
    if let Err(e) = slint::invoke_from_event_loop(crate::reassert_fullscreen_after_reload) {
        tracing::debug!(
            "[reassert] fullscreen restore UI-thread hop failed (pre-loop): {}",
            e
        );
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

            // 3. Line-level masks + fullscreen restore only: the restore is
            //    consultation-gated and debounced, so it cannot re-arm the
            //    4-actor race the UNCONDITIONAL event-path cycle caused
            //    (event return vs step1/step2 timers, incl. an animated move
            //    AFTER fade-in). The single-owner finale still owns every
            //    cycle inside a theme transition; this path only re-applies
            //    the masks each reload wipes and restores the immersive
            //    fullscreen a reload dropped (visible + not fullscreen +
            //    outside a transition + outside the 1.5s debounce window).
            //    Line-level handling happens in connect_and_listen
            //    (bypasses the 3s throttle) via on_configreloaded_line.
            let _ = ();
        });
        tracing::info!("[HVE] Hyprland IPC listener started");
        handle
    } else {
        tracing::warn!("[HVE] Could not find Hyprland IPC socket");
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
            tracing::warn!("[HVE] Could not find Hyprland IPC socket (focus listener skipped)");
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
                    tracing::warn!("[HVE] Focus listener error: {}, reconnecting in 2s...", e);
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

    tracing::info!("[HVE] Hyprland focus listener started");
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── configreloaded line handling (fullscreen survives a reload) ────

    /// The line-level handler must run its actions (masks + fullscreen
    /// restore) on EVERY `configreloaded` line, while the heavy throttled
    /// reload runs at most once per 3s window.
    #[test]
    fn configreloaded_line_always_runs_restore_before_the_throttle() {
        let last_reload: Mutex<Option<Instant>> = Mutex::new(None);
        let mut on_line = 0;
        let mut heavy = 0;

        let ran = handle_configreloaded_line(
            &last_reload,
            Instant::now(),
            || on_line += 1,
            || heavy += 1,
        );
        assert!(ran, "the first line inside a 3s window runs the throttled reload");
        assert_eq!(on_line, 1, "line actions (masks + restore) run on EVERY line");
        assert_eq!(heavy, 1);

        let ran2 = handle_configreloaded_line(
            &last_reload,
            Instant::now(),
            || on_line += 1,
            || heavy += 1,
        );
        assert!(!ran2, "a second line inside the 3s window is throttled");
        assert_eq!(on_line, 2, "line actions still run on the throttled line");
        assert_eq!(heavy, 1, "the heavy reload does not run again inside the window");

        let ran3 = handle_configreloaded_line(
            &last_reload,
            Instant::now() + Duration::from_secs(4),
            || on_line += 1,
            || heavy += 1,
        );
        assert!(ran3, "a line after the 3s window reloads again");
        assert_eq!(on_line, 3);
        assert_eq!(heavy, 2);
    }

    /// The line-level `configreloaded` handler must log a failed UI-thread
    /// hop honestly (bounded and best-effort) instead of silently
    /// discarding the error — the unit's logging contract. The check is
    /// scoped to the `on_configreloaded_line` body (comment-stripped), so
    /// neither the test's own assertion text nor a commented-out call can
    /// satisfy it (house precedent).
    #[test]
    fn configreloaded_hop_failure_is_logged_not_silently_dropped() {
        let src = std::fs::read_to_string("src/hypr_ipc.rs").expect("src/hypr_ipc.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // Scope to the on_configreloaded_line body (up to the next fn).
        let start = code
            .find("fn on_configreloaded_line")
            .expect("on_configreloaded_line must exist");
        let body = &code[start..];
        let end = body
            .find("\npub fn ")
            .or_else(|| body.find("\nfn "))
            .unwrap_or(body.len());
        let body = &body[..end];
        // The restore must still hop to the UI thread.
        assert!(
            body.contains("invoke_from_event_loop(crate::reassert_fullscreen_after_reload)"),
            "the fullscreen restore must hop to the UI thread"
        );
        // The hop result must be inspected and logged — never `let _ =`.
        assert!(
            !body.contains("let _ = slint::invoke_from_event_loop"),
            "a silent hop failure contradicts the unit's logging contract"
        );
        assert!(
            body.contains("tracing::debug!") || body.contains("tracing::warn!"),
            "the hop failure must be logged at debug or warn"
        );
    }
}
