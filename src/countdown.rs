//! Countdown auto-minimize cuando la ventana pierde foco.
//!
//! El Timer se guarda en un thread_local del thread principal (event loop de Slint).
//! El listener de Hyprland envía señales vía invoke_from_event_loop.

use slint::{ComponentHandle, Timer, TimerMode, Weak};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const COUNTDOWN_SECONDS: i32 = 5;

/// Bandera compartida para indicar si el countdown está activo.
static COUNTDOWN_ACTIVE: AtomicBool = AtomicBool::new(false);

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

    let Some(window) = window_weak.upgrade() else {
        COUNTDOWN_ACTIVE.store(false, Ordering::Relaxed);
        return;
    };

    tracing::info!("[countdown] Iniciando countdown de {}s", COUNTDOWN_SECONDS);
    window.set_countdown_active(true);
    window.set_countdown_seconds(COUNTDOWN_SECONDS);
    window.set_countdown_progress(1.0);

    let weak = window.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(1000), move || {
        tick(&weak);
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
        window.set_countdown_active(false);
        window.set_countdown_seconds(COUNTDOWN_SECONDS);
        window.set_countdown_progress(1.0);
    }
}

/// Click en el botón X → minimiza inmediatamente.
pub fn minimize_now(window_weak: Weak<crate::MainWindow>) {
    tracing::info!("[countdown] Minimizando ventana...");
    cancel_countdown(window_weak.clone());

    if let Some(window) = window_weak.upgrade() {
        // hide() en Wayland con run_event_loop_until_quit funciona
        match window.window().hide() {
            Ok(_) => tracing::info!("[countdown] Window hidden OK"),
            Err(e) => tracing::error!("[countdown] hide() falló: {:?}", e),
        }
        // Best-effort: también pedir a Hyprland que minimice
        let _ = std::process::Command::new("hyprctl")
            .args(["dispatch", "movetoworkspacesilent", "special:minimized"])
            .output();
        crate::ipc::WINDOW_HIDDEN.store(true, Ordering::Relaxed);
    }
}

/// Tick interno: decrementa el contador; al llegar a 0 minimiza.
fn tick(window_weak: &Weak<crate::MainWindow>) {
    if !COUNTDOWN_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let Some(window) = window_weak.upgrade() else {
        COUNTDOWN_ACTIVE.store(false, Ordering::Relaxed);
        return;
    };

    let current = window.get_countdown_seconds();
    let next = current - 1;
    let progress = next as f32 / COUNTDOWN_SECONDS as f32;
    tracing::debug!("[countdown] Tick: {}s, progress: {:.2}", next, progress);
    window.set_countdown_seconds(next);
    window.set_countdown_progress(progress);

    if next <= 0 {
        COUNTDOWN_ACTIVE.store(false, Ordering::Relaxed);
        COUNTDOWN_TIMER.with(|t| {
            *t.borrow_mut() = None;
        });
        tracing::info!("[countdown] Cuenta regresiva terminada, ocultando ventana");
        match window.window().hide() {
            Ok(_) => tracing::info!("[countdown] Window hidden OK"),
            Err(e) => tracing::error!("[countdown] hide() falló: {:?}", e),
        }
        let _ = std::process::Command::new("hyprctl")
            .args(["dispatch", "movetoworkspacesilent", "special:minimized"])
            .output();
        window.set_countdown_active(false);
        window.set_countdown_seconds(COUNTDOWN_SECONDS);
        window.set_countdown_progress(1.0);
        crate::ipc::WINDOW_HIDDEN.store(true, Ordering::Relaxed);
    }
}

/// Wrapper para conectar el listener de Hyprland.
pub fn setup_countdown(window_weak: Weak<crate::MainWindow>) {
    let weak_for_lost = window_weak.clone();
    let weak_for_gained = window_weak.clone();

    crate::hypr_ipc::spawn_focus_listener(
        move || {
            tracing::info!("[countdown] Foco perdido, iniciando countdown");
            let w = weak_for_lost.clone();
            match slint::invoke_from_event_loop(move || start_countdown(w)) {
                Ok(_) => {}
                Err(e) => tracing::error!("[countdown] invoke_from_event_loop falló (lost): {:?}", e),
            }
        },
        move || {
            tracing::info!("[countdown] Foco recuperado, cancelando countdown");
            let w = weak_for_gained.clone();
            match slint::invoke_from_event_loop(move || cancel_countdown(w)) {
                Ok(_) => {}
                Err(e) => tracing::error!("[countdown] invoke_from_event_loop falló (gained): {:?}", e),
            }
        },
    );
}
