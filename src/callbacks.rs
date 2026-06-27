use crate::engine::Engine;
use slint::ComponentHandle;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Generates a toggle callback for the animation/border/shader pattern.
///
/// These three callbacks are structurally identical — only the config field
/// name, the Engine method, and the UI setter differ.
macro_rules! make_toggle_callback {
    ($eng:expr, $cfg:expr, $weak:expr, $active_field:ident, $apply_method:ident, $set_ui:ident, $err_label:expr) => {
        move |idx, file| {
            let file_str = file.to_string();
            let is_deactivate = {
                let cfg = $cfg.lock().unwrap();
                cfg.$active_field == file_str
            };
            let new_file = if is_deactivate {
                String::new()
            } else {
                file_str
            };

            let arg = if is_deactivate { "none" } else { &new_file };
            let result = $eng.$apply_method(arg);
            if let Err(e) = result {
                eprintln!("[HVE] {} error: {}", $err_label, e);
            }

            {
                let mut cfg = $cfg.lock().unwrap();
                cfg.$active_field = new_file;
                let _ = cfg.save();
            }

            if let Some(w) = $weak.upgrade() {
                w.$set_ui(if is_deactivate { -1 } else { idx });
            }
        }
    };
}

pub fn setup_callbacks(
    window: &crate::MainWindow,
    cfg: &crate::config::Config,
    proj: PathBuf,
    tray_active: Arc<AtomicBool>,
) {
    // Single Engine + Config instances shared across all callbacks
    let eng = Arc::new(Engine::new(&proj));
    let cfg = Arc::new(Mutex::new(cfg.clone()));

    // System toggle — skip first call (UI fires on init)
    {
        let eng = eng.clone();
        let cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_toggle_system(move |active| {
            tray_active.store(active, Ordering::Relaxed);
            {
                let mut cfg = cfg.lock().unwrap();
                cfg.is_system_active = active;
                let _ = cfg.save();
            }
            let result = if active {
                eng.init_enable()
            } else {
                eng.init_disable()
            };
            if let Err(e) = result {
                eprintln!("[HVE] Init error: {}", e);
            }
            if let Some(w) = weak.upgrade() {
                w.set_system_active(active);
            }
        });
    }

    // Animation toggle
    {
        let eng = eng.clone();
        let cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_apply_animation(make_toggle_callback!(
            eng,
            cfg,
            weak,
            active_anim_file,
            apply_animation,
            set_active_anim_index,
            "Animation"
        ));
    }

    // Border toggle
    {
        let eng = eng.clone();
        let cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_apply_border(make_toggle_callback!(
            eng,
            cfg,
            weak,
            active_border_file,
            apply_border,
            set_active_border_index,
            "Border"
        ));
    }

    // Shader toggle
    {
        let eng = eng.clone();
        let cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_apply_shader(make_toggle_callback!(
            eng,
            cfg,
            weak,
            active_shader_file,
            apply_shader,
            set_active_shader_index,
            "Shader"
        ));
    }

    // Geometry change — skip first call (Slider fires on init)
    {
        let eng = eng.clone();
        let cfg = cfg.clone();
        let weak = window.as_weak();
        let mut geo_init = true;
        window.on_apply_geometry(move |size| {
            if geo_init {
                geo_init = false;
                return;
            }
            {
                let cfg = cfg.lock().unwrap();
                if size == cfg.border_size {
                    return;
                }
            }
            {
                let mut cfg = cfg.lock().unwrap();
                cfg.border_size = size;
                let _ = cfg.save();
            }
            let result = eng.apply_geometry(size);
            if let Err(e) = result {
                eprintln!("[HVE] Geometry error: {}", e);
            }
            // Update UI slider value
            if let Some(w) = weak.upgrade() {
                w.set_border_size(size);
            }
        });
    }
}
