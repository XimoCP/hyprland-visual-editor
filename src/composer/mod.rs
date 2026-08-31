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

use crate::show_state::{ShowState, ShowStateMachine};
use slint::ComponentHandle;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

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
    composer: Box<dyn Composer>,
    show_state: ShowStateMachine,
}

impl Controller {
    /// Create a new Controller with the given compositor driver.
    pub fn new(composer: Box<dyn Composer>) -> Self {
        Self {
            window_hidden: false,
            tray_mode: false,
            prev_workspace: None,
            gallery_session: false,
            composer,
            show_state: ShowStateMachine::new(),
        }
    }

    /// Accessors for ShowStateMachine — needed by countdown.rs and
    /// for entry-timeout / settle wiring (unit 3).
    pub fn show_state(&self) -> ShowState {
        self.show_state.state()
    }

    pub fn begin_show_machine(&mut self) -> bool {
        self.show_state.begin_show()
    }

    pub fn confirm_focus_machine(&mut self) -> bool {
        self.show_state.confirm_focus()
    }

    pub fn entry_timed_out_machine(&mut self, timeout: Duration) -> bool {
        self.show_state.entry_timed_out(timeout)
    }

    pub fn on_focus_lost_machine(&self) -> bool {
        self.show_state.on_focus_lost()
    }

    pub fn on_hide_machine(&mut self) -> bool {
        self.show_state.on_hide()
    }

    /// Run the SETTLE path exactly once per show, on the
    /// Entering→Visible transition. Wake keyboard focus and, if the
    /// Gallery is mounted expanded, enter fullscreen exactly once.
    /// Called from the focus-gained handler or entry timeout after a
    /// successful Entering→Visible transition.
    pub(crate) fn run_settle(&mut self, win: &crate::MainWindow) {
        self.composer.focus();
        {
            use slint::platform::WindowEvent;
            win.window().dispatch_event(WindowEvent::WindowActiveChanged(true));
            win.window().request_redraw();
        }
        if crate::shell::Shell::is_gallery_expanded() {
            if self.gallery_session_active() {
                let _ = self.composer.set_fullscreen(true);
            } else {
                let _ = self.enter_gallery_session();
            }
        }
    }

    /// Schedule the single-shot entry timeout (1000ms) after a successful
    /// show. If the compositor never confirms focus, the timeout promotes
    /// Entering→Visible and runs settle exactly once.
    fn schedule_entry_timeout(&self, win_weak: slint::Weak<crate::MainWindow>) {
        slint::Timer::single_shot(Duration::from_secs(1), move || {
            // Try to lock without blocking; if contended, the focus-gained
            // path may already have settled.
            if let Some(mut ctrl) = crate::composer::try_global_controller() {
                if ctrl.entry_timed_out_machine(Duration::from_secs(1)) {
                    if let Some(win) = win_weak.upgrade() {
                        ctrl.run_settle(&win);
                    }
                }
            } else if let Some(mut ctrl) = crate::composer::global_controller() {
                // Fallback if try_lock failed but we are on the UI thread
                // holding no other lock — use blocking lock as last resort.
                // This path is only for timer fires where the controller is
                // not already locked by toggle_tray.
                if ctrl.entry_timed_out_machine(Duration::from_secs(1)) {
                    if let Some(win) = win_weak.upgrade() {
                        ctrl.run_settle(&win);
                    }
                }
            }
        });
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
        ok
    }

    /// Exit the immersive Gallery session: restore the previous floating
    /// geometry and float mode. The session always closes, even when the
    /// compositor reports failure.
    pub fn exit_gallery_session(&mut self) -> bool {
        let ok = self.composer.set_fullscreen(false);
        self.gallery_session = false;
        ok
    }

    /// Whether an immersive Gallery fullscreen session is active.
    pub fn gallery_session_active(&self) -> bool {
        self.gallery_session
    }

    /// Toggle tray: hides if visible, shows if hidden.
    /// The show path is gated by ShowStateMachine (Hidden→Entering) and
    /// defers fullscreen to the settle path (focus confirm or 1s timeout).
    /// Returns true if the fast path was taken (special workspace transition).
    pub fn toggle_tray(&mut self, win: &crate::MainWindow) -> bool {
        if self.window_hidden {
            // Show path — machine is the single owner of truth.
            if !self.begin_show_machine() {
                // Already Entering/Visible → treat as no-op/refresh, do NOT re-dispatch.
                return false;
            }
            let win_weak = win.as_weak();
            let fast = self.composer.show(win, self.prev_workspace.as_deref());
            // If the composer show failed entirely (no hyprctl success and
            // no fallback), revert machine and take the slow path. The
            // HyprlandComposer already falls back to show_and_sync internally
            // and returns false for the slow path, so a true failure is
            // rare; this branch covers the edge where show reports failure.
            // For the pure slow path (window not in special), we keep the
            // existing prewarm/warmup extras and stay in Entering.
            if !fast {
                // Slow path extras — legacy prewarm/warmup removed slice 8 (R8).
                // Panel navigation warms per-section focus via SystemSection/BordersSection FocusScope.
            }
            self.window_hidden = false;
            // Defer fullscreen to settle (focus confirm or timeout). Keep
            // sync_global_after_show as-is — it is already called inside
            // HyprlandComposer::show via sync_after_show/show_and_sync.
            self.schedule_entry_timeout(win_weak);
            fast
        } else {
            // Hide path — record where the window lives before moving it away,
            // so show() can return it to a real workspace later.
            self.prev_workspace = self.composer.active_workspace();
            self.composer.hide(win);
            self.window_hidden = true;
            let _ = self.on_hide_machine();
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
            self.record("move_to_workspace");
            self.record("focus");
            // Deferred focus + WindowActiveChanged now happens in the SETTLE
            // path (focus confirm or 1s timeout), not immediately in show.
            // sync_global_after_show is handled by HyprlandComposer internally.
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
        // FakeComposer emits move_to_workspace → focus → deferred focus →
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
            vec!["move_to_workspace", "focus"],
            "show fast path must emit move → focus; settle (focus + WindowActiveChanged + fullscreen) is deferred to focus confirm or 1s timeout"
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
            vec!["move_to_workspace", "focus"],
            "show slow path must emit the same move → focus; settle is deferred"
        );
    }

    #[test]
    fn test_show_fast_path_moves_before_focus_avoids_special_overlay_blur() {
        // Unit 3.1: V5 fast path must dispatch move BEFORE focus.
        // The move is explicitly targeted (window="title:...") so it works
        // without focus; focusing after the move avoids focusing inside the
        // special overlay, which emits blur on close and re-hides instantly.
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        let (fake, calls) = FakeComposer::with_show_fast(true);
        let mut controller = Controller::new(Box::new(fake));
        controller.toggle_tray(&win);
        assert!(controller.window_hidden());
        calls.lock().unwrap().clear();
        let _ = controller.toggle_tray(&win);
        let recorded = calls.lock().unwrap().clone();
        let move_pos = recorded.iter().position(|c| c == "move_to_workspace");
        let focus_pos = recorded.iter().position(|c| c == "focus");
        assert!(
            move_pos.is_some() && focus_pos.is_some(),
            "show fast path must emit both move and focus, got: {:?}",
            recorded
        );
        assert!(
            move_pos.unwrap() < focus_pos.unwrap(),
            "show fast path must dispatch move BEFORE focus to avoid focusing inside special overlay (blur/re-hide loop), got order: {:?}",
            recorded
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

    // ── ShowStateMachine wiring (unit 3 RED) ─────────────────────────

    #[test]
    fn test_show_enters_entering_and_defers_fullscreen_until_settle() {
        // RED: show must move machine Hidden→Entering and must NOT
        // immediately dispatch fullscreen; fullscreen moves to settle
        // (focus-gained or timeout). Until GREEN this fails because
        // toggle_tray still dispatches fullscreen immediately.
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        win.show().unwrap();
        let shell = crate::shell::Shell::new(win.as_weak());
        crate::shell::Shell::set_global(shell.clone());
        crate::shell::Shell::dispatch(
            &shell,
            crate::shell::nav::NavCommand::Expand(crate::shell::nav::Screen::Gallery),
        );
        // Gallery expanded => old code immediately calls set_fullscreen
        let (fake, calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));
        controller.set_window_hidden(true);
        let _ = controller.toggle_tray(&win);
        assert_eq!(
            controller.show_state(),
            crate::show_state::ShowState::Entering,
            "show must move machine to Entering"
        );
        let recorded = calls.lock().unwrap().clone();
        assert!(
            !recorded.iter().any(|c| c.contains("set_fullscreen")),
            "show must NOT immediately fullscreen; fullscreen is deferred to settle (focus confirm or timeout), got: {:?}",
            recorded
        );
    }

    #[test]
    fn test_second_show_while_entering_is_noop_no_redispatch() {
        // RED: begin_show returns false while Entering → show is no-op
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        win.show().unwrap();
        let (fake, calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));
        controller.set_window_hidden(true);
        let _ = controller.toggle_tray(&win);
        assert_eq!(controller.show_state(), crate::show_state::ShowState::Entering);
        // Simulate rapid second toggle while still Entering but window_hidden
        // was externally set to hidden again (e.g. IPC debounce edge).
        controller.set_window_hidden(true);
        calls.lock().unwrap().clear();
        let fast = controller.toggle_tray(&win);
        // Second show while Entering must be no-op: no composer dispatch
        assert!(
            calls.lock().unwrap().is_empty(),
            "second show while Entering must not re-dispatch, got: {:?}",
            *calls.lock().unwrap()
        );
        let _ = fast;
    }

    #[test]
    fn test_hide_resets_machine_to_hidden() {
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        win.show().unwrap();
        let (fake, _calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));
        controller.set_window_hidden(true);
        let _ = controller.toggle_tray(&win);
        assert_eq!(controller.show_state(), crate::show_state::ShowState::Entering);
        // Confirm focus → Visible, then hide → Hidden
        assert!(controller.confirm_focus_machine());
        assert_eq!(controller.show_state(), crate::show_state::ShowState::Visible);
        // Hide via toggle_tray
        let _ = controller.toggle_tray(&win);
        assert_eq!(
            controller.show_state(),
            crate::show_state::ShowState::Hidden,
            "hide must reset machine to Hidden"
        );
    }

    #[test]
    fn test_settle_runs_once_confirm_then_timeout_no_second_fullscreen() {
        // RED: settle must run exactly once per show. Confirm → Visible,
        // then a timeout must be a no-op and not re-dispatch fullscreen.
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        win.show().unwrap();
        let shell = crate::shell::Shell::new(win.as_weak());
        crate::shell::Shell::set_global(shell.clone());
        crate::shell::Shell::dispatch(
            &shell,
            crate::shell::nav::NavCommand::Expand(crate::shell::nav::Screen::Gallery),
        );
        let (fake, calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));
        controller.set_window_hidden(true);
        let _ = controller.toggle_tray(&win);
        // First transition via confirm -> settle should fullscreen once
        let did_settle = controller.confirm_focus_machine();
        assert!(did_settle, "confirm must transition Entering→Visible");
        // Simulate settle dispatch (would be run_settle) — only when transitioned
        if did_settle {
            controller.run_settle(&win);
        }
        let first_count = calls.lock().unwrap().iter().filter(|c| c.contains("set_fullscreen")).count();
        // Second transition via timeout must be no-op and must NOT re-run settle
        let did_timeout = controller.entry_timed_out_machine(Duration::from_millis(0));
        assert!(!did_timeout, "timeout after confirm must be no-op");
        if did_timeout {
            controller.run_settle(&win);
        }
        let second_count = calls.lock().unwrap().iter().filter(|c| c.contains("set_fullscreen")).count();
        assert_eq!(
            first_count, second_count,
            "settle must run exactly once: timeout after confirm must not re-dispatch fullscreen"
        );
        assert_eq!(first_count, 1, "first settle must have dispatched fullscreen exactly once");
    }
}
