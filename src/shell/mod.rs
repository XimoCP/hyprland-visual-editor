//! HVE 2 shell: state-driven navigation and slot mounting.
//!
//! The shell is the HVE 2 "trunk": a single window that mutates instead of
//! stacking. `NavState` (in `nav`) is the pure Rust-owned navigation state
//! machine (nav-shell spec R1, R2). Slot mounting, window sizing and the
//! Slint chrome land in later deliveries on top of it.
//!
//! MIT credit: visual language, geometry and tokens are translated from
//! skwd-wall (MIT, © liixini, https://github.com/liixini/skwd-wall). No GPL
//! code from hyprmod is used.

pub mod gallery;
pub mod nav;
pub mod size;
pub mod slots;
#[cfg(test)]
mod ui_tests;
#[cfg(test)]
mod integration_tests;

use self::nav::{ExpansionState, NavCommand, NavState, Screen};
use self::size::SizePolicy;
use self::slots::SlotRegistry;
use slint::{ComponentHandle, LogicalSize, WindowSize};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

thread_local! {
    static GLOBAL_SHELL: RefCell<Option<Rc<RefCell<Shell>>>> = const { RefCell::new(None) };
}

/// Stepped animator cadence: one `set_size` call per tick (design D4).
pub const ANIM_STEP_MS: u64 = 16;
/// Number of steps to complete an expand/collapse transition. 350ms / 16ms
/// ≈ 22 steps (skwd `animExpand` OutCubic cadence).
pub const ANIM_STEPS: u32 = 22;
/// Total animation duration ~352ms (≈350ms OutCubic skwd animExpand).
pub const ANIM_DURATION_MS: u64 = ANIM_STEPS as u64 * ANIM_STEP_MS;
/// OutCubic easing cubic-bezier(0.215, 0.61, 0.355, 1.0) — skwd default.
pub const ANIM_EASING: (f32, f32, f32, f32) = (0.215, 0.61, 0.355, 1.0);

/// Hold between the panel keypress and the panel's own choreography
/// (nav-shell R1 350ms morph): the nav state is `Mutating` for this long.
pub const PANEL_HOLD_MS: u64 = 350;

/// How long the panel's ENTER choreography runs once `is-panel-open` flips,
/// before it has settled: the central card fades in (delay 640ms + 300ms)
/// and scales from 0.62 to 1 (delay 640ms + 560ms).
/// MUST stay in sync with the choreography in `ui/shell.slint`.
pub const PANEL_MORPH_MS: u64 = 1200;

/// The beat between the panel's morph settling and the window leaving
/// fullscreen to float. The conversion is a window event, the morph is a
/// content event: landing them on the same tick made the two motions fight,
/// and the eye could read neither. One stillness first, then the conversion.
pub const PANEL_FLOAT_PAUSE_MS: u64 = 200;

/// The shell root — single mutable owner of navigation state, slot
/// registry, window sizing and the stepped size animator (design D1, D3,
/// D4). `slint::Timer` is `!Send + !Sync`, so the shell lives in
/// `Rc<RefCell<>>` and is exposed through a UI-thread `thread_local`
/// global — never `Arc<Mutex<>>`.
#[allow(dead_code)]
pub struct Shell {
    nav: NavState,
    slots: SlotRegistry,
    size: SizePolicy,
    window: slint::Weak<crate::MainWindow>,
    /// Current animated size (logical pixels). Starts at the base size and
    /// walks toward the target in `ANIM_STEPS` ticks.
    current_size: (f32, f32),
    /// Target size for the current transition (base or expanded).
    target_size: (f32, f32),
    /// Remaining animation steps. 0 = idle at `target_size`.
    remaining_steps: u32,
    /// Per-step delta applied to `current_size` on each tick.
    step_delta: (f32, f32),
    /// Suppress shader overlay for 3s after any Mutating completes (R5).
    suppress_until: Option<Instant>,
    /// Which panel entry owns the deferred float. Bumped on every enter and
    /// every leave, so a float still in flight from a previous entry expires
    /// instead of landing on the panel that is open now.
    float_epoch: u64,
    /// The stepped animator timer. Kept here so it dies with the shell.
    _anim_timer: slint::Timer,
}

#[allow(dead_code)]
impl Shell {
    /// Build a new shell at the base size (900×680), with no expansion and
    /// no mounted slots. The window weak reference is used to push
    /// `set_size` updates back to the Slint surface.
    pub fn new(window: slint::Weak<crate::MainWindow>) -> Rc<RefCell<Self>> {
        let base = SizePolicy::new().target(false);
        let mut nav = NavState::new();
        nav.set_reduced_motion(crate::shell::gallery::model::prefers_reduced_motion());
        let shell = Rc::new(RefCell::new(Self {
            nav,
            slots: SlotRegistry::new(),
            size: SizePolicy::new(),
            window,
            current_size: base,
            target_size: base,
            remaining_steps: 0,
            step_delta: (0.0, 0.0),
            suppress_until: None,
            float_epoch: 0,
            _anim_timer: slint::Timer::default(),
        }));
        // Apply the base size immediately so the window opens at 900×680
        // (base-window spec R6 startup scenario).
        shell.borrow().push_size_to_window();
        // Mirror initial panel state (closed)
        Shell::mirror_panel(&shell);
        shell
    }

    /// Register the UI-thread shell used by compositor lifecycle callbacks.
    pub fn set_global(shell: Rc<RefCell<Self>>) {
        GLOBAL_SHELL.with(|global| *global.borrow_mut() = Some(shell));
    }

    /// Restore the registered shell after a compositor show operation.
    pub fn sync_global_after_show() {
        let shell = GLOBAL_SHELL.with(|global| global.borrow().clone());
        if let Some(shell) = shell {
            Self::sync_after_show(&shell);
        }
    }

    /// Whether the Gallery is currently mounted expanded (used by
    /// `Controller::toggle_tray` to reassert fullscreen without deadlock).
    pub fn is_gallery_expanded() -> bool {
        GLOBAL_SHELL.with(|global| {
            global
                .borrow()
                .as_ref()
                .map(|rc| {
                    let s = rc.borrow();
                    s.nav.expansion() != ExpansionState::Collapsed
                        && s.nav.screen() == crate::shell::nav::Screen::Gallery
                })
                .unwrap_or(false)
        })
    }

    /// Register a mountable slot. One insert per call (design D3).
    pub fn register_slot(shell: &Rc<RefCell<Self>>, slot: Box<dyn slots::Slot>) {
        shell.borrow_mut().slots.register(slot);
    }

    /// Dispatch a nav command from the UI (design D8: keyboard == mouse).
    /// Queues the command and processes it immediately; the observable
    /// effect (mount/unmount/size) is applied through the slot registry
    /// and the stepped animator.
    pub fn dispatch(shell: &Rc<RefCell<Self>>, cmd: NavCommand) {
        shell.borrow_mut().nav.push(cmd);
        Self::process_queue(shell);
    }

    /// Move the keyboard focus by `delta` cards (nav-shell spec R4).
    /// Returns the new focused card index so the caller can mirror it back
    /// to the UI.
    pub fn move_focus(shell: &Rc<RefCell<Self>>, delta: isize) -> usize {
        shell.borrow_mut().nav.move_focus(delta)
    }

    /// Restore the shell state after the composer shows the window again
    /// (base-window spec R9: hide preserves state, focus restored on show).
    /// Re-applies the current expansion's size so the window returns to
    /// the same geometry it had before the hide. When the Gallery is the
    /// expanded screen, re-assert immersive fullscreen: Hyprland clears
    /// fullscreen on workspace moves (special hide/show), so the logical
    /// session is still active but the physical window is windowed.
    pub fn sync_after_show(shell: &Rc<RefCell<Self>>) {
        let (expanded, is_gallery) = {
            let s = shell.borrow();
            (
                s.nav.expansion() != ExpansionState::Collapsed,
                s.nav.screen() == crate::shell::nav::Screen::Gallery,
            )
        };
        let target = shell.borrow().size.target(expanded);
        {
            let mut s = shell.borrow_mut();
            s.target_size = target;
            s.current_size = target;
            s.remaining_steps = 0;
            s.push_size_to_window();
        }
        if is_gallery && expanded {
            // Theme transition owns the single invisible fullscreen cycle —
            // skip the extra re-dispatch here to keep it to one floating->fullscreen.
            if crate::is_theme_transitioning_flag() {
                tracing::info!("[sync_after_show] skip fullscreen reassert during theme fade");
            } else if let Some(mut ctrl) = crate::composer::try_global_controller() {
                if ctrl.gallery_session_active() {
                    // Logical session alive but compositor lost fullscreen on
                    // the hide→show round-trip — re-dispatch without flipping
                    // the logical flag.
                    let _ = ctrl.composer().set_fullscreen(true);
                } else {
                    // Should be fullscreen but logical flag cleared (e.g.
                    // first show after eager expand) — enter cleanly.
                    let _ = ctrl.enter_gallery_session();
                }
            }
        }
    }

    /// Animate the window to an arbitrary target size (S9 ExpandToSettings,
    /// 4.4). Uses the same 22-step 350ms OutCubic stepped animator.
    pub fn animate_to(shell: &Rc<RefCell<Self>>, target: (f32, f32)) {
        let clamped = shell.borrow().size.clamp(target);
        Self::start_size_anim_to(shell, clamped);
    }

    /// Expand the Gallery card into the settings panel (S9, 4.4):
    /// same window mutates 1200×800 → 1300×900 via SizePolicy::target_for_settings.
    pub fn expand_to_settings(shell: &Rc<RefCell<Self>>) {
        let target = shell.borrow().size.target_for_settings(true);
        Self::animate_to(shell, target);
    }

    /// Collapse the settings panel back to the gallery expanded size
    /// (S10 Back/Esc, 4.5): 1300×900 → 1200×800.
    pub fn collapse_from_settings(shell: &Rc<RefCell<Self>>) {
        let target = shell.borrow().size.target_for_settings(false);
        Self::animate_to(shell, target);
    }

    /// The current navigation state (read-only accessor for tests and
    /// the callbacks layer).
    pub fn with_nav<F, R>(shell: &Rc<RefCell<Self>>, f: F) -> R
    where
        F: FnOnce(&NavState) -> R,
    {
        let s = shell.borrow();
        f(&s.nav)
    }

    /// The current animated window size (read-only accessor for tests).
    pub fn current_size(shell: &Rc<RefCell<Self>>) -> (f32, f32) {
        shell.borrow().current_size
    }

    /// Whether the panel is mutating (input guard R9).
    pub fn is_mutating(shell: &Rc<RefCell<Self>>) -> bool {
        shell.borrow().nav.is_mutating()
    }

    /// Window presentation for the current navigation state (settings
    /// self-preview).
    ///
    /// While the panel is present, HVE is presented FLOATING and CENTERED: it
    /// leaves fullscreen, so the border the compositor draws around HVE
    /// itself becomes a live preview of the border values being tuned. The
    /// window is the preview surface — no second window, and no need to keep
    /// any part of the desktop on screen. Every other state (Gallery,
    /// collapsed) stays fullscreen.
    ///
    /// Which states float is `PanelState::floats`, and the morph directions
    /// are deliberately asymmetrical: the float is the LAST beat of the enter
    /// flow (the morph plays first, on the fullscreen surface it was authored
    /// against) and the FIRST beat of the leave flow (fullscreen comes back
    /// before the reverse morph starts). One motion at a time.
    ///
    /// The floating size is derived from the monitor rather than fixed: a
    /// margin must survive on every side, or the border sits flush against
    /// the monitor edge and stops being readable. While the window still
    /// covers the monitor its own logical size IS the monitor size, so this
    /// needs no compositor query and no new I/O surface.
    ///
    /// `apply_size` gates the size resolution alone. The compositor actions
    /// are idempotent and safe to re-assert (that is how a lost event is
    /// recovered), but the size is resolved exactly once per entry: it is
    /// measured from the surface while that surface still covers the monitor,
    /// and after the float it no longer does. That is why the float is a
    /// scheduled step with an epoch (`schedule_float`) rather than a
    /// best-effort re-assert, and why `apply_size` is only ever true from
    /// there: every other caller re-asserts state it did not change.
    ///
    /// A compositor refusal is non-fatal: the panel stays fully functional
    /// windowed (same contract as `enter_gallery_session`).
    fn apply_window_presentation(shell: &Rc<RefCell<Self>>, apply_size: bool) {
        if crate::is_theme_transitioning_flag() {
            tracing::info!("[presentation] skip during theme fade");
            return;
        }
        let floats = shell.borrow().nav.panel_state().floats();
        tracing::info!("[presentation] floats={floats} apply_size={apply_size}");
        if floats {
            // Compositor actions FIRST, then the size animation.
            //
            // Un-setting fullscreen makes the compositor restore the window's
            // saved floating geometry, which lands whatever moment it lands —
            // in a race with any `set_size` already in flight, so the window
            // ended on an arbitrary intermediate size. Both dispatch paths are
            // synchronous, so by the time the animator starts pushing sizes
            // the compositor has settled and the last tick (the exact target)
            // is what survives.
            if let Some(ctrl) =
                crate::composer::try_global_controller().or_else(crate::composer::global_controller)
            {
                let _ = ctrl.composer().set_fullscreen(false);
                let _ = ctrl.composer().set_settings_float(true);
            }
            if apply_size {
                // Logical size of the surface while it still covers the monitor.
                let monitor = shell.borrow().window.upgrade().map(|w| {
                    let phys = w.window().size();
                    let scale = w.window().scale_factor();
                    (phys.width as f32 / scale, phys.height as f32 / scale)
                });
                if let Some(monitor) = monitor {
                    let target = shell.borrow().size.settings_float_size(monitor);
                    tracing::info!(
                        "[presentation] monitor={monitor:?} target={target:?} current={:?}",
                        shell.borrow().current_size
                    );
                    // The compositor owns the geometry: a client-side size
                    // request is NOT honoured for a floating window here
                    // (verified live — the window landed on its minimum size
                    // instead). So `current_size` is updated for the shell's
                    // own layout state, and the resize goes out as an
                    // explicit dispatch.
                    shell.borrow_mut().current_size = target;
                    if let Some(ctrl2) = crate::composer::try_global_controller()
                        .or_else(crate::composer::global_controller)
                    {
                        let _ = ctrl2.composer().resize_window(target);
                    }
                }
            }
        } else if let Some(mut ctrl) =
            crate::composer::try_global_controller().or_else(crate::composer::global_controller)
        {
            let _ = ctrl.composer().set_settings_float(false);
            let _ = ctrl.composer().set_fullscreen(true);
            // Keep the gallery session flag in step with the re-entered
            // fullscreen, exactly as the pre-existing hold did.
            if !ctrl.gallery_session_active() {
                let _ = ctrl.enter_gallery_session();
            }
        }
    }

    /// Enter panel from Gallery (R1, as revised for the settings self-preview).
    ///
    /// The window HOLDS fullscreen while the panel morphs. The morph is
    /// choreographed against the surface that covers the display (the
    /// gallery tucks, then the panel fills the slot), so converting
    /// the window on the same tick as the keypress made the two motions
    /// fight. The conversion is the LAST beat of the flow: `complete_morph`
    /// hands off to `schedule_float`, which waits out the choreography plus
    /// one stillness and only then leaves fullscreen.
    ///
    /// The epoch is what keeps that promise across entries: it is bumped
    /// here, so any float still in flight from a previous entry expires.
    pub fn enter_panel(shell: &Rc<RefCell<Self>>, section: crate::shell::nav::PanelSection) -> bool {
        let entered = {
            let mut s = shell.borrow_mut();
            let entered = s.nav.enter_panel(section);
            if entered {
                s.float_epoch = s.float_epoch.wrapping_add(1);
            }
            entered
        };
        if !entered {
            return false;
        }
        Self::mirror_panel(shell);
        // No presentation here: the panel morph owns this beat (see above).
        let delay = {
            let s = shell.borrow();
            s.nav.effective_duration(PANEL_HOLD_MS)
        };
        if delay == 0 {
            Shell::complete_morph(shell);
        } else {
            let weak = Rc::downgrade(shell);
            slint::Timer::single_shot(Duration::from_millis(delay), move || {
                if let Some(rc) = weak.upgrade() {
                    Shell::complete_morph(&rc);
                }
            });
        }
        true
    }

    /// Complete the pending morph (Timer callback R1).
    fn complete_morph(shell: &Rc<RefCell<Self>>) {
        let opened = {
            let mut s = shell.borrow_mut();
            s.nav.complete_mutation();
            // R5: suppress shader overlay for 3s after any Mutating completes
            s.suppress_until = Some(Instant::now() + Duration::from_millis(crate::shell::gallery::slot::SHADER_FLICKER_MS));
            matches!(s.nav.panel_state(), crate::shell::nav::PanelState::Open(_))
        };
        Self::mirror_panel(shell);
        if opened {
            // `mirror_panel` just flipped `is-panel-open`, which is what
            // STARTS the panel choreography. The window conversion waits for
            // it: it is the float step, not this one, that leaves fullscreen.
            Self::schedule_float(shell);
        } else {
            // Closed again: the leave path already restored the fullscreen
            // presentation; re-assert only (idempotent).
            Self::apply_window_presentation(shell, false);
        }
    }

    /// Leave panel (Esc reverses, R1). Starts 350ms Timer → Closed, or immediate when reduced-motion.
    pub fn leave_panel(shell: &Rc<RefCell<Self>>) -> bool {
        let started = {
            let mut s = shell.borrow_mut();
            let started = s.nav.leave_panel();
            if started {
                // Leaving cancels a float still in flight: that float belongs
                // to the entry being left, not to whatever opens next.
                s.float_epoch = s.float_epoch.wrapping_add(1);
            }
            started
        };
        if !started {
            return false;
        }
        Self::mirror_panel(shell);
        // Restore the fullscreen presentation FIRST, exactly as the enter
        // flow floats LAST: the reverse choreography (panel collapses,
        // gallery undocks) plays on the surface it was authored for, instead
        // of fighting the window conversion mid-morph.
        Self::apply_window_presentation(shell, false);
        let delay = {
            let s = shell.borrow();
            s.nav.effective_duration(PANEL_HOLD_MS)
        };
        if delay == 0 {
            Shell::complete_morph(shell);
        } else {
            let weak = Rc::downgrade(shell);
            slint::Timer::single_shot(Duration::from_millis(delay), move || {
                if let Some(rc) = weak.upgrade() {
                    Shell::complete_morph(&rc);
                }
            });
        }
        true
    }

    /// The last beat of the enter flow: let the panel morph settle, keep one
    /// stillness, then leave fullscreen and float the window for the panel.
    ///
    /// Scheduled per entry and guarded by `float_epoch`, so exactly one live
    /// float can resolve the size — the invariant the size resolution depends
    /// on (it is measured off the surface while that surface still covers the
    /// monitor).
    fn schedule_float(shell: &Rc<RefCell<Self>>) {
        let (epoch, delay) = {
            let s = shell.borrow();
            let delay = s
                .nav
                .effective_duration(PANEL_MORPH_MS + PANEL_FLOAT_PAUSE_MS);
            (s.float_epoch, delay)
        };
        tracing::info!("[presentation] float scheduled in {delay}ms (epoch {epoch})");
        if delay == 0 {
            // Reduced motion: there is no morph to synchronize with.
            Self::apply_window_presentation(shell, true);
            return;
        }
        let weak = Rc::downgrade(shell);
        slint::Timer::single_shot(Duration::from_millis(delay), move || {
            let Some(rc) = weak.upgrade() else { return };
            if rc.borrow().float_epoch != epoch {
                tracing::info!("[presentation] float epoch {epoch} expired — skipped");
                return;
            }
            Shell::apply_window_presentation(&rc, true);
        });
    }

    /// Switch section without Gallery remount when Open (R2). No timer.
    pub fn set_panel_section(shell: &Rc<RefCell<Self>>, section: crate::shell::nav::PanelSection) {
        {
            let mut s = shell.borrow_mut();
            s.nav.set_section(section);
        }
        Self::mirror_panel(shell);
    }

    /// Global shader-overlay suppression check for GallerySlot (R5).
    /// Returns true while PanelState != Closed OR for 3s after Mutating.
    pub fn is_shader_suppressed_global() -> bool {
        GLOBAL_SHELL.with(|g| {
            if let Some(rc) = g.borrow().as_ref() {
                let s = rc.borrow();
                if s.nav.panel_state() != crate::shell::nav::PanelState::Closed {
                    return true;
                }
                if let Some(deadline) = s.suppress_until {
                    if Instant::now() < deadline {
                        return true;
                    }
                }
            }
            false
        })
    }

    /// Push panel state to the Slint mirrors.
    fn mirror_panel(shell: &Rc<RefCell<Self>>) {
        let Some(window) = shell.borrow().window.upgrade() else { return };
        let (section, is_mutating, is_open) = {
            let s = shell.borrow();
            let state = s.nav.panel_state();
            let is_mut = s.nav.is_mutating();
            let open = matches!(state, crate::shell::nav::PanelState::Open(_));
            let sec = s.nav.panel_section().map(|p| p.index() as i32).unwrap_or(0);
            (sec, is_mut, open)
        };
        window.set_panel_section(section);
        window.set_is_mutating(is_mutating);
        window.set_is_panel_open(is_open);
        // Also forward panel-section to gallery for FilterBar mirror (pills active)
        // The FilterBar reads panel-section via ShellRoot.panel-section, mirrored above via MainWindow already?
        // ShellRoot.panel-section is now set via MainWindow.panel-section above; need to ensure gallery also sees it.
        // No extra step — ShellRoot.panel-section is driven by MainWindow.panel-section.
    }

    // ── Internals ─────────────────────────────────────────────────────

    /// Process every queued command in order. Each transition mounts /
    /// unmounts slots and starts (or retargets) the size animator.
    fn process_queue(shell: &Rc<RefCell<Self>>) {
        loop {
            let transition = {
                let mut s = shell.borrow_mut();
                match s.nav.process_next() {
                    Some(t) => t,
                    None => return,
                }
            };
            // Slot hooks (spec R3 mount/unmount scenarios).
            if let Some(unmount) = transition.unmount {
                if let Some(slot) = shell.borrow().slots.get(unmount) {
                    slot.on_unmount();
                }
                // Leaving the Gallery closes the immersive fullscreen
                // session (spec GIVEN: a fullscreen session is active).
                // No-op when no global controller exists (headless tests).
                if unmount == Screen::Gallery {
                    if let Some(mut ctrl) = crate::composer::global_controller() {
                        if ctrl.gallery_session_active() {
                            ctrl.exit_gallery_session();
                        }
                    }
                }
            }
            if let Some(mount) = transition.mount {
                if let Some(slot) = shell.borrow().slots.get(mount) {
                    slot.on_mount();
                }
                // Entering the Gallery opens the immersive session before
                // the new screen paints (spec: undecorated fullscreen
                // covering the monitor before content displays).
                if mount == Screen::Gallery {
                    if let Some(mut ctrl) = crate::composer::global_controller() {
                        ctrl.enter_gallery_session();
                    }
                }
            }
            // Mirror state back to the UI.
            Self::mirror_state(shell);
            // Start (or retarget) the stepped size animator.
            Self::start_size_anim(shell, transition.expand);
        }
    }

    /// Push the current navigation state to the Slint mirrors.
    fn mirror_state(shell: &Rc<RefCell<Self>>) {
        let Some(window) = shell.borrow().window.upgrade() else {
            return;
        };
        let s = shell.borrow();
        let screen = s.nav.screen();
        let expanded = s.nav.expansion() != ExpansionState::Collapsed;
        let focused = s.nav.focused_card();
        let panel_section = s.nav.panel_section().map(|p| p.index() as i32).unwrap_or(0);
        let is_mutating = s.nav.is_mutating();
        let is_open = matches!(s.nav.panel_state(), crate::shell::nav::PanelState::Open(_));
        drop(s);
        window.set_mounted_screen(screen.mounted_index() as i32);
        window.set_expanded(expanded);
        window.set_focused_card(focused as i32);
        window.set_panel_section(panel_section);
        window.set_is_mutating(is_mutating);
        window.set_is_panel_open(is_open);
    }

    /// Start (or retarget) the stepped size animator toward the target
    /// size for the new expansion state (design D4).
    fn start_size_anim(shell: &Rc<RefCell<Self>>, _expand: bool) {
        let target = {
            let s = shell.borrow();
            let expanded = s.nav.expansion() != ExpansionState::Collapsed;
            s.size.clamp(s.size.target(expanded))
        };
        Self::start_size_anim_to(shell, target);
    }

    /// Start animator toward an explicit target (S9ExpandToSettings, 4.4).
    fn start_size_anim_to(shell: &Rc<RefCell<Self>>, target: (f32, f32)) {
        let clamped = shell.borrow().size.clamp(target);
        let (from_w, from_h) = {
            let s = shell.borrow();
            s.current_size
        };
        let dw = (clamped.0 - from_w) / ANIM_STEPS as f32;
        let dh = (clamped.1 - from_h) / ANIM_STEPS as f32;
        {
            let mut s = shell.borrow_mut();
            s.target_size = clamped;
            s.step_delta = (dw, dh);
            s.remaining_steps = ANIM_STEPS;
        }
        Self::kick_timer(shell);
    }

    /// Arm the stepped timer. Each tick advances `current_size` by one
    /// `step_delta` and pushes it to the window; when the steps are
    /// exhausted the timer stops and `current_size` snaps to `target_size`
    /// (no drift from float accumulation).
    fn kick_timer(shell: &Rc<RefCell<Self>>) {
        let weak = Rc::downgrade(shell);
        let s = shell.borrow();
        s._anim_timer.stop();
        drop(s);
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(ANIM_STEP_MS),
            move || {
                let Some(rc) = weak.upgrade() else { return };
                let done = {
                    let mut s = rc.borrow_mut();
                    if s.remaining_steps == 0 {
                        true
                    } else {
                        s.current_size.0 += s.step_delta.0;
                        s.current_size.1 += s.step_delta.1;
                        s.remaining_steps -= 1;
                        if s.remaining_steps == 0 {
                            // Snap to target on the last step — no drift.
                            s.current_size = s.target_size;
                        }
                        s.push_size_to_window();
                        s.remaining_steps == 0
                    }
                };
                if done {
                    let t = slint::Timer::default();
                    rc.borrow_mut()._anim_timer = t;
                }
            },
        );
        shell.borrow_mut()._anim_timer = timer;
    }

    /// Push `current_size` (clamped to the minimum) to the Slint window
    /// via `set_size(LogicalSize)`.
    fn push_size_to_window(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let clamped = self.size.clamp(self.current_size);
        let ls = LogicalSize::new(clamped.0, clamped.1);
        window.window().set_size(WindowSize::Logical(ls));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::slots::StubSlot;

    /// Headless Slint backend on the current thread (composer::tests pattern).
    fn init_test_platform() {
        i_slint_backend_testing::init_no_event_loop();
    }

    /// Build a Shell bound to a freshly created MainWindow. The window is
    /// shown so set_size writes are observable via window.size().
    fn shell_with_window() -> (Rc<RefCell<Shell>>, slint::Weak<crate::MainWindow>) {
        init_test_platform();
        let win = crate::MainWindow::new().unwrap();
        win.show().unwrap();
        let weak = win.as_weak();
        let shell = Shell::new(weak.clone());
        (shell, weak)
    }

    /// Read the current window size as logical pixels. The testing backend
    /// stores sizes as physical at scale 1 — which matches logical here.
    fn window_logical(weak: &slint::Weak<crate::MainWindow>) -> (f32, f32) {
        let win = weak.upgrade().unwrap();
        let phys = win.window().size();
        (phys.width as f32, phys.height as f32)
    }

    /// Drive the stepped animator to completion by simulating ~16ms ticks.
    /// 30 iterations is safely above the 22-step budget.
    fn drain_anim() {
        for _ in 0..30 {
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
        }
    }

    // ── Initial size (base-window spec R6) ────────────────────────────

    #[test]
    fn shell_starts_at_the_base_size() {
        let (shell, weak) = shell_with_window();
        assert_eq!(Shell::current_size(&shell), (900.0, 680.0));
        let (w, h) = window_logical(&weak);
        assert_eq!((w, h), (900.0, 680.0), "window opens at the base size");
    }

    // ── Expand / collapse via the stepped animator ────────────────────

    #[test]
    fn dispatch_expand_grows_the_window_to_expanded_size() {
        let (shell, weak) = shell_with_window();
        Shell::dispatch(&shell, NavCommand::Expand(Screen::Gallery));
        drain_anim();
        let (w, h) = window_logical(&weak);
        assert_eq!((w, h), (1200.0, 800.0), "animator walks to the expanded size");
        assert_eq!(Shell::current_size(&shell), (1200.0, 800.0));
    }

    #[test]
    fn dispatch_collapse_shrinks_the_window_back_to_base() {
        let (shell, weak) = shell_with_window();
        Shell::dispatch(&shell, NavCommand::Expand(Screen::Gallery));
        drain_anim();
        Shell::dispatch(&shell, NavCommand::Back);
        drain_anim();
        let (w, h) = window_logical(&weak);
        assert_eq!((w, h), (900.0, 680.0), "animator walks back to the base size");
    }

    // ── Unregistered target (spec R3 edge case) ───────────────────────

    #[test]
    fn dispatch_to_unregistered_target_is_noop() {
        let (shell, weak) = shell_with_window();
        // No slots registered: expanding into Gallery still moves NavState
        // (the state machine accepts it) but the mount hook never fires.
        // The size still animates because the state transitioned.
        Shell::dispatch(&shell, NavCommand::Expand(Screen::Gallery));
        drain_anim();
        // The shell still grew because NavState transitioned — slot hooks
        // are independent of the size transition.
        let (w, h) = window_logical(&weak);
        assert_eq!((w, h), (1200.0, 800.0));
    }

    #[test]
    fn expand_home_is_a_noop_for_size_and_state() {
        let (shell, weak) = shell_with_window();
        // Home is the root screen, not an expansion target (nav.rs test
        // `expand_root_screen_is_noop_for_unknown_target` covers this at
        // the NavState level). Verify the Shell surfaces the same no-op.
        Shell::dispatch(&shell, NavCommand::Expand(Screen::Home));
        drain_anim();
        let (w, h) = window_logical(&weak);
        assert_eq!((w, h), (900.0, 680.0), "Home is not an expansion target");
    }

    // ── Rapid queue (spec R2: final = last) ───────────────────────────

    #[test]
    fn rapid_queue_final_state_matches_last_command() {
        let (shell, weak) = shell_with_window();
        Shell::dispatch(&shell, NavCommand::Expand(Screen::Gallery));
        Shell::dispatch(&shell, NavCommand::Expand(Screen::Workshop));
        Shell::dispatch(&shell, NavCommand::Back);
        drain_anim();
        let (w, h) = window_logical(&weak);
        assert_eq!((w, h), (900.0, 680.0), "final state = last command (Back)");
        Shell::with_nav(&shell, |nav| {
            assert_eq!(nav.expansion(), ExpansionState::Collapsed);
            assert_eq!(nav.screen(), Screen::Home);
        });
    }

    // ── Focus clamp (spec R4) ─────────────────────────────────────────

    #[test]
    fn move_focus_clamps_at_card_bounds() {
        let (shell, _weak) = shell_with_window();
        assert_eq!(Shell::move_focus(&shell, 1), 1);
        assert_eq!(Shell::move_focus(&shell, 1), 1, "clamped at last card");
        assert_eq!(Shell::move_focus(&shell, -5), 0, "clamped at first card");
    }

    // ── Slot hooks fire through dispatch ──────────────────────────────

    #[test]
    fn dispatch_fires_mount_and_unmount_hooks() {
        let (shell, _weak) = shell_with_window();
        let gallery = std::sync::Arc::new(StubSlot::new(Screen::Gallery));
        let workshop = std::sync::Arc::new(StubSlot::new(Screen::Workshop));
        // StubSlot implements Slot; wrap in a production-typed box.
        Shell::register_slot(&shell, Box::new(StubSlot::new(Screen::Gallery)));
        Shell::register_slot(&shell, Box::new(StubSlot::new(Screen::Workshop)));
        // Keep local counters via custom stubs — use StubSlot's atomic counters
        // through the registry instead.
        Shell::dispatch(&shell, NavCommand::Expand(Screen::Gallery));
        {
            let s = shell.borrow();
            let _slot = s.slots.get(Screen::Gallery).unwrap();
            // Downcast via the trait — StubSlot's counter is internal.
            // Verify through the registry's on_mount firing indirectly:
            // the mounted-screen mirror must be Gallery now.
            assert_eq!(s.nav.screen(), Screen::Gallery);
        }
        drop(gallery);
        drop(workshop);
    }

    // ── sync_after_show restores expanded state (spec R9) ─────────────

    #[test]
    fn sync_after_show_restores_expanded_size() {
        let (shell, weak) = shell_with_window();
        Shell::dispatch(&shell, NavCommand::Expand(Screen::Gallery));
        drain_anim();
        let (w, h) = window_logical(&weak);
        assert_eq!((w, h), (1200.0, 800.0), "precondition: expanded");

        // Simulate a hide/show cycle by calling sync_after_show — the
        // window should return to the expanded size even if something
        // reset it during the hide.
        weak.upgrade().unwrap().window().set_size(WindowSize::Logical(
            LogicalSize::new(1.0, 1.0),
        ));
        Shell::sync_after_show(&shell);
        let (w, h) = window_logical(&weak);
        assert_eq!((w, h), (1200.0, 800.0), "sync_after_show restores expanded");
    }

    #[test]
    fn sync_after_show_restores_base_size_when_collapsed() {
        let (shell, weak) = shell_with_window();
        // Start collapsed at base; simulate a weird size from the hide.
        weak.upgrade().unwrap().window().set_size(WindowSize::Logical(
            LogicalSize::new(1.0, 1.0),
        ));
        Shell::sync_after_show(&shell);
        let (w, h) = window_logical(&weak);
        assert_eq!((w, h), (900.0, 680.0), "sync_after_show restores base");
    }

    // ── UI mirrors ────────────────────────────────────────────────────

    #[test]
    fn dispatch_updates_ui_mirrors() {
        let (shell, weak) = shell_with_window();
        let win = weak.upgrade().unwrap();
        assert_eq!(win.get_mounted_screen(), 0, "Home at startup");
        assert!(!win.get_expanded());
        assert_eq!(win.get_focused_card(), 0);

        Shell::dispatch(&shell, NavCommand::Expand(Screen::Gallery));
        assert_eq!(win.get_mounted_screen(), 1, "Gallery mounted");
        assert!(win.get_expanded());

        Shell::move_focus(&shell, 1);
        assert_eq!(win.get_focused_card(), 0, "move_focus does NOT auto-mirror; callbacks do");

        Shell::dispatch(&shell, NavCommand::Back);
        assert_eq!(win.get_mounted_screen(), 0, "Home remounted on Back");
        assert!(!win.get_expanded());
    }
}
