use ksni::blocking::TrayMethods;
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::tr::Tr;

/// Global tray handle so IPC commands (refresh-theme) can update the tray icon.
static GLOBAL_TRAY: OnceLock<Mutex<TrayHandle>> = OnceLock::new();

/// One request for the tray WORKER thread.
///
/// Why a worker and not a direct call: ksni 0.3's blocking
/// [`ksni::blocking::Handle::update`] WAITS for the tray service to apply the
/// change (`compat::block_on`), where 0.2 only ENQUEUED it. The callers of
/// [`TrayHandle::update_icon`] and [`TrayHandle::refresh_menu`] run on the UI
/// thread — the hide path (`main.rs`, "toggle_tray" + refresh) is one of them —
/// and that path HOLDS the composer's controller mutex, which the tray's own
/// `menu()` needs on the tray thread (`crate::composer::global_controller()`).
/// Waiting there is a deadlock: the UI thread waits for the tray while the tray
/// waits for the UI thread's lock. The UI thread then stops answering
/// Hyprland's pings, the compositor shows its ANR ("not responding") dialog,
/// and choosing close SIGKILLs the process — observed live on 2026-10-14,
/// after every hide. Enqueueing restores 0.2's shape on 0.3.
enum TrayTask {
    /// New RGBA icon pixels, already converted to the BGRA ksni wants.
    Icon(Vec<u8>),
    /// Rebuild the menu so it reads fresh controller state.
    RefreshMenu,
}

/// Handle to control the tray icon after creation. Every method only ENQUEUES
/// work for the tray worker thread, so a caller can never wait on the tray.
pub struct TrayHandle {
    tx: std::sync::mpsc::Sender<TrayTask>,
}

/// Convert one RGBA buffer to the BGRA (Cairo ARGB32 little-endian) order ksni
/// wants. Extracted so the conversion — the only place a malformed icon could
/// take the process down — is testable without a live tray service.
///
/// `chunks_exact(4)`, not `chunks(4)`: a buffer whose length is not a multiple
/// of 4 has a short LAST chunk, and indexing `p[2]`/`p[3]` on it panics
/// (`index out of bounds: the len is 3 but the index is 3`). A truncated icon is
/// malformed data, not a reason to lose the process: the tail below one whole
/// pixel is dropped.
fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect()
}

impl TrayHandle {
    /// Force a tray menu refresh so the UI picks up the latest visibility
    /// state. Enqueued, never waited for: see [`TrayTask`].
    pub fn refresh_menu(&self) {
        let _ = self.tx.send(TrayTask::RefreshMenu);
    }

    /// Update the tray icon with new RGBA pixel data (converted to BGRA here,
    /// on the caller's thread, so the worker only moves bytes). Enqueued, never
    /// waited for: see [`TrayTask`].
    pub fn update_icon(&self, rgba: Vec<u8>) {
        let _ = self.tx.send(TrayTask::Icon(rgba_to_bgra(&rgba)));
    }
}

/// Register a tray handle as the global instance, so IPC commands (refresh-theme)
/// can update the tray icon.
pub fn init_global(handle: TrayHandle) {
    let _ = GLOBAL_TRAY.set(Mutex::new(handle));
}

/// Try to update the tray icon via the global handle. Does nothing if no handle
/// has been registered yet.
pub fn update_global_icon(rgba: Vec<u8>) {
    if let Some(tray) = GLOBAL_TRAY.get() {
        if let Ok(tray) = tray.lock() {
            tray.update_icon(rgba);
        }
    }
}

/// Force the tray menu to refresh so the next open reads fresh controller state.
/// Safe to call from any thread (just sends via mpsc channel).
pub fn refresh_global_menu() {
    if let Some(tray) = GLOBAL_TRAY.get() {
        if let Ok(tray) = tray.lock() {
            tray.refresh_menu();
        }
    }
}

/// Start the system tray icon in a background thread.
///
/// `icon_rgba` is the raw RGBA pixel data rendered from the SVG template,
/// at the given `icon_w × icon_h` resolution. `system_active` is the shared
/// "Toggle System" state, owned by the caller because the callers need it
/// whether or not a tray exists.
///
/// Returns `None` when the tray cannot be created (no D-Bus session bus, no
/// usable connection). The caller keeps running WITHOUT a tray and only gets a
/// warning: a missing notification area must not stop the editor. Under ksni
/// 0.2 this same failure was an `unwrap()` inside a spawned thread — a panic
/// nobody could see, and the process survived it anyway; 0.3 does the
/// registration synchronously and returns the error instead.
pub fn start_tray(
    window: slint::Weak<crate::MainWindow>,
    tr: Arc<Tr>,
    icon_rgba: Vec<u8>,
    icon_w: u32,
    icon_h: u32,
    system_active: Arc<AtomicBool>,
) -> Option<TrayHandle> {
    // Convert RGBA → BGRA (ksni expects Cairo ARGB32 = BGRA in LE)
    let icon_bgra = rgba_to_bgra(&icon_rgba);

    let tray = HveTray {
        window,
        system_active,
        tr,
        icon_bgra,
        icon_w,
        icon_h,
    };

    // `assume_sni_available(true)` keeps the 0.2 behaviour of a panel that
    // starts LATER: without it, a missing `org.kde.StatusNotifierWatcher` makes
    // `spawn` return `Err` for good, while 0.2 subscribed to `NameOwnerChanged`
    // and registered the item as soon as the watcher appeared. With it, the
    // missing watcher is a soft error routed to `Tray::watcher_offline`
    // (default: keep running) and the item registers when the panel shows up.
    match tray.assume_sni_available(true).spawn() {
        Ok(handle) => {
            // ONE worker thread owns the blocking `update`: every caller only
            // enqueues (see `TrayTask`).
            let (tx, rx) = std::sync::mpsc::channel::<TrayTask>();
            let worker = handle.clone();
            let spawned = std::thread::Builder::new()
                .name("hve-tray-updates".to_string())
                .spawn(move || {
                    while let Ok(task) = rx.recv() {
                        match task {
                            TrayTask::Icon(bgra) => {
                                let _ = worker.update(move |t: &mut HveTray| t.icon_bgra = bgra);
                            }
                            TrayTask::RefreshMenu => {
                                let _ = worker.update(|_| {});
                            }
                        }
                    }
                });
            if let Err(err) = spawned {
                // No worker: the tray exists but its icon/menu cannot be
                // updated. Log it and drop the handle rather than let every
                // update block the UI thread (the bug this worker fixes).
                tracing::warn!("tray update worker could not start: {err}");
                return None;
            }
            Some(TrayHandle { tx })
        }
        Err(err) => {
            tracing::warn!(
                "system tray unavailable, continuing without a tray icon: {err} \
                 (no D-Bus session bus, or no StatusNotifier support)"
            );
            None
        }
    }
}

struct HveTray {
    window: slint::Weak<crate::MainWindow>,
    system_active: Arc<AtomicBool>,
    tr: Arc<Tr>,
    icon_bgra: Vec<u8>,
    icon_w: u32,
    icon_h: u32,
}

impl ksni::Tray for HveTray {
    fn icon_name(&self) -> String {
        String::new() // empty → use icon_pixmap data
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![ksni::Icon {
            width: self.icon_w as i32,
            height: self.icon_h as i32,
            data: self.icon_bgra.clone(),
        }]
    }

    fn title(&self) -> String {
        "HVE — Hyprland Visual Editor".to_string()
    }

    fn id(&self) -> String {
        "hve".to_string()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;

        // Read the hidden state from the Controller (the single source of
        // truth) WITHOUT waiting for it: this runs on the tray thread while the
        // UI thread may hold that lock (any hide path does), and waiting here
        // wedges the tray service — no menu for the panel, and — through the
        // blocking update this struct no longer uses — the whole app. A
        // momentary stale label is the price, and the next refresh corrects it.
        let is_hidden = crate::composer::try_global_controller()
            .map(|c| c.window_hidden())
            .unwrap_or(false);

        vec![
            // ── App title (read-only) ──
            StandardItem {
                label: "Hyprland Visual Editor".to_string(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            // ── Separator ──
            ksni::MenuItem::Separator,
            // ── Toggle System ──
            StandardItem {
                label: self
                    .tr
                    .tr_or("tray.toggle_system", "Toggle System")
                    .to_string(),
                activate: Box::new(|tray: &mut Self| {
                    let w = tray.window.clone();
                    let next_active = !tray.system_active.load(Ordering::Relaxed);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = w.upgrade() {
                            win.invoke_toggle_system(next_active);
                        }
                    });
                }),
                ..Default::default()
            }
            .into(),
            // ── Next Animation ──
            StandardItem {
                label: self
                    .tr
                    .tr_or("tray.next_animation", "Next Animation")
                    .to_string(),
                activate: Box::new(|tray: &mut Self| {
                    let w = tray.window.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = w.upgrade() {
                            use slint::Model;
                            let files = win.get_anim_files();
                            let count = files.row_count();
                            if count == 0 {
                                return;
                            }
                            let current = win.get_active_anim_index();
                            let next = crate::ipc::get_next_index(current, count);
                            if let Some(file) = files.row_data(next as usize) {
                                win.invoke_panel_apply_animation(next, file);
                            }
                        }
                    });
                }),
                ..Default::default()
            }
            .into(),
            // ── Next Border ──
            StandardItem {
                label: self.tr.tr_or("tray.next_border", "Next Border").to_string(),
                activate: Box::new(|tray: &mut Self| {
                    let w = tray.window.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = w.upgrade() {
                            use slint::Model;
                            let files = win.get_border_files();
                            let count = files.row_count();
                            if count == 0 {
                                return;
                            }
                            let current = win.get_active_border_index();
                            let next = crate::ipc::get_next_index(current, count);
                            if let Some(file) = files.row_data(next as usize) {
                                win.invoke_panel_apply_border(next, file);
                            }
                        }
                    });
                }),
                ..Default::default()
            }
            .into(),
            // ── Next Shader ──
            StandardItem {
                label: self.tr.tr_or("tray.next_shader", "Next Shader").to_string(),
                activate: Box::new(|tray: &mut Self| {
                    let w = tray.window.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = w.upgrade() {
                            use slint::Model;
                            let files = win.get_shader_files();
                            let count = files.row_count();
                            if count == 0 {
                                return;
                            }
                            let current = win.get_active_shader_index();
                            let next = crate::ipc::get_next_index(current, count);
                            if let Some(file) = files.row_data(next as usize) {
                                win.invoke_panel_apply_shader(next, file);
                            }
                        }
                    });
                }),
                ..Default::default()
            }
            .into(),
            // ── Separator ──
            ksni::MenuItem::Separator,
            // ── Toggle Window (hide/show) — keeps the window mapped on the special workspace ──
            StandardItem {
                label: if is_hidden {
                    self.tr.tr_or("tray.show_window", "Show Window").to_string()
                } else {
                    self.tr.tr_or("tray.hide_window", "Hide Window").to_string()
                },
                activate: Box::new(|tray: &mut Self| {
                    let w = tray.window.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = w.upgrade() {
                            // Delegate the whole toggle (show/hide + state +
                            // prev_workspace + warmup) through the Controller —
                            // single source of truth. The controller is always
                            // initialized before the tray starts.
                            if let Some(mut ctrl) = crate::composer::global_controller() {
                                ctrl.toggle_tray(&win);
                            }
                        }
                        // Force tray menu refresh so label reflects new visibility
                        crate::tray::refresh_global_menu();
                    });
                }),
                ..Default::default()
            }
            .into(),
            // ── Separator ──
            ksni::MenuItem::Separator,
            // ── Quit ──
            StandardItem {
                label: self.tr.tr_or("tray.quit", "Quit").to_string(),
                activate: Box::new(|tray: &mut Self| {
                    let w = tray.window.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = w.upgrade() {
                            let _ = win.window().hide();
                        }
                        let _ = slint::quit_event_loop();
                    });
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    /// The tray must NEVER make a caller wait for it.
    ///
    /// Regression pin for the 2026-10-14 hang: ksni 0.3's blocking
    /// `Handle::update` waits for the tray service, and its callers run on the
    /// UI thread while holding the composer's controller mutex — which the
    /// tray's own `menu()` needs. Both sides waited for each other, the UI
    /// thread stopped answering Hyprland, the compositor showed its ANR dialog
    /// and the app had to be killed. Every update must go through the worker
    /// channel, and the menu must read the state WITHOUT waiting.
    #[test]
    fn tray_updates_never_block_the_caller() {
        let src = std::fs::read_to_string("src/tray.rs").expect("src/tray.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // The PRODUCTION body only: this test's own source names the thing it
        // forbids, so it must not be scanned (the same cut `architecture_contract`
        // makes with its own `production_body`).
        let code = code.split("#[cfg(test)]").next().unwrap_or(&code);
        assert!(
            !code.contains("block_on"),
            "the tray must not block a caller: `update` waits for the service, so it belongs \
             on the worker thread"
        );
        assert_eq!(
            code.matches(".update(").count(),
            2,
            "the blocking `update` may only be called by the tray worker (its two arms: the \
             icon and the menu refresh); a third call site means a caller can wait again"
        );
        // Counting call sites is not enough: a third `.update(` is not the only
        // way back to the bug, a call MOVED into one of these methods keeps the
        // count at two. Pin the place, not the tally: no `TrayHandle` method may
        // reach `update` at all — the worker is its only owner.
        let handle_at = code
            .find("impl TrayHandle {")
            .expect("the gate must keep a `TrayHandle` impl");
        let handle = code[handle_at..]
            .split("\n}")
            .next()
            .expect("the `TrayHandle` impl must close at column 0");
        assert!(
            !handle.contains(".update("),
            "a `TrayHandle` method must only ENQUEUE; a `.update(` here is a caller that waits \
             for the tray service (the deadlock of 2026-10-14)"
        );
        // The TRAY THREAD is the one that must never wait for that lock. Every
        // ksni method runs on it, but the menu ITEM CLOSURES do not: they are
        // handed to `slint::invoke_from_event_loop` and run on the UI thread, so
        // the legitimately blocking `global_controller()` inside the Toggle
        // Window item is fine. What runs on the tray thread when the service
        // asks for the menu is the part BEFORE the `vec!` — that is the scope
        // this pin owns, and it must not widen into the closures.
        let menu_at = code
            .find("fn menu(&self)")
            .expect("the gate must keep a `menu()` for the tray");
        let menu = code[menu_at..]
            .split("vec![")
            .next()
            .expect("the gate must keep the `vec!` menu body");
        assert!(
            menu.contains("crate::composer::try_global_controller("),
            "`menu()` runs on the tray thread and must read the hidden state WITHOUT waiting \
             for the controller lock"
        );
        assert!(
            !menu.contains("crate::composer::global_controller("),
            "the part of `menu()` the tray thread runs must not WAIT for the composer's \
             controller lock (the UI thread holds it while it asks the tray to refresh)"
        );
    }

    use super::{rgba_to_bgra, start_tray};
    use slint::ComponentHandle;
    use std::sync::Arc;

    /// Restores an environment variable when it goes out of scope, so the tests
    /// that point the session bus at nothing cannot leak that into other tests.
    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    /// Spec S2/S6 of `odd/tasks/ksni-0.3-bump.md`: a tray that cannot be created
    /// must not take anything down. Under ksni 0.2 a registration failure was an
    /// `unwrap()` inside a spawned thread — an invisible panic — while 0.3
    /// resolves the service synchronously and hands the caller the error (search
    /// the crate: `blocking.rs` -> `spawn_with_options` -> `compat::block_on(...)?`).
    /// The failure is now observable, so it must come back as `None`.
    #[test]
    #[serial_test::serial]
    fn start_tray_without_a_session_bus_returns_none() {
        i_slint_backend_testing::init_no_event_loop();
        let win = crate::MainWindow::new().expect("test window");
        let tr = Arc::new(crate::tr::Tr::with_lang("en"));
        let system_active = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // Nothing to connect to: the address points at a socket that does not
        // exist, and the fallback (the runtime dir) is pointed at an empty one.
        let _bus = EnvVarGuard::set("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent-hve-bus");
        let _runtime = EnvVarGuard::set("XDG_RUNTIME_DIR", "/nonexistent-hve-runtime");

        let tray = start_tray(win.as_weak(), tr, Vec::new(), 48, 48, system_active.clone());

        assert!(
            tray.is_none(),
            "no session bus means no tray, and HVE must keep running without one"
        );
    }

    /// Lane C, S3: the conversion indexes `p[2]`/`p[3]` for every chunk, so a
    /// buffer whose length is not a multiple of 4 PANICS on its last, short
    /// chunk — a malformed icon taking the process down (the audit's only real
    /// panic candidate outside `Mutex::lock().unwrap()`). Only whole pixels are
    /// pixels: the tail is dropped and the conversion keeps going.
    #[test]
    fn rgba_to_bgra_drops_a_truncated_tail_instead_of_panicking() {
        // Two whole pixels plus three stray bytes.
        let rgba = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110];
        assert_eq!(
            rgba_to_bgra(&rgba),
            vec![30, 20, 10, 40, 70, 60, 50, 80],
            "the channels swap (R and B) and the truncated tail is dropped"
        );
    }

    /// The cases the tray actually feeds it: a 1x1 icon and an empty buffer.
    #[test]
    fn rgba_to_bgra_converts_whole_pixels() {
        assert_eq!(rgba_to_bgra(&[]), Vec::<u8>::new(), "empty stays empty");
        assert_eq!(rgba_to_bgra(&[1, 2, 3, 4]), vec![3, 2, 1, 4]);
        assert_eq!(
            rgba_to_bgra(&[255, 0, 128, 255]),
            vec![128, 0, 255, 255],
            "a 1x1 icon: only R and B move"
        );
    }
}
