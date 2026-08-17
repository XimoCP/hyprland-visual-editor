//! Shared test utilities.
//!
//! Tests that mutate process-global environment variables (`HOME`,
//! `XDG_CACHE_HOME`, `XDG_CONFIG_HOME`) must do so under the SAME mutex,
//! otherwise parallel test threads corrupt each other's environment.
//!
//! Historical note: `config.rs` and `main.rs` each defined their own
//! `ENV_LOCK` static. They raced because both mutated the same global
//! `HOME`/`XDG_CACHE_HOME` while holding *different* locks. This module
//! unifies them into one lock plus one RAII guard.

use std::sync::{Mutex, MutexGuard};

/// Serializes tests that mutate global env vars (`HOME`, `XDG_*`).
pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard: redirects `HOME`, `XDG_CACHE_HOME` and `XDG_CONFIG_HOME` to a
/// private temp directory for the duration of the test, then restores the
/// original values on drop.
///
/// - `HOME` → fresh `tempfile::TempDir` (auto-removed on drop)
/// - `XDG_CACHE_HOME` / `XDG_CONFIG_HOME` → unset, so `dirs::cache_dir()` /
///   `dirs::config_dir()` fall through to `$HOME/.cache` / `$HOME/.config`.
pub(crate) struct TempEnv {
    old_home: Option<String>,
    old_cache_home: Option<String>,
    old_config_home: Option<String>,
    _tmp: tempfile::TempDir,
    _guard: MutexGuard<'static, ()>,
}

impl TempEnv {
    pub(crate) fn new() -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().expect("create temp dir for env sandbox");
        let old_home = std::env::var("HOME").ok();
        let old_cache_home = std::env::var("XDG_CACHE_HOME").ok();
        let old_config_home = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("HOME", tmp.path());
        std::env::remove_var("XDG_CACHE_HOME");
        std::env::remove_var("XDG_CONFIG_HOME");
        Self {
            old_home,
            old_cache_home,
            old_config_home,
            _tmp: tmp,
            _guard: guard,
        }
    }
}

impl Drop for TempEnv {
    fn drop(&mut self) {
        match &self.old_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        match &self.old_cache_home {
            Some(c) => std::env::set_var("XDG_CACHE_HOME", c),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        match &self.old_config_home {
            Some(c) => std::env::set_var("XDG_CONFIG_HOME", c),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}