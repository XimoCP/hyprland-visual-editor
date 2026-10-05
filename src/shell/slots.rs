//! Slot mounting contract for the HVE shell (nav-shell spec R3).
//!
//! `Slot` is the trait every module (gallery, workshop, ...) implements to
//! be mountable inside the single shell window. `SlotRegistry` maps each
//! `Screen` to its slot; registration is a single insert (design D3) and
//! the shell mounts/unmounts slots as `NavState` reports transitions. The
//! registry replaces the QML `Loader` of skwd-wall — Slint has no runtime
//! loader, so an `if` + mounted-screen enum drives the UI instead.
//!
//! MIT credit: visual language, geometry and tokens are translated from
//! skwd-wall (MIT, © liixini, https://github.com/liixini/skwd-wall). No GPL
//! code from hyprmod is used.

use super::nav::Screen;
use crate::tr::Tr;
use slint::SharedString;
use std::collections::HashMap;

/// A mountable shell module.
///
/// Slots are read-only views: all navigation state lives in `NavState` and
/// a slot never holds shell state of its own (nav-shell spec R1). The mount
/// and unmount hooks let a module react to being shown/hidden, and `label`
/// provides its i18n name for the chrome.
#[allow(dead_code)]
pub trait Slot: Send + Sync {
    /// The screen this slot is mounted for.
    fn screen(&self) -> Screen;
    /// Called by the shell when this slot is shown.
    fn on_mount(&self);
    /// Called by the shell when this slot is hidden.
    fn on_unmount(&self);
    /// Translated label for this slot (shell i18n keys, `tr.rs` fallback).
    fn label(&self, tr: &Tr) -> SharedString;
}

/// Registry of mountable slots keyed by screen (design D3).
///
/// `register` is a single insert — overwriting an existing slot replaces
/// it. `get` returns `None` for unregistered screens, which is what keeps
/// the shell on the current screen for unknown targets (spec R3 edge case).
pub struct SlotRegistry {
    slots: HashMap<Screen, Box<dyn Slot>>,
}

impl SlotRegistry {
    /// Empty registry with no mounted slots.
    pub fn new() -> Self {
        Self {
            slots: HashMap::new(),
        }
    }

    /// Register (or replace) a slot for its screen. One insert per call.
    pub fn register(&mut self, slot: Box<dyn Slot>) {
        let screen = slot.screen();
        self.slots.insert(screen, slot);
    }

    /// The slot registered for `screen`, or `None` when the screen has no
    /// registered slot (spec R3 "unknown target").
    pub fn get(&self, screen: Screen) -> Option<&dyn Slot> {
        self.slots.get(&screen).map(|slot| slot.as_ref())
    }
}

/// Test-only stub slot used by trunk tests (spec R3: "Trunk tests MAY
/// register stub slots") and later by the headless integration suite.
///
/// Each stub counts its mount/unmount hook invocations so tests can assert
/// that the shell actually showed and hid the slot.
///
/// Production code: registered by main.rs for Gallery and Workshop until
/// modules 2 and 4 provide real slot views.
#[cfg_attr(not(test), allow(dead_code))]
pub struct StubSlot {
    screen: Screen,
    mount_count: std::sync::atomic::AtomicU32,
    unmount_count: std::sync::atomic::AtomicU32,
}

impl StubSlot {
    /// A stub mounted for `screen` with zero hook invocations.
    pub fn new(screen: Screen) -> Self {
        Self {
            screen,
            mount_count: std::sync::atomic::AtomicU32::new(0),
            unmount_count: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// How many times the mount hook has fired on this stub.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn mount_count(&self) -> u32 {
        self.mount_count.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// How many times the unmount hook has fired on this stub.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn unmount_count(&self) -> u32 {
        self.unmount_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Slot for StubSlot {
    fn screen(&self) -> Screen {
        self.screen
    }

    fn on_mount(&self) {
        self.mount_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn on_unmount(&self) {
        self.unmount_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn label(&self, tr: &Tr) -> SharedString {
        match self.screen {
            Screen::Home => tr.tr_shared("shell.slots.home", "Home"),
            Screen::Gallery => tr.tr_shared("shell.slots.gallery", "Gallery"),
            Screen::Workshop => tr.tr_shared("shell.slots.workshop", "Workshop"),
        }
    }
}

/// Input guard for mutating window (R9). Returns true while `NavState` is in `Mutating`.
/// Stub for slots layer — real guard lives in NavState; kept here so UI layers can
/// import `slots::is_mutating` without coupling to nav internals directly.
#[allow(dead_code)]
pub fn is_mutating(nav: &super::nav::NavState) -> bool {
    nav.is_mutating()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Configurable slot double: distinct label + externally observable
    /// hook counters, so tests can tell two registrations apart and verify
    /// that hooks fire through registry dispatch.
    struct MockSlot {
        screen: Screen,
        label: &'static str,
        mounts: Arc<AtomicU32>,
        unmounts: Arc<AtomicU32>,
    }

    impl MockSlot {
        fn new(screen: Screen, label: &'static str) -> (Self, Arc<AtomicU32>, Arc<AtomicU32>) {
            let mounts = Arc::new(AtomicU32::new(0));
            let unmounts = Arc::new(AtomicU32::new(0));
            let slot = Self {
                screen,
                label,
                mounts: mounts.clone(),
                unmounts: unmounts.clone(),
            };
            (slot, mounts, unmounts)
        }
    }

    impl Slot for MockSlot {
        fn screen(&self) -> Screen {
            self.screen
        }

        fn on_mount(&self) {
            self.mounts.fetch_add(1, Ordering::SeqCst);
        }

        fn on_unmount(&self) {
            self.unmounts.fetch_add(1, Ordering::SeqCst);
        }

        fn label(&self, _tr: &Tr) -> SharedString {
            SharedString::from(self.label)
        }
    }

    // ── Registration (design D3) ────────────────────────────────────

    #[test]
    fn register_then_get_returns_the_registered_slot() {
        let mut registry = SlotRegistry::new();
        registry.register(Box::new(StubSlot::new(Screen::Gallery)));

        let slot = registry.get(Screen::Gallery).expect("registered slot must be retrievable");
        assert_eq!(slot.screen(), Screen::Gallery);
    }

    #[test]
    fn register_twice_overwrites_the_previous_slot() {
        let mut registry = SlotRegistry::new();
        let (first, _, _) = MockSlot::new(Screen::Gallery, "first");
        let (second, _, _) = MockSlot::new(Screen::Gallery, "second");
        registry.register(Box::new(first));
        registry.register(Box::new(second));

        let slot = registry.get(Screen::Gallery).expect("slot present after overwrite");
        assert_eq!(
            slot.label(&Tr::with_lang("en")).as_str(),
            "second",
            "the last registration wins"
        );
    }

    // ── Unknown target (spec R3 edge case) ──────────────────────────

    #[test]
    fn get_unregistered_screen_returns_none() {
        let registry = SlotRegistry::new();
        assert!(registry.get(Screen::Workshop).is_none(), "unknown target stays put");
        assert!(registry.get(Screen::Home).is_none(), "Home is never a registered slot");
    }

    // ── Hooks (spec R3 mount/unmount scenarios) ─────────────────────

    #[test]
    fn mount_and_unmount_hooks_fire_through_registry_dispatch() {
        let mut registry = SlotRegistry::new();
        let (slot, mounts, unmounts) = MockSlot::new(Screen::Workshop, "workshop");
        registry.register(Box::new(slot));

        let mounted = registry.get(Screen::Workshop).expect("registered slot");
        mounted.on_mount();
        mounted.on_unmount();

        assert_eq!(mounts.load(Ordering::SeqCst), 1, "on_mount fired once");
        assert_eq!(unmounts.load(Ordering::SeqCst), 1, "on_unmount fired once");
    }

    // ── StubSlot contract ───────────────────────────────────────────

    #[test]
    fn stub_slot_implements_contract_with_i18n_label_and_hook_counts() {
        let stub = StubSlot::new(Screen::Gallery);
        assert_eq!(stub.screen(), Screen::Gallery);
        assert_eq!(stub.label(&Tr::with_lang("en")).as_str(), "Gallery");

        stub.on_mount();
        stub.on_mount();
        stub.on_unmount();
        assert_eq!(stub.mount_count(), 2);
        assert_eq!(stub.unmount_count(), 1);
    }
}