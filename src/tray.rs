use ksni::{self, Handle, TrayService};
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::tr::Tr;

/// Global tray handle so IPC commands (refresh-theme) can update the tray icon.
static GLOBAL_TRAY: OnceLock<Mutex<TrayHandle>> = OnceLock::new();

/// Handle to control the tray icon after creation.
pub struct TrayHandle {
    pub system_active: Arc<AtomicBool>,
    handle: Handle<HveTray>,
}

impl TrayHandle {
    /// Force a tray menu refresh so the UI picks up the latest WINDOW_HIDDEN state.
    pub fn refresh_menu(&self) {
        self.handle.update(|_| {});
    }

    /// Update the tray icon with new RGBA pixel data (will be converted to BGRA internally).
    pub fn update_icon(&self, rgba: Vec<u8>) {
        let bgra: Vec<u8> = rgba
            .chunks(4)
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect();
        self.handle.update(move |tray: &mut HveTray| {
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

/// Force the tray menu to refresh so the next open reads fresh WINDOW_HIDDEN state.
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
/// at the given `icon_w × icon_h` resolution.
///
/// Returns a `TrayHandle` that can be used to update the icon later.
pub fn start_tray(
    window: slint::Weak<crate::MainWindow>,
    tr: Arc<Tr>,
    icon_rgba: Vec<u8>,
    icon_w: u32,
    icon_h: u32,
) -> TrayHandle {
    // Convert RGBA → BGRA (ksni expects Cairo ARGB32 = BGRA in LE)
    let icon_bgra: Vec<u8> = icon_rgba
        .chunks(4)
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect();

    let system_active = Arc::new(AtomicBool::new(false));
    let tray = HveTray {
        window,
        system_active: system_active.clone(),
        tr,
        icon_bgra,
        icon_w,
        icon_h,
    };
    let service = TrayService::new(tray);
    let handle = service.handle();
    service.spawn();
    TrayHandle { system_active, handle }
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
        // Falls back to the static for backward compat if Controller not yet initialized.
        let is_hidden = crate::composer::global_controller()
            .map(|c| c.window_hidden())
            .unwrap_or_else(|| crate::ipc::WINDOW_HIDDEN.load(Ordering::Relaxed));

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
                label: self.tr.tr_or("tray.toggle_system", "Toggle System").to_string(),
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
                label: self.tr.tr_or("tray.next_animation", "Next Animation").to_string(),
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
                                win.invoke_apply_animation(next, file);
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
                                win.invoke_apply_border(next, file);
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
                                win.invoke_apply_shader(next, file);
                            }
                        }
                    });
                }),
                ..Default::default()
            }
            .into(),
            // ── Separator ──
            ksni::MenuItem::Separator,
            // ── Toggle Window (hide/show) — mantiene ventana mapeada en special workspace ──
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
                            // single source of truth. The WINDOW_HIDDEN static
                            // stays in sync as a backward-compat fallback.
                            if let Some(mut ctrl) = crate::composer::global_controller() {
                                ctrl.toggle_tray(&win);
                                crate::ipc::WINDOW_HIDDEN
                                    .store(ctrl.window_hidden(), Ordering::Relaxed);
                            } else {
                                // Fallback: no Controller available, use legacy path.
                                if crate::ipc::WINDOW_HIDDEN.load(Ordering::Relaxed) {
                                    let fast = crate::ipc::show_window(&win);
                                    if !fast {
                                        crate::prewarm_tabs(win.as_weak(), 1);
                                        crate::warmup_navigation(&win);
                                    }
                                    crate::ipc::WINDOW_HIDDEN.store(false, Ordering::Relaxed);
                                } else {
                                    crate::ipc::hide_window(&win);
                                    crate::ipc::WINDOW_HIDDEN.store(true, Ordering::Relaxed);
                                }
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
