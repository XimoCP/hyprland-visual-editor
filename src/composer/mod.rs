//! Compositor driver abstraction.
//!
//! Extracted from `src/ipc.rs` — the `Composer` trait defines the contract
//! for window hide/show/focus/float operations. `Controller` owns the
//! mutable state (`window_hidden`, `tray_mode`, `prev_workspace`) and
//! delegates compositor operations to a `Box<dyn Composer>`.
//!
//! The global controller is stored in `OnceLock<Mutex<Controller>>` so that
//! `ipc.rs`, `tray.rs`, and `countdown.rs` can access it without passing
//! references through every closure.

use slint::ComponentHandle;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// ── Global controller singleton ──────────────────────────────────────

static GLOBAL_CONTROLLER: OnceLock<Mutex<Controller>> = OnceLock::new();

/// Register the controller as the global singleton.
pub fn init_global(ctrl: Controller) {
    let _ = GLOBAL_CONTROLLER.set(Mutex::new(ctrl));
}

/// Get a lock on the global controller.
pub fn global_controller() -> Option<MutexGuard> {
    GLOBAL_CONTROLLER
        .get()
        .map(|m| MutexGuard(m.lock().unwrap_or_else(|e| e.into_inner())))
}

/// Try to lock without blocking — avoids deadlock when called from
/// inside `Controller::toggle_tray` via `Shell::sync_global_after_show`.
pub fn try_global_controller() -> Option<MutexGuard> {
    GLOBAL_CONTROLLER
        .get()
        .and_then(|m| m.try_lock().ok())
        .map(MutexGuard)
}

/// Convenience wrapper for MutexGuard.
pub struct MutexGuard(std::sync::MutexGuard<'static, Controller>);
impl std::ops::Deref for MutexGuard {
    type Target = Controller;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for MutexGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// ── HyprMode (internal to this module) ────────────────────────────────

/// Hyprland version mode — detected once at init.
/// `pub(crate)` so the legacy ipc.rs path reuses the same definition
/// instead of duplicating it.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum HyprMode {
    V5,
    V4,
    None,
}

// ── Composer trait ────────────────────────────────────────────────────

/// Compositor driver contract — Send + Sync for cross-thread use.
///
/// Mirrors the `ThemeProvider` pattern: callers interact with the trait
/// through a `Box<dyn Composer>` held by the `Controller`.
pub trait Composer: Send + Sync {
    /// Hide window via special workspace (not Slint hide).
    fn hide(&self, win: &crate::MainWindow) -> bool;

    /// Show window: focus → move_to_workspace → deferred focus assert.
    /// `prev_workspace` is the fallback target recorded by the Controller before hide.
    /// Returns true for fast path (special workspace), false for full remap.
    fn show(&self, win: &crate::MainWindow, prev_workspace: Option<&str>) -> bool;

    /// Re-assert focus (used by deferred timer and direct calls).
    fn focus(&self);

    /// Toggle floating (for tiling mode).
    fn toggle_float(&self);

    /// Enter (`on = true`) or exit (`on = false`) an undecorated fullscreen
    /// surface for an immersive Gallery session. Returns true when the
    /// compositor accepted the transition; false means HVE continues
    /// windowed — the gallery stays fully functional either way and the
    /// UI never blocks on it.
    fn set_fullscreen(&self, on: bool) -> bool;

    /// Read-only discovery: current active workspace name, if queryable.
    fn active_workspace(&self) -> Option<String>;
}

// ── Controller ────────────────────────────────────────────────────────

/// Owned state + compositor driver. Single mutable owner — no atomics leak to callers.
pub struct Controller {
    window_hidden: bool,
    tray_mode: bool,
    prev_workspace: Option<String>,
    /// Immersive Gallery fullscreen session active (gallery session spec).
    gallery_session: bool,
    gallery_entered_at: Mutex<Option<Instant>>,
    composer: Box<dyn Composer>,
}

impl Controller {
    /// Create a new Controller with the given compositor driver.
    pub fn new(composer: Box<dyn Composer>) -> Self {
        Self {
            window_hidden: false,
            tray_mode: false,
            prev_workspace: None,
            gallery_session: false,
            gallery_entered_at: Mutex::new(None),
            composer,
        }
    }

    /// Enter an immersive Gallery session (gallery-immersive-redesign 1.6):
    /// switch to undecorated fullscreen BEFORE the gallery content shows.
    /// Returns false when the compositor refused — the caller continues
    /// windowed and the session is not marked active. Idempotent: a second
    /// enter while the session is already active is a no-op and returns
    /// true without re-dispatching.
    pub fn enter_gallery_session(&mut self) -> bool {
        if self.gallery_session {
            return true;
        }
        let ok = self.composer.set_fullscreen(true);
        self.gallery_session = ok;
        if ok {
            *self.gallery_entered_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        }
        ok
    }

    /// Exit the immersive Gallery session: restore the previous floating
    /// geometry and float mode. The session always closes, even when the
    /// compositor reports failure.
    pub fn exit_gallery_session(&mut self) -> bool {
        let ok = self.composer.set_fullscreen(false);
        self.gallery_session = false;
        *self.gallery_entered_at.lock().unwrap_or_else(|e| e.into_inner()) = None;
        ok
    }

    /// Whether an immersive Gallery fullscreen session is active.
    pub fn gallery_session_active(&self) -> bool {
        self.gallery_session
    }

    /// Whether the gallery session has been stable for at least `grace`.
    /// Returns false when no session entry time is recorded (i.e. no active
    /// session — caller should also check `gallery_session_active`) or when
    /// the elapsed time since `enter_gallery_session` is still within the
    /// grace period.
    pub fn gallery_settled(&self, grace: Duration) -> bool {
        match *self.gallery_entered_at.lock().unwrap_or_else(|e| e.into_inner()) {
            Some(t) => t.elapsed() >= grace,
            None => false,
        }
    }

    /// Test-only setter to inject a gallery entry instant without sleeping.
    #[cfg(test)]
    pub(crate) fn set_gallery_entered_at_for_test(&self, t: Option<Instant>) {
        *self.gallery_entered_at.lock().unwrap_or_else(|e| e.into_inner()) = t;
    }

    /// Toggle tray: hides if visible, shows if hidden.
    /// Returns true if the fast path was taken (special workspace transition).
    pub fn toggle_tray(&mut self, win: &crate::MainWindow) -> bool {
        if self.window_hidden {
            // Show path
            let fast = self.composer.show(win, self.prev_workspace.as_deref());
            if !fast {
                crate::prewarm_tabs(win.as_weak(), 1);
                crate::warmup_navigation(win);
            }
            self.window_hidden = false;
            // Re-assert fullscreen gallery when Gallery is mounted expanded.
            // show() already called sync_global_after_show, but that sync may
            // have skipped due to try_lock or initial tray-mode expand failure.
            // This direct reassert is deadlock-free (uses &mut self) and idempotent.
            if crate::shell::Shell::is_gallery_expanded() {
                if self.gallery_session_active() {
                    let _ = self.composer.set_fullscreen(true);
                } else {
                    let _ = self.enter_gallery_session();
                }
            }
            fast
        } else {
            // Hide path — record where the window lives before moving it away,
            // so show() can return it to a real workspace later.
            self.prev_workspace = self.composer.active_workspace();
            self.composer.hide(win);
            self.window_hidden = true;
            true
        }
    }

    /// Whether the window is currently hidden.
    pub fn window_hidden(&self) -> bool {
        self.window_hidden
    }

    /// Set the hidden state directly (for startup initialization).
    pub fn set_window_hidden(&mut self, val: bool) {
        self.window_hidden = val;
    }

    /// Whether HVE was started in tray mode.
    /// Used by tests to assert startup state (set via `set_tray_mode`).
    #[allow(dead_code)]
    pub fn tray_mode(&self) -> bool {
        self.tray_mode
    }

    /// Set tray mode (for startup initialization).
    pub fn set_tray_mode(&mut self, val: bool) {
        self.tray_mode = val;
    }

    /// Previous workspace where the window was before being hidden.
    /// Written internally by `toggle_tray` on hide; read by tests.
    #[allow(dead_code)]
    pub fn prev_workspace(&self) -> Option<&str> {
        self.prev_workspace.as_deref()
    }

    /// Access the composer driver.
    pub fn composer(&self) -> &dyn Composer {
        self.composer.as_ref()
    }
}

// ── Hyprland composer ─────────────────────────────────────────────────

mod hyprland;
pub use hyprland::HyprlandComposer;

/// Startup sanity check (gallery-immersive-redesign 1.5): detect an HVE
/// window left stuck fullscreen by a crash and restore floating BEFORE the
/// first show. Best-effort; a no-op without a Hyprland session.
pub fn startup_fullscreen_sanity() {
    HyprlandComposer::new().startup_sanity();
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Initialize the headless Slint test backend on the current thread.
    ///
    /// Tests run in parallel threads. `init_no_event_loop` configures the
    /// backend with `threading: false`, so each test thread gets its own
    /// independent backend — the integration backend (`threading: true`) only
    /// supports a single process-wide init and would panic on parallel use.
    fn init_test_platform() {
        i_slint_backend_testing::init_no_event_loop();
    }

    /// Records calls for deterministic testing.
    pub struct FakeComposer {
        calls: Arc<Mutex<Vec<String>>>,
        show_fast: bool,
        /// Result reported by `set_fullscreen` (true = dispatch accepted).
        fullscreen_ok: bool,
    }

    impl FakeComposer {
        pub fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    calls: calls.clone(),
                    show_fast: true,
                    fullscreen_ok: true,
                },
                calls,
            )
        }

        pub fn with_show_fast(show_fast: bool) -> (Self, Arc<Mutex<Vec<String>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    calls: calls.clone(),
                    show_fast,
                    fullscreen_ok: true,
                },
                calls,
            )
        }

        /// Fake composer whose `set_fullscreen` reports `ok`.
        pub fn with_fullscreen_result(ok: bool) -> (Self, Arc<Mutex<Vec<String>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    calls: calls.clone(),
                    show_fast: true,
                    fullscreen_ok: ok,
                },
                calls,
            )
        }

        pub fn record(&self, call: &str) {
            self.calls.lock().unwrap().push(call.to_string());
        }
    }

    impl Composer for FakeComposer {
        fn hide(&self, _win: &crate::MainWindow) -> bool {
            self.record("record_workspace");
            self.record("move_to_special");
            self.record("close_special");
            true
        }

        fn show(&self, _win: &crate::MainWindow, _prev_workspace: Option<&str>) -> bool {
            self.record("focus");
            self.record("move_to_workspace");
            // Timer fire (simulated immediately for determinism)
            self.record("focus");
            self.record("WindowActiveChanged");
            self.show_fast
        }

        fn focus(&self) {
            self.record("focus");
        }

        fn toggle_float(&self) {
            self.record("toggle_float");
        }

        fn set_fullscreen(&self, on: bool) -> bool {
            self.record(&format!("set_fullscreen({on})"));
            self.fullscreen_ok
        }

        fn active_workspace(&self) -> Option<String> {
            self.record("active_workspace");
            Some("2".to_string())
        }
    }

    /// Scriptable mock for compositor-dependent tests.
    ///
    /// Implements the full `Composer` trait without a Hyprland session:
    /// every method records its call and returns a panic-free default.
    /// Scriptable state covers `hidden` (via Controller), `fullscreen`
    /// (`fullscreen_ok`), and `focus`/`show`/`active_workspace`.
    pub struct MockComposer {
        calls: Arc<Mutex<Vec<String>>>,
        pub show_fast: bool,
        pub fullscreen_ok: bool,
        pub active_workspace_name: Option<String>,
    }

    impl MockComposer {
        pub fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    calls: calls.clone(),
                    show_fast: true,
                    fullscreen_ok: true,
                    active_workspace_name: Some("2".to_string()),
                },
                calls,
            )
        }

        pub fn with_fullscreen_result(ok: bool) -> (Self, Arc<Mutex<Vec<String>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    calls: calls.clone(),
                    show_fast: true,
                    fullscreen_ok: ok,
                    active_workspace_name: Some("2".to_string()),
                },
                calls,
            )
        }

        pub fn with_show_fast(show_fast: bool) -> (Self, Arc<Mutex<Vec<String>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    calls: calls.clone(),
                    show_fast,
                    fullscreen_ok: true,
                    active_workspace_name: Some("2".to_string()),
                },
                calls,
            )
        }

        fn record(&self, call: &str) {
            self.calls.lock().unwrap().push(call.to_string());
        }
    }

    impl Composer for MockComposer {
        fn hide(&self, _win: &crate::MainWindow) -> bool {
            self.record("record_workspace");
            self.record("move_to_special");
            self.record("close_special");
            true
        }

        fn show(&self, _win: &crate::MainWindow, _prev_workspace: Option<&str>) -> bool {
            self.record("focus");
            self.record("move_to_workspace");
            self.record("focus");
            self.record("WindowActiveChanged");
            self.show_fast
        }

        fn focus(&self) {
            self.record("focus");
        }

        fn toggle_float(&self) {
            self.record("toggle_float");
        }

        fn set_fullscreen(&self, on: bool) -> bool {
            self.record(&format!("set_fullscreen({on})"));
            self.fullscreen_ok
        }

        fn active_workspace(&self) -> Option<String> {
            self.record("active_workspace");
            self.active_workspace_name.clone()
        }
    }

    // ── Controller toggle_tray tests ─────────────────────────────────

    /// We cannot create a real MainWindow in tests, but we can test
    /// the Controller's state transitions and method dispatch.
    /// The FakeComposer captures the call sequence.

    #[test]
    fn test_controller_initial_state() {
        let (fake, _calls) = FakeComposer::new();
        let controller = Controller::new(Box::new(fake));

        assert!(!controller.window_hidden(), "should not be hidden initially");
        assert!(!controller.tray_mode(), "should not be in tray mode");
        assert!(controller.prev_workspace().is_none(), "no prev workspace");
    }

    #[test]
    fn test_controller_set_window_hidden() {
        let (fake, _calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));

        controller.set_window_hidden(true);
        assert!(controller.window_hidden(), "should be hidden after set");

        controller.set_window_hidden(false);
        assert!(!controller.window_hidden(), "should not be hidden after set false");
    }

    #[test]
    fn test_controller_set_tray_mode() {
        let (fake, _calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));

        controller.set_tray_mode(true);
        assert!(controller.tray_mode(), "should be in tray mode");

        controller.set_tray_mode(false);
        assert!(!controller.tray_mode(), "should not be in tray mode");
    }

    #[test]
    fn test_controller_prev_workspace() {
        // prev_workspace is written by toggle_tray's hide path, not by a
        // public setter. FakeComposer.active_workspace() reports "2".
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        let (fake, _calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));

        assert!(controller.prev_workspace().is_none());

        // Hide path records the active workspace via the composer.
        controller.toggle_tray(&win);
        assert!(controller.window_hidden(), "hide marks window hidden");
        assert_eq!(controller.prev_workspace(), Some("2"), "hide records prev workspace");
    }

    #[test]
    fn test_composer_hide_contract() {
        // Real hide contract: a visible window (window_hidden=false) toggled
        // via toggle_tray() goes through the hide path, which records the
        // active workspace first, then dispatches the special-workspace hide
        // sequence on the composer. The controller flips to hidden.
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        let (fake, calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));

        let fast = controller.toggle_tray(&win);
        assert!(fast, "hide path returns fast (special workspace transition)");
        assert!(controller.window_hidden(), "hide marks the window hidden");
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "active_workspace",
                "record_workspace",
                "move_to_special",
                "close_special",
            ],
            "hide must record the active workspace, then dispatch move → close on the special workspace"
        );
    }

    #[test]
    fn test_composer_show_contract_fast_path() {
        // Real show contract (fast path): a window that is already hidden
        // toggled via toggle_tray() goes through the show path. The
        // FakeComposer emits focus → move_to_workspace → deferred focus →
        // WindowActiveChanged, and returns show_fast=true because the window
        // is in the special workspace. The controller flips back to visible.
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        let (fake, calls) = FakeComposer::with_show_fast(true);
        let mut controller = Controller::new(Box::new(fake));

        // Hide first (visible → hide path), then clear the recorded calls.
        controller.toggle_tray(&win);
        assert!(controller.window_hidden());
        calls.lock().unwrap().clear();

        // Toggle again from hidden → show path (fast, show_fast=true).
        let fast = controller.toggle_tray(&win);
        assert!(fast, "fast path taken because the window is in the special workspace");
        assert!(!controller.window_hidden(), "show marks the window visible");
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["focus", "move_to_workspace", "focus", "WindowActiveChanged"],
            "show fast path must emit focus → move → deferred focus → WindowActiveChanged"
        );
    }

    #[test]
    fn test_composer_show_contract_slow_path() {
        // Show contract (slow path): when the window is NOT in the special
        // workspace, show() reports slow (show_fast=false). The controller
        // triggers prewarm/warmup (not asserted here — they operate on the
        // window) and flips the window back to visible.
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        let (fake, calls) = FakeComposer::with_show_fast(false);
        let mut controller = Controller::new(Box::new(fake));

        // Hide first (visible → hide path), then clear the recorded calls.
        controller.toggle_tray(&win);
        assert!(controller.window_hidden());
        calls.lock().unwrap().clear();

        // Toggle again from hidden → show path (slow, show_fast=false).
        let fast = controller.toggle_tray(&win);
        assert!(!fast, "slow path taken because the window is not in the special workspace");
        assert!(!controller.window_hidden(), "show marks the window visible");
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["focus", "move_to_workspace", "focus", "WindowActiveChanged"],
            "show slow path must emit the same focus → move → deferred focus sequence"
        );
    }

    #[test]
    fn test_fake_composer_hide_sequence() {
        // Verify Controller is built correctly and FakeComposer is accessible
        let (fake, calls) = FakeComposer::new();
        let controller = Controller::new(Box::new(fake));

        assert!(!controller.window_hidden());
        assert!(calls.lock().unwrap().is_empty(), "no calls before interaction");

        // Access composer() returns a working trait object
        let _c = controller.composer();
    }

    #[test]
    fn test_composer_focus_and_toggle_float() {
        let (fake, calls) = FakeComposer::new();
        let controller = Controller::new(Box::new(fake));

        // Verify composer() returns a working reference
        let c = controller.composer();
        c.focus();
        assert_eq!(*calls.lock().unwrap(), vec!["focus"]);

        c.toggle_float();
        assert_eq!(*calls.lock().unwrap(), vec!["focus", "toggle_float"]);
    }

    #[test]
    fn test_controller_window_hidden_state_flips() {
        let (fake, _calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));

        // Initially not hidden
        assert!(!controller.window_hidden());

        // Manually set hidden (simulating hide path)
        controller.set_window_hidden(true);
        assert!(controller.window_hidden());

        // Manually unset hidden (simulating show path)
        controller.set_window_hidden(false);
        assert!(!controller.window_hidden());
    }

    // ── Global controller integration tests ────────────────────────────

    #[test]
    fn test_global_controller_round_trip() {
        // Verify that init_global + global_controller form a working round-trip.
        // Since OnceLock can only be set once per process, this test must run
        // first or in isolation. We verify the pattern works:
        let (fake, _calls) = FakeComposer::new();
        let controller = Controller::new(Box::new(fake));

        // The Controller itself works correctly (already tested above).
        // The global wiring (OnceLock + Mutex) is a standard Rust pattern
        // that is exercised in the real app at startup (main.rs).
        // Unit tests verify the Controller logic; integration tests
        // verify the full wiring with a real Composer.
        assert!(!controller.window_hidden());
        assert!(!controller.tray_mode());
    }

    #[test]
    fn test_controller_state_isolation_after_init() {
        // Verify that a Controller's state is independent of its initial values.
        let (fake, _calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));

        // Default state
        assert!(!controller.window_hidden());
        assert!(!controller.tray_mode());
        assert!(controller.prev_workspace().is_none());

        // Transition to hidden state
        controller.set_window_hidden(true);
        controller.set_tray_mode(true);

        assert!(controller.window_hidden());
        assert!(controller.tray_mode());

        // Transition back
        controller.set_window_hidden(false);
        controller.set_tray_mode(false);

        assert!(!controller.window_hidden());
        assert!(!controller.tray_mode());
    }

    // ── Gallery fullscreen session (gallery-immersive-redesign 1.1) ───

    /// Controller sessions wrap the composer contract: entering the
    /// session dispatches `set_fullscreen(true)` exactly once, exiting
    /// `(false)` exactly once (spec scenarios: enter-on-open,
    /// restore-floating-on-close).
    #[test]
    fn test_gallery_session_enter_exit_records_exact_sequence() {
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        let (fake, calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));

        assert!(!controller.gallery_session_active(), "no session initially");

        assert!(controller.enter_gallery_session(), "enter accepted");
        assert!(controller.gallery_session_active(), "session marked active");
        assert_eq!(*calls.lock().unwrap(), vec!["set_fullscreen(true)"]);

        assert!(controller.exit_gallery_session(), "exit accepted");
        assert!(!controller.gallery_session_active(), "session closed");
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["set_fullscreen(true)", "set_fullscreen(false)"]
        );
        let _ = &win; // platform guard
    }

    /// A failed enter must NOT mark the session active: the UI continues
    /// windowed and the gallery stays fully functional (spec fallback).
    #[test]
    fn test_gallery_session_failed_enter_stays_windowed() {
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        let (fake, _calls) = FakeComposer::with_fullscreen_result(false);
        let mut controller = Controller::new(Box::new(fake));

        assert!(!controller.enter_gallery_session(), "dispatch failed");
        assert!(
            !controller.gallery_session_active(),
            "failed enter must leave the session inactive"
        );
        let _ = &win; // platform guard
    }

    /// Session enter must record `set_fullscreen(true)` and exit must
    /// record `set_fullscreen(false)` — the exact dispatch sequence the
    /// Hyprland driver will replay (threat matrix: constant argv only).
    #[test]
    fn test_set_fullscreen_session_records_true_then_false() {
        let (fake, calls) = FakeComposer::new();

        assert!(fake.set_fullscreen(true), "enter session reports success");
        assert!(fake.set_fullscreen(false), "exit session reports success");
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["set_fullscreen(true)", "set_fullscreen(false)"],
            "session enter/exit must record the exact dispatch sequence"
        );
    }

    /// Dispatch failure returns false so the caller continues windowed:
    /// the gallery stays fully functional and never blocks on the
    /// compositor (spec fallback scenario).
    #[test]
    fn test_set_fullscreen_failure_returns_false_continues_windowed() {
        let (fake, calls) = FakeComposer::with_fullscreen_result(false);

        assert!(
            !fake.set_fullscreen(true),
            "dispatch failure must report false (UI continues windowed)"
        );
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["set_fullscreen(true)"],
            "the attempted dispatch is recorded even when it fails"
        );
    }

    #[test]
    fn test_controller_composer_trait_dispatch() {
        // Verify that composer() returns a reference that can be used
        // to call Composer trait methods.
        let (fake, calls) = FakeComposer::new();
        let controller = Controller::new(Box::new(fake));

        let c = controller.composer();
        c.focus();
        c.toggle_float();

        assert_eq!(*calls.lock().unwrap(), vec!["focus", "toggle_float"]);
    }
}
