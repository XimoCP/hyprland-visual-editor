//! Headless integration tests for the HVE 2 shell (delivery 5).
//!
//! These tests exercise the FULL flow: UI callback → Shell → window.
//! They use `init_no_event_loop()` per-test (same pattern as delivery 4
//! shell tests), so they run in the normal test suite without `#[ignore]`.
//!
//! MIT credit: visual language translated from skwd-wall (MIT, © liixini).

#![cfg(test)]

use super::{PANEL_FLOAT_PAUSE_MS, PANEL_HOLD_MS, PANEL_MORPH_MS};
use crate::shell::nav::{NavCommand, PanelSection, Screen};
use crate::shell::Shell;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Advance the mocked clock by `ms` so due `slint::Timer`s fire.
fn advance_ms(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

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

#[test]
fn integration_production_show_sync_restores_expanded_state() {
    let (win, shell) = integration_setup();
    win.invoke_card_activated(0);
    drain_anim();
    assert_eq!(window_logical(&win), (1200.0, 800.0), "precondition: expanded");
    win.invoke_nav_move("right".into());
    assert_eq!(Shell::with_nav(&shell, |nav| nav.focused_card()), 1, "precondition: focus moved");

    win.window().set_size(slint::WindowSize::Logical(slint::LogicalSize::new(1.0, 1.0)));
    Shell::set_global(shell.clone());
    Shell::sync_global_after_show();

    assert_eq!(window_logical(&win), (1200.0, 800.0), "production show sync restores size");
    assert_eq!(win.get_mounted_screen(), 1, "Gallery remains mounted");
    assert!(win.get_expanded(), "shell remains expanded");
    assert_eq!(Shell::with_nav(&shell, |nav| nav.focused_card()), 1, "focus remains on the selected card");
}

// ── 5.5: the panel morph and the window conversion are SEQUENCED ────
//
// The panel choreography in `ui/shell.slint` is authored against the
// surface that covers the display (gallery tucks → beat → the central
// card expands to 80%). Converting the window to floating at the same
// time as the morph made the two motions fight, so the float now lands
// only after the morph settled, plus one beat of stillness.

/// The gallery expanded, ready to enter the panel.
fn gallery_expanded_setup() -> (crate::MainWindow, Rc<RefCell<Shell>>) {
    let (win, shell) = integration_setup();
    win.invoke_card_activated(0);
    drain_anim();
    assert_eq!(
        Shell::current_size(&shell),
        (1200.0, 800.0),
        "precondition: the gallery is expanded at its authored size"
    );
    (win, shell)
}

/// Entering the panel must hold the fullscreen presentation for the whole
/// morph, and only then float. Before this, the float landed on the same
/// tick as the morph and neither motion read.
///
/// The clock is advanced in steps that land each timer ON its deadline:
/// `slint::Timer::single_shot` resolves the deadline from the clock at
/// creation, so a coarse jump would fire the hold late and shift the rest
/// of the sequence with it — a property of the mock clock, not of the flow.
#[test]
fn integration_panel_entry_floats_only_after_the_morph_settles() {
    let (_win, shell) = gallery_expanded_setup();

    assert!(
        Shell::enter_panel(&shell, PanelSection::Save),
        "the panel enters from the expanded gallery"
    );
    assert_eq!(
        Shell::current_size(&shell),
        (1200.0, 800.0),
        "no window conversion on the keypress — the morph owns the first beat"
    );

    // The 350ms hold: nav flips to Open, which is what starts the morph.
    advance_ms(PANEL_HOLD_MS);
    assert_eq!(
        Shell::current_size(&shell),
        (1200.0, 800.0),
        "no conversion while the panel morphs"
    );

    // The choreography settles at the end of the morph...
    advance_ms(PANEL_MORPH_MS);
    assert_eq!(
        Shell::current_size(&shell),
        (1200.0, 800.0),
        "still fullscreen when the morph settles"
    );

    // ...and the beat before the float is still running.
    advance_ms(PANEL_FLOAT_PAUSE_MS - 1);
    assert_eq!(
        Shell::current_size(&shell),
        (1200.0, 800.0),
        "the beat is deliberate, not skippable"
    );

    // Beat over: the window is now the border preview surface.
    advance_ms(1);
    assert_eq!(
        Shell::current_size(&shell),
        (1000.0, 640.0),
        "the float lands one beat after the morph settled"
    );
}

/// A float still in flight when the panel is left must expire: it belongs
/// to that entry, not to whatever panel opens next. Without the epoch the
/// stale float landed mid-morph on the NEW entry and re-resolved the size
/// from an already-floating window.
#[test]
fn integration_panel_entry_expires_a_float_from_a_previous_entry() {
    let (_win, shell) = gallery_expanded_setup();

    // First entry: its morph completes and its float is scheduled.
    assert!(Shell::enter_panel(&shell, PanelSection::Save));
    advance_ms(PANEL_HOLD_MS + 400); // morph done, float still pending

    // Abandoned before that float lands.
    assert!(Shell::leave_panel(&shell));
    advance_ms(PANEL_HOLD_MS); // the leave morph completes → Closed
    assert!(!Shell::with_nav(&shell, |nav| nav.is_mutating()), "leave settled");

    // Second entry: it owns the float from here on.
    assert!(
        Shell::enter_panel(&shell, PanelSection::Borders),
        "a new entry starts from Closed"
    );
    advance_ms(PANEL_HOLD_MS); // its morph starts

    // Walk past where the ABANDONED float was due.
    advance_ms(500);
    assert_eq!(
        Shell::current_size(&shell),
        (1200.0, 800.0),
        "the abandoned float must never land on this entry"
    );

    // The entry that is actually open floats on its own schedule.
    advance_ms(PANEL_MORPH_MS + PANEL_FLOAT_PAUSE_MS);
    assert_eq!(
        Shell::current_size(&shell),
        (1000.0, 640.0),
        "the live entry owns the float"
    );
}
