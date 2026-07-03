use ksni::{self, TrayService};
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::tr::Tr;

/// Start the system tray icon in a background thread.
///
/// Creates a `ksni` tray with a programmatic 32×32 icon (does not depend on
/// a system icon theme) and a context menu with dynamic status display and
/// IPC quick commands.
///
/// Returns an `Arc<AtomicBool>` that the caller can share with callbacks
/// to keep the tray icon and status label in sync with system state.
pub fn start_tray(window: slint::Weak<crate::MainWindow>, tr: Arc<Tr>) -> Arc<AtomicBool> {
    let system_active = Arc::new(AtomicBool::new(false));
    let service = TrayService::new(HveTray {
        window,
        system_active: system_active.clone(),
        tr,
    });
    service.spawn();
    system_active
}

struct HveTray {
    window: slint::Weak<crate::MainWindow>,
    system_active: Arc<AtomicBool>,
    tr: Arc<Tr>,
}

impl ksni::Tray for HveTray {
    fn icon_name(&self) -> String {
        "hve".to_string()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        if self.system_active.load(Ordering::Relaxed) {
            vec![make_icon(32, 32, 0x22, 0xcc, 0x66)] // green
        } else {
            vec![make_icon(32, 32, 0x66, 0x66, 0x66)] // gray
        }
    }

    fn title(&self) -> String {
        "HVE — Hyprland Visual Editor".to_string()
    }

    fn id(&self) -> String {
        "hve".to_string()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;

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
                            let next = if current < 0 || current + 1 >= count as i32 {
                                0
                            } else {
                                current + 1
                            };
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
                            let next = if current < 0 || current + 1 >= count as i32 {
                                0
                            } else {
                                current + 1
                            };
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
                            let next = if current < 0 || current + 1 >= count as i32 {
                                0
                            } else {
                                current + 1
                            };
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
            // ── Toggle Window (hide/show) — seguro con run_event_loop_until_quit ──
            StandardItem {
                label: if crate::ipc::WINDOW_HIDDEN.load(Ordering::Relaxed) {
                    self.tr.tr_or("tray.show_window", "Show Window").to_string()
                } else {
                    self.tr.tr_or("tray.hide_window", "Hide Window").to_string()
                },
                activate: Box::new(|tray: &mut Self| {
                    let w = tray.window.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = w.upgrade() {
                            if crate::ipc::WINDOW_HIDDEN.load(Ordering::Relaxed) {
                                let _ = win.window().show();
                                crate::ipc::WINDOW_HIDDEN.store(false, Ordering::Relaxed);
                            } else {
                                let _ = win.window().hide();
                                crate::ipc::WINDOW_HIDDEN.store(true, Ordering::Relaxed);
                            }
                        }
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

// ─── Programmatic icon (avoids depending on system icon theme) ───────

/// Build a simple 32×32 RGBA icon — filled circle on transparent background.
fn make_icon(width: i32, height: i32, r: u8, g: u8, b: u8) -> ksni::Icon {
    let mut data = vec![0u8; (width * height * 4) as usize];

    let cx = width as f32 / 2.0;
    let cy = height as f32 / 2.0;
    let radius = (width.min(height) as f32 / 2.0) - 1.5;

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();

            let idx = ((y * width + x) * 4) as usize;

            if dist <= radius {
                data[idx] = r;
                data[idx + 1] = g;
                data[idx + 2] = b;
                data[idx + 3] = 0xff;
            } else {
                data[idx] = 0;
                data[idx + 1] = 0;
                data[idx + 2] = 0;
                data[idx + 3] = 0;
            }
        }
    }

    ksni::Icon { width, height, data }
}
