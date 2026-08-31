//! Central view-model for the application.
//!
//! `AppState` aggregates the three state pieces that used to travel
//! separately through `main()` and every UI callback — the user `Config`,
//! the `Engine` and the `ThemeManager` — behind a single lock, so the UI
//! layer talks to one object instead of juggling three handles.

use crate::config::Config;
use crate::engine::Engine;
use crate::theme_manager::ThemeManager;
use std::sync::{Arc, Mutex};

/// Aggregated application state (Config + Engine + ThemeManager).
pub struct AppState {
    cfg: Config,
    engine: Engine,
    theme_manager: ThemeManager,
}

/// Shared handle used across the UI layer.
///
/// INVARIANT: `SharedState` is only ever acquired from the UI/event-loop
/// thread (`main()` and the callbacks in `callbacks.rs`). No other thread
/// (tray/ksni, IPC server, watcher, countdown) may lock it — those threads
/// build their own file-based `Engine`/`Config` and must keep doing so.
/// Violating this introduces UI stalls, since theme operations can run
/// blocking engine subprocesses while the lock is held.
pub type SharedState = Arc<Mutex<AppState>>;

impl AppState {
    pub fn new(cfg: Config, engine: Engine, theme_manager: ThemeManager) -> Self {
        Self {
            cfg,
            engine,
            theme_manager,
        }
    }

    // ── Fine-grained accessors ──────────────────────────────────────

    pub fn cfg(&self) -> &Config {
        &self.cfg
    }

    pub fn cfg_mut(&mut self) -> &mut Config {
        &mut self.cfg
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn theme_manager(&self) -> &ThemeManager {
        &self.theme_manager
    }

    pub fn theme_manager_mut(&mut self) -> &mut ThemeManager {
        &mut self.theme_manager
    }

    /// Apply a mutation to the config and persist it to disk.
    pub fn update_cfg(&mut self, f: impl FnOnce(&mut Config)) {
        f(&mut self.cfg);
        let _ = self.cfg.save();
    }

    // ── Composite operations (previously spread across callbacks) ──

    /// Persist the system active flag and run the engine init script.
    pub fn toggle_system(&mut self, active: bool) -> Result<String, crate::engine::EngineError> {
        self.cfg.is_system_active = active;
        let _ = self.cfg.save();
        if active {
            self.engine.init_enable()
        } else {
            self.engine.init_disable()
        }
    }

    /// Persist the current geometry fields and apply them to Hyprland.
    pub fn apply_geometry(&mut self) -> Result<String, crate::engine::EngineError> {
        let _ = self.cfg.save();
        self.engine.apply_geometry(
            self.cfg.border_size,
            self.cfg.border_radius,
            self.cfg.gaps_in,
            self.cfg.gaps_out,
        )
    }

    /// Reload the config from disk after a theme apply and mark the theme
    /// as last applied. Returns the freshly loaded config so callers can
    /// sync the UI preset indices against it.
    #[allow(dead_code)]
    pub fn reload_config_after_theme(&mut self, name: &str) -> Config {
        let updated = Config::load();
        self.cfg = updated.clone();
        self.cfg.last_applied_theme = name.to_string();
        let _ = self.cfg.save();
        updated
    }

    /// Convenience wrapper: refresh every window visual that depends on the
    /// engine palette and the given theme preference.
    pub fn refresh_visual_state(
        &self,
        window: &crate::MainWindow,
        theme_pref: &str,
    ) -> Option<(Vec<u8>, String)> {
        crate::refresh_visual_state(window, &self.engine, theme_pref)
    }

    /// Convenience wrapper: refresh the Slint theme list from the theme manager.
    pub fn refresh_theme_list(&self, window: &crate::MainWindow) {
        crate::refresh_theme_list(window, &self.theme_manager);
    }
}