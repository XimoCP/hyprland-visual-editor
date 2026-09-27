//! Countdown auto-minimize when the window loses focus.
//!
//! The Timer is stored in a thread_local of the main thread (Slint's event loop).
//! The Hyprland listener sends signals via invoke_from_event_loop.

use slint::{ComponentHandle, Timer, TimerMode, Weak};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Shared flag indicating whether the countdown is active.
static COUNTDOWN_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Until when the auto-minimize is suppressed. It is set during a theme
/// apply: the hyprctl reload and the regeneration of window rules make the
/// window lose and regain focus several times, and those events (queued in
/// the event loop) must not trigger a spurious countdown.
static SUPPRESS_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Suppresses the auto-minimize for `duration` (e.g. while a theme is being
/// applied). Safe to call from any thread.
#[allow(dead_code)]
pub fn suppress_auto_minimize(duration: Duration) {
    let until = Instant::now() + duration;
    *SUPPRESS_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) = Some(until);
    tracing::debug!("[countdown] Auto-minimize suprimido por {:?}", duration);
}

fn is_suppressed() -> bool {
    let mut guard = SUPPRESS_UNTIL.lock().unwrap_or_else(|e| e.into_inner());
    match *guard {
        Some(until) if Instant::now() < until => true,
        _ => {
            *guard = None;
            false
        }
    }
}

thread_local! {
    /// Countdown timer (only accessible from the main thread).
    static COUNTDOWN_TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) };
}

/// Starts the countdown. Must be called from the main thread.
pub fn start_countdown(window_weak: Weak<crate::MainWindow>) {
    if COUNTDOWN_ACTIVE.swap(true, Ordering::Relaxed) {
        tracing::debug!("[countdown] Ya activo, ignorando start");
        return;
    }

    if is_suppressed() {
        COUNTDOWN_ACTIVE.store(false, Ordering::Relaxed);
        tracing::debug!("[countdown] Suprimido (apply en curso), ignorando start");
        return;
    }

    // Read hidden state from Controller. The controller is initialized in
    // main() before the countdown listener starts, so it is always available.
    let hidden = crate::composer::global_controller()
        .map(|c| c.window_hidden())
        .unwrap_or(false);
    if hidden {
        COUNTDOWN_ACTIVE.store(false, Ordering::Relaxed);
        tracing::debug!("[countdown] Ventana oculta, ignorando start");
        return;
    }

    let Some(window) = window_weak.upgrade() else {
        COUNTDOWN_ACTIVE.store(false, Ordering::Relaxed);
        return;
    };

    // If auto-minimize is disabled, don't start countdown
    if !window.get_auto_minimize() {
        COUNTDOWN_ACTIVE.store(false, Ordering::Relaxed);
        return;
    }

    let seconds = window.get_minimize_seconds();
    if resolve_grace(seconds) == GraceDecision::Immediate {
        tracing::info!("[countdown] Margen desactivado (0s): ocultando al instante");
        minimize_now(window_weak);
        return;
    }
    tracing::info!("[countdown] Iniciando countdown de {}s", seconds);
    window.set_countdown_active(true);
    window.set_countdown_seconds(seconds);
    window.set_countdown_progress(1.0);

    let weak = window.as_weak();
    let total = seconds; // capture for tick closure
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(1000), move || {
        tick(&weak, total);
    });

    COUNTDOWN_TIMER.with(|t| {
        *t.borrow_mut() = Some(timer);
    });
}

/// Cancels the active countdown. Must be called from the main thread.
pub fn cancel_countdown(window_weak: Weak<crate::MainWindow>) {
    if !COUNTDOWN_ACTIVE.swap(false, Ordering::Relaxed) {
        return;
    }

    tracing::info!("[countdown] Cancelado");

    COUNTDOWN_TIMER.with(|t| {
        *t.borrow_mut() = None;
    });

    if let Some(window) = window_weak.upgrade() {
        let seconds = window.get_minimize_seconds();
        window.set_countdown_active(false);
        window.set_countdown_seconds(seconds);
        window.set_countdown_progress(1.0);
    }
}

/// Decide whether focus-lost should hide immediately (gallery immersive session).
///
/// Pure decision function — no IPC or Slint side-effects.
///
/// `gallery_active` is true when an immersive gallery session (fullscreen)
/// is active. `hidden` is true when the window is already hidden (idempotent
/// guard). Returns true iff immediate hide is required.
///
/// Gallery hide now respects the user's delay setting: with auto-hide enabled
/// and a positive delay it becomes a countdown instead of an instant hide.
/// Only `Off` (0s) or disabled auto-hide keeps the old instant behaviour.
pub(crate) fn should_hide_immediately(gallery_active: bool, hidden: bool) -> bool {
    if hidden {
        return false;
    }
    gallery_active
}

/// Grace decision for focus loss: Off (0 or negative) hides immediately,
/// any positive value runs the visible countdown first (returning focus
/// cancels it). Pure + tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraceDecision {
    Immediate,
    Countdown,
}

pub(crate) fn resolve_grace(seconds: i32) -> GraceDecision {
    if seconds <= 0 {
        GraceDecision::Immediate
    } else {
        GraceDecision::Countdown
    }
}

/// Pure blur decision for focus-lost, gated by ShowStateMachine.
/// `machine_allows` is `ShowStateMachine::on_focus_lost()` — false during
/// Hidden/Entering, true only in Visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlurDecision {
    ImmediateHide,
    StartCountdown,
    Nothing,
}

pub(crate) fn blur_decision(
    machine_allows: bool,
    gallery_active: bool,
    hidden: bool,
    auto_minimize: bool,
    minimize_seconds: i32,
) -> BlurDecision {
    if !machine_allows {
        return BlurDecision::Nothing;
    }
    if hidden {
        return BlurDecision::Nothing;
    }
    if should_hide_immediately(gallery_active, hidden) {
        // Gallery fullscreen now honours the hide delay. Disabled auto-hide
        // means "never auto-hide" (user closes with X/Esc). Off (0s) means
        // instant hide. Positive delay → visible countdown so you can return
        // from the other monitor before it hides.
        if !auto_minimize {
            return BlurDecision::Nothing;
        }
        return match resolve_grace(minimize_seconds) {
            GraceDecision::Immediate => BlurDecision::ImmediateHide,
            GraceDecision::Countdown => BlurDecision::StartCountdown,
        };
    }
    if !gallery_active && auto_minimize {
        return BlurDecision::StartCountdown;
    }
    BlurDecision::Nothing
}

/// Click on the X button → minimizes immediately.
pub fn minimize_now(window_weak: Weak<crate::MainWindow>) {
    tracing::info!("[countdown] Minimizando ventana...");
    cancel_countdown(window_weak.clone());

    if let Some(window) = window_weak.upgrade() {
        if let Some(mut ctrl) = crate::composer::global_controller() {
            // toggle_tray from visible → hide path, which also records
            // prev_workspace so re-showing returns to the original workspace.
            ctrl.toggle_tray(&window);
        }
        crate::tray::refresh_global_menu();
    }
}

/// Result of processing a countdown tick.
///
/// This struct is pure (no side effects) and lets us test the countdown
/// logic without having to mock Slint or IPC.
pub(crate) struct CountdownTick {
    /// New value of remaining seconds after decrementing.
    pub(crate) next_seconds: i32,
    /// Normalized progress 0.0..1.0 (1.0 = complete, 0.0 = done).
    pub(crate) progress: f32,
    /// true when the countdown reaches 0 and it is time to minimize.
    pub(crate) should_minimize: bool,
}

/// Processes a countdown tick: decrements, computes progress,
/// decides whether it must minimize.
///
/// This function is pure (no side effects) and testable:
/// - `next_seconds` = current_seconds - 1
/// - `progress` = next_seconds / total_seconds (0.0 if total == 0)
/// - `should_minimize` = true if next_seconds <= 0
///
/// For invalid cases (current > total, current < 0) it keeps
/// consistency: it decrements and computes progress as an f32 division.
pub(crate) fn process_tick(current_seconds: i32, total_seconds: i32) -> CountdownTick {
    let next_seconds = current_seconds - 1;
    let progress = if total_seconds == 0 {
        0.0
    } else {
        next_seconds as f32 / total_seconds as f32
    };
    let should_minimize = next_seconds <= 0;

    CountdownTick {
        next_seconds,
        progress,
        should_minimize,
    }
}

/// Internal tick: decrements the counter; minimizes when it reaches 0.
fn tick(window_weak: &Weak<crate::MainWindow>, total_seconds: i32) {
    if !COUNTDOWN_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let Some(window) = window_weak.upgrade() else {
        COUNTDOWN_ACTIVE.store(false, Ordering::Relaxed);
        return;
    };

    let current = window.get_countdown_seconds();
    let result = process_tick(current, total_seconds);
    tracing::debug!("[countdown] Tick: {}s, progress: {:.2}", result.next_seconds, result.progress);
    window.set_countdown_seconds(result.next_seconds);
    window.set_countdown_progress(result.progress);

    if result.should_minimize {
        COUNTDOWN_ACTIVE.store(false, Ordering::Relaxed);
        COUNTDOWN_TIMER.with(|t| {
            *t.borrow_mut() = None;
        });
        tracing::info!("[countdown] Cuenta regresiva terminada, ocultando ventana");
        if let Some(mut ctrl) = crate::composer::global_controller() {
            // toggle_tray from visible → hide path, which also records
            // prev_workspace so re-showing returns to the original workspace.
            ctrl.toggle_tray(&window);
        }
        window.set_countdown_active(false);
        window.set_countdown_seconds(total_seconds);
        window.set_countdown_progress(1.0);
        crate::tray::refresh_global_menu();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── should_hide_immediately — gallery focus-lost immediate hide ────

    #[test]
    fn test_should_hide_when_gallery_active_and_visible() {
        assert!(
            should_hide_immediately(true, false),
            "gallery active + visible → immediate hide"
        );
    }

    #[test]
    fn test_should_not_hide_when_gallery_not_active() {
        assert!(
            !should_hide_immediately(false, false),
            "not in gallery → do not hide, use countdown"
        );
    }

    #[test]
    fn test_should_not_hide_when_already_hidden_gallery() {
        assert!(
            !should_hide_immediately(true, true),
            "already hidden → never hide again"
        );
    }

    #[test]
    fn test_should_not_hide_when_already_hidden_not_gallery() {
        assert!(
            !should_hide_immediately(false, true),
            "already hidden + not gallery → false"
        );
    }

    // ── Casos normales ────────────────────────────────────────────────

    #[test]
    fn test_process_tick_normal_decrement() {
        let result = process_tick(5, 10);
        assert_eq!(result.next_seconds, 4, "debe decrementar en 1");
        assert_eq!(result.progress, 0.4, "progress = 4/10");
        assert!(!result.should_minimize, "no debe minimizar con segundos restantes");
    }

    #[test]
    fn test_process_tick_one_second_remaining() {
        let result = process_tick(1, 10);
        assert_eq!(result.next_seconds, 0, "decrementa a 0");
        assert_eq!(result.progress, 0.0, "progress = 0/10");
        assert!(result.should_minimize, "debe minimizar al llegar a 0");
    }

    #[test]
    fn test_process_tick_at_zero() {
        let result = process_tick(0, 10);
        assert_eq!(result.next_seconds, -1);
        assert_eq!(result.progress, -0.1);
        assert!(result.should_minimize, "debe minimizar cuando ya está en 0");
    }

    // ── Edge cases ────────────────────────────────────────────────────

    #[test]
    fn test_process_tick_current_exceeds_total() {
        let result = process_tick(15, 10);
        assert_eq!(result.next_seconds, 14, "decrementa igual");
        assert!(result.progress > 1.0, "progress > 1.0 cuando current > total");
        assert!(!result.should_minimize, "no debe minimizar con current > total");
    }

    #[test]
    fn test_process_tick_total_is_zero() {
        let result = process_tick(5, 0);
        assert_eq!(result.next_seconds, 4, "decrementa igual");
        assert_eq!(result.progress, 0.0, "progress = 0.0 para evitar división por cero");
        assert!(!result.should_minimize, "no minimiza aún: next > 0");
    }

    #[test]
    fn test_process_tick_total_zero_and_current_zero() {
        let result = process_tick(0, 0);
        assert_eq!(result.next_seconds, -1);
        assert_eq!(result.progress, 0.0, "progress = 0.0 para evitar división por cero");
        assert!(result.should_minimize, "minimiza cuando next <= 0");
    }

    #[test]
    fn test_process_tick_current_negative() {
        let result = process_tick(-3, 10);
        assert_eq!(result.next_seconds, -4, "decrementa consistentemente");
        assert!(result.progress < 0.0, "progress negativo cuando current es negativo");
        assert!(result.should_minimize, "debe minimizar con current negativo");
    }

    #[test]
    fn test_process_tick_current_equals_one_and_total_equals_ten() {
        let result = process_tick(1, 10);
        assert_eq!(result.next_seconds, 0);
        assert_eq!(result.progress, 0.0);
        assert!(result.should_minimize);
    }

    #[test]
    fn test_process_tick_last_second() {
        let result = process_tick(1, 1);
        assert_eq!(result.next_seconds, 0);
        assert_eq!(result.progress, 0.0);
        assert!(result.should_minimize);
    }

    #[test]
    fn test_process_tick_full_countdown() {
        let result = process_tick(3, 3);
        assert_eq!(result.next_seconds, 2);
        assert_eq!(result.progress, 2.0 / 3.0);
        assert!(!result.should_minimize);
    }

    // ── resolve_grace — Off hides at once, positive counts down ───────

    #[test]
    fn test_grace_zero_is_immediate() {
        assert_eq!(resolve_grace(0), GraceDecision::Immediate);
    }

    #[test]
    fn test_grace_negative_is_immediate() {
        assert_eq!(resolve_grace(-3), GraceDecision::Immediate);
    }

    #[test]
    fn test_grace_positive_counts_down() {
        for s in [1, 2, 4, 6, 8, 10] {
            assert_eq!(resolve_grace(s), GraceDecision::Countdown, "{s}s must countdown");
        }
    }

    // ── blur_decision — gated by ShowStateMachine (unit 3) ──────────────

    #[test]
    fn test_blur_during_entering_no_hide_no_countdown() {
        // THE regression: gallery auto-minimize-on-open. Blur while Entering
        // must be ignored even with gallery active + auto_minimize.
        assert_eq!(
            blur_decision(false, true, false, true, 4),
            BlurDecision::Nothing,
            "blur during Entering → Nothing (machine_allows=false)"
        );
        assert_eq!(
            blur_decision(false, false, false, true, 4),
            BlurDecision::Nothing,
            "blur during Entering without gallery also Nothing"
        );
    }

    #[test]
    fn test_blur_in_visible_with_gallery_immediate_hide() {
        // Gallery Off (0s) → instant hide even with auto-hide enabled
        assert_eq!(
            blur_decision(true, true, false, true, 0),
            BlurDecision::ImmediateHide,
            "blur in Visible + gallery + 0s → ImmediateHide"
        );
        // Gallery with delay → countdown, so you can return from other monitor
        assert_eq!(
            blur_decision(true, true, false, true, 4),
            BlurDecision::StartCountdown,
            "blur in Visible + gallery + 4s → StartCountdown"
        );
        // Gallery but auto-hide disabled → never auto-hide (user closes with X/Esc)
        assert_eq!(
            blur_decision(true, true, false, false, 4),
            BlurDecision::Nothing,
            "gallery with auto_minimize=false → Nothing (never hide)"
        );
    }

    #[test]
    fn test_blur_in_visible_without_gallery_auto_minimize_countdown() {
        assert_eq!(
            blur_decision(true, false, false, true, 4),
            BlurDecision::StartCountdown,
            "blur in Visible + !gallery + auto_minimize → StartCountdown"
        );
    }

    #[test]
    fn test_blur_in_visible_without_gallery_no_auto_minimize_nothing() {
        assert_eq!(
            blur_decision(true, false, false, false, 4),
            BlurDecision::Nothing,
            "blur in Visible + !gallery + !auto_minimize → Nothing"
        );
    }

    #[test]
    fn test_blur_while_hidden_nothing() {
        assert_eq!(
            blur_decision(false, true, true, true, 4),
            BlurDecision::Nothing,
            "blur while Hidden → Nothing even with gallery"
        );
        assert_eq!(
            blur_decision(false, false, true, true, 4),
            BlurDecision::Nothing,
            "blur while Hidden → Nothing"
        );
        // Even if machine_allows were true but hidden flag set, should stay Nothing
        assert_eq!(
            blur_decision(true, true, true, true, 4),
            BlurDecision::Nothing,
            "hidden flag overrides ImmediateHide"
        );
    }

    #[test]
    fn test_blur_entering_vs_visible_hidden_flag_interaction() {
        // Machine allows false must win over every other flag
        assert_eq!(blur_decision(false, true, false, false, 4), BlurDecision::Nothing);
        assert_eq!(blur_decision(false, false, false, false, 4), BlurDecision::Nothing);
    }
}

/// Wrapper to connect the Hyprland listener.
/// Returns a `ListenerHandle` that keeps the focus listener thread alive.
/// When the handle is dropped (app shutdown), the thread stops cleanly.
pub fn setup_countdown(window_weak: Weak<crate::MainWindow>) -> crate::hypr_ipc::ListenerHandle {
    let weak_for_lost = window_weak.clone();
    let weak_for_gained = window_weak.clone();

    crate::hypr_ipc::spawn_focus_listener(
        move || {
            let w = weak_for_lost.clone();
            let _ = slint::invoke_from_event_loop(move || {
                // Keep theme-apply suppression as-is.
                if is_suppressed() {
                    tracing::debug!("[countdown] Suprimido (apply en curso), ignorando focus lost");
                    return;
                }
                // Theme transition owns focus: the reload-induced focus flaps
                // must NEVER trigger the gallery immediate-hide (that parked
                // HVE in special:minimized mid-fade — blurred, bar-less special).
                if crate::is_theme_transitioning_flag() {
                    tracing::debug!("[countdown] Theme transitioning — ignoring focus lost");
                    return;
                }
                let Some(win) = w.upgrade() else { return };
                let machine_allows = crate::composer::try_global_controller()
                    .map(|c| c.on_focus_lost_machine())
                    .unwrap_or(false);
                let (hidden, gallery_active) = crate::composer::try_global_controller()
                    .map(|c| (c.window_hidden(), c.gallery_session_active()))
                    .unwrap_or((false, false));
                let auto_minimize = win.get_auto_minimize();
                let minimize_seconds = win.get_minimize_seconds();
                let decision = blur_decision(machine_allows, gallery_active, hidden, auto_minimize, minimize_seconds);
                match decision {
                    BlurDecision::ImmediateHide => {
                        tracing::info!("[countdown] Gallery focus lost → immediate hide (Esc path)");
                        minimize_now(w.clone());
                    }
                    BlurDecision::StartCountdown => {
                        tracing::debug!("[countdown] Foco perdido, posible countdown (machine allows)");
                        start_countdown(w.clone());
                    }
                    BlurDecision::Nothing => {
                        tracing::debug!("[countdown] Foco perdido ignorado (machine gate or hidden)");
                    }
                }
            });
        },
        move || {
            let w = weak_for_gained.clone();
            let _ = slint::invoke_from_event_loop(move || {
                // Focus GAINED with HVE's title: confirm focus. If it
                // transitioned Entering→Visible, run the SETTLE path
                // exactly once (focus + WindowActiveChanged + fullscreen).
                // If Hidden, ignore (no settle, prevents drift when parked
                // window is somehow focused).
                let prev_state = crate::composer::try_global_controller()
                    .map(|c| c.show_state())
                    .unwrap_or(crate::show_state::ShowState::Hidden);
                let did_confirm = crate::composer::try_global_controller()
                    .map(|mut c| c.confirm_focus_machine())
                    .unwrap_or(false);
                if prev_state == crate::show_state::ShowState::Entering && did_confirm {
                    if let Some(win) = w.upgrade() {
                        if let Some(mut ctrl) = crate::composer::try_global_controller() {
                            ctrl.run_settle(&win);
                        } else if let Some(mut ctrl) = crate::composer::global_controller() {
                            ctrl.run_settle(&win);
                        }
                        tracing::info!("[countdown] Focus gained confirmed Entering→Visible, settle ran");
                    }
                } else if !did_confirm && prev_state == crate::show_state::ShowState::Hidden {
                    tracing::debug!("[countdown] Focus gained ignored (Hidden)");
                }
                // Always cancel countdown on focus gained (existing behavior).
                cancel_countdown(w.clone());
            });
        },
    )
}
