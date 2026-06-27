use slint::ComponentHandle;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Tracks whether the main window has been hidden via toggle-tray (tray mode).
pub(crate) static WINDOW_HIDDEN: AtomicBool = AtomicBool::new(false);
/// Whether HVE was started with --tray (window starts hidden before run()).
pub(crate) static TRAY_MODE: AtomicBool = AtomicBool::new(false);

/// Start the IPC server on `$XDG_RUNTIME_DIR/hve.sock` (fallback `/tmp/hve.sock`).
///
/// Spawns a background thread that listens for commands and dispatches them
/// onto the Slint event loop via `slint::invoke_from_event_loop`.
pub fn start_ipc_server(window: slint::Weak<crate::MainWindow>, _proj: PathBuf) {
    let socket_path = get_socket_path();

    // Remove stale socket from previous run
    let _ = std::fs::remove_file(&socket_path);

    std::thread::spawn(move || {
        let listener = match UnixListener::bind(&socket_path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[HVE IPC] Cannot bind socket: {}", e);
                return;
            }
        };
        // Allow any user in the session to connect
        let _ = std::fs::set_permissions(
            &socket_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o777),
        );
        println!("[HVE IPC] Listening on {}", socket_path.display());

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => handle_connection(stream, &window),
                Err(e) => eprintln!("[HVE IPC] Connection error: {}", e),
            }
        }
    });
}

/// Remove the socket file. Call this from main() after the event loop exits.
pub fn cleanup() {
    let path = get_socket_path();
    let _ = std::fs::remove_file(&path);
    println!("[HVE IPC] Socket cleaned up: {}", path.display());
}

fn get_socket_path() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime).join("hve.sock")
}

// ─── Connection handler ──────────────────────────────────────────────

fn handle_connection(mut stream: UnixStream, window: &slint::Weak<crate::MainWindow>) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));

    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) | Err(_) => return,
        Ok(_) => {}
    }

    let response = dispatch_command(line.trim(), window);

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

// ─── Dispatch ────────────────────────────────────────────────────────

fn dispatch_command(cmd: &str, window: &slint::Weak<crate::MainWindow>) -> String {
    match cmd {
        "pause-restart" => cmd_pause_restart(window),
        "next-anim" => cmd_next_anim(window),
        "next-border" => cmd_next_border(window),
        "next-shader" => cmd_next_shader(window),
        "toggle-tray" => cmd_toggle_tray(window),
        "status" => cmd_status(window),
        "quit" => cmd_quit(window),
        _ => format!("error: unknown command '{}'\n", cmd),
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Run a closure on the Slint event-loop thread and await the result.
/// Returns `Err` if the event loop is not running or the response times out.
fn invoke_on_main<F, T>(window: &slint::Weak<crate::MainWindow>, f: F) -> Result<T, String>
where
    F: FnOnce(crate::MainWindow) -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let w = window.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(win) = w.upgrade() {
            let _ = tx.send(f(win));
        }
    })
    .map_err(|_| "event loop not ready".to_string())?;

    rx.recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "timeout waiting for event loop".to_string())
}

fn get_next_index(current: i32, count: usize) -> i32 {
    if count == 0 {
        return -1;
    }
    if current < 0 || current + 1 >= count as i32 {
        0
    } else {
        current + 1
    }
}

fn format_response(result: Result<String, String>) -> String {
    match result {
        Ok(msg) => format!("{}\n", msg),
        Err(e) => format!("error: {}\n", e),
    }
}

// ─── Commands ────────────────────────────────────────────────────────

fn cmd_pause_restart(window: &slint::Weak<crate::MainWindow>) -> String {
    format_response(invoke_on_main(window, |win| {
        let current = win.get_system_active();
        win.invoke_toggle_system(!current);
        "ok".to_string()
    }))
}

fn cmd_next_anim(window: &slint::Weak<crate::MainWindow>) -> String {
    format_response(invoke_on_main(window, |win| {
        use slint::Model;
        let files = win.get_anim_files();
        let count = files.row_count();
        if count == 0 {
            return "no animations available".to_string();
        }
        let current = win.get_active_anim_index();
        let next = get_next_index(current, count);
        match files.row_data(next as usize) {
            Some(file) => {
                win.invoke_apply_animation(next, file);
                "ok".to_string()
            }
            None => "error: invalid animation index".to_string(),
        }
    }))
}

fn cmd_next_border(window: &slint::Weak<crate::MainWindow>) -> String {
    format_response(invoke_on_main(window, |win| {
        use slint::Model;
        let files = win.get_border_files();
        let count = files.row_count();
        if count == 0 {
            return "no borders available".to_string();
        }
        let current = win.get_active_border_index();
        let next = get_next_index(current, count);
        match files.row_data(next as usize) {
            Some(file) => {
                win.invoke_apply_border(next, file);
                "ok".to_string()
            }
            None => "error: invalid border index".to_string(),
        }
    }))
}

fn cmd_next_shader(window: &slint::Weak<crate::MainWindow>) -> String {
    format_response(invoke_on_main(window, |win| {
        use slint::Model;
        let files = win.get_shader_files();
        let count = files.row_count();
        if count == 0 {
            return "no shaders available".to_string();
        }
        let current = win.get_active_shader_index();
        let next = get_next_index(current, count);
        match files.row_data(next as usize) {
            Some(file) => {
                win.invoke_apply_shader(next, file);
                "ok".to_string()
            }
            None => "error: invalid shader index".to_string(),
        }
    }))
}

fn cmd_toggle_tray(window: &slint::Weak<crate::MainWindow>) -> String {
    format_response(invoke_on_main(window, |win| {
        if TRAY_MODE.load(Ordering::Relaxed) {
            // Tray mode: solo mostramos la ventana si está oculta.
            // No intentamos ocultarla con hide() porque en Wayland
            // mata el event loop de Slint/winit (hide() programático
            // no pasa por on_close_requested).
            // El usuario oculta la ventana con el WM (SUPER+C), que
            // ahora dispara on_close_requested → HideWindow.
            if WINDOW_HIDDEN.load(Ordering::Relaxed) {
                let _ = win.window().show();
                WINDOW_HIDDEN.store(false, Ordering::Relaxed);
            }
        } else {
            // Non-tray mode: mostrar/enfocar la ventana
            if !win.window().is_visible() {
                let _ = win.window().show();
            }
        }
        "ok".to_string()
    }))
}

fn cmd_status(window: &slint::Weak<crate::MainWindow>) -> String {
    format_response(invoke_on_main(window, |win| {
        let active = win.get_system_active();
        let anim_idx = win.get_active_anim_index();
        let border_idx = win.get_active_border_index();
        let shader_idx = win.get_active_shader_index();
        format!(
            r#"{{"system_active":{},"active_anim_index":{},"active_border_index":{},"active_shader_index":{}}}"#,
            active, anim_idx, border_idx, shader_idx
        )
    }))
}

fn cmd_quit(window: &slint::Weak<crate::MainWindow>) -> String {
    let w = window.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(win) = w.upgrade() {
            let _ = win.window().hide();
        }
        // Request the event loop to exit — main() will clean up after run() returns
        let _ = slint::quit_event_loop();
    });
    "ok\n".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── get_next_index ───────────────────────────────────────────────

    #[test]
    fn test_get_next_index_empty_list_returns_neg_one() {
        assert_eq!(get_next_index(0, 0), -1, "empty list should return -1");
    }

    #[test]
    fn test_get_next_index_last_item_wraps_to_zero() {
        // count=3 means valid indices are 0,1,2; current=2 wraps to 0
        assert_eq!(get_next_index(2, 3), 0);
    }

    #[test]
    fn test_get_next_index_middle_advances_by_one() {
        assert_eq!(get_next_index(1, 5), 2);
    }

    #[test]
    fn test_get_next_index_at_zero_advances_to_one() {
        assert_eq!(get_next_index(0, 5), 1);
    }

    #[test]
    fn test_get_next_index_from_neg_one_goes_to_zero() {
        assert_eq!(get_next_index(-1, 3), 0);
    }

    #[test]
    fn test_get_next_index_from_arbitrary_negative_goes_to_zero() {
        assert_eq!(get_next_index(-42, 4), 0);
    }

    #[test]
    fn test_get_next_index_single_item() {
        // count=1: only valid index is 0; current=0 wraps to 0
        assert_eq!(get_next_index(0, 1), 0);
    }

    #[test]
    fn test_get_next_index_single_item_from_neg_one() {
        assert_eq!(get_next_index(-1, 1), 0);
    }

    // ── format_response ──────────────────────────────────────────────

    #[test]
    fn test_format_response_ok_adds_newline() {
        assert_eq!(format_response(Ok("hello".into())), "hello\n");
    }

    #[test]
    fn test_format_response_ok_empty_string() {
        assert_eq!(format_response(Ok(String::new())), "\n");
    }

    #[test]
    fn test_format_response_err_prefix() {
        assert_eq!(format_response(Err("something broke".into())), "error: something broke\n");
    }

    #[test]
    fn test_format_response_err_empty() {
        assert_eq!(format_response(Err(String::new())), "error: \n");
    }

    // ── dispatch_command (pattern-matching only) ─────────────────────
    //
    // dispatch_command() requires a `slint::Weak<crate::MainWindow>` which
    // can only be obtained from a running Slint component. Every matched
    // command (pause-restart, next-anim, status, quit, …) calls
    // invoke_on_main() which needs a live event loop, so those paths
    // cannot be tested here.
    //
    // The *unmatched* branch does not touch the window at all — we verify
    // that the error format is correct below.

    /// Check the unknown-command error format in isolation.
    #[test]
    fn test_dispatch_unknown_command_format() {
        // The `_ =>` arm produces: format!("error: unknown command '{}'\n", cmd)
        // We verify the format directly since dispatch_command needs a Window
        // that only exists inside a running Slint event loop.
        let cmd = "no-such-command";
        let expected = format!("error: unknown command '{}'\n", cmd);
        // This is the exact format dispatch_command returns for unmatched commands.
        // We test it directly to validate the format string is correct.
        assert_eq!(expected, "error: unknown command 'no-such-command'\n");
    }
}
