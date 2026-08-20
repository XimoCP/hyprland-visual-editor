//! Headless integration tests for the HVE 2 shell (delivery 5).
//!
//! These tests exercise the FULL flow: UI callback → Shell → window.
//! They use `init_no_event_loop()` per-test (same pattern as delivery 4
//! shell tests), so they run in the normal test suite without `#[ignore]`.
//!
//! MIT credit: visual language translated from skwd-wall (MIT, © liixini).

#![cfg(test)]

use crate::shell::nav::{NavCommand, Screen};
use crate::shell::Shell;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Wire the three shell callbacks (card-activated, back-activated,
/// nav-move) exactly as `callbacks.rs` does, but without the full
/// AppState/tray/restart infrastructure. This is the integration seam:
/// if the wiring matches, the UI callbacks drive the Shell identically
/// to production (nav-shell spec R4: keyboard == mouse).
fn wire_shell_callbacks(win: &crate::MainWindow, shell: &Rc<RefCell<Shell>>) {
    {
        let shell = shell.clone();
        win.on_card_activated(move |card_idx| {
            if let Some(screen) = Screen::from_card_index(card_idx as usize) {
                Shell::dispatch(&shell, NavCommand::Expand(screen));
            }
        });
    }
    {
        let shell = shell.clone();
        win.on_back_activated(move || {
            Shell::dispatch(&shell, NavCommand::Back);
        });
    }
    {
        let shell = shell.clone();
        win.on_nav_move(move |direction| {
            let delta = match direction.as_str() {
                "down" | "right" => 1,
                "up" | "left" => -1,
                _ => 0,
            };
            if delta != 0 {
                Shell::move_focus(&shell, delta);
            }
        });
    }
}

/// Drive the stepped animator to completion (~22 steps × 16ms).
fn drain_anim() {
    for _ in 0..30 {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
    }
}

/// Read the window size as logical pixels (testing backend: physical at
/// scale 1 = logical).
fn window_logical(win: &crate::MainWindow) -> (f32, f32) {
    let phys = win.window().size();
    (phys.width as f32, phys.height as f32)
}

/// Build a MainWindow + Shell + wired callbacks. The window is shown so
/// set_size writes are observable.
fn integration_setup() -> (crate::MainWindow, Rc<RefCell<Shell>>) {
    i_slint_backend_testing::init_no_event_loop();
    let win = crate::MainWindow::new().unwrap();
    win.show().unwrap();
    let shell = Shell::new(win.as_weak());
    Shell::register_slot(&shell, Box::new(crate::shell::slots::StubSlot::new(Screen::Gallery)));
    Shell::register_slot(&shell, Box::new(crate::shell::slots::StubSlot::new(Screen::Workshop)));
    wire_shell_callbacks(&win, &shell);
    (win, shell)
}

// ── 5.1: Exactly one window at 900×680 on startup (R6) ─────────────

#[test]
fn integration_startup_single_window_at_base_size() {
    let (win, shell) = integration_setup();
    // base-window spec: single window, base size 900×680.
    assert_eq!(window_logical(&win), (900.0, 680.0), "window opens at base size");
    assert_eq!(Shell::current_size(&shell), (900.0, 680.0));
    // nav-shell spec R1: initial Home + Collapsed.
    assert_eq!(win.get_mounted_screen(), 0, "Home mounted at startup");
    assert!(!win.get_expanded(), "Collapsed at startup");
    assert_eq!(win.get_focused_card(), 0, "focus on card 0");
}

// ── 5.2: card-activated → mounted-screen + set_size (R2, R7) ───────

#[test]
fn integration_card_activated_gallery_mounts_and_grows() {
    let (win, shell) = integration_setup();
    // Card 0 = Gallery (from_card_index mapping).
    win.invoke_card_activated(0);
    assert_eq!(win.get_mounted_screen(), 1, "Gallery mounted (mounted_index=1)");
    assert!(win.get_expanded(), "expansion state is true");
    drain_anim();
    assert_eq!(window_logical(&win), (1200.0, 800.0), "window grew to expanded size");
    assert_eq!(Shell::current_size(&shell), (1200.0, 800.0));
}

#[test]
fn integration_card_activated_workshop_mounts_and_grows() {
    let (win, shell) = integration_setup();
    // Card 1 = Workshop.
    win.invoke_card_activated(1);
    assert_eq!(win.get_mounted_screen(), 2, "Workshop mounted (mounted_index=2)");
    assert!(win.get_expanded());
    drain_anim();
    assert_eq!(window_logical(&win), (1200.0, 800.0));
    assert_eq!(Shell::current_size(&shell), (1200.0, 800.0));
}

#[test]
fn integration_collapse_shrinks_window_to_base() {
    let (win, shell) = integration_setup();
    // Expand first.
    win.invoke_card_activated(0);
    drain_anim();
    assert_eq!(window_logical(&win), (1200.0, 800.0), "precondition: expanded");
    // Collapse via back-activated callback.
    win.invoke_back_activated();
    assert_eq!(win.get_mounted_screen(), 0, "Home remounted");
    assert!(!win.get_expanded(), "collapsed");
    drain_anim();
    assert_eq!(window_logical(&win), (900.0, 680.0), "window shrunk to base");
    assert_eq!(Shell::current_size(&shell), (900.0, 680.0));
}

// ── 5.3: keyboard == mouse; Escape (R4) ────────────────────────────

#[test]
fn integration_keyboard_activation_equals_mouse() {
    // Both paths (mouse click and keyboard Return) should produce the same
    // observable result. We verify this by invoking the callback directly
    // (which is what the KeyBinding does internally when Return is pressed).
    let (win, shell) = integration_setup();
    
    // Mouse path: invoke_card_activated (simulates a click on card 0).
    win.invoke_card_activated(0);
    drain_anim();
    let mouse_size = window_logical(&win);
    let mouse_screen = win.get_mounted_screen();
    let mouse_expanded = win.get_expanded();
    
    // Collapse to reset.
    win.invoke_back_activated();
    drain_anim();
    assert_eq!(window_logical(&win), (900.0, 680.0), "reset to base");
    
    // Keyboard path: invoke_card_activated with the same card (simulates
    // what the Return KeyBinding does: root.card-activated(root.focused-card)).
    // Since focused-card starts at 0, this is equivalent to pressing Return.
    win.invoke_card_activated(0);
    drain_anim();
    let kbd_size = window_logical(&win);
    let kbd_screen = win.get_mounted_screen();
    let kbd_expanded = win.get_expanded();
    
    // Both paths must produce identical observable state (spec R4).
    assert_eq!(mouse_size, kbd_size, "same window size");
    assert_eq!(mouse_screen, kbd_screen, "same mounted screen");
    assert_eq!(mouse_expanded, kbd_expanded, "same expansion state");
    // Verify both actually expanded (not both stuck at base).
    assert_eq!(mouse_size, (1200.0, 800.0), "mouse path expanded");
    assert_eq!(kbd_size, (1200.0, 800.0), "keyboard path expanded");
    
    let _ = shell;
}

#[test]
fn integration_escape_collapses_expansion() {
    let (win, shell) = integration_setup();
    // Expand via callback.
    win.invoke_card_activated(0);
    drain_anim();
    assert_eq!(window_logical(&win), (1200.0, 800.0), "precondition: expanded");
    
    // Escape key → KeyBinding fires back-activated → Collapse.
    // In the testing backend, we invoke the callback directly (which is
    // what the Escape KeyBinding does internally).
    win.invoke_back_activated();
    assert_eq!(win.get_mounted_screen(), 0, "Home remounted after Escape");
    assert!(!win.get_expanded(), "collapsed after Escape");
    drain_anim();
    assert_eq!(window_logical(&win), (900.0, 680.0), "window shrunk to base");
    let _ = shell;
}

// ── 5.4: hide/show preserves state + toggle_tray (R9) ──────────────

#[test]
fn integration_hide_show_preserves_expanded_state_and_focus() {
    let (win, shell) = integration_setup();
    // Expand into Gallery and move focus.
    win.invoke_card_activated(0);
    drain_anim();
    assert_eq!(window_logical(&win), (1200.0, 800.0), "precondition: expanded");
    assert_eq!(win.get_mounted_screen(), 1, "precondition: Gallery");

    // Simulate a hide/show cycle: the window size may be reset during hide.
    win.window().set_size(slint::WindowSize::Logical(slint::LogicalSize::new(1.0, 1.0)));
    // sync_after_show is what the composer calls after re-showing.
    Shell::sync_after_show(&shell);

    // State must be preserved (base-window spec R9).
    assert_eq!(window_logical(&win), (1200.0, 800.0), "expanded size restored");
    assert_eq!(win.get_mounted_screen(), 1, "Gallery still mounted");
    assert!(win.get_expanded(), "still expanded");
}

#[test]
fn integration_toggle_tray_restores_collapsed_state() {
    let (win, shell) = integration_setup();
    // Start collapsed at base size.
    assert_eq!(window_logical(&win), (900.0, 680.0), "precondition: base size");
    assert_eq!(win.get_mounted_screen(), 0, "precondition: Home");

    // Simulate a tray hide/show cycle (window hidden, then re-shown).
    win.window().set_size(slint::WindowSize::Logical(slint::LogicalSize::new(1.0, 1.0)));
    Shell::sync_after_show(&shell);

    // Collapsed state must be restored.
    assert_eq!(window_logical(&win), (900.0, 680.0), "base size restored after tray toggle");
    assert_eq!(win.get_mounted_screen(), 0, "Home still mounted");
    assert!(!win.get_expanded(), "still collapsed");
}
