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
                            // WINDOW_HIDDEN única fuente de verdad.
                            // is_visible() no es fiable en Wayland.
                            if crate::ipc::WINDOW_HIDDEN.load(Ordering::Relaxed) {
                                let _ = win.window().show();
                                win.window().request_redraw();
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

/// Build a 32×32 RGBA icon — rounded-square app icon with HVE letters.
///
/// Looks like a modern app icon: rounded rect background with subtle
/// gloss, and "H V E" drawn as a continuous interlocked mark:
///
///   █ █   ███
///   █ █   █
///   ███   ███   ← H crossbar
///   █ █\  █     ← V left diagonal starts at H right bar
///   █ █ \ █
///   █ █  ███   ← V connects to E
///
fn make_icon(width: i32, height: i32, r: u8, g: u8, b: u8) -> ksni::Icon {
    let mut data = vec![0u8; (width * height * 4) as usize];
    let cr = 5.0;
    let half_w = width as f32 / 2.0;
    let half_h = height as f32 / 2.0;
    let irx = half_w - cr;
    let iry = half_h - cr;

    // ── Point-to-line distance helper ──
    fn line_dist(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let len_sq = dx * dx + dy * dy;
        if len_sq == 0.0 {
            return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
        }
        let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
        let cx = x1 + t * dx;
        let cy = y1 + t * dy;
        ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
    }

    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            // ── Rounded rect SDF ──
            let dx = (px - half_w).abs() - irx;
            let dy = (py - half_h).abs() - iry;

            let sdf = if dx > 0.0 && dy > 0.0 {
                (dx * dx + dy * dy).sqrt()
            } else if dx > 0.0 {
                dx
            } else if dy > 0.0 {
                dy
            } else {
                -irx.min(iry)
            };

            let dist = (sdf / 1.2).clamp(0.0, 1.0);
            let alpha = (1.0 - dist).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }

            // ── Base color with subtle diagonal gloss ──
            let gloss = 0.82 + 0.18 * (1.0 - ((px / width as f32) * 0.5 + (py / height as f32) * 0.5));
            let mut pr = (r as f32 * gloss) as u8;
            let mut pg = (g as f32 * gloss) as u8;
            let mut pb = (b as f32 * gloss) as u8;

            // ── H V E interlocked mark (stroke ~3px) ──
            let stroke = 2.5;

            // H: left bar, right bar, crossbar
            let h_left = x >= 4 && x <= 7 && y >= 5 && y <= 27;
            let h_right = x >= 10 && x <= 13 && y >= 5 && y <= 27;
            let h_cross = y >= 15 && y <= 17 && x >= 4 && x <= 13;

            // V: two diagonal strokes from (14,16) down to (21,28)
            let v_left = line_dist(px, py, 14.0, 16.0, 21.0, 28.0) <= stroke;
            let v_right = line_dist(px, py, 21.0, 28.0, 28.0, 16.0) <= stroke;

            // E: vertical bar + top, middle, bottom bars
            let e_vert = x >= 28 && x <= 31 && y >= 5 && y <= 27;
            let e_top = y >= 5 && y <= 7 && x >= 28 && x <= 31;
            let e_mid = y >= 15 && y <= 17 && x >= 28 && x <= 31;
            let e_bot = y >= 25 && y <= 27 && x >= 28 && x <= 31;

            let in_mark = h_left || h_right || h_cross
                || v_left || v_right
                || e_vert || e_top || e_mid || e_bot;

            if in_mark {
                // Letters: push toward white for maximum contrast
                pr = pr.saturating_add(80).max(pr * 2);
                pg = pg.saturating_add(80).max(pg * 2);
                pb = pb.saturating_add(80).max(pb * 2);
            }

            // Premultiply alpha for ksni
            data[idx] = (pr as f32 * alpha) as u8;
            data[idx + 1] = (pg as f32 * alpha) as u8;
            data[idx + 2] = (pb as f32 * alpha) as u8;
            data[idx + 3] = (alpha * 255.0) as u8;
        }
    }

    ksni::Icon { width, height, data }
}
