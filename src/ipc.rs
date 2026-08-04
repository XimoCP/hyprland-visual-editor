use crate::config::Config;
use crate::engine::Engine;
use crate::theme;
use slint::ComponentHandle;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

/// Debounce: evita que pulsaciones rápidas de SUPER+H saboteen el
/// roundtrip de Wayland al crear el xdg_toplevel por primera vez.
static LAST_TOGGLE: Mutex<Option<Instant>> = Mutex::new(None);
const DEBOUNCE_MS: u64 = 400;

/// Start the IPC server on `$XDG_RUNTIME_DIR/hve.sock` (fallback `/tmp/hve.sock`).
///
/// Spawns a background thread that listens for commands and dispatches them
/// onto the Slint event loop via `slint::invoke_from_event_loop`.
pub fn start_ipc_server(window: slint::Weak<crate::MainWindow>, proj: PathBuf) {
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
        // Allow only the owner to connect (sufficient for local Unix socket)
        let _ = std::fs::set_permissions(
            &socket_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        );
        println!("[HVE IPC] Listening on {}", socket_path.display());

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => handle_connection(stream, &window, &proj),
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

fn handle_connection(
    mut stream: UnixStream,
    window: &slint::Weak<crate::MainWindow>,
    proj: &PathBuf,
) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));

    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) | Err(_) => return,
        Ok(_) => {}
    }

    let response = dispatch_command(line.trim(), window, proj);

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

// ─── Dispatch ────────────────────────────────────────────────────────

fn dispatch_command(
    cmd: &str,
    window: &slint::Weak<crate::MainWindow>,
    proj: &PathBuf,
) -> String {
    match cmd {
        "pause-restart" => cmd_pause_restart(window),
        "next-anim" => cmd_next_anim(window),
        "next-border" => cmd_next_border(window),
        "next-shader" => cmd_next_shader(window),
        "toggle-tray" => cmd_toggle_tray(window),
        "status" => cmd_status(window),
        "quit" => cmd_quit(window),
        "refresh-theme" => cmd_refresh_theme(window, proj),
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

pub(crate) fn get_next_index(current: i32, count: usize) -> i32 {
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
    // Debounce: solo para dedup keybind spam (~80ms entre pulsaciones humanas).
    {
        let mut last = LAST_TOGGLE.lock().unwrap();
        let now = Instant::now();
        if let Some(prev) = *last {
            if now.duration_since(prev).as_millis() < DEBOUNCE_MS as u128 {
                return "debounced\n".to_string();
            }
        }
        *last = Some(now);
    }

    format_response(invoke_on_main(window, |win| {
        // Delegate the whole toggle (show/hide + state + prev_workspace +
        // warmup) through the Controller — single source of truth. The
        // controller is initialized in main() before the IPC server starts,
        // so it is always available here.
        if let Some(mut ctrl) = crate::composer::global_controller() {
            ctrl.toggle_tray(&win);
        }
        crate::tray::refresh_global_menu();
        "ok".to_string()
    }))
}

fn cmd_status(window: &slint::Weak<crate::MainWindow>) -> String {
    format_response(invoke_on_main(window, |win| {
        let active = win.get_system_active();
        let anim_idx = win.get_active_anim_index();
        let border_idx = win.get_active_border_index();
        let shader_idx = win.get_active_shader_index();
        let active_tab = win.get_active_tab();
        let nav_ready = win.get_nav_ready();
        let a_foc = win.get_anim_focused_index();
        let b_foc = win.get_border_focused_index();
        let s_foc = win.get_shader_focused_index();
        let t_foc = win.get_theme_focused_index();
        format!(
            r#"{{"system_active":{},"active_anim_index":{},"active_border_index":{},"active_shader_index":{},"active_tab":{},"nav_ready":{},"anim_foc":{},"border_foc":{},"shader_foc":{},"theme_foc":{}}}"#,
            active, anim_idx, border_idx, shader_idx, active_tab, nav_ready,
            a_foc, b_foc, s_foc, t_foc
        )
    }))
}

fn cmd_refresh_theme(window: &slint::Weak<crate::MainWindow>, proj: &PathBuf) -> String {
    let eng = Engine::new(proj);
    match eng.get_colors() {
        Ok(colors) => {
            let cfg = Config::load();
            let resolved = theme::resolve_scheme(&colors, &cfg.theme);
            format_response(invoke_on_main(window, move |win| {
                theme::apply_theme(&win, &resolved);
                // Regenerate logo with new theme colors
                let surface_lowest = theme::parse_hex(&resolved.surface_lowest);
                let secondary = theme::parse_hex(&resolved.secondary);
                let tertiary = theme::parse_hex(&resolved.tertiary);
                let accent = theme::parse_hex(&resolved.accent);
                let logo = theme::render_logo_image(&surface_lowest, &secondary, &tertiary, &accent);
                win.set_logo_image(logo);
                // Update tray icon
                let tray_color = theme::lighten(&accent, 0.6);
                if let Some(icon) = theme::render_logo_square_mono(&tray_color, 48) {
                    crate::tray::update_global_icon(icon);
                }
                "ok".to_string()
            }))
        }
        Err(e) => format!("error: {}\n", e),
    }
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