use crate::app_state::{AppState, SharedState};
use crate::config::Config;
use crate::settings::{set_autostart, set_keybinds, set_tiling_window_rules};
use crate::shell::nav::{NavCommand, Screen};
use crate::shell::gallery::views::mosaic::MosaicPages;
use crate::shell::Shell;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Lock the shared state, recovering from a poisoned mutex (same policy the
/// previous per-piece locks used).
fn lock_state(state: &SharedState) -> MutexGuard<'_, AppState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// Generates a toggle callback for the animation/border/shader pattern.
///
/// These three callbacks are structurally identical — only the config field
/// name, the Engine method, and the UI setter differ. All callbacks run on
/// the Slint event-loop thread, so holding the single `AppState` lock across
/// the engine subprocess call is safe (no other thread contends on it).
macro_rules! make_toggle_callback {
    ($state:expr, $weak:expr, $active_field:ident, $apply_method:ident, $set_ui:ident, $err_label:expr) => {
        move |idx, file| {
            let file_str = file.to_string();
            let (is_deactivate, result) = {
                let mut state = lock_state(&$state);
                let is_deact = state.cfg().$active_field == file_str;
                let new = if is_deact { String::new() } else { file_str.clone() };
                state.cfg_mut().$active_field = new.clone();
                let _ = state.cfg().save();
                let arg = if is_deact { "none" } else { &new };
                let result = state.engine().$apply_method(arg);
                (is_deact, result)
            };
            if let Err(e) = result {
                tracing::error!("[HVE] {} error: {}", $err_label, e);
            }

            if let Some(w) = $weak.upgrade() {
                w.$set_ui(if is_deactivate { -1 } else { idx });
            }
        }
    };
}

/// Registers every Slint UI callback. Call once from `main()` after the
/// window and the shared `AppState` are initialized.
///
/// Dependencies are passed by reference; each callback clones what it
/// captures. All Slint callbacks run on the same thread, so
/// clone-then-save is safe.
pub fn setup_callbacks(
    window: &crate::MainWindow,
    state: &SharedState,
    proj: PathBuf,
    tray_active: Arc<AtomicBool>,
    restart_lock: &Arc<Mutex<Option<std::fs::File>>>,
    shell: &Rc<RefCell<Shell>>,
    mosaic_pages: std::sync::Arc<std::sync::Mutex<MosaicPages>>,
    refresh_mosaic_page: std::sync::Arc<dyn Fn(bool) + Send + Sync>,
    refresh_slice_ring: std::sync::Arc<dyn Fn() + Send + Sync>,
    animate_slice_step: std::sync::Arc<dyn Fn(isize) + Send + Sync>,
) {
    // Single AppState instance shared across all callbacks
    let state = state.clone();

    // ── HVE 2 shell callbacks (delivery 4/5 + PR4 4.7/4.5) ──
    // Map shell UI callbacks to NavCommand (nav-shell spec R4; design D8).
    // PR4: Home→Expand (S1) via card_activated, arrow→apply (S2/S12) via nav_move,
    //      Back/Esc collapse settings S10/11 + ExpandToSettings S9 size mutation.
    {
        let shell = shell.clone();
        window.on_card_activated(move |card_idx| {
            // If already in Gallery expanded, card click is GallerySlot action:
            // current card → ExpandToSettings (S9) 1200x800→1300x900, non-current → instant apply via state (handled below via gallery apply path)
            let is_gallery = Shell::with_nav(&shell, |n| {
                n.screen() == Screen::Gallery && n.expansion() != crate::shell::nav::ExpansionState::Collapsed
            });
            if is_gallery {
                // Check if window already at settings size — if so, ignore
                let (w, h) = Shell::current_size(&shell);
                let at_settings = (w - 1300.0).abs() < 1.0 && (h - 900.0).abs() < 1.0;
                if !at_settings {
                    // Expand card to settings panel (same window mutates, 22 steps 350ms OutCubic)
                    Shell::expand_to_settings(&shell);
                }
                return;
            }
            if let Some(screen) = Screen::from_card_index(card_idx as usize) {
                Shell::dispatch(&shell, NavCommand::Expand(screen));
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_back_activated(move || {
            // Esc now hides/minimizes the window (same as tray minimize via
            // Controller::toggle_tray hide path → composer.hide). Idempotent:
            // if already hidden, do nothing. Supersedes spec Settings Mutation
            // & Esc Collapse — gallery/settings no longer collapses via Esc.
            let Some(win) = weak.upgrade() else { return; };
            let Some(mut ctrl) = crate::composer::global_controller() else { return; };
            if ctrl.window_hidden() {
                return;
            }
            ctrl.toggle_tray(&win);
            crate::tray::refresh_global_menu();
        });
    }
    {
        let shell = shell.clone();
        let weak = window.as_weak();
        // Clone the page-model handles so the later page-step handler (and
        // this closure) each own a reference without consuming the originals.
        let mosaic_pages = mosaic_pages.clone();
        let refresh_mosaic_page = refresh_mosaic_page.clone();
        // V3: Slice keys flow directionally via animate_slice_step (fluid
        // strip), not instant refresh. Pre-clone to avoid moving the original
        // before the wheel handler below.
        let animate_nav = animate_slice_step.clone();
        let refresh_slice_ring_nav = refresh_slice_ring.clone();
        let _ = &refresh_slice_ring_nav; // keep for non-slice fallback if needed
        window.on_nav_move(move |direction| {
            use crate::shell::nav::ExpansionState;
            use crate::shell::nav::Screen;
            use slint::Model;
            let is_gallery = Shell::with_nav(&shell, |n| {
                n.screen() == Screen::Gallery && n.expansion() != ExpansionState::Collapsed
            });
            let delta: i32 = match direction.as_str() {
                "down" | "right" => 1,
                "up" | "left" => -1,
                _ => 0,
            };
            if delta == 0 {
                return;
            }
            if is_gallery {
                // Mosaic style: Left/Up = previous page, Right/Down = next.
                // Reuses the page model; geometry is not rebuilt on a flip.
                let style = weak.upgrade().map(|w| w.get_gallery_style()).unwrap_or(0);
                if style == 2 {
                    let changed = mosaic_pages.lock().unwrap().step(delta as isize);
                    if changed {
                        refresh_mosaic_page(true); // mosaic page step is a flip
                    }
                    return;
                }
                // Slice style 0: fluid directional strip (V3)
                if style == 0 {
                    animate_nav(delta as isize);
                    return;
                }
                if let Some(w) = weak.upgrade() {
                    let len = w.get_gallery_cards().row_count() as i32;
                    if len > 0 {
                        let cur = w.get_gallery_focused();
                        let max = len - 1;
                        // Non-slice gallery styles clamp (S13) — slice uses wrap above.
                        let next = (cur + delta).clamp(0, max);
                        w.set_gallery_focused(next);
                    }
                }
            } else {
                Shell::move_focus(&shell, delta as isize);
            }
        });
    }

    // ── Gallery wheel (S2 ring): debounce Timer fires once per idle gesture;
    // V3 directional fluid — animate strip, not instant.
    {
        let weak = window.as_weak();
        let animate = animate_slice_step.clone();
        let _refresh = refresh_slice_ring.clone();
        let _ = &_refresh;
        window.on_gallery_wheel_step(move |dir| {
            let _ = &weak;
            animate(dir as isize);
        });
    }

    // ── Mosaic page flip (Mosaic final scheme): the MosaicView wheel debounce
    // fires ±1; we step the Rust page model and re-render the current page.
    // Geometry is NOT rebuilt on a flip (MosaicPages::step is pure). Clamped,
    // no wrap. ──
    {
        let pages = mosaic_pages.clone();
        let refresh = refresh_mosaic_page.clone();
        window.on_gallery_mosaic_page_step(move |dir| {
            let changed = pages.lock().unwrap().step(dir as isize);
            if changed {
                refresh(true); // page flip → snapshot under layer
            }
        });
    }

    // System toggle — skip first call (UI fires on init)
    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_toggle_system(move |active| {
            tray_active.store(active, Ordering::Relaxed);
            let result = {
                let mut state = lock_state(&state);
                state.toggle_system(active)
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
        let state = state.clone();
        let weak = window.as_weak();
        window.on_apply_animation(make_toggle_callback!(
            state,
            weak,
            active_anim_file,
            apply_animation,
            set_active_anim_index,
            "Animation"
        ));
    }

    // Border toggle
    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_apply_border(make_toggle_callback!(
            state,
            weak,
            active_border_file,
            apply_border,
            set_active_border_index,
            "Border"
        ));
    }

    // Shader toggle
    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_apply_shader(make_toggle_callback!(
            state,
            weak,
            active_shader_file,
            apply_shader,
            set_active_shader_index,
            "Shader"
        ));
    }

    // Geometry change — skip first call (Slider fires on init)
    {
        let state = state.clone();
        let weak = window.as_weak();
        let mut geo_init = true;
        window.on_apply_geometry(move |size| {
            if geo_init {
                geo_init = false;
                return;
            }
            let result = {
                let mut state = lock_state(&state);
                if size == state.cfg().border_size {
                    return;
                }
                state.cfg_mut().border_size = size;
                state.apply_geometry()
            };
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
        let state = state.clone();
        let weak = window.as_weak();
        let mut radius_init = true;
        window.on_apply_geometry_radius(move |radius| {
            if radius_init {
                radius_init = false;
                return;
            }
            let result = {
                let mut state = lock_state(&state);
                if radius == state.cfg().border_radius {
                    return;
                }
                state.cfg_mut().border_radius = radius;
                state.apply_geometry()
            };
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
        let state = state.clone();
        let weak = window.as_weak();
        let mut gaps_init = true;
        window.on_apply_geometry_gaps_in(move |gap| {
            if gaps_init {
                gaps_init = false;
                return;
            }
            let result = {
                let mut state = lock_state(&state);
                if gap == state.cfg().gaps_in {
                    return;
                }
                state.cfg_mut().gaps_in = gap;
                state.apply_geometry()
            };
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
        let state = state.clone();
        let weak = window.as_weak();
        let mut gaps_init = true;
        window.on_apply_geometry_gaps_out(move |gap| {
            if gaps_init {
                gaps_init = false;
                return;
            }
            let result = {
                let mut state = lock_state(&state);
                if gap == state.cfg().gaps_out {
                    return;
                }
                state.cfg_mut().gaps_out = gap;
                state.apply_geometry()
            };
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

    // ── Close button callback ──
    {
        let weak = window.as_weak();
        window.on_close_button_clicked(move || {
            crate::countdown::minimize_now(weak.clone());
        });
    }

    // ── Settings callbacks ──
    // We use clone-then-save since all Slint callbacks run on the same thread
    {
        let window_weak = window.as_weak();
        window.on_toggle_settings(move || {
            let w = window_weak.upgrade().unwrap();
            w.set_settings_open(!w.get_settings_open());
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_toggle_auto_minimize(move |enabled| {
            lock_state(&state).update_cfg(|c| c.auto_minimize_enabled = enabled);
            if let Some(w) = weak.upgrade() {
                w.set_auto_minimize(enabled);
            }
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_change_minimize_seconds(move |secs| {
            lock_state(&state).update_cfg(|c| c.minimize_seconds = secs);
            if let Some(w) = weak.upgrade() {
                w.set_minimize_seconds(secs);
            }
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_change_language(move |lang| {
            let lang_str = lang.to_string();
            lock_state(&state).update_cfg(|c| c.language = lang_str.clone());
            if let Some(w) = weak.upgrade() {
                w.set_language(lang);
                w.set_restart_required(true);
            }
            tracing::info!("Language changed to {}. Restart to apply fully.", lang_str);
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_toggle_tiling_mode(move |enabled| {
            lock_state(&state).update_cfg(|c| c.tiling_mode = enabled);
            if let Some(w) = weak.upgrade() {
                w.set_tiling_mode(enabled);
                w.set_restart_required(false);
            }

            // Toggle window rules in hyprland.conf — this persists across restarts
            set_tiling_window_rules(enabled);

            // Try to toggle the CURRENT window immediately via Composer
            if let Some(ctrl) = crate::composer::global_controller() {
                ctrl.composer().toggle_float();
            }

            // Show restart banner explaining that full effect requires restart
            if let Some(w) = weak.upgrade() {
                w.set_restart_required(true);
            }
            tracing::info!(
                "[settings] Tiling mode {} — config updated + togglefloating dispatched",
                if enabled { "ON" } else { "OFF" }
            );
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_toggle_keybinds(move |enabled| {
            lock_state(&state).update_cfg(|c| c.keybinds_enabled = enabled);
            if let Some(w) = weak.upgrade() {
                w.set_keybinds_mode(enabled);
            }
            set_keybinds(enabled);
            tracing::info!("[settings] Keyboard shortcuts {}", if enabled { "ON" } else { "OFF" });
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_toggle_autostart(move |enabled| {
            lock_state(&state).update_cfg(|c| c.auto_start = enabled);
            if let Some(w) = weak.upgrade() {
                w.set_autostart(enabled);
            }
            set_autostart(enabled);
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_change_theme(move |theme| {
            let theme_str = theme.to_string();
            {
                let mut state = lock_state(&state);
                state.update_cfg(|c| c.theme = theme_str.clone());
                if let Some(w) = weak.upgrade() {
                    // Apply new palette immediately (no restart needed)
                    state.refresh_visual_state(&w, &theme_str);
                    w.set_theme(theme);
                }
            }
            tracing::info!("Theme changed to {}", theme_str);
        });
    }

    // Restart the app: release the exclusive lock so the new instance can
    // acquire it, spawn ourselves again, then quit the event loop.
    {
        let restart_lock = restart_lock.clone();
        window.on_restart_app(move || {
            tracing::info!("[settings] Restarting app...");
            // 1. Release the exclusive lock so the new instance can start
            if let Ok(mut guard) = restart_lock.lock() {
                drop(guard.take());
            }
            // 2. Spawn the new instance
            let exe = resolve_exe();
            match std::process::Command::new(&exe)
                .args(std::env::args().skip(1))
                .spawn()
            {
                Ok(child) => {
                    tracing::info!(
                        "[settings] New instance spawned (PID: {}), quitting event loop",
                        child.id()
                    );
                }
                Err(e) => {
                    tracing::error!("[settings] Failed to restart: {}", e);
                    return;
                }
            }
            // 3. Gracefully quit the event loop — the main function continues past
            //    run_event_loop_until_quit() and returns Ok(()), letting the process
            //    die naturally. The new instance already has the lock released.
            let _ = slint::quit_event_loop();
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_reset_presets(move || {
            tracing::info!("[settings] Resetting presets...");
            if let Some(w) = weak.upgrade() {
                w.set_active_anim_index(-1);
                w.set_active_border_index(-1);
                w.set_active_shader_index(-1);
            }
            lock_state(&state).update_cfg(|c| {
                c.active_anim_file = String::new();
                c.active_border_file = String::new();
                c.active_shader_file = String::new();
            });
            tracing::info!("[settings] Presets reset complete.");
        });
    }

    // ── Theme callbacks ──
    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_save_theme(move |name| {
            let name_str = name.to_string();
            let result = {
                let mut state = lock_state(&state);
                let provider_ids = state.theme_manager().provider_ids();
                state.theme_manager_mut().save(&name_str, &provider_ids)
            };
            match result {
                Ok(_) => {
                    tracing::info!("[themes] Saved theme: {}", name_str);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_busy(true);
                        lock_state(&state).refresh_theme_list(&w);
                        w.set_theme_busy(false);
                    }
                }
                Err(e) => {
                    tracing::error!("[themes] Save failed: {}", e);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_error_text(e.into());
                    }
                }
            }
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_apply_theme(move |name| {
            let name_str = name.to_string();
            // Suprime el auto-minimize mientras se aplica: hyprctl reload y la
            // regeneración de window rules hacen que la ventana pierda foco
            // varias veces, y eso no debe disparar un countdown espurio.
            crate::countdown::suppress_auto_minimize(std::time::Duration::from_secs(4));
            let result = {
                let mut state = lock_state(&state);
                state.theme_manager_mut().apply(&name_str, || {
                    // Reload Hyprland AFTER all providers' apply() + post_apply() are done.
                    // This includes wallpaper IPC which runs inside NoctaliaV4Provider::post_apply().
                    tracing::info!("[themes] Reloading Hyprland after theme apply...");
                    std::process::Command::new("hyprctl")
                        .arg("reload")
                        .output()
                        .map(|_| ())
                        .map_err(|e| format!("hyprctl reload failed: {e}"))
                })
            };
            match result {
                Ok(_) => {
                    tracing::info!("[themes] Applied theme: {}", name_str);
                    // Reload config from disk — the provider may have updated it
                    let updated_cfg = {
                        let mut state = lock_state(&state);
                        state.reload_config_after_theme(&name_str)
                    };
                    if let Some(w) = weak.upgrade() {
                        sync_preset_indices(&w, &updated_cfg);
                        let state = lock_state(&state);
                        // Re-read colors from the restored files and update the UI
                        state.refresh_visual_state(&w, &w.get_theme().to_string());
                        state.refresh_theme_list(&w);
                        w.set_home_active_theme_name(name_str.clone().into());
                    }
                }
                Err(e) => {
                    tracing::error!("[themes] Apply failed: {}", e);
                }
            }
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_delete_theme(move |name| {
            let name_str = name.to_string();
            let result = {
                let mut state = lock_state(&state);
                state.theme_manager_mut().delete(&name_str)
            };
            match result {
                Ok(_) => {
                    tracing::info!("[themes] Deleted theme: {}", name_str);
                    if let Some(w) = weak.upgrade() {
                        lock_state(&state).refresh_theme_list(&w);
                    }
                }
                Err(e) => {
                    tracing::error!("[themes] Delete failed: {}", e);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_error_text(e.into());
                    }
                }
            }
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_rename_theme(move |old, new| {
            let old_str = old.to_string();
            let new_str = new.to_string();
            let result = {
                let mut state = lock_state(&state);
                state.theme_manager_mut().rename(&old_str, &new_str)
            };
            match result {
                Ok(_) => {
                    tracing::info!("[themes] Renamed: {} -> {}", old_str, new_str);
                    if let Some(w) = weak.upgrade() {
                        let state = lock_state(&state);
                        state.refresh_theme_list(&w);
                        w.set_home_active_theme_name(state.theme_manager().last_applied.clone().into());
                    }
                }
                Err(e) => {
                    tracing::error!("[themes] Rename failed: {}", e);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_error_text(e.into());
                    }
                }
            }
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_overwrite_theme(move |name| {
            let name_str = name.to_string();
            let result = {
                let mut state = lock_state(&state);
                let provider_ids = state.theme_manager().provider_ids();
                state.theme_manager_mut().save(&name_str, &provider_ids)
            };
            match result {
                Ok(_) => {
                    tracing::info!("[themes] Overwritten theme: {}", name_str);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_busy(true);
                        lock_state(&state).refresh_theme_list(&w);
                        w.set_theme_busy(false);
                    }
                }
                Err(e) => {
                    tracing::error!("[themes] Overwrite failed: {}", e);
                    if let Some(w) = weak.upgrade() {
                        w.set_theme_error_text(e.into());
                    }
                }
            }
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_refresh_themes(move || {
            if let Some(w) = weak.upgrade() {
                let mut state = lock_state(&state);
                // 1. Re-read colors from the current system state and update the UI
                state.refresh_visual_state(&w, &w.get_theme().to_string());

                // 2. If there's a last applied theme, overwrite it with the current state
                //    so the ↻ acts as "save current changes to active theme"
                let provider_ids = state.theme_manager().provider_ids();
                let last = state.theme_manager().last_applied.clone();
                if !last.is_empty() {
                    let _ = state.theme_manager_mut().save(&last, &provider_ids);
                }

                // 3. Refresh the theme list
                state.refresh_theme_list(&w);
                let last_applied = state.theme_manager().last_applied.clone();
                drop(state);
                w.set_home_active_theme_name(last_applied.into());
            }
        });
    }

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_search_query_changed(move |query| {
            if let Some(w) = weak.upgrade() {
                let state = lock_state(&state);
                let all = state.theme_manager().list().unwrap_or_default();
                let q = query.to_lowercase();
                let filtered: Vec<_> = all.iter()
                    .filter(|t| t.name.to_lowercase().contains(&q))
                    .collect();

                use slint::{ModelRc, SharedString};
                let names: Vec<SharedString> = filtered.iter().map(|t| SharedString::from(&t.name)).collect();
                let saved_ats: Vec<SharedString> = filtered.iter().map(|t| SharedString::from(&t.saved_at)).collect();
                let is_actives: Vec<bool> = filtered.iter().map(|t| t.is_active).collect();

                w.set_theme_names(ModelRc::from(names.as_slice()));
                w.set_theme_saved_ats(ModelRc::from(saved_ats.as_slice()));
                w.set_theme_is_actives(ModelRc::from(is_actives.as_slice()));
            }
        });
    }

    // ── Intercept close events — always hide instead of destroying
    //     the window so the global event loop keeps running. ──
    {
        let win_weak_for_close = window.as_weak();
        window.window().on_close_requested(move || {
            // "Secuestramos" el cierre (SUPER+C) para que haga lo MISMO que el
            // toggle de SUPER+H: esconder la ventana en el escritorio oculto
            // (special workspace) en lugar de destruir la superficie. Así nunca se
            // recrea y el foco de teclado se conserva. KeepWindowShown rechaza la
            // destrucción y el hide() del compositor la retira a pantalla aparte.
            let needs_slint_hide = {
                if let Some(win) = win_weak_for_close.upgrade() {
                    if let Some(mut ctrl) = crate::composer::global_controller() {
                        // La ventana está visible al cerrar → hide path (al escondite).
                        ctrl.toggle_tray(&win);
                        // Si el compositor no pudo mover al escondite (hide() cayó al
                        // fallback Slint), la ventana no quedó oculta por hyprctl →
                        // hay que ocultarla por Slint.
                        !ctrl.window_hidden()
                    } else {
                        // Unreachable in production: the controller is initialized
                        // in main() before the event loop runs. Keep the window.
                        false
                    }
                } else {
                    // Ventana ya muerta: nada que ocultar, que respete el codigo default.
                    true
                }
            };
            crate::tray::refresh_global_menu();
            if needs_slint_hide {
                slint::CloseRequestResponse::HideWindow
            } else {
                // El compositor ya la movió al escondite: rechazamos la destrucción
                // y dejamos que la superficie siga viva.
                slint::CloseRequestResponse::KeepWindowShown
            }
        });
    }
}

/// After applying a theme, sync the GUI preset indices (anim, border, shader, border-size)
/// so they reflect what the theme restored, not the stale values from before apply.
fn sync_preset_indices(
    window: &crate::MainWindow,
    cfg: &Config,
) {
    use slint::Model;

    let find = |files: &slint::ModelRc<slint::SharedString>, target: &str| -> i32 {
        if target.is_empty() {
            return -1;
        }
        for i in 0..files.row_count() {
            if files.row_data(i).as_ref().map(|s| s.as_str()) == Some(target) {
                return i as i32;
            }
        }
        -1
    };

    window.set_active_anim_index(find(&window.get_anim_files(), &cfg.active_anim_file));
    window.set_active_border_index(find(&window.get_border_files(), &cfg.active_border_file));
    window.set_active_shader_index(find(&window.get_shader_files(), &cfg.active_shader_file));
    window.set_border_size(cfg.border_size);
    window.set_corner_radius(cfg.border_radius);
    window.set_gap_in(cfg.gaps_in);
    window.set_gap_out(cfg.gaps_out);
}

/// Resolve the current executable path for the restart action.
fn resolve_exe() -> PathBuf {
    if let Ok(path) = std::env::current_exe() {
        if path.is_file() {
            return path;
        }
    }
    if let Some(arg0) = std::env::args().next() {
        let p = PathBuf::from(&arg0);
        if p.is_absolute() {
            if p.is_file() {
                return p;
            }
        } else if let Ok(paths) = std::env::var("PATH") {
            for dir in std::env::split_paths(&paths) {
                let candidate = dir.join(&arg0);
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    PathBuf::from("hve")
}