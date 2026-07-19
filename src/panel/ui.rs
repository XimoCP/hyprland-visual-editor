//! PanelWindow binding: creation, callback wiring to MainWindow actions,
//! and periodic property updates for preset indicators.

use crate::config::Config;
use slint::ComponentHandle;

/// Opaque handle that keeps the panel window alive for the app lifetime.
/// Drop hides the window.
pub struct PanelUi {
    window: slint::Weak<crate::PanelWindow>,
}

impl PanelUi {
    /// Create a new PanelWindow and wire its callbacks.
    ///
    /// `main_weak` is the MainWindow weak reference used to dispatch
    /// quick-control callbacks (toggle-system, next-anim, etc.).
    pub fn new(main_weak: slint::Weak<crate::MainWindow>) -> Result<Self, slint::PlatformError> {
        let window = crate::PanelWindow::new()?;

        // ── Wire callbacks ──
        {
            let mw = main_weak.clone();
            window.on_toggle_system(move || {
                let mw = mw.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = mw.upgrade() {
                        let next = !win.get_system_active();
                        win.invoke_toggle_system(next);
                    }
                });
            });
        }

        {
            let mw = main_weak.clone();
            window.on_next_anim(move || {
                let mw = mw.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = mw.upgrade() {
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
            });
        }

        {
            let mw = main_weak.clone();
            window.on_next_border(move || {
                let mw = mw.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = mw.upgrade() {
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
            });
        }

        {
            let mw = main_weak.clone();
            window.on_next_shader(move || {
                let mw = mw.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = mw.upgrade() {
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
            });
        }

        {
            let mw = main_weak.clone();
            window.on_open_settings(move || {
                let mw = mw.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = mw.upgrade() {
                        if crate::ipc::WINDOW_HIDDEN
                            .load(std::sync::atomic::Ordering::Relaxed)
                        {
                            let _ = win.window().show();
                            crate::ipc::WINDOW_HIDDEN
                                .store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                        win.set_settings_open(true);
                        let _ = win.window().show();
                    }
                });
            });
        }

        // ── Periodic preset indicator updater ──
        {
            let w = window.as_weak();
            let timer = slint::Timer::default();
            timer.start(
                slint::TimerMode::Repeated,
                std::time::Duration::from_secs(1),
                move || {
                    if let Some(panel) = w.upgrade() {
                        let cfg = Config::load();
                        panel.set_active_anim_name(
                            extract_preset_name(&cfg.active_anim_file).into(),
                        );
                        panel.set_active_border_name(
                            extract_preset_name(&cfg.active_border_file).into(),
                        );
                        panel.set_active_shader_name(
                            extract_preset_name(&cfg.active_shader_file).into(),
                        );
                    }
                },
            );
            // Keep timer alive for the app lifetime
            std::mem::forget(timer);
        }

        Ok(Self {
            window: window.as_weak(),
        })
    }

    /// Return a weak reference to the PanelWindow, for the panel thread.
    pub fn weak(&self) -> slint::Weak<crate::PanelWindow> {
        self.window.clone()
    }
}

impl Drop for PanelUi {
    fn drop(&mut self) {
        let w = self.window.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(panel) = w.upgrade() {
                let _ = panel.window().hide();
            }
        });
    }
}

/// Extract a display name from a preset file path like "bounce.json" → "bounce".
fn extract_preset_name(file: &str) -> &str {
    if file.is_empty() {
        return "None";
    }
    std::path::Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
}
