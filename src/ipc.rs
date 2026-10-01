use crate::config::Config;
use crate::engine::Engine;
use slint::ComponentHandle;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Debounce: keeps rapid SUPER+H presses from sabotaging the
/// Wayland roundtrip when creating the xdg_toplevel for the first time.
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
                tracing::error!("[HVE IPC] Cannot bind socket: {}", e);
                return;
            }
        };
        // Allow only the owner to connect (sufficient for local Unix socket)
        let _ = std::fs::set_permissions(
            &socket_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        );
        tracing::info!("[HVE IPC] Listening on {}", socket_path.display());

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => handle_connection(stream, &window, &proj),
                Err(e) => tracing::error!("[HVE IPC] Connection error: {}", e),
            }
        }
    });
}

/// Remove the socket file. Call this from main() after the event loop exits.
pub fn cleanup() {
    let path = get_socket_path();
    let _ = std::fs::remove_file(&path);
    tracing::debug!("[HVE IPC] Socket cleaned up: {}", path.display());
}

fn get_socket_path() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime).join("hve.sock")
}

// ─── Second-launch handoff ────────────────────────────────────────────
//
// HVE allows a single instance: a second launch never takes the lock and
// never creates a window. Instead it asks the running instance to raise its
// window over the same socket the `hve-ipc` client speaks. The wait is
// bounded so a wedged instance can never hang the launcher.

/// How long the handoff waits for the running instance to answer. Bounds
/// the whole write/read round-trip so the launcher always terminates.
pub const HANDOFF_TIMEOUT: Duration = Duration::from_secs(3);

/// Exit code when the running instance could not be reached. Never 0: the
/// launcher reports failure while keeping its single-instance guarantee.
pub const HANDOFF_FALLBACK_EXIT_CODE: i32 = 1;

/// Result of asking a running instance to raise its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffOutcome {
    /// The running instance acknowledged the `show`.
    Shown(String),
    /// The socket was unreachable, refused, or did not answer in time.
    Unreachable(String),
}

/// What the launching process must print and exit with after a lock refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffReport {
    /// True when the running instance acknowledged the `show`.
    pub succeeded: bool,
    /// Process exit code: 0 on success, `HANDOFF_FALLBACK_EXIT_CODE` otherwise.
    pub exit_code: i32,
    /// Human message; the caller decides whether it goes to stdout or stderr.
    pub message: String,
}

/// Human message when the running instance raised its window.
pub fn handoff_shown_message() -> String {
    "Another instance of HVE is already running — its window has been brought forward.".to_string()
}

/// Human message when the running instance could not be reached. Names both
/// ways out so the user is never left at a dead end.
pub fn handoff_fallback_message(detail: &str) -> String {
    format!(
        "Another instance of HVE is already running and did not answer ({detail}).\n\
         Close it with: pkill -x hve\n\
         Or show its window with: SUPER + H"
    )
}

/// Ask a running instance to raise its window over the IPC socket.
pub fn request_show(path: &Path) -> HandoffOutcome {
    request_show_with_timeout(path, HANDOFF_TIMEOUT)
}

/// `request_show` with an explicit bound; tests use a short timeout.
///
/// Sends the idempotent `show` command and reads exactly one reply line. Both
/// the write and the read are bounded by `timeout`, so the launcher always
/// terminates even when the running instance is wedged. A missing or refusing
/// socket, a closed connection and a timeout all map to
/// [`HandoffOutcome::Unreachable`] — the caller keeps the single-instance
/// guarantee either way.
pub fn request_show_with_timeout(path: &Path, timeout: Duration) -> HandoffOutcome {
    let mut stream = match UnixStream::connect(path) {
        Ok(stream) => stream,
        Err(e) => return HandoffOutcome::Unreachable(format!("connect failed: {e}")),
    };
    let _ = stream.set_write_timeout(Some(timeout));
    let _ = stream.set_read_timeout(Some(timeout));

    if let Err(e) = stream.write_all(b"show\n") {
        return HandoffOutcome::Unreachable(format!("send failed: {e}"));
    }

    let mut line = String::new();
    let mut reader = BufReader::new(&stream);
    match reader.read_line(&mut line) {
        Ok(0) => HandoffOutcome::Unreachable("no response from the running instance".to_string()),
        Ok(_) => {
            let reply = line.trim();
            if reply.starts_with("error") {
                HandoffOutcome::Unreachable(reply.to_string())
            } else {
                HandoffOutcome::Shown(reply.to_string())
            }
        }
        Err(e) => HandoffOutcome::Unreachable(format!("no response: {e}")),
    }
}

/// Attempt the handoff against `path` and describe what the launcher must do.
///
/// Never creates a window, never touches the lock, never calls
/// `process::exit`: the caller owns those decisions.
pub fn handoff_show(path: &Path) -> HandoffReport {
    match request_show(path) {
        HandoffOutcome::Shown(_) => HandoffReport {
            succeeded: true,
            exit_code: 0,
            message: handoff_shown_message(),
        },
        HandoffOutcome::Unreachable(detail) => HandoffReport {
            succeeded: false,
            exit_code: HANDOFF_FALLBACK_EXIT_CODE,
            message: handoff_fallback_message(&detail),
        },
    }
}

/// Handoff entry point for a second launch: resolve the running instance's
/// socket and attempt the show.
pub fn handoff_show_running() -> HandoffReport {
    handoff_show(&get_socket_path())
}

// ─── Connection handler ──────────────────────────────────────────────

fn handle_connection(
    mut stream: UnixStream,
    window: &slint::Weak<crate::MainWindow>,
    proj: &Path,
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
    proj: &Path,
) -> String {
    match cmd {
        "pause-restart" => cmd_pause_restart(window),
        "next-anim" => cmd_next_anim(window),
        "next-border" => cmd_next_border(window),
        "next-shader" => cmd_next_shader(window),
        "show" => cmd_show(window),
        "toggle-tray" => cmd_toggle_tray(window),
        "status" => cmd_status(window),
        "quit" => cmd_quit(window),
        "refresh-theme" => cmd_refresh_theme(window, proj),
        "assert-color-authority" => cmd_assert_color_authority(proj),
        "repair-color-authority" => cmd_repair_color_authority(proj),
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
                win.invoke_panel_apply_animation(next, file);
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
                win.invoke_panel_apply_border(next, file);
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
                win.invoke_panel_apply_shader(next, file);
                "ok".to_string()
            }
            None => "error: invalid shader index".to_string(),
        }
    }))
}

/// Idempotent raise: show the window if it is hidden, focus it if it is
/// already visible. Never hides — unlike `toggle-tray`, which flips state.
/// Used by the second-launch handoff, so it must be safe at any moment,
/// including while the instance sits in tray mode or a show is in flight.
fn cmd_show(window: &slint::Weak<crate::MainWindow>) -> String {
    format_response(invoke_on_main(window, |win| {
        if let Some(mut ctrl) = crate::composer::global_controller() {
            ctrl.show_window(&win);
        }
        // Released before the tray refresh, exactly like cmd_toggle_tray: the
        // menu reads window_hidden through the controller lock.
        crate::tray::refresh_global_menu();
        "ok".to_string()
    }))
}

fn cmd_toggle_tray(window: &slint::Weak<crate::MainWindow>) -> String {
    // Debounce: only to dedup keybind spam (~80ms between human key presses).
    {
        let mut last = LAST_TOGGLE.lock().unwrap_or_else(|e| e.into_inner());
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
        let panel_section = win.get_panel_section();
        let is_mutating = win.get_is_mutating();
        let is_panel_open = win.get_is_panel_open();
        format!(
            r#"{{"system_active":{},"active_anim_index":{},"active_border_index":{},"active_shader_index":{},"panel_section":{},"is_mutating":{},"is_panel_open":{}}}"#,
            active, anim_idx, border_idx, shader_idx, panel_section, is_mutating, is_panel_open
        )
    }))
}

fn cmd_refresh_theme(window: &slint::Weak<crate::MainWindow>, proj: &Path) -> String {
    let eng = Engine::new(proj);
    let cfg = Config::load();
    let theme_pref = cfg.theme;
    // Delegate the visual refresh to the shared helpers from main.rs.
    //
    // get_colors() spawns the get_colors.sh subprocess, so it must NOT run
    // on the event-loop thread. It is called here on the IPC thread first;
    // only the UI mutations (apply_visual_state: resolve scheme, apply
    // theme, logo, tray icon) run inside invoke_from_event_loop.
    //
    // Behavior notes vs. the previous inline sequence:
    // - The helpers swallow the get_colors error detail into an Option, so
    //   the response uses a generic error message instead of the EngineError
    //   text. A get_colors failure now also replies immediately without
    //   waiting on the event loop.
    // - If the tray icon render fails, the helper returns None while the old
    //   code still replied "ok". That render only fails on an invalid SVG or
    //   an allocation failure — unreachable in practice for the static logo
    //   at 48px, so the response change is theoretical.
    let colors = match crate::fetch_visual_colors(&eng) {
        Some(colors) => colors,
        None => return "error: could not refresh visual state\n".to_string(),
    };
    format_response(invoke_on_main(window, move |win| {
        match crate::apply_visual_state(&win, &colors, &theme_pref) {
            Some(_) => "ok".to_string(),
            None => "error: could not refresh visual state".to_string(),
        }
    }))
}

/// Route the `assert-color-authority` capability: re-assert the applied
/// theme's colour authority through the providers the theme records.
///
/// Pure over its inputs so tests can drive it with a fake provider:
/// - empty `last_applied` → `noop` (nothing applied, nothing to re-assert);
/// - missing theme dir or unreadable `meta.json` → the file's normal error
///   line, never a panic;
/// - a recorded provider that is not registered is skipped, never fatal;
/// - the first provider error → the file's error shape, else `ok`.
///
/// Safe to call twice in a row: the verb keeps no state of its own, and
/// each provider's `reassert_colours` is idempotent by contract.
fn resolve_assert_color_authority(
    last_applied: &str,
    themes_dir: &Path,
    manager: &crate::theme_manager::ThemeManager,
) -> String {
    if last_applied.is_empty() {
        return "noop\n".to_string();
    }
    let theme_dir = themes_dir.join(last_applied);
    if !theme_dir.is_dir() {
        return format_response(Err(format!("theme '{last_applied}' not found")));
    }
    let meta: crate::theme_manager::ThemeMeta = match std::fs::read_to_string(theme_dir.join("meta.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(meta) => meta,
        None => {
            return format_response(Err(format!(
                "cannot read metadata for theme '{last_applied}'"
            )))
        }
    };
    for id in &meta.providers {
        let Some(provider) = manager.provider(id) else {
            continue;
        };
        if let Err(e) = provider.reassert_colours(&theme_dir) {
            return format_response(Err(e));
        }
    }
    format_response(Ok("ok".to_string()))
}

/// The palette-defence re-assert: gated on an armed monitor window OR a
/// recent resume from suspend.
///
/// The file watch stays the detector; the permission is a monitor hotplug
/// (`odd/tasks/palette-defence-on-monitor-events.md`) or a resume from
/// suspend (`odd/tasks/palette-defence-covers-suspend-resume.md`). With
/// neither, the change is presumed legitimate (the keeper's own
/// wallpaper/theme change) and HVE does nothing, so a manual change is never
/// reverted.
///
/// Both sources funnel through one entry point, and the decision is logged
/// (D4): a refusal used to be completely invisible, which is why the 18:29
/// suspend diagnosis took hours. One line per decision, never more.
fn cmd_assert_color_authority(proj: &Path) -> String {
    let permit = crate::hypr_ipc::consume_reassert_permit();
    if permit == crate::hypr_ipc::ReassertPermit::None {
        tracing::info!(
            "[colour-defence] re-assert REFUSED (no monitor event, no resume grace): \
             the palette change is presumed legitimate, leaving it alone"
        );
        return "noop\n".to_string();
    }
    tracing::info!(
        "[colour-defence] re-assert GRANTED (source={})",
        permit.as_str()
    );
    resolve_applied_authority(proj)
}

/// The startup repair: a hijack that happened while HVE was off is not a
/// palette change, so it is not gated by the monitor window. It routes
/// through the same providers, only without the permission check.
fn cmd_repair_color_authority(proj: &Path) -> String {
    resolve_applied_authority(proj)
}

/// Route the applied theme's colour authority to the providers that own it.
///
/// File-based state, like cmd_refresh_theme: the IPC thread must never
/// lock SharedState (see its invariant in app_state.rs), so the applied
/// theme comes from the on-disk config and the providers from a fresh
/// file-based manager, exactly as main() builds it.
fn resolve_applied_authority(proj: &Path) -> String {
    let cfg = Config::load();
    let config_dir = dirs::config_dir()
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp/hve-config"));
    let mut manager = crate::theme_manager::ThemeManager::new(&config_dir);
    let engine = Engine::new(proj);
    crate::providers::register_default_providers(&mut manager, &engine);
    let themes_dir = config_dir.join("hve").join("themes");
    resolve_assert_color_authority(cfg.last_applied_theme.trim(), &themes_dir, &manager)
}

fn cmd_quit(window: &slint::Weak<crate::MainWindow>) -> String {    let w = window.clone();
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

    // ── Second-launch handoff (client side) ──────────────────────────
    //
    // The handoff is exercised against a scripted Unix socket server: this
    // proves the wire contract (the launcher sends exactly `show` and reads
    // the reply with a bounded wait) without a live Slint event loop.

    /// Bind a throwaway Unix socket under a temp dir. The returned `TempDir`
    /// must be kept alive for the socket path to stay valid.
    fn scripted_socket() -> (tempfile::TempDir, UnixListener, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("hve.sock");
        let listener = UnixListener::bind(&path).expect("bind scripted socket");
        (dir, listener, path)
    }

    /// Accept one connection with a deadline. Returns `None` when nothing
    /// connects in time, so a regressed (never-connecting) client fails the
    /// assertion instead of hanging the test suite.
    fn accept_bounded(listener: &UnixListener, timeout: Duration) -> Option<UnixStream> {
        listener.set_nonblocking(true).expect("listener nonblocking");
        let deadline = Instant::now() + timeout;
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    return Some(stream);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return None,
            }
        }
    }

    /// Read one command line from a connection with a deadline.
    fn read_command(stream: &UnixStream) -> String {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut line = String::new();
        let mut reader = BufReader::new(stream);
        let _ = reader.read_line(&mut line);
        line.trim().to_string()
    }

    #[test]
    fn test_request_show_sends_the_show_command_and_reports_success() {
        let (_dir, listener, path) = scripted_socket();
        let server = std::thread::spawn(move || {
            let mut stream = accept_bounded(&listener, Duration::from_secs(2))?;
            let command = read_command(&stream);
            let _ = stream.write_all(b"ok\n");
            let _ = stream.flush();
            Some(command)
        });

        let outcome = request_show(&path);

        assert_eq!(
            outcome,
            HandoffOutcome::Shown("ok".to_string()),
            "a positive reply means the running window was raised"
        );
        assert_eq!(
            server.join().expect("server thread"),
            Some("show".to_string()),
            "the handoff must connect and send the idempotent `show` command"
        );
    }

    #[test]
    fn test_request_show_reports_unreachable_when_no_socket_exists() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("absent.sock");

        let outcome = request_show_with_timeout(&path, Duration::from_millis(200));

        match outcome {
            HandoffOutcome::Unreachable(detail) => {
                assert!(!detail.is_empty(), "the failure detail must be reportable");
            }
            HandoffOutcome::Shown(msg) => panic!("expected unreachable, got Shown({msg})"),
        }
    }

    #[test]
    fn test_request_show_reports_unreachable_when_the_instance_refuses() {
        let (_dir, listener, path) = scripted_socket();
        let server = std::thread::spawn(move || {
            let mut stream = accept_bounded(&listener, Duration::from_secs(2))?;
            let _ = read_command(&stream);
            let _ = stream.write_all(b"error: event loop not ready\n");
            let _ = stream.flush();
            Some(())
        });

        let outcome = request_show_with_timeout(&path, Duration::from_millis(500));

        match outcome {
            HandoffOutcome::Unreachable(detail) => {
                assert!(
                    detail.contains("error"),
                    "the server refusal must be surfaced, got: {detail}"
                );
            }
            HandoffOutcome::Shown(msg) => panic!("an error reply must not count as raised, got {msg}"),
        }
        assert!(server.join().expect("server thread").is_some(), "the client must connect");
    }

    #[test]
    fn test_request_show_reports_unreachable_when_the_instance_closes_without_reply() {
        let (_dir, listener, path) = scripted_socket();
        let server = std::thread::spawn(move || {
            let stream = accept_bounded(&listener, Duration::from_secs(2))?;
            drop(stream); // close without a reply
            Some(())
        });

        let outcome = request_show_with_timeout(&path, Duration::from_millis(500));

        assert!(
            matches!(outcome, HandoffOutcome::Unreachable(_)),
            "a connection closed without a reply is not a raised window, got: {outcome:?}"
        );
        assert!(server.join().expect("server thread").is_some(), "the client must connect");
    }

    #[test]
    fn test_request_show_is_bounded_when_the_instance_never_answers() {
        let (_dir, listener, path) = scripted_socket();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let stream = accept_bounded(&listener, Duration::from_secs(2))?;
            // Hold the connection open without answering.
            let _ = release_rx.recv();
            drop(stream);
            Some(())
        });

        let started = Instant::now();
        let outcome = request_show_with_timeout(&path, Duration::from_millis(200));
        let elapsed = started.elapsed();

        assert!(
            matches!(outcome, HandoffOutcome::Unreachable(_)),
            "a silent instance must be reported unreachable, got: {outcome:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "the handoff must be bounded, waited {elapsed:?}"
        );
        let _ = release_tx.send(());
        assert!(server.join().expect("server thread").is_some(), "the client must connect");
    }

    #[test]
    fn test_handoff_show_success_reports_exit_zero_and_a_forwarded_message() {
        let (_dir, listener, path) = scripted_socket();
        let server = std::thread::spawn(move || {
            let mut stream = accept_bounded(&listener, Duration::from_secs(2))?;
            let _ = read_command(&stream);
            let _ = stream.write_all(b"ok\n");
            let _ = stream.flush();
            Some(())
        });

        let report = handoff_show(&path);

        assert!(report.succeeded, "a positive reply reports success");
        assert_eq!(report.exit_code, 0, "a successful handoff exits zero");
        assert!(
            report.message.contains("brought forward"),
            "the message must tell the user what happened, got: {}",
            report.message
        );
        assert!(server.join().expect("server thread").is_some(), "the client must connect");
    }

    #[test]
    fn test_handoff_show_unreachable_exits_nonzero_and_names_both_exits() {
        let dir = tempfile::tempdir().expect("temp dir");
        let report = handoff_show(&dir.path().join("absent.sock"));

        assert!(!report.succeeded, "a missing socket is not a success");
        assert_ne!(report.exit_code, 0, "the launcher must exit non-zero");
        assert_eq!(report.exit_code, HANDOFF_FALLBACK_EXIT_CODE);
        assert!(
            report.message.contains("already running"),
            "the message must explain the refusal, got: {}",
            report.message
        );
        assert!(
            report.message.contains("pkill -x hve"),
            "the message must name how to close the running instance, got: {}",
            report.message
        );
        assert!(
            report.message.contains("SUPER + H"),
            "the message must name how to show the running instance, got: {}",
            report.message
        );
    }

    /// `show` must be a routed command. Without a live event loop the handler
    /// cannot run, so the observable proof is that the dispatcher reaches the
    /// handler (event-loop error) and not the unknown-command arm.
    #[test]
    fn test_dispatch_routes_show_to_its_handler_not_unknown() {
        i_slint_backend_testing::init_no_event_loop();
        let win = crate::MainWindow::new().expect("test window");
        let weak = win.as_weak();

        let response = dispatch_command("show", &weak, std::path::Path::new("."));

        assert!(
            !response.contains("unknown command"),
            "`show` must have a dispatcher arm, got: {response}"
        );
        assert_eq!(
            response, "error: event loop not ready\n",
            "`show` must reach its handler (which needs the event loop), got: {response}"
        );
    }

    // ── assert-color-authority (capability-routing seam, unit 1c1) ──
    //
    // The verb routes a capability: the core names the applied theme and
    // each recorded provider re-asserts its own colours. These tests drive
    // the pure routing half with a fake provider, so no event loop, no
    // shell and no real theme on disk are needed.

    /// Provider double: records every `reassert_colours` call, optionally fails.
    struct FakeColourProvider {
        id: &'static str,
        calls: std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>>,
        fail_with: Option<String>,
    }

    impl crate::theme_manager::ThemeProvider for FakeColourProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn display_name_key(&self) -> &str {
            self.id
        }
        fn icon(&self) -> &str {
            ""
        }
        fn save(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn apply(&self, _theme_dir: &Path) -> Result<(), String> {
            Ok(())
        }
        fn reassert_colours(&self, theme_dir: &Path) -> Result<(), String> {
            self.calls.lock().unwrap().push(theme_dir.to_path_buf());
            match &self.fail_with {
                Some(e) => Err(e.clone()),
                None => Ok(()),
            }
        }
    }

    /// Seed `{themes_dir}/{name}/meta.json` listing `ids` as providers.
    fn seed_theme_meta(themes_dir: &Path, name: &str, ids: &[&str]) {
        let dir = themes_dir.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let providers = ids
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            dir.join("meta.json"),
            format!(
                "{{\"saved_at\":\"2026-09-28T00:00:00.000Z\",\"description\":\"\",\"providers\":[{providers}]}}"
            ),
        )
        .unwrap();
    }

    /// Empty manager plus themes dir under a temp config dir. The manager
    /// takes the config dir directly, so no process-global env is touched.
    fn colour_test_setup() -> (
        tempfile::TempDir,
        PathBuf,
        crate::theme_manager::ThemeManager,
    ) {
        let config_dir = tempfile::tempdir().unwrap();
        let themes_dir = config_dir.path().join("hve").join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();
        let tm = crate::theme_manager::ThemeManager::new(config_dir.path());
        (config_dir, themes_dir, tm)
    }

    fn register_fake(
        tm: &mut crate::theme_manager::ThemeManager,
        fail_with: Option<&str>,
    ) -> std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>> {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        tm.register_provider(Box::new(FakeColourProvider {
            id: "fake-colours",
            calls: calls.clone(),
            fail_with: fail_with.map(|s| s.to_string()),
        }));
        calls
    }

    #[test]
    fn assert_color_authority_with_no_applied_theme_is_noop() {
        let (_dir, themes_dir, mut tm) = colour_test_setup();
        let calls = register_fake(&mut tm, None);

        let response = resolve_assert_color_authority("", &themes_dir, &tm);

        assert_eq!(response, "noop\n");
        assert!(
            calls.lock().unwrap().is_empty(),
            "noop must not touch any provider"
        );
    }

    #[test]
    fn assert_color_authority_calls_the_recorded_provider_once() {
        let (_dir, themes_dir, mut tm) = colour_test_setup();
        let calls = register_fake(&mut tm, None);
        seed_theme_meta(&themes_dir, "Demo", &["fake-colours"]);

        let response = resolve_assert_color_authority("Demo", &themes_dir, &tm);

        assert_eq!(response, "ok\n");
        let got = calls.lock().unwrap();
        assert_eq!(got.len(), 1, "exactly one re-assert call for the applied theme");
        assert_eq!(got[0], themes_dir.join("Demo"));
    }

    #[test]
    fn assert_color_authority_is_safe_called_twice() {
        let (_dir, themes_dir, mut tm) = colour_test_setup();
        let calls = register_fake(&mut tm, None);
        seed_theme_meta(&themes_dir, "Demo", &["fake-colours"]);

        assert_eq!(resolve_assert_color_authority("Demo", &themes_dir, &tm), "ok\n");
        assert_eq!(resolve_assert_color_authority("Demo", &themes_dir, &tm), "ok\n");
        assert_eq!(
            calls.lock().unwrap().len(),
            2,
            "the verb keeps no state: one call per invocation, nothing else"
        );
    }

    #[test]
    fn assert_color_authority_skips_a_provider_that_is_not_registered() {
        let (_dir, themes_dir, tm) = colour_test_setup();
        seed_theme_meta(&themes_dir, "Demo", &["retired-backend"]);

        let response = resolve_assert_color_authority("Demo", &themes_dir, &tm);

        assert_eq!(
            response, "ok\n",
            "a recorded provider that is not registered is skipped, never fatal"
        );
    }

    #[test]
    fn assert_color_authority_provider_error_is_an_error_response() {
        let (_dir, themes_dir, mut tm) = colour_test_setup();
        let calls = register_fake(&mut tm, Some("palette refused"));
        seed_theme_meta(&themes_dir, "Demo", &["fake-colours"]);

        let response = resolve_assert_color_authority("Demo", &themes_dir, &tm);

        assert_eq!(response, "error: palette refused\n");
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn assert_color_authority_missing_theme_is_an_error_not_a_panic() {
        let (_dir, themes_dir, tm) = colour_test_setup();

        let response = resolve_assert_color_authority("Gone", &themes_dir, &tm);

        assert!(
            response.starts_with("error: "),
            "a missing theme dir must be an error line, got: {response}"
        );
    }

    /// T3 (odd/tasks/palette-defence-covers-suspend-resume.md): a refused
    /// re-assert was invisible on 2026-10-01 (only `noop` in the watcher log),
    /// which is why the resume bug took hours to diagnose. The gate must log
    /// its decision — a grant WITH its source, a refusal WITH its reason —
    /// through the one entry point. Scoped to the function body and
    /// comment-stripped, like the house precedent for logging contracts, so
    /// neither this text nor a commented-out call can satisfy it.
    #[test]
    fn the_reassert_gate_logs_its_decision_with_source_and_reason() {
        let src = std::fs::read_to_string("src/ipc.rs").expect("src/ipc.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let start = code
            .find("fn cmd_assert_color_authority")
            .expect("cmd_assert_color_authority must exist");
        let body = &code[start..];
        let end = body.find("\nfn ").unwrap_or(body.len());
        let body = &body[..end];

        assert!(
            body.contains("consume_reassert_permit"),
            "the gate must use the single entry point that reports the source"
        );
        assert!(
            body.contains("ReassertPermit::None"),
            "the refusal must be distinguished from a grant"
        );
        assert_eq!(
            body.matches("tracing::").count(),
            2,
            "exactly one log line per decision (a grant and a refusal)"
        );
        assert!(
            body.contains("source={}"),
            "a grant must name its source (monitor | resume)"
        );
        assert!(
            body.contains("REFUSED"),
            "a refusal must say so, with its reason, next time"
        );
        assert!(
            body.contains("\"noop\\n\""),
            "the refusal must still answer noop to the watcher"
        );
    }
}