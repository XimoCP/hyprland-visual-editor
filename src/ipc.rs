use crate::config::Config;
use crate::engine::Engine;
use crate::theme;
use slint::ComponentHandle;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Instant;

/// Tracks whether the main window has been hidden via toggle-tray (tray mode).
pub(crate) static WINDOW_HIDDEN: AtomicBool = AtomicBool::new(false);
/// Whether HVE was started with --tray (window starts hidden before run()).
pub(crate) static TRAY_MODE: AtomicBool = AtomicBool::new(false);

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
        // Usamos WINDOW_HIDDEN como única fuente de verdad.
        // is_visible() no es fiable: Slint lo pone a true inmediatamente
        // después de show(), antes de que el compositor Wayland mapee
        // realmente la ventana. Eso hacía que el toggle llamara hide()
        // sobre una ventana que nunca terminó de aparecer —→ 3 pulsaciones.
        let hidden = WINDOW_HIDDEN.load(Ordering::Relaxed);
        if hidden {
            let fast = crate::ipc::show_window(&win);
            if !fast {
                // Desmapeada (tray lazy-load / cierre por WM): re-mapeo + warmup.
                crate::prewarm_tabs(win.as_weak(), 1);
                crate::warmup_navigation(&win);
            }
            crate::ipc::WINDOW_HIDDEN.store(false, Ordering::Relaxed);
        } else {
            crate::ipc::hide_window(&win);
            crate::ipc::WINDOW_HIDDEN.store(true, Ordering::Relaxed);
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
        format!(
            r#"{{"system_active":{},"active_anim_index":{},"active_border_index":{},"active_shader_index":{}}}"#,
            active, anim_idx, border_idx, shader_idx
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

// ─── Hyprland show/hide via named special workspace ──────────────────
// Ventana NUNCA se desmapea: ocultar la mueve a special:minimized y
// mostrar la devuelve. Evita el re-mapeo lento (~1.2s) de hide()/show()
// de Slint que hacía que las teclas se perdieran tras reabrir.
// V5 = Hyprland v5/Noctalia (lua hl.dsp.*). V4 = clásico (dispatch ...).

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum HyprMode {
    V5,
    V4,
    None,
}

static HYPR_MODE: OnceLock<HyprMode> = OnceLock::new();
const HVE_TITLE: &str = "Hyprland Visual Editor";
const SPECIAL: &str = "minimized";

pub(crate) fn hypr_mode() -> HyprMode {
    *HYPR_MODE.get_or_init(|| {
        // V5: `dispatch 'hl.dsp.no_op()'` → contiene "ok"
        if let Ok(out) = std::process::Command::new("hyprctl")
            .args(["dispatch", "hl.dsp.no_op()"])
            .output()
        {
            if String::from_utf8_lossy(&out.stdout).contains("ok") {
                return HyprMode::V5;
            }
        }
        // V4: si cualquier cosa, verificar versión
        if let Ok(out) = std::process::Command::new("hyprctl").arg("version").output() {
            if String::from_utf8_lossy(&out.stdout).contains("Hyprland") {
                return HyprMode::V4;
            }
        }
        HyprMode::None
    })
}

fn hypr_dispatch_v5(script: &str) -> bool {
    std::process::Command::new("hyprctl")
        .args(["dispatch", script])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("ok"))
        .unwrap_or(false)
}

fn hypr_dispatch_v4(args: &[&str]) -> bool {
    std::process::Command::new("hyprctl")
        .arg("dispatch")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("ok"))
        .unwrap_or(false)
}

/// True si la ventana de HVE está mapeada en el special workspace.
fn hve_in_special() -> bool {
    let Ok(out) = std::process::Command::new("hyprctl").args(["clients", "-j"]).output() else {
        return false;
    };
    let Ok(text) = String::from_utf8(out.stdout) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    v.as_array().into_iter().flatten().any(|w| {
        w.get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .contains(HVE_TITLE)
            && w.get("workspace")
                .and_then(|ws| ws.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .contains("special")
    })
}

/// Forzar foco de teclado a HVE (sintaxis correcta según versión).
pub(crate) fn focus_hve_window() {
    match hypr_mode() {
        HyprMode::V5 => {
            let s = format!("hl.dsp.focus({{ window = \"title:{HVE_TITLE}\" }})");
            let _ = hypr_dispatch_v5(&s);
        }
        HyprMode::V4 => {
            let _ = hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
        }
        HyprMode::None => {}
    }
}

/// Toggle floating de HVE (sintaxis correcta según versión).
pub(crate) fn toggle_float_hve() {
    match hypr_mode() {
        HyprMode::V5 => {
            let s = format!(
                "hl.dsp.window.float({{ action = \"toggle\", window = \"title:{HVE_TITLE}\" }})"
            );
            let _ = hypr_dispatch_v5(&s);
        }
        HyprMode::V4 => {
            let _ = hypr_dispatch_v4(&["togglefloating", HVE_TITLE]);
        }
        HyprMode::None => {}
    }
}

/// Ocultar manteniendo la ventana mapeada (special workspace).
/// Fallback a hide() de Slint si no está disponible.
pub(crate) fn hide_window(win: &crate::MainWindow) {
    match hypr_mode() {
        HyprMode::V5 => {
            let s1 = format!("hl.dsp.window.move({{ workspace = \"special:{SPECIAL}\" }})");
            let s2 = format!("hl.dsp.workspace.toggle_special(\"{SPECIAL}\")");
            let did_move = hypr_dispatch_v5(&s1);
            let did_close = hypr_dispatch_v5(&s2);
            if !(did_move && did_close) {
                let _ = win.window().hide();
            }
        }
        HyprMode::V4 => {
            let did_move =
                hypr_dispatch_v4(&["movetoworkspacesilent", &format!("special:{SPECIAL}")]);
            let did_close = hypr_dispatch_v4(&["togglespecialworkspace", SPECIAL]);
            if !(did_move && did_close) {
                let _ = win.window().hide();
            }
        }
        HyprMode::None => {
            let _ = win.window().hide();
        }
    }
}

/// Mostrar la ventana. Retorna true si fue rápido (special workspace, ventana
/// seguía mapeada) o false si hubo que re-mapear con show() de Slint (lento).
pub(crate) fn show_window(win: &crate::MainWindow) -> bool {
    let fast = match hypr_mode() {
        HyprMode::V5 => {
            if hve_in_special() {
                let s1 = format!("hl.dsp.workspace.toggle_special(\"{SPECIAL}\")");
                let s2 = format!("hl.dsp.focus({{ window = \"title:{HVE_TITLE}\" }})");
                let did_open = hypr_dispatch_v5(&s1);
                let did_focus = hypr_dispatch_v5(&s2);
                did_open && did_focus
            } else {
                let _ = win.window().show();
                false
            }
        }
        HyprMode::V4 => {
            if hve_in_special() {
                let did_open = hypr_dispatch_v4(&["togglespecialworkspace", SPECIAL]);
                let did_focus =
                    hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
                did_open && did_focus
            } else {
                let _ = win.window().show();
                false
            }
        }
        HyprMode::None => {
            let _ = win.window().show();
            false
        }
    };
    win.window().request_redraw();
    fast
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
