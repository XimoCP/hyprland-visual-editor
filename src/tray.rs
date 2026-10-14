use ksni::blocking::{Handle, TrayMethods};
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::tr::Tr;

/// Global tray handle so IPC commands (refresh-theme) can update the tray icon.
static GLOBAL_TRAY: OnceLock<Mutex<TrayHandle>> = OnceLock::new();

/// Handle to control the tray icon after creation.
pub struct TrayHandle {
    handle: Handle<HveTray>,
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
    /// Force a tray menu refresh so the UI picks up the latest visibility state.
    pub fn refresh_menu(&self) {
        // `update` answers `None` once the service has shut down (ksni 0.3);
        // there is nothing to refresh then, and nothing to report.
        let _ = self.handle.update(|_| {});
    }

    /// Update the tray icon with new RGBA pixel data (will be converted to BGRA internally).
    pub fn update_icon(&self, rgba: Vec<u8>) {
        let bgra = rgba_to_bgra(&rgba);
        let _ = self.handle.update(move |tray: &mut HveTray| {
            tray.icon_bgra = bgra;
        });
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
        Ok(handle) => Some(TrayHandle { handle }),
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

        // Read hidden state from Controller (the single source of truth).
        let is_hidden = crate::composer::global_controller()
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
