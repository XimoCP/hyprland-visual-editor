//! Countdown auto-minimize cuando la ventana pierde foco.
//!
//! El Timer se guarda en un thread_local del thread principal (event loop de Slint).
//! El listener de Hyprland envía señales vía invoke_from_event_loop.

use slint::{ComponentHandle, Timer, TimerMode, Weak};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Grace period before a newly entered gallery session allows immediate hide
/// on focus loss. Single tuning point for the fullscreen transition race.
pub const GALLERY_HIDE_GRACE: Duration = Duration::from_millis(900);

/// Bandera compartida para indicar si el countdown está activo.
static COUNTDOWN_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Hasta cuándo se suprime el auto-minimize. Se setea durante un apply de
/// tema: el hyprctl reload y la regeneración de window rules hacen que la
/// ventana pierda y recupere foco varias veces, y esos eventos (encolados en
/// el event loop) no deben disparar un countdown espurio.
static SUPPRESS_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Suprime el auto-minimize durante `duration` (p. ej. mientras se aplica un
/// tema). Safe to call desde cualquier thread.
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
    /// Timer del countdown (solo accesible desde el thread principal).
    static COUNTDOWN_TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) };
}

/// Inicia el countdown. Debe llamarse desde el thread principal.
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

/// Cancela el countdown activo. Debe llamarse desde el thread principal.
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
/// guard). `settled` is true when the gallery session has been stable for
/// at least `GALLERY_HIDE_GRACE` (avoids racing the fullscreen transition).
/// Returns true iff immediate hide is required.
///
/// Note: gallery immediate-hide is unconditional once settled — it is the
/// immersive session contract. Even when `auto_minimize_enabled` is globally
/// disabled, losing focus during gallery must hide immediately (same as Esc).
pub(crate) fn should_hide_immediately(gallery_active: bool, hidden: bool, settled: bool) -> bool {
    if hidden {
        return false;
    }
    gallery_active && settled
}

/// Pure routing for focus-lost: gallery hide, countdown, or no-op.
///
/// Keeps `should_hide_immediately` semantics intact (gallery hide
/// unconditional on settled; non-gallery respects `auto_minimize` gate).
/// The window-side glue (setup_countdown closure) is trivial: query state,
/// call this, map the enum.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum FocusAction {
    HideNow,
    StartCountdown,
    Nothing,
}

pub(crate) fn decide_focus_action(
    gallery_active: bool,
    hidden: bool,
    settled: bool,
    auto_minimize: bool,
) -> FocusAction {
    if should_hide_immediately(gallery_active, hidden, settled) {
        return FocusAction::HideNow;
    }
    // Gallery active but not settled (within grace) or already hidden:
    // suppress — do not start countdown and do not hide.
    if gallery_active {
        return FocusAction::Nothing;
    }
    if hidden {
        return FocusAction::Nothing;
    }
    if !auto_minimize {
        return FocusAction::Nothing;
    }
    FocusAction::StartCountdown
}

/// Click en el botón X → minimiza inmediatamente.
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

/// Resultado de procesar un tick de countdown.
///
/// Este struct es puro (sin side effects) y permite testear la lógica
/// de countdown sin necesidad de mockear Slint ni IPC.
pub(crate) struct CountdownTick {
    /// Nuevo valor de segundos restantes tras decrementar.
    pub(crate) next_seconds: i32,
    /// Progreso normalizado 0.0..1.0 (1.0 = completo, 0.0 = terminado).
    pub(crate) progress: f32,
    /// true cuando el countdown llegó a 0 y hay que minimizar.
    pub(crate) should_minimize: bool,
}

/// Procesa un tick del countdown: decrementa, calcula progreso,
/// decide si debe minimizar.
///
/// Esta función es pura (sin side effects) y testeable:
/// - `next_seconds` = current_seconds - 1
/// - `progress` = next_seconds / total_seconds (0.0 si total == 0)
/// - `should_minimize` = true si next_seconds <= 0
///
/// Para casos inválidos (current > total, current < 0) mantiene
/// coherencia: decrementa y calcula progress como f32 division.
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

/// Tick interno: decrementa el contador; al llegar a 0 minimiza.
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
            should_hide_immediately(true, false, true),
            "gallery active + visible + settled → immediate hide"
        );
    }

    #[test]
    fn test_should_not_hide_when_gallery_not_active() {
        assert!(
            !should_hide_immediately(false, false, true),
            "not in gallery → do not hide, use countdown"
        );
    }

    #[test]
    fn test_should_not_hide_when_already_hidden_gallery() {
        assert!(
            !should_hide_immediately(true, true, true),
            "already hidden → never hide again"
        );
    }

    #[test]
    fn test_should_not_hide_when_already_hidden_not_gallery() {
        assert!(
            !should_hide_immediately(false, true, true),
            "already hidden + not gallery → false"
        );
    }

    #[test]
    fn test_should_not_hide_within_grace() {
        assert!(
            !should_hide_immediately(true, false, false),
            "gallery active but not yet settled (within grace) → do not hide"
        );
    }

    #[test]
    fn test_should_hide_after_grace() {
        assert!(
            should_hide_immediately(true, false, true),
            "gallery active + settled (after grace) → hide"
        );
    }

    #[test]
    fn test_should_not_hide_when_inactive_regardless_of_settled() {
        assert!(
            !should_hide_immediately(false, false, false),
            "inactive + not settled → false"
        );
        assert!(
            !should_hide_immediately(false, false, true),
            "inactive even if settled flag true → false (settled alone never hides)"
        );
    }

    #[test]
    fn test_gallery_hide_grace_is_900ms() {
        assert_eq!(
            GALLERY_HIDE_GRACE,
            Duration::from_millis(900),
            "grace constant must be 900ms"
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

    // ── FocusAction regression (c5dcc60 — gallery flicker) ───────────────

    #[test]
    fn test_focus_lost_within_grace_returns_nothing_and_session_stays_active() {
        // Simulate gallery just entered (settled=false within 900ms grace):
        // focus lost must NOT hide.
        let action = decide_focus_action(true, false, false, true);
        assert_eq!(
            action,
            FocusAction::Nothing,
            "within grace gallery focus-lost must be Nothing (regression c5dcc60)"
        );
    }

    #[test]
    fn test_focus_lost_after_grace_returns_hide_now() {
        // Gallery settled (elapsed >= grace): immediate hide required.
        let action = decide_focus_action(true, false, true, true);
        assert_eq!(
            action,
            FocusAction::HideNow,
            "settled gallery focus-lost must be HideNow"
        );
    }

    #[test]
    fn test_focus_lost_outside_gallery_with_auto_minimize_returns_countdown() {
        let action = decide_focus_action(false, false, false, true);
        assert_eq!(
            action,
            FocusAction::StartCountdown,
            "non-gallery with auto_minimize on → StartCountdown"
        );
        // auto_minimize off → Nothing
        let action_off = decide_focus_action(false, false, false, false);
        assert_eq!(
            action_off,
            FocusAction::Nothing,
            "non-gallery with auto_minimize off → Nothing"
        );
    }

    #[test]
    fn test_double_enter_gallery_does_not_reset_settled_clock() {
        // Controller double-enter must not reset gallery_entered_at.
        // Use injected instants so no real sleep is needed.
        use crate::composer::Controller;
        use crate::composer::tests::FakeComposer;
        use std::time::{Duration, Instant};

        let (fake, _calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));
        assert!(controller.enter_gallery_session());
        // Inject an entered_at far in the past so gallery is settled.
        let past = Instant::now() - Duration::from_millis(2000);
        controller.set_gallery_entered_at_for_test(Some(past));
        assert!(
            controller.gallery_settled(GALLERY_HIDE_GRACE),
            "should be settled after injected past instant"
        );
        let settled_before = controller.gallery_settled(GALLERY_HIDE_GRACE);
        // Second enter is a no-op and must not reset the clock.
        assert!(controller.enter_gallery_session(), "second enter is no-op true");
        let settled_after = controller.gallery_settled(GALLERY_HIDE_GRACE);
        assert_eq!(
            settled_before, settled_after,
            "double enter must not reset settled clock"
        );
        assert!(
            settled_after,
            "still settled after double enter (c5dcc60)"
        );
    }

    #[test]
    fn test_gallery_hide_within_grace_via_controller_settled() {
        use crate::composer::Controller;
        use crate::composer::tests::FakeComposer;
        use std::time::Duration;

        let (fake, _calls) = FakeComposer::new();
        let mut controller = Controller::new(Box::new(fake));
        controller.enter_gallery_session();
        // Just entered → not settled.
        assert!(
            !controller.gallery_settled(GALLERY_HIDE_GRACE),
            "fresh enter must not be settled within grace"
        );
        let gallery_active = controller.gallery_session_active();
        let settled = controller.gallery_settled(GALLERY_HIDE_GRACE);
        let action = decide_focus_action(gallery_active, false, settled, true);
        assert_eq!(action, FocusAction::Nothing);
        assert!(
            controller.gallery_session_active(),
            "session must remain active after within-grace focus lost"
        );
        // Tiny grace injection: past instant should make it settled.
        controller.set_gallery_entered_at_for_test(Some(
            std::time::Instant::now() - Duration::from_millis(10),
        ));
        let settled_tiny = controller.gallery_settled(Duration::from_millis(5));
        assert!(settled_tiny, "injected past instant with tiny grace must be settled");
        let action_settled = decide_focus_action(true, false, settled_tiny, true);
        assert_eq!(action_settled, FocusAction::HideNow);
    }
}

/// Wrapper para conectar el listener de Hyprland.
/// Returns a `ListenerHandle` that keeps the focus listener thread alive.
/// When the handle is dropped (app shutdown), the thread stops cleanly.
pub fn setup_countdown(window_weak: Weak<crate::MainWindow>) -> crate::hypr_ipc::ListenerHandle {
    let weak_for_lost = window_weak.clone();
    let weak_for_gained = window_weak.clone();

    crate::hypr_ipc::spawn_focus_listener(
        move || {
            // Snapshot compositor state on the IPC thread; decide on the
            // Slint main thread where `Weak::upgrade` + `get_auto_minimize`
            // are thread-safe. Gallery HideNow is unconditional on
            // auto_minimize; `decide_focus_action` keeps the pure truth table
            // (should_hide_immediately + gallery suppression + auto gate).
            let (hidden, gallery_active, settled) = crate::composer::global_controller()
                .map(|c| {
                    (
                        c.window_hidden(),
                        c.gallery_session_active(),
                        c.gallery_settled(GALLERY_HIDE_GRACE),
                    )
                })
                .unwrap_or((false, false, false));
            let weak = weak_for_lost.clone();
            let res = slint::invoke_from_event_loop(move || {
                let Some(window) = weak.upgrade() else {
                    return;
                };
                let auto_minimize = window.get_auto_minimize();
                let weak2 = window.as_weak();
                match decide_focus_action(gallery_active, hidden, settled, auto_minimize) {
                    FocusAction::HideNow => {
                        tracing::info!("[countdown] Gallery focus lost → immediate hide (Esc path)");
                        minimize_now(weak2);
                    }
                    FocusAction::StartCountdown => {
                        tracing::debug!("[countdown] Foco perdido, posible countdown");
                        start_countdown(weak2);
                    }
                    FocusAction::Nothing => {}
                }
            });
            if let Err(e) = res {
                tracing::error!("[countdown] invoke_from_event_loop falló (lost): {:?}", e);
            }
        },
        move || {
            tracing::debug!("[countdown] Foco recuperado, cancelando countdown");
            let w = weak_for_gained.clone();
            match slint::invoke_from_event_loop(move || cancel_countdown(w)) {
                Ok(_) => {}
                Err(e) => tracing::error!("[countdown] invoke_from_event_loop falló (gained): {:?}", e),
            }
        },
    )
}
