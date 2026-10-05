use crate::app_state::{AppState, SharedState};
use crate::config::Config;
use crate::settings::{set_autostart, set_tiling_window_rules};
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
#[allow(unused_macros)]
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

    // ── HVE shell callbacks (delivery 4/5 + PR4 4.7/4.5) ──
    // Map shell UI callbacks to NavCommand (nav-shell spec R4; design D8).
    // PR4: Home→Expand (S1) via card_activated, arrow→apply (S2/S12) via nav_move,
    //      Back/Esc collapse settings S10/11 + ExpandToSettings S9 size mutation.
    {
        let shell = shell.clone();
        window.on_card_activated(move |card_idx| {
            tracing::debug!("{}", mouse_trace(&format!("card-activated idx={card_idx} (home card click)")));
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
            tracing::debug!("{}", mouse_trace("back-activated (chrome back / Esc-hide)"));
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
            // R9 input guard: ignore arrows while mutating
            if crate::shell::Shell::is_mutating(&shell) {
                tracing::debug!("[shell] nav-move ignored — mutating");
                return;
            }
            use crate::shell::nav::ExpansionState;
            use crate::shell::nav::Screen;
            use slint::Model;
            let is_gallery = Shell::with_nav(&shell, |n| {
                n.screen() == Screen::Gallery && n.expansion() != ExpansionState::Collapsed
            });
            // Plain Up/Down in the Gallery touch NOTHING here: the drawers are
            // Ctrl-only (`shell-kbd`'s Ctrl branch plus the Ctrl-gated
            // `gallery-keys`), and the vertical axis is reserved for a future
            // use. Only Left/Right move the carousel — the `is_gallery` branch
            // below. With no drawer case left, the gallery's up/down delta is
            // already 0, which IS "do nothing".
            let delta: i32 = match direction.as_str() {
                "right" => 1,
                "left" => -1,
                "down" | "up" if !is_gallery => {
                    // In Home, Up/Down keep moving the vertical focus
                    if direction.as_str() == "down" { 1 } else { -1 }
                },
                _ => 0,
            };
            if delta == 0 {
                return;
            }
            if is_gallery {
                // If a drawer is open, Left/Right navigates it — do not move the carousel
                let drawer_open = weak.upgrade().map(|w| w.get_gallery_top_open() || w.get_gallery_bottom_open()).unwrap_or(false);
                if drawer_open {
                    return;
                }
                let style = weak.upgrade().map(|w| w.get_gallery_style()).unwrap_or(0);
                if style == 2 {
                    let changed = mosaic_pages.lock().unwrap().step(delta as isize);
                    if changed {
                        refresh_mosaic_page(true);
                    }
                    return;
                }
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
    // V3 directional fluid — animate strip, not instant. Gated while mutating (R9).
    {
        let weak = window.as_weak();
        let animate = animate_slice_step.clone();
        let shell_c = shell.clone();
        let _refresh = refresh_slice_ring.clone();
        let _ = &_refresh;
        window.on_gallery_wheel_step(move |dir| {
            if crate::shell::Shell::is_mutating(&shell_c) {
                tracing::debug!("[gallery] wheel-step ignored — mutating");
                return;
            }
            let _ = &weak;
            animate(dir as isize);
        });
    }

    // ── Mosaic page flip (Mosaic final scheme): the MosaicView wheel debounce
    // fires ±1; we step the Rust page model and re-render the current page.
    // Geometry is NOT rebuilt on a flip (MosaicPages::step is pure). Clamped,
    // no wrap. Gated while mutating (R9).
    {
        let pages = mosaic_pages.clone();
        let refresh = refresh_mosaic_page.clone();
        let shell_c = shell.clone();
        let weak = window.as_weak();
        window.on_gallery_mosaic_page_step(move |dir| {
            if crate::shell::Shell::is_mutating(&shell_c) {
                tracing::debug!("[gallery] mosaic-page-step ignored — mutating");
                return;
            }
            let changed = pages.lock().unwrap().step(dir as isize);
            if changed {
                // A page flip invalidates the grown tile: it belongs to the
                // page it was opened on, and re-anchoring it onto the new
                // page's tile would show one theme while applying another.
                if let Some(w) = weak.upgrade() {
                    crate::close_gallery_info(&w);
                }
                refresh(true); // page flip → snapshot under layer
            }
        });
    }

    // System toggle — skip first call (UI fires on init)
    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_toggle_system(move |active| {
            tracing::debug!("[system][mouse|kbd] toggle-system active={} (row 0)", active);
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

    // ── Legacy toggle/geometry/settings handlers — REMOVED slice 8 (R8) ──
    // Panel sections handle via panel-apply-* in src/main.rs; overlay handlers deleted.
    let _ = &state;
    let _ = &window; // keep bindings used

    // ── Legacy toggle/geometry/settings handlers — REMOVED slice 8 (R8) ──
    // Panel sections handle via panel-apply-* in src/main.rs; overlay handlers deleted.
    let _ = &state;
    let _ = &window; // keep bindings used

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
            tracing::debug!("{}", mouse_trace("close-button-clicked (chrome X)"));
            crate::countdown::minimize_now(weak.clone());
        });
    }

    // ── Legacy toggle/geometry/settings handlers — REMOVED slice 8 (R8) ──
    // Panel sections handle via panel-apply-* in src/main.rs; overlay handlers deleted.
    let _ = &state;
    let _ = &window; // keep bindings used

    {
        let state = state.clone();
        let weak = window.as_weak();
        window.on_toggle_auto_minimize(move |enabled| {
            tracing::debug!("[system][mouse|kbd] toggle-auto-minimize enabled={} (row 1)", enabled);
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
            tracing::debug!("[system][mouse|kbd] change-minimize-seconds {} (row 2 chips)", secs);
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
            tracing::debug!("[system][mouse|kbd] change-language {} (row 3 pills)", lang);
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
        window.on_toggle_autostart(move |enabled| {
            tracing::debug!("[system][mouse|kbd] toggle-autostart {} (row 4)", enabled);
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
            tracing::debug!("[system][mouse|kbd] change-theme {} (row 5 pills)", theme);
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

    // ── Legacy ThemesModule callbacks — REMOVED slice 8 (R8) ──
    // Gallery cards + panel SaveSection replace list/search/delete/rename UI.
    // Theme apply now via GallerySlot + panel_save_theme dual-sync in main.rs.

    // ── Intercept close events — always hide instead of destroying
    //     the window so the global event loop keeps running. ──
    {
        let win_weak_for_close = window.as_weak();
        window.window().on_close_requested(move || {
            // We "hijack" the close (SUPER+C) so it does the SAME as the
            // SUPER+H toggle: hide the window on the hidden desktop
            // (special workspace) instead of destroying the surface. That way it is never
            // recreated and keyboard focus is kept. KeepWindowShown rejects the
            // destruction and the compositor's hide() moves it to a screen of its own.
            let needs_slint_hide = {
                if let Some(win) = win_weak_for_close.upgrade() {
                    if let Some(mut ctrl) = crate::composer::global_controller() {
                        // The window is visible at close → hide path (to the hideout).
                        ctrl.toggle_tray(&win);
                        // If the compositor could not move it to the hideout (hide() fell to
                        // the Slint fallback), the window was not hidden by hyprctl →
                        // it must be hidden by Slint.
                        !ctrl.window_hidden()
                    } else {
                        // Unreachable in production: the controller is initialized
                        // in main() before the event loop runs. Keep the window.
                        false
                    }
                } else {
                    // Window already dead: nothing to hide, let it respect the default code.
                    true
                }
            };
            crate::tray::refresh_global_menu();
            if needs_slint_hide {
                slint::CloseRequestResponse::HideWindow
            } else {
                // The compositor already moved it to the hideout: we reject the destruction
                // and let the surface stay alive.
                slint::CloseRequestResponse::KeepWindowShown
            }
        });
    }
}

/// After applying a theme, sync the GUI preset indices (anim, border, shader, border-size)
/// so they reflect what the theme restored, not the stale values from before apply.
#[allow(dead_code)]
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

/// Panel Save dual-sync (mutating-window R10, slice 2).
/// Validates `name` trimmed: empty → error message, no save call.
/// Otherwise writes BOTH theme_managers via `save(name, provider_ids)` then
/// invokes `sync_cards` and `refresh_mosaic_page` callbacks so the Gallery
/// shows the new card instantly without restart.
#[allow(dead_code)]
pub fn handle_panel_save<F, G>(
    name: &str,
    tm_main: &mut crate::theme_manager::ThemeManager,
    tm_gallery: &mut crate::theme_manager::ThemeManager,
    sync_cards: F,
    refresh_mosaic_page: G,
    error_out: &mut String,
) -> bool
where
    F: FnOnce(),
    G: FnOnce(),
{
    let trimmed = name.trim();
    if trimmed.is_empty() {
        *error_out = "name-empty".to_string();
        return false;
    }
    let provider_ids = tm_main.provider_ids();
    let res_main = tm_main.save(trimmed, &provider_ids);
    if let Err(e) = res_main {
        *error_out = e;
        return false;
    }
    // Gallery TM must stay in sync — use same provider_ids slice (ids are same set)
    // If gallery save fails, we still report error but main already saved (design: both writes)
    let res_gallery = tm_gallery.save(trimmed, &provider_ids);
    if let Err(e) = res_gallery {
        *error_out = e;
        return false;
    }
    error_out.clear();
    sync_cards();
    refresh_mosaic_page();
    true
}

/// Save "My Themes" list rows (panel SaveSection).
/// Maps ThemeManager.list() → (name, saved_at, is_active) in list order,
/// the same source refresh_theme_list feeds into theme-names/saved-ats/
/// is-actives. Pure helper so the list shows name + date + active mark.
#[allow(dead_code)]
pub fn save_list_rows(tm: &crate::theme_manager::ThemeManager) -> Vec<(String, String, bool)> {
    tm.list()
        .unwrap_or_default()
        .iter()
        .map(|t| (t.name.clone(), t.saved_at.clone(), t.is_active))
        .collect()
}

/// One-time active-theme backfill resolver (startup, empty
/// `cfg.last_applied_theme` legacy configs). Old applies never recorded the
/// active theme, so after a restart no card was active and the carousel fell
/// back to the first one. Pure decision: adopt the FIRST theme (list order,
/// alphabetical) whose saved wallpaper equals the LIVE wallpaper. Zero or
/// several distinct wallpapers never guess. Identical duplicates (test
/// variants of the same look) resolve to the first — they are visually the
/// same state, so the first IS the applied look for every practical purpose.
/// `themes` arrives in `ThemeManager::list()` order: `(name, saved_wallpaper)`
/// where `None` = theme without a noctalia-v5 wallpaper snapshot.
#[allow(dead_code)]
pub fn backfill_active_candidate(
    themes: &[(String, Option<String>)],
    live_wallpaper: &str,
) -> Option<String> {
    if live_wallpaper.is_empty() {
        return None;
    }
    themes
        .iter()
        .find(|(_, wp)| wp.as_deref() == Some(live_wallpaper))
        .map(|(name, _)| name.clone())
}

/// Panel Save rename dual-sync (tranche B). Gallery side goes through the
/// GallerySlot (mutates gallery_tm + refreshes the slot model + counters);
/// main side follows via ThemeManager::rename. Both managers share one
/// themes dir on disk, so when the slot already moved the directory the main
/// rename reports "not found" — then we only converge last_applied.
#[allow(dead_code)]
pub fn handle_panel_rename(
    old: &str,
    new: &str,
    tm_main: &mut crate::theme_manager::ThemeManager,
    slot: &crate::shell::gallery::GallerySlot,
    error_out: &mut String,
) -> bool {
    if let Err(e) = slot.rename_theme(old, new) {
        *error_out = e;
        return false;
    }
    if let Err(e) = tm_main.rename(old, new) {
        let (old_t, new_t) = (old.trim(), new.trim());
        let converged = tm_main.list().unwrap_or_default().iter().any(|t| t.name == new_t);
        if !converged {
            *error_out = e;
            return false;
        }
        if tm_main.last_applied == old_t {
            tm_main.last_applied = new_t.to_string();
        }
    }
    error_out.clear();
    true
}

/// Panel Save delete dual-sync (tranche B). Same shared-storage pattern:
/// slot first (gallery_tm + model), main converges (clears last_applied).
#[allow(dead_code)]
pub fn handle_panel_delete(
    name: &str,
    tm_main: &mut crate::theme_manager::ThemeManager,
    slot: &crate::shell::gallery::GallerySlot,
    error_out: &mut String,
) -> bool {
    if let Err(e) = slot.delete_theme(name) {
        *error_out = e;
        return false;
    }
    if let Err(e) = tm_main.delete(name) {
        let gone = !tm_main.list().unwrap_or_default().iter().any(|t| t.name == name.trim());
        if !gone {
            *error_out = e;
            return false;
        }
        if tm_main.last_applied == name.trim() {
            tm_main.last_applied.clear();
        }
    }
    error_out.clear();
    true
}

/// Panel Save refresh (tranche B, master on_refresh_themes 1:1). Re-saves the
/// active theme with the current provider state in BOTH managers, so ↻ means
/// "store current look into the active theme". Empty last_applied → no-op ok.
#[allow(dead_code)]
pub fn handle_panel_refresh(
    tm_main: &mut crate::theme_manager::ThemeManager,
    tm_gallery: &mut crate::theme_manager::ThemeManager,
    error_out: &mut String,
) -> bool {
    let last = tm_main.last_applied.clone();
    if last.is_empty() {
        error_out.clear();
        return true;
    }
    let provider_ids = tm_main.provider_ids();
    if let Err(e) = tm_main.save(&last, &provider_ids) {
        *error_out = e;
        return false;
    }
    if let Err(e) = tm_gallery.save(&last, &provider_ids) {
        *error_out = e;
        return false;
    }
    error_out.clear();
    true
}

/// Live search filter (tranche B, master on_search_query_changed 1:1).
/// Case-insensitive substring on the name; empty query restores the full list.
#[allow(dead_code)]
pub fn filter_saved_rows(
    query: &str,
    rows: &[(String, String, bool)],
) -> Vec<(String, String, bool)> {
    let q = query.to_lowercase();
    if q.is_empty() {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|(n, _, _)| n.to_lowercase().contains(&q))
        .cloned()
        .collect()
}

/// Focus neighbor after delete (tranche B). The next card takes the deleted
/// slot; deleting the last one focuses the previous; empty list → -1.
#[allow(dead_code)]
pub fn focus_after_delete(deleted_idx: usize, new_len: usize) -> i32 {
    if new_len == 0 {
        return -1;
    }
    deleted_idx.min(new_len - 1) as i32
}

/// Save-list arrow navigation step (keyboard R11 v2) — 1:1 legacy wrap.
/// Unfocused (-1) starts at 0 on the FIRST step; steps wrap circularly
/// like master ui/main.slint step-down/step-up (last -> 0, 0 -> last);
/// empty list stays -1 (nothing focused).
#[allow(dead_code)]
pub fn save_nav_step(current: i32, delta: i32, len: usize) -> i32 {
    if len == 0 {
        return -1;
    }
    if current < 0 {
        return 0;
    }
    let last = len as i32 - 1;
    if delta > 0 {
        if current >= last { 0 } else { current + 1 }
    } else if delta < 0 {
        if current <= 0 { last } else { current - 1 }
    } else {
        current
    }
}

/// Borders pick layer helpers (mutating-window R3, slice 3 slice).
/// Scan is engine.scan("borders") → PresetInfo list; apply wraps
/// engine.apply_border with the file string from the card.
#[allow(dead_code)]
pub fn handle_borders_scan(engine: &crate::engine::Engine) -> Vec<crate::engine::PresetInfo> {
    engine.scan("borders").unwrap_or_default()
}
#[allow(dead_code)]
pub fn handle_borders_apply(engine: &crate::engine::Engine, file: &str) -> Result<String, crate::engine::EngineError> {
    let arg = if file.is_empty() { "none" } else { file };
    engine.apply_border(arg)
}

/// Motion pick layer helpers (mutating-window R4, slice 5).
/// Scan is engine.scan("animations") → PresetInfo list; apply wraps
/// engine.apply_animation with the file string from the card (empty→"none").
#[allow(dead_code)]
pub fn handle_motion_scan(engine: &crate::engine::Engine) -> Vec<crate::engine::PresetInfo> {
    engine.scan("animations").unwrap_or_default()
}
#[allow(dead_code)]
pub fn handle_motion_apply(engine: &crate::engine::Engine, file: &str) -> Result<String, crate::engine::EngineError> {
    let arg = if file.is_empty() || file == "none" { "none" } else { file };
    engine.apply_animation(arg)
}

/// Filters pick layer helpers (mutating-window R5, slice 6).
/// Scan is engine.scan("shaders") → PresetInfo list; apply wraps
/// engine.apply_shader with the file string from the card (empty→"none").
#[allow(dead_code)]
pub fn handle_filters_scan(engine: &crate::engine::Engine) -> Vec<crate::engine::PresetInfo> {
    engine.scan("shaders").unwrap_or_default()
}
#[allow(dead_code)]
pub fn handle_filters_apply(engine: &crate::engine::Engine, file: &str) -> Result<String, crate::engine::EngineError> {
    let arg = if file.is_empty() || file == "none" { "none" } else { file };
    engine.apply_shader(arg)
}

/// Motion bezier — 4 float sliders a,b,c,d (R4 tune).
/// x1=a and x2=c in [0,1], y1=b and y2=d may extend beyond per CSS cubic-bezier.
/// Defaults a=0.25,b=0.1,c=0.25,d=1.0 match the spec.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionBezier {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
}

impl Default for MotionBezier {
    fn default() -> Self {
        Self { a: 0.25, b: 0.1, c: 0.25, d: 1.0 }
    }
}

#[allow(dead_code)]
impl MotionBezier {
    pub fn clamped(self) -> Self {
        Self {
            a: self.a.clamp(0.0, 1.0),
            b: self.b.clamp(-1.0, 2.0),
            c: self.c.clamp(0.0, 1.0),
            d: self.d.clamp(-1.0, 2.0),
        }
    }
    pub fn step_a(self, delta: f32) -> Self {
        Self { a: (self.a + delta).clamp(0.0, 1.0), ..self }.clamped()
    }
    pub fn step_b(self, delta: f32) -> Self {
        Self { b: (self.b + delta).clamp(-1.0, 2.0), ..self }.clamped()
    }
    pub fn step_c(self, delta: f32) -> Self {
        Self { c: (self.c + delta).clamp(0.0, 1.0), ..self }.clamped()
    }
    pub fn step_d(self, delta: f32) -> Self {
        Self { d: (self.d + delta).clamp(-1.0, 2.0), ..self }.clamped()
    }
}

/// Borders tune geometry — 4 sliders + 80ms debounce + skip-init + snap (R3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BorderGeometry {
    pub size: i32,
    pub radius: i32,
    pub gap_in: i32,
    pub gap_out: i32,
}

impl Default for BorderGeometry {
    fn default() -> Self {
        Self { size: 2, radius: 32, gap_in: 5, gap_out: 5 }
    }
}

impl BorderGeometry {
    /// The one conversion from the persisted geometry the loaded theme wrote
    /// (`Config.border_size/border_radius/gaps_in/gaps_out`) into the shape the
    /// Borders sliders read. This is the pane's geometry source: the values
    /// here are exactly what Hyprland renders, so the pane cannot drift from
    /// the desktop and a border card click cannot invent new ones.
    ///
    /// The former `preset_geometry_for` lookup table was retired (D1): its keys
    /// (`thin-rounded.ron`, `sharp.ron`, `thick.ron`, `01_cascade.conf`) match
    /// no shipped preset — every one of them is `*.lua` — so it always fell
    /// back to `Default` and lied about the desktop.
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        Self {
            size: cfg.border_size,
            radius: cfg.border_radius,
            gap_in: cfg.gaps_in,
            gap_out: cfg.gaps_out,
        }
    }
}

/// Debouncer for 4 geometry sliders: 80ms last-wins, compare-then-apply,
/// skip first init event (legacy BordersModule pattern).
#[allow(dead_code)]
pub struct GeometryDebouncer {
    last_applied: BorderGeometry,
    pending: Option<BorderGeometry>,
    init_done: bool,
}

#[allow(dead_code)]
impl GeometryDebouncer {
    pub fn new(initial: BorderGeometry) -> Self {
        Self { last_applied: initial, pending: None, init_done: false }
    }
    /// Push a new slider value. Returns true if a debounce should be scheduled.
    /// First call is skipped (Slider init), compare-then-apply skips no-ops.
    pub fn push(&mut self, geo: BorderGeometry) -> bool {
        if !self.init_done {
            self.init_done = true;
            return false;
        }
        if geo == self.last_applied {
            return false;
        }
        self.pending = Some(geo);
        true
    }
    /// Flush after 80ms idle — last-wins. Returns Some if something to apply.
    /// Compare-then-apply prevents redundant engine calls.
    pub fn flush(&mut self) -> Option<BorderGeometry> {
        let p = self.pending.take()?;
        if p == self.last_applied {
            return None;
        }
        self.last_applied = p;
        Some(p)
    }
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn last_applied(&self) -> BorderGeometry {
        self.last_applied
    }
}

/// Pure helper: should we call apply_geometry? (compare-then-apply)
#[allow(dead_code)]
pub fn should_apply_geometry(current: &BorderGeometry, pending: &BorderGeometry) -> bool {
    current != pending
}

/// Build the ONE Lua chunk for the geometry options that actually changed.
///
/// The live path is `hyprctl eval 'hl.config({...})'` (~5 ms, measured) instead
/// of `geometry.sh` + `assemble.sh` (~175 ms). Only changed options may travel:
/// re-sending unchanged ones is a needless write, and a stale value for an
/// option the user did not touch (the old literal-0 gaps/radius) would wipe it.
/// `None` when nothing changed — the caller must push nothing then.
///
/// Paths mirror `assets/scripts/geometry.sh` exactly: `general.border_size`,
/// `general.gaps_in`, `general.gaps_out` and `decoration.rounding`.
pub fn geometry_lua_chunk(prev: &BorderGeometry, next: &BorderGeometry) -> Option<String> {
    let mut entries: Vec<(String, String)> = Vec::new();
    if next.size != prev.size {
        entries.push(("general.border_size".to_string(), next.size.to_string()));
    }
    if next.gap_in != prev.gap_in {
        entries.push(("general.gaps_in".to_string(), next.gap_in.to_string()));
    }
    if next.gap_out != prev.gap_out {
        entries.push(("general.gaps_out".to_string(), next.gap_out.to_string()));
    }
    if next.radius != prev.radius {
        entries.push(("decoration.rounding".to_string(), next.radius.to_string()));
    }
    crate::lua_config_payload(&entries)
}

/// Last-wins slot for the DEFERRED geometry persist.
///
/// The hot apply is immediate; the persistent write (`geometry.sh` +
/// `assemble.sh`) is expensive, so it is coalesced behind a timer instead of
/// running per tick. Every request overwrites the single slot with the latest
/// value; the timer drains it ONCE when the drag goes quiet. N rapid requests
/// therefore cost one persist, never N — and a value can never be stranded:
/// the slot only empties on `take`, and any request after that re-fills it.
#[derive(Debug, Default)]
pub struct GeometryPersistCoalescer {
    pending: Option<BorderGeometry>,
}

impl GeometryPersistCoalescer {
    /// Record the latest geometry the persistent path owes. Overwrites any
    /// previous value (last-wins); the caller (re)arms the drain timer.
    pub fn request(&mut self, geo: BorderGeometry) {
        self.pending = Some(geo);
    }

    /// Consume the owed persist, if any. `None` means nothing is owed — the
    /// timer fired with no fresh work and must do nothing.
    pub fn take(&mut self) -> Option<BorderGeometry> {
        self.pending.take()
    }

    /// Test-only accessor: no production path needs to ask, so the method is
    /// compiled only for the test build. Without the gate the bin target
    /// reports it as dead code on every `cargo run`.
    #[cfg(test)]
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

#[cfg(test)]
mod panel_save_tests {
    use super::handle_panel_save;
    use crate::theme_manager::{ProviderCapabilities, ThemeManager, ThemeProvider};
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    struct DummyProvider;
    impl ThemeProvider for DummyProvider {
        fn id(&self) -> &str { "dummy" }
        fn display_name_key(&self) -> &str { "dummy" }
        fn icon(&self) -> &str { "dummy" }
        fn save(&self, _theme_dir: &Path) -> Result<(), String> { Ok(()) }
        fn apply(&self, _theme_dir: &Path) -> Result<(), String> { Ok(()) }
        fn capabilities(&self) -> ProviderCapabilities { ProviderCapabilities::empty() }
    }

    fn two_managers() -> (ThemeManager, ThemeManager, TempDir, TempDir) {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        let mut tm1 = ThemeManager::new(dir1.path());
        let mut tm2 = ThemeManager::new(dir2.path());
        tm1.register_provider(Box::new(DummyProvider));
        tm2.register_provider(Box::new(DummyProvider));
        (tm1, tm2, dir1, dir2)
    }

    #[test]
    fn test_save_writes_both_managers() {
        let (mut tm1, mut tm2, _d1, _d2) = two_managers();
        let sync = AtomicUsize::new(0);
        let refresh = AtomicUsize::new(0);
        let mut err = String::new();
        let ok = handle_panel_save(
            "MyMix",
            &mut tm1,
            &mut tm2,
            || { sync.fetch_add(1, Ordering::SeqCst); },
            || { refresh.fetch_add(1, Ordering::SeqCst); },
            &mut err,
        );
        assert!(ok, "save should succeed");
        assert!(err.is_empty(), "no error on success, got {err}");
        assert_eq!(sync.load(Ordering::SeqCst), 1, "sync_cards must be called once");
        assert_eq!(refresh.load(Ordering::SeqCst), 1, "refresh_mosaic_page must be called once");
        let list1 = tm1.list().unwrap();
        assert!(list1.iter().any(|t| t.name == "MyMix"), "tm_main must contain MyMix");
        let list2 = tm2.list().unwrap();
        assert!(list2.iter().any(|t| t.name == "MyMix"), "gallery_tm must contain MyMix — dual-sync");
    }

    #[test]
    fn test_save_empty_shows_error() {
        let (mut tm1, mut tm2, _d1, _d2) = two_managers();
        let sync = AtomicUsize::new(0);
        let refresh = AtomicUsize::new(0);
        let mut err = String::new();
        let ok = handle_panel_save(
            "   ",
            &mut tm1,
            &mut tm2,
            || { sync.fetch_add(1, Ordering::SeqCst); },
            || { refresh.fetch_add(1, Ordering::SeqCst); },
            &mut err,
        );
        assert!(!ok, "empty name must not save");
        assert_eq!(err, "name-empty");
        assert_eq!(sync.load(Ordering::SeqCst), 0, "no sync on empty");
        assert_eq!(refresh.load(Ordering::SeqCst), 0, "no refresh on empty");
        assert!(tm1.list().unwrap().is_empty(), "no theme in tm_main");
        assert!(tm2.list().unwrap().is_empty(), "no theme in gallery_tm");
    }

    #[test]
    fn test_save_overwrite_confirms() {        let (mut tm1, mut tm2, _d1, _d2) = two_managers();
        // pre-seed MyMix as last_applied
        tm1.save("MyMix", &["dummy".to_string()]).unwrap();
        tm1.last_applied = "MyMix".to_string();
        tm2.save("MyMix", &["dummy".to_string()]).unwrap();
        tm2.last_applied = "MyMix".to_string();
        let sync = AtomicUsize::new(0);
        let refresh = AtomicUsize::new(0);
        let mut err = String::new();
        // overwrite same name — should succeed and still sync+refresh
        let ok = handle_panel_save(
            "MyMix",
            &mut tm1,
            &mut tm2,
            || { sync.fetch_add(1, Ordering::SeqCst); },
            || { refresh.fetch_add(1, Ordering::SeqCst); },
            &mut err,
        );
        assert!(ok, "overwrite should succeed");
        assert!(err.is_empty());
        assert_eq!(sync.load(Ordering::SeqCst), 1, "sync_cards on overwrite");
        assert_eq!(refresh.load(Ordering::SeqCst), 1, "refresh on overwrite");
        assert!(tm1.list().unwrap().iter().any(|t| t.name == "MyMix"));
        assert!(tm2.list().unwrap().iter().any(|t| t.name == "MyMix"));
    }
}

/// Borders tune stop count (keyboard R11 recompute).
/// Order: size, radius, gap-in, gap-out, angle, inactive,
/// [strip, add, remove ONLY while slots exist], glow-enabled,
/// (+range, power, color, inactive when on), save-name, save-button.
/// The three animation leaves and the floating window rule were REMOVED from
/// the tune pane in B3; they survive in the model (`tune-animations` /
/// `tune-rule-enabled`) but have no keyboard seat any more. The three geometry
/// stops (radius, gap-in, gap-out) were recovered in B2. `slot_count` no longer
/// adds one stop per slot — the strip is ONE stop — but ZERO colours still
/// removes the three slot-management stops, because the strip / add / remove
/// controls mount only while colours do. Mirrors `BordersSection.tune-count`
/// (and the pane's `slot-stops`).
#[allow(dead_code)]
pub fn borders_tune_stop_count(slot_count: i32, glow_enabled: bool) -> i32 {
    // 9 fixed stops (size, radius, gap-in, gap-out, angle, inactive,
    // glow-enable, save-name, save-button) + the colour-slot block (3 while
    // colours exist).
    let mut total = 9 + if slot_count > 0 { 3 } else { 0 };
    if glow_enabled {
        total += 4;
    }
    total
}

/// Borders full-sequence step (cards-first: 0..list-1 cards, then tune).
/// Wraps circularly; empty total stays 0.
#[allow(dead_code)]
pub fn borders_nav_step(current: i32, delta: i32, total: usize) -> i32 {
    if total == 0 {
        return 0;
    }
    let t = total as i32;
    if delta > 0 {
        if current >= t - 1 { 0 } else { current + 1 }
    } else if delta < 0 {
        if current <= 0 { t - 1 } else { current - 1 }
    } else {
        current.clamp(0, t - 1)
    }
}

#[cfg(test)]
mod keyboard_nav_tests {
    use super::{borders_nav_step, borders_tune_stop_count, save_nav_step};

    #[test]
    fn test_save_nav_step_clamps_and_starts() {
        assert_eq!(save_nav_step(-1, 1, 3), 0, "unfocused starts at first");
        assert_eq!(save_nav_step(0, 1, 3), 1, "down steps forward");
        assert_eq!(save_nav_step(2, 1, 3), 0, "forward wraps last -> 0 (1:1 legacy)");
        assert_eq!(save_nav_step(0, -1, 3), 2, "backward wraps 0 -> last (1:1 legacy)");
        assert_eq!(save_nav_step(2, -1, 3), 1, "up steps backward");
        assert_eq!(save_nav_step(1, 1, 0), -1, "empty list has no focus");
    }

    #[test]
    fn test_borders_tune_count_covers_full_inventory() {
        assert_eq!(borders_tune_stop_count(2, false), 12);
        assert_eq!(borders_tune_stop_count(8, true), 16);
        assert_eq!(borders_tune_stop_count(3, true), 16);
        // Zero colours: the strip / add / remove controls are not mounted, so
        // their three stops are not counted either.
        assert_eq!(borders_tune_stop_count(0, false), 9);
        // Above zero the count is flat: the strip is ONE stop, not N.
        assert_eq!(borders_tune_stop_count(99, false), 12);
    }

    #[test]
    fn test_borders_nav_wraps_over_full_sequence() {
        assert_eq!(borders_nav_step(0, 1, 16), 1);
        assert_eq!(borders_nav_step(15, 1, 16), 0, "last tune wraps to first card");
        assert_eq!(borders_nav_step(0, -1, 16), 15, "first card wraps to last tune");
        assert_eq!(borders_nav_step(5, -1, 16), 4);
        assert_eq!(borders_nav_step(0, 0, 0), 0, "empty stays 0");
    }
}

#[cfg(test)]
mod save_list_tests {
    use super::save_list_rows;
    use crate::theme_manager::{ProviderCapabilities, ThemeManager, ThemeProvider};
    use std::path::Path;
    use tempfile::TempDir;

    struct DummyProvider;
    impl ThemeProvider for DummyProvider {
        fn id(&self) -> &str { "dummy" }
        fn display_name_key(&self) -> &str { "dummy" }
        fn icon(&self) -> &str { "dummy" }
        fn save(&self, _theme_dir: &Path) -> Result<(), String> { Ok(()) }
        fn apply(&self, _theme_dir: &Path) -> Result<(), String> { Ok(()) }
        fn capabilities(&self) -> ProviderCapabilities { ProviderCapabilities::empty() }
    }

    #[test]
    fn test_save_list_shows_names_dates_active() {
        let dir = TempDir::new().unwrap();
        let mut tm = ThemeManager::new(dir.path());
        tm.register_provider(Box::new(DummyProvider));
        tm.save("Alpha", &["dummy".to_string()]).unwrap();
        tm.save("Beta", &["dummy".to_string()]).unwrap();
        tm.last_applied = "Beta".to_string();
        let rows = save_list_rows(&tm);
        assert_eq!(rows.len(), 2, "list must show both saved themes");
        assert!(rows.iter().any(|(n, d, a)| *n == "Alpha" && !d.is_empty() && !a),
            "Alpha shows name + date, not active");
        assert!(rows.iter().any(|(n, d, a)| *n == "Beta" && !d.is_empty() && *a),
            "Beta shows name + date + active mark");
        let active_idx = rows.iter().position(|(_, _, a)| *a).unwrap();
        assert_eq!(rows[active_idx].0, "Beta");
    }
}

#[cfg(test)]
mod backfill_tests {
    use super::backfill_active_candidate;
    use std::string::String;

    fn row(name: &str, wp: Option<&str>) -> (String, Option<String>) {
        (name.to_string(), wp.map(|w| w.to_string()))
    }

    #[test]
    fn test_backfill_single_match_adopts_it() {
        let themes = vec![
            row("Alpha", Some("/w/a.png")),
            row("Beta", Some("/w/b.png")),
        ];
        let got = backfill_active_candidate(&themes, "/w/b.png");
        assert_eq!(got.as_deref(), Some("Beta"), "single wallpaper match wins");
    }

    #[test]
    fn test_backfill_duplicate_group_takes_first() {
        // Test variants of the same look share the wallpaper: the first in
        // list order IS that state — the applied look for every purpose.
        let themes = vec![
            row("Alpha", Some("/w/a.png")),
            row("Bor1", Some("/w/joker.png")),
            row("Bor2", Some("/w/joker.png")),
            row("Bor3", Some("/w/joker.png")),
        ];
        let got = backfill_active_candidate(&themes, "/w/joker.png");
        assert_eq!(got.as_deref(), Some("Bor1"), "first of an identical group wins");
    }

    #[test]
    fn test_backfill_no_match_returns_none() {
        let themes = vec![row("Alpha", Some("/w/a.png")), row("Beta", None)];
        assert_eq!(backfill_active_candidate(&themes, "/w/other.png"), None);
        assert_eq!(backfill_active_candidate(&themes, "/w/a.png").as_deref(), Some("Alpha"));
        assert_eq!(
            backfill_active_candidate(&themes, "").is_none(),
            true,
            "empty live wallpaper never guesses"
        );
    }
}

#[cfg(test)]
mod save_tranche_b_tests {
    use super::{filter_saved_rows, focus_after_delete, handle_panel_delete, handle_panel_refresh, handle_panel_rename};
    use crate::shell::gallery::GallerySlot;
    use crate::theme_manager::{ProviderCapabilities, ThemeManager, ThemeProvider};
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    struct DummyProvider;
    impl ThemeProvider for DummyProvider {
        fn id(&self) -> &str { "dummy" }
        fn display_name_key(&self) -> &str { "dummy" }
        fn icon(&self) -> &str { "dummy" }
        fn save(&self, _theme_dir: &Path) -> Result<(), String> { Ok(()) }
        fn apply(&self, _theme_dir: &Path) -> Result<(), String> { Ok(()) }
        fn capabilities(&self) -> ProviderCapabilities { ProviderCapabilities::empty() }
    }

    fn main_plus_slot(dir: &TempDir) -> (ThemeManager, GallerySlot, Arc<Mutex<ThemeManager>>) {
        let mut tm = ThemeManager::new(dir.path());
        tm.register_provider(Box::new(DummyProvider));
        let gtm = Arc::new(Mutex::new(ThemeManager::new(dir.path())));
        gtm.lock().unwrap().register_provider(Box::new(DummyProvider));
        let slot = GallerySlot::new(gtm.clone());
        (tm, slot, gtm)
    }

    #[test]
    fn test_rename_dual_sync_moves_last_applied() {
        let dir = TempDir::new().unwrap();
        let (mut tm, slot, gtm) = main_plus_slot(&dir);
        // Shared themes dir (production layout): one save is visible to both
        tm.save("Old", &["dummy".to_string()]).unwrap();
        tm.last_applied = "Old".to_string();
        gtm.lock().unwrap().last_applied = "Old".to_string();
        let mut err = String::new();
        assert!(handle_panel_rename("Old", "New", &mut tm, &slot, &mut err), "rename must succeed");
        assert!(err.is_empty());
        assert_eq!(slot.rename_calls(), 1, "gallery side goes through the slot");
        for names in [tm.list().unwrap(), gtm.lock().unwrap().list().unwrap()] {
            assert!(names.iter().any(|t| t.name == "New"), "renamed on shared storage");
        }
        assert_eq!(tm.last_applied, "New", "main active follows rename");
        assert_eq!(gtm.lock().unwrap().last_applied, "New", "gallery active follows rename");
    }

    #[test]
    fn test_rename_collision_reports_error() {
        let dir = TempDir::new().unwrap();
        let (mut tm, slot, _gtm) = main_plus_slot(&dir);
        tm.save("A", &["dummy".to_string()]).unwrap();
        tm.save("B", &["dummy".to_string()]).unwrap();
        let mut err = String::new();
        assert!(!handle_panel_rename("A", "B", &mut tm, &slot, &mut err), "collision must fail");
        assert!(!err.is_empty(), "error message surfaces to UI");
    }

    #[test]
    fn test_delete_dual_sync_clears_active() {
        let dir = TempDir::new().unwrap();
        let (mut tm, slot, gtm) = main_plus_slot(&dir);
        tm.save("Gone", &["dummy".to_string()]).unwrap();
        tm.last_applied = "Gone".to_string();
        gtm.lock().unwrap().last_applied = "Gone".to_string();
        let mut err = String::new();
        assert!(handle_panel_delete("Gone", &mut tm, &slot, &mut err), "delete must succeed");
        assert!(err.is_empty());
        assert_eq!(slot.delete_calls(), 1, "gallery side goes through the slot");
        assert!(tm.list().unwrap().is_empty(), "deleted from shared storage");
        assert!(tm.last_applied.is_empty(), "main active cleared");
        assert!(gtm.lock().unwrap().last_applied.is_empty(), "gallery active cleared");
    }

    #[test]
    fn test_refresh_rewrites_active_in_both() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        let mut tm1 = ThemeManager::new(dir1.path());
        let mut tm2 = ThemeManager::new(dir2.path());
        tm1.register_provider(Box::new(DummyProvider));
        tm2.register_provider(Box::new(DummyProvider));
        for tm in [&mut tm1, &mut tm2] {
            tm.save("Live", &["dummy".to_string()]).unwrap();
            tm.last_applied = "Live".to_string();
        }
        let mut err = String::new();
        assert!(handle_panel_refresh(&mut tm1, &mut tm2, &mut err), "refresh must succeed");
        assert!(err.is_empty());
        for tm in [&tm1, &tm2] {
            assert!(tm.list().unwrap().iter().any(|t| t.name == "Live"), "active rewritten in both");
        }
    }

    #[test]
    fn test_search_filters_case_insensitive() {
        let rows = vec![
            ("Alpha".to_string(), "d1".to_string(), false),
            ("Beta".to_string(), "d2".to_string(), true),
            ("Alpine".to_string(), "d3".to_string(), false),
        ];
        let out = filter_saved_rows("alp", &rows);
        assert_eq!(out.len(), 2, "live filter matches substring, case-insensitive");
        let out_all = filter_saved_rows("", &rows);
        assert_eq!(out_all.len(), 3, "empty query restores full list");
    }

    #[test]
    fn test_focus_moves_to_neighbor_after_delete() {
        assert_eq!(focus_after_delete(1, 2), 1, "next takes the deleted slot");
        assert_eq!(focus_after_delete(2, 2), 1, "last deleted focuses previous");
        assert_eq!(focus_after_delete(0, 0), -1, "empty list has no focus");
    }
}

#[cfg(test)]
mod borders_pick_tests {
    use super::{handle_borders_apply, handle_borders_scan};
    use crate::engine::Engine;
    use std::fs;
    use tempfile::TempDir;

    fn temp_engine_with_borders() -> (Engine, TempDir) {
        let dir = TempDir::new().unwrap();
        let scripts = dir.path().join("assets").join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        // scan.sh: if $1 == borders echo 2 presets else []
        fs::write(
            scripts.join("scan.sh"),
            r#"#!/bin/bash
if [ "$1" = "borders" ]; then
  echo '[{"file":"thin-rounded.ron","rawTitle":"Thin Rounded","tag":"SYSTEM"},{"file":"sharp.ron","rawTitle":"Sharp","tag":"SYSTEM"}]'
else
  echo '[]'
fi
"#,
        )
        .unwrap();
        fs::write(
            scripts.join("border.sh"),
            r#"#!/bin/bash
echo "applied $1"
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["scan.sh", "border.sh"] {
                let p = scripts.join(name);
                let mut perm = fs::metadata(&p).unwrap().permissions();
                perm.set_mode(0o755);
                fs::set_permissions(&p, perm).unwrap();
            }
        }
        let engine = Engine::new(dir.path());
        (engine, dir)
    }

    #[test]
    fn test_borders_scan_returns_cards() {
        let (engine, _dir) = temp_engine_with_borders();
        let cards = handle_borders_scan(&engine);
        assert_eq!(cards.len(), 2, "scan(borders) must return 2 cards");
        assert_eq!(cards[0].file, "thin-rounded.ron");
        assert_eq!(cards[1].file, "sharp.ron");
    }

    #[test]
    fn test_apply_border_calls_engine() {
        let (engine, _dir) = temp_engine_with_borders();
        let res = handle_borders_apply(&engine, "thin-rounded.ron");
        assert!(res.is_ok(), "apply_border should succeed, got {:?}", res.err());
        assert!(res.unwrap().contains("thin-rounded.ron"));
    }
}

#[cfg(test)]
mod geometry_tune_tests {
    use super::{
        geometry_lua_chunk, BorderGeometry, GeometryDebouncer,
        GeometryPersistCoalescer, should_apply_geometry,
    };

    // ── Geometry source of truth: persisted state, never a preset lookup ──
    // The Borders pane must show the geometry the loaded theme actually wrote
    // into `Config` (the same values Hyprland renders). `BorderGeometry` is the
    // one shape both the sliders and `Config` share, so this conversion is the
    // single place the pane reads geometry from.
    #[test]
    fn border_geometry_reads_persisted_config() {
        let mut cfg = crate::config::Config::default();
        cfg.border_size = 5;
        cfg.border_radius = 20;
        cfg.gaps_in = 10;
        cfg.gaps_out = 10;
        assert_eq!(
            BorderGeometry::from_config(&cfg),
            BorderGeometry { size: 5, radius: 20, gap_in: 10, gap_out: 10 }
        );
    }

    // ── B1: hot apply pushes ONLY the options that actually changed ──────
    // The live path is one `hl.config({...})` chunk through `hyprctl eval`;
    // re-sending unchanged options would be a needless write, and sending the
    // wrong ones (the old literal-0 gaps/radius) is the bug that wiped the
    // user's rounding. The exact string is the contract, so it is asserted.

    #[test]
    fn hot_chunk_size_only() {
        let prev = BorderGeometry { size: 2, radius: 32, gap_in: 5, gap_out: 5 };
        let next = BorderGeometry { size: 3, radius: 32, gap_in: 5, gap_out: 5 };
        assert_eq!(
            geometry_lua_chunk(&prev, &next).as_deref(),
            Some("hl.config({ general = { border_size = 3 } })")
        );
    }

    #[test]
    fn hot_chunk_radius_only() {
        let prev = BorderGeometry { size: 2, radius: 32, gap_in: 5, gap_out: 5 };
        let next = BorderGeometry { size: 2, radius: 40, gap_in: 5, gap_out: 5 };
        assert_eq!(
            geometry_lua_chunk(&prev, &next).as_deref(),
            Some("hl.config({ decoration = { rounding = 40 } })")
        );
    }

    #[test]
    fn hot_chunk_gaps_only() {
        let prev = BorderGeometry { size: 2, radius: 32, gap_in: 5, gap_out: 5 };
        let next = BorderGeometry { size: 2, radius: 32, gap_in: 8, gap_out: 10 };
        assert_eq!(
            geometry_lua_chunk(&prev, &next).as_deref(),
            Some("hl.config({ general = { gaps_in = 8, gaps_out = 10 } })")
        );
    }

    #[test]
    fn hot_chunk_nothing_changed_is_no_call() {
        let same = BorderGeometry { size: 2, radius: 32, gap_in: 5, gap_out: 5 };
        assert_eq!(geometry_lua_chunk(&same, &same), None, "no change must not push");
    }

    #[test]
    fn hot_chunk_mixed_fields_one_merged_call() {
        let prev = BorderGeometry { size: 2, radius: 32, gap_in: 5, gap_out: 5 };
        let next = BorderGeometry { size: 4, radius: 40, gap_in: 5, gap_out: 5 };
        assert_eq!(
            geometry_lua_chunk(&prev, &next).as_deref(),
            Some("hl.config({ decoration = { rounding = 40 }, general = { border_size = 4 } })")
        );
    }

    // ── B1: the persistent apply is coalesced ────────────────────────────
    // A drag fires one tick per 80 ms; each tick would otherwise run
    // geometry.sh + assemble.sh (~175 ms measured live). The coalescer keeps
    // the LAST requested value in one slot, so N rapid requests drain as ONE
    // persist.

    #[test]
    fn persist_burst_drains_once() {
        let mut c = GeometryPersistCoalescer::default();
        for size in [3, 4, 5, 5, 4] {
            c.request(BorderGeometry { size, radius: 32, gap_in: 5, gap_out: 5 });
        }
        assert!(c.has_pending(), "a burst owes exactly one persist");
        let drained = c.take().expect("one persist for the whole burst");
        assert_eq!(drained.size, 4, "last-wins: the drag's final value persists");
        assert_eq!(c.take(), None, "the burst must not drain a second time");
        assert!(!c.has_pending());
    }

    #[test]
    fn persist_second_burst_drains_again() {
        let mut c = GeometryPersistCoalescer::default();
        c.request(BorderGeometry { size: 3, radius: 32, gap_in: 5, gap_out: 5 });
        assert_eq!(c.take().map(|g| g.size), Some(3));
        // Draining clears the slot; a later drag must still be owed a persist.
        c.request(BorderGeometry { size: 5, radius: 32, gap_in: 5, gap_out: 5 });
        assert_eq!(c.take().map(|g| g.size), Some(5), "a second burst is not swallowed");
    }

    #[test]
    fn test_geometry_debounce_last_wins() {
        let initial = BorderGeometry { size: 2, radius: 32, gap_in: 5, gap_out: 5 };
        let mut d = GeometryDebouncer::new(initial);
        // first event is init and must be skipped — prime the debouncer
        d.push(initial);
        assert!(!d.has_pending(), "init event must not schedule");
        // rapid 60Hz drag: 5 quick pushes, last-wins after 80ms idle
        let seq = [
            BorderGeometry { size: 2, radius: 10, gap_in: 5, gap_out: 5 },
            BorderGeometry { size: 2, radius: 20, gap_in: 5, gap_out: 5 },
            BorderGeometry { size: 2, radius: 30, gap_in: 5, gap_out: 5 },
            BorderGeometry { size: 2, radius: 40, gap_in: 5, gap_out: 5 },
            BorderGeometry { size: 2, radius: 80, gap_in: 5, gap_out: 5 },
        ];
        for g in seq {
            assert!(d.push(g), "each distinct change should schedule debounce");
        }
        // only one flush after 80ms idle, last-wins
        let flushed = d.flush().expect("must flush pending after debounce");
        assert_eq!(flushed.radius, 80, "debounce last-wins: only last radius survives");
        assert!(!d.has_pending(), "no pending after flush");
        assert_eq!(d.flush(), None, "second flush without new push is None (no stall)");
        assert!(should_apply_geometry(&initial, &flushed), "flushed differs from initial so should apply");
        // same value again → compare-then-apply skips
        assert!(!d.push(flushed), "pushing same as last_applied must not schedule");
    }

    #[test]
    fn test_skip_init_event() {
        let initial = BorderGeometry::default();
        let mut d = GeometryDebouncer::new(initial);
        // very first push is the Slider init event — must be skipped
        assert!(!d.push(BorderGeometry { size: 3, radius: 10, gap_in: 5, gap_out: 5 }), "first event ignored");
        assert!(!d.has_pending(), "init skip must leave no pending");
        assert_eq!(d.flush(), None, "flush after only init is None");
        // second push is real user interaction — must schedule
        assert!(d.push(BorderGeometry { size: 3, radius: 10, gap_in: 5, gap_out: 5 }));
        assert!(d.has_pending());
    }
}

#[cfg(test)]
mod motion_pick_tests {
    use super::{handle_motion_apply, handle_motion_scan};
    use crate::engine::Engine;
    use std::fs;
    use tempfile::TempDir;

    fn temp_engine_with_motions() -> (Engine, TempDir) {
        let dir = TempDir::new().unwrap();
        let scripts = dir.path().join("assets").join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        // scan.sh: if $1 == animations echo 19 presets else []
        let mut json = String::from("[");
        for i in 1..=19 {
            if i > 1 {
                json.push(',');
            }
            json.push_str(&format!(
                r#"{{"file":"anim{:02}.conf","rawTitle":"Anim {:02}","tag":"SYSTEM"}}"#,
                i, i
            ));
        }
        json.push(']');
        let scan_content = format!(
            r#"#!/bin/bash
if [ "$1" = "animations" ]; then
  echo '{}'
else
  echo '[]'
fi
"#,
            json
        );
        fs::write(scripts.join("scan.sh"), scan_content).unwrap();
        fs::write(
            scripts.join("apply_animation.sh"),
            r#"#!/bin/bash
echo "applied $1"
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["scan.sh", "apply_animation.sh"] {
                let p = scripts.join(name);
                let mut perm = fs::metadata(&p).unwrap().permissions();
                perm.set_mode(0o755);
                fs::set_permissions(&p, perm).unwrap();
            }
        }
        let engine = Engine::new(dir.path());
        (engine, dir)
    }

    #[test]
    fn test_motion_19_cards() {
        let (engine, _dir) = temp_engine_with_motions();
        let cards = handle_motion_scan(&engine);
        assert_eq!(cards.len(), 19, "scan(animations) must return 19 cards");
    }

    #[test]
    fn test_apply_animation_off_is_none() {
        let (engine, _dir) = temp_engine_with_motions();
        // active off → none: empty file maps to "none"
        let res = handle_motion_apply(&engine, "");
        assert!(res.is_ok(), "apply_animation off (none) should succeed, got {:?}", res.err());
        assert!(res.unwrap().contains("none"), "empty file must map to none");
        // also explicit "none"
        let res2 = handle_motion_apply(&engine, "none");
        assert!(res2.is_ok());
        assert!(res2.unwrap().contains("none"));
        // normal file
        let res3 = handle_motion_apply(&engine, "anim01.conf");
        assert!(res3.is_ok());
        assert!(res3.unwrap().contains("anim01.conf"));
    }
}

#[cfg(test)]
mod filters_tests {
    use super::{handle_filters_apply, handle_filters_scan};
    use crate::engine::Engine;
    use std::fs;
    use tempfile::TempDir;

    fn temp_engine_with_shaders() -> (Engine, TempDir) {
        let dir = TempDir::new().unwrap();
        let scripts = dir.path().join("assets").join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(
            scripts.join("scan.sh"),
            r#"#!/bin/bash
if [ "$1" = "shaders" ]; then
  echo '[{"file":"blue-light.ron","rawTitle":"Blue Light","tag":"SYSTEM"},{"file":"cyberpunk.ron","rawTitle":"Cyberpunk","tag":"SYSTEM"}]'
else
  echo '[]'
fi
"#,
        )
        .unwrap();
        fs::write(
            scripts.join("shader.sh"),
            r#"#!/bin/bash
echo "applied $1"
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["scan.sh", "shader.sh"] {
                let p = scripts.join(name);
                let mut perm = fs::metadata(&p).unwrap().permissions();
                perm.set_mode(0o755);
                fs::set_permissions(&p, perm).unwrap();
            }
        }
        let engine = Engine::new(dir.path());
        (engine, dir)
    }

    #[test]
    fn test_filters_shader_cards() {
        let (engine, _dir) = temp_engine_with_shaders();
        let cards = handle_filters_scan(&engine);
        assert_eq!(cards.len(), 2, "scan(shaders) must return 2 cards");
        assert_eq!(cards[0].file, "blue-light.ron");
        assert_eq!(cards[1].file, "cyberpunk.ron");
    }

    #[test]
    fn test_repick_active_applies_none() {
        let (engine, _dir) = temp_engine_with_shaders();
        // active off → none: empty file maps to "none"
        let res = handle_filters_apply(&engine, "");
        assert!(res.is_ok(), "apply_shader off (none) should succeed, got {:?}", res.err());
        assert!(res.unwrap().contains("none"), "empty file must map to none");
        // also explicit "none"
        let res2 = handle_filters_apply(&engine, "none");
        assert!(res2.is_ok());
        assert!(res2.unwrap().contains("none"));
        // normal file
        let res3 = handle_filters_apply(&engine, "blue-light.ron");
        assert!(res3.is_ok());
        assert!(res3.unwrap().contains("blue-light.ron"));
    }

    #[test]
    fn test_shader_overlay_suppressed_3s() {
        use crate::shell::gallery::slot::{GallerySlot, SHADER_FLICKER_MS};
        use crate::theme_manager::{ThemeManager, ThemeProvider, ProviderCapabilities};
        use std::path::Path;
        use std::sync::{Arc, Mutex};
        use std::time::Duration;
        struct DummyProvider;
        impl ThemeProvider for DummyProvider {
            fn id(&self) -> &str { "dummy" }
            fn display_name_key(&self) -> &str { "dummy" }
            fn icon(&self) -> &str { "dummy" }
            fn save(&self, _theme_dir: &Path) -> Result<(), String> { Ok(()) }
            fn apply(&self, _theme_dir: &Path) -> Result<(), String> { Ok(()) }
            fn capabilities(&self) -> ProviderCapabilities { ProviderCapabilities::empty() }
        }
        let dir = TempDir::new().unwrap();
        let mut tm = ThemeManager::new(dir.path());
        tm.register_provider(Box::new(DummyProvider));
        let tm_arc = Arc::new(Mutex::new(tm));
        let slot = GallerySlot::new(tm_arc);
        // Not suppressed when panel Closed and no recent mutation
        slot.dismiss_shader_overlay();
        assert!(!slot.is_shader_overlay_visible());
        // Simulate PanelState != Closed → suppressed
        slot.set_panel_closed(false);
        slot.trigger_shader_overlay();
        assert!(!slot.is_shader_overlay_visible(), "overlay must be SUPPRESSED while PanelState != Closed");
        // Simulate PanelState == Closed but within 3s after Mutating
        slot.set_panel_closed(true);
        slot.notify_panel_mutation_finished();
        slot.trigger_shader_overlay();
        assert!(!slot.is_shader_overlay_visible(), "overlay must be suppressed for 3s after Mutating");
        // After 3s + 100ms, should trigger normally
        std::thread::sleep(Duration::from_millis(SHADER_FLICKER_MS + 100));
        slot.trigger_shader_overlay();
        assert!(slot.is_shader_overlay_visible(), "overlay should appear after 3s window");
        // Dismiss and verify not 300ms (the old wrong duration) — ensure 300ms is not enough
        slot.dismiss_shader_overlay();
        slot.notify_panel_mutation_finished();
        std::thread::sleep(Duration::from_millis(400));
        slot.trigger_shader_overlay();
        assert!(!slot.is_shader_overlay_visible(), "300ms must NOT be enough — requires 3000ms");
    }
}

/// Mouse verbose trace formatter.
///
/// Every mouse interaction inside the app must leave a `[panel][mouse]`
/// trace in --verbose mode, even when the click never reaches Rust today
/// (Slint-only empty rows, hover-only zones, card bodies, mutating guard).
/// Slint-only clicks travel through the generic `panel-mouse-trace`
/// cable and land here for one uniform debug line.
pub fn mouse_trace(reason: &str) -> String {
    format!("[panel][mouse] {reason}")
}

/// Focus verbose trace formatter.
///
/// Every keyboard-focus move between the panel scopes (menu-fs rail, kb
/// System content, shell-kbd shell) leaves a `[focus]` trace in --verbose
/// mode, with scope + direction + FocusReason. Slint gain/loss handlers
/// travel through the generic `panel-focus-trace` cable and land here for
/// one uniform debug line.
pub fn focus_trace(reason: &str) -> String {
    format!("[focus] {reason}")
}

/// Mouse verbose trace (TDD RED first).
///
/// Every mouse interaction inside the app must leave a `[panel][mouse]`
/// trace in --verbose mode, even when the click never reaches Rust today
/// (Slint-only empty rows, hover-only zones, card bodies, mutating guard).
/// Pure formatter so unit tests prove the trace shape without a window.
#[cfg(test)]
mod mouse_verbose_tests {
    use super::{focus_trace, mouse_trace};

    #[test]
    fn test_every_mouse_click_leaves_verbose_trace() {
        // Save handlers (today mute in main.rs) + close + back.
        for reason in [
            "save-theme name=MyMix (save button/enter)",
            "apply-saved-theme idx=0",
            "rename-saved-theme old=A new=B",
            "delete-saved-theme name=Gone",
            "refresh-saved-theme",
            "overwrite-saved-theme name=MyMix",
            "save-search-changed q=alp",
            "close-button-clicked (chrome X)",
            "back-activated (chrome back / Esc-hide)",
            // Slint-only clicks forwarded via the generic trace cable.
            "save-empty-clicked",
            "save-dialog-cancelled",
            "save-card-body idx=1",
            "save-open-rename name=Alpha",
            "save-open-delete name=Alpha",
            "save-open-refresh name=Alpha",
            "mutating-guard-click ignored — mutating",
            // Gallery + shell clicks (today mute or silently gated).
            "gallery card-clicked idx=2",
            "gallery card-right-clicked idx=2",
            "gallery style-selected style=1",
            "card-activated idx=0 (home card click)",
        ] {
            let msg = mouse_trace(reason);
            assert_eq!(msg, format!("[panel][mouse] {reason}"), "trace shape for {reason}");
        }
    }

    #[test]
    fn test_every_focus_move_leaves_verbose_trace() {
        // Gain + loss of each traced scope with the reason tag.
        for reason in [
            "menu-fs gained via programmatic",
            "menu-fs lost via pointer-click",
            "kb gained via programmatic",
            "kb lost via tab-navigation",
            "shell-kbd gained via window-activation",
            "shell-kbd lost via popup-activation",
        ] {
            let msg = focus_trace(reason);
            assert_eq!(msg, format!("[focus] {reason}"), "trace shape for {reason}");
        }
    }
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