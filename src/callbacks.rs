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
            let (is_deactivate, new_file) = {
                let mut cfg = $cfg.lock().unwrap_or_else(|e| e.into_inner());
                let is_deact = cfg.$active_field == file_str;
                let new = if is_deact { String::new() } else { file_str.clone() };
                cfg.$active_field = new.clone();
                let _ = cfg.save();
                (is_deact, new)
            };

            let arg = if is_deactivate { "none" } else { &new_file };
            let result = $eng.$apply_method(arg);
            if let Err(e) = result {
                tracing::error!("[HVE] {} error: {}", $err_label, e);
            }

            if let Some(w) = $weak.upgrade() {
                w.$set_ui(if is_deactivate { -1 } else { idx });
            }
        }
    };
}

pub fn setup_callbacks(
    window: &crate::MainWindow,
    cfg: &Arc<Mutex<crate::config::Config>>,
    proj: PathBuf,
    tray_active: Arc<AtomicBool>,
) {
    // Single Engine + Config instances shared across all callbacks
    let eng = Arc::new(Engine::new(&proj));
    let cfg = cfg.clone();

    // System toggle — skip first call (UI fires on init)
    {
        let eng = eng.clone();
        let cfg = cfg.clone();
        let weak = window.as_weak();
        window.on_toggle_system(move |active| {
            tray_active.store(active, Ordering::Relaxed);
            {
                let mut cfg = cfg.lock().unwrap_or_else(|e| e.into_inner());
                cfg.is_system_active = active;
                let _ = cfg.save();
            }
            let result = if active {
                eng.init_enable()
            } else {
                eng.init_disable()
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Init error: {}", e);
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
            let (radius, gaps_in, gaps_out, current) = {
                let cfg = cfg.lock().unwrap_or_else(|e| e.into_inner());
                (
                    cfg.border_radius,
                    cfg.gaps_in,
                    cfg.gaps_out,
                    cfg.border_size,
                )
            };
            if size == current {
                return;
            }
            {
                let mut cfg = cfg.lock().unwrap_or_else(|e| e.into_inner());
                cfg.border_size = size;
                let _ = cfg.save();
            }
            let result = eng.apply_geometry(size, radius, gaps_in, gaps_out);
            if let Err(e) = result {
                tracing::error!("[HVE] Geometry error: {}", e);
            }
            // Update UI slider value
            if let Some(w) = weak.upgrade() {
                w.set_border_size(size);
            }
        });
    }

    // Corner radius change — skip first call (Slider fires on init)
    {
        let eng = eng.clone();
        let cfg = cfg.clone();
        let weak = window.as_weak();
        let mut radius_init = true;
        window.on_apply_geometry_radius(move |radius| {
            if radius_init {
                radius_init = false;
                return;
            }
            let (size, gaps_in, gaps_out, current) = {
                let cfg = cfg.lock().unwrap_or_else(|e| e.into_inner());
                (
                    cfg.border_size,
                    cfg.gaps_in,
                    cfg.gaps_out,
                    cfg.border_radius,
                )
            };
            if radius == current {
                return;
            }
            {
                let mut cfg = cfg.lock().unwrap_or_else(|e| e.into_inner());
                cfg.border_radius = radius;
                let _ = cfg.save();
            }
            let result = eng.apply_geometry(size, radius, gaps_in, gaps_out);
            if let Err(e) = result {
                tracing::error!("[HVE] Radius error: {}", e);
            }
            // Update UI slider value
            if let Some(w) = weak.upgrade() {
                w.set_corner_radius(radius);
            }
        });
    }

    // Gaps-in change (between windows) — skip first call (Slider fires on init)
    {
        let eng = eng.clone();
        let cfg = cfg.clone();
        let weak = window.as_weak();
        let mut gaps_init = true;
        window.on_apply_geometry_gaps_in(move |gap| {
            if gaps_init {
                gaps_init = false;
                return;
            }
            let (size, radius, current_in, current_out) = {
                let cfg = cfg.lock().unwrap_or_else(|e| e.into_inner());
                (
                    cfg.border_size,
                    cfg.border_radius,
                    cfg.gaps_in,
                    cfg.gaps_out,
                )
            };
            if gap == current_in {
                return;
            }
            {
                let mut cfg = cfg.lock().unwrap_or_else(|e| e.into_inner());
                cfg.gaps_in = gap;
                let _ = cfg.save();
            }
            let result = eng.apply_geometry(size, radius, gap, current_out);
            if let Err(e) = result {
                tracing::error!("[HVE] Gaps-in error: {}", e);
            }
            // Update UI slider value
            if let Some(w) = weak.upgrade() {
                w.set_gap_in(gap);
            }
        });
    }

    // Gaps-out change (windows ↔ monitor edges) — skip first call
    {
        let eng = eng.clone();
        let cfg = cfg.clone();
        let weak = window.as_weak();
        let mut gaps_init = true;
        window.on_apply_geometry_gaps_out(move |gap| {
            if gaps_init {
                gaps_init = false;
                return;
            }
            let (size, radius, current_in, current_out) = {
                let cfg = cfg.lock().unwrap_or_else(|e| e.into_inner());
                (
                    cfg.border_size,
                    cfg.border_radius,
                    cfg.gaps_in,
                    cfg.gaps_out,
                )
            };
            if gap == current_out {
                return;
            }
            {
                let mut cfg = cfg.lock().unwrap_or_else(|e| e.into_inner());
                cfg.gaps_out = gap;
                let _ = cfg.save();
            }
            let result = eng.apply_geometry(size, radius, current_in, gap);
            if let Err(e) = result {
                tracing::error!("[HVE] Gaps-out error: {}", e);
            }
            // Update UI slider value
            if let Some(w) = weak.upgrade() {
                w.set_gap_out(gap);
            }
        });
    }

    // Open project documentation (WIKI.md / README.md) in the default viewer
    {
        let wiki_path = proj.join("WIKI.md");
        window.on_open_docs(move || {
            let target = if wiki_path.exists() {
                wiki_path.clone()
            } else {
                proj.join("README.md")
            };
            tracing::info!("[about] Opening documentation: {}", target.display());
            let _ = std::process::Command::new("xdg-open").arg(&target).spawn();
        });
    }
}
