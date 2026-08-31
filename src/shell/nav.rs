//! Pure navigation state machine for the HVE 2 shell.
//!
//! `NavState` owns ALL shell navigation state (nav-shell spec R1): the
//! current screen, the expansion state, the FIFO command queue and the
//! focused card index. The UI is a read-only view of this state — it never
//! holds navigation state of its own. The module is intentionally free of
//! Slint types so it stays unit-testable headless (R2: deterministic
//! transitions).
//!
//! MIT credit: visual language, geometry and tokens are translated from
//! skwd-wall (MIT, © liixini, https://github.com/liixini/skwd-wall). No GPL
//! code from hyprmod is used.

use std::collections::VecDeque;

/// Number of expandable cards on the Home screen (Gallery, Workshop).
/// Home itself is the root screen and cannot be an expansion target.
const CARD_COUNT: usize = 2;

/// The three shell screens. `Home` is the root screen that hosts the
/// expandable cards; `Gallery` and `Workshop` are expansion targets.
/// `Hash` is required because `SlotRegistry` keys its `HashMap` by screen
/// (design D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    Home,
    Gallery,
    Workshop,
}

impl Screen {
    /// Map a card-activated index to its expansion target.
    ///
    /// The Home screen hosts `CARD_COUNT` cards; index 0 is Gallery, index
    /// 1 is Workshop. Out-of-range indices return `None` (delivery 4:
    /// callbacks.rs maps the UI callback to a NavCommand through this).
    pub fn from_card_index(idx: usize) -> Option<Screen> {
        match idx {
            0 => Some(Screen::Gallery),
            1 => Some(Screen::Workshop),
            _ => None,
        }
    }

    /// The UI mirror index for this screen: `Home=0`, `Gallery=1`,
    /// `Workshop=2`. This is what the `mounted-screen` Slint property
    /// carries (delivery 4: Shell writes it back to the chrome).
    pub fn mounted_index(self) -> usize {
        match self {
            Screen::Home => 0,
            Screen::Gallery => 1,
            Screen::Workshop => 2,
        }
    }
}

/// Whether the window is collapsed (base size, Home visible) or expanded
/// into a target screen (window grown, target slot mounted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpansionState {
    Collapsed,
    Expanded(Screen),
}

/// Panel sections inside the mutating window (mutating-window R1-R7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSection {
    Save,
    Borders,
    Motion,
    Filters,
    Wallpaper,
    System,
}

impl PanelSection {
    /// Ordered list of all sections for iteration/Tab order (pick-before-tune).
    #[allow(dead_code)]
    pub const ALL: [PanelSection; 6] = [
        PanelSection::Save,
        PanelSection::Borders,
        PanelSection::Motion,
        PanelSection::Filters,
        PanelSection::Wallpaper,
        PanelSection::System,
    ];

    /// Integer index for Slint `panel-section` property (0..5).
    pub fn index(self) -> usize {
        match self {
            PanelSection::Save => 0,
            PanelSection::Borders => 1,
            PanelSection::Motion => 2,
            PanelSection::Filters => 3,
            PanelSection::Wallpaper => 4,
            PanelSection::System => 5,
        }
    }

    /// Reverse mapping from index (0..5), or None out of range.
    pub fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(PanelSection::Save),
            1 => Some(PanelSection::Borders),
            2 => Some(PanelSection::Motion),
            3 => Some(PanelSection::Filters),
            4 => Some(PanelSection::Wallpaper),
            5 => Some(PanelSection::System),
            _ => None,
        }
    }

    /// Keyboard shortcut mapping (R9, R11). Alt+A/B/M/F/W/Y are the canonical
    /// shortcuts for the 6 dropdown pills. Case-insensitive and accepts both
    /// `Alt+A` and bare `a`.
    #[allow(dead_code)]
    pub fn from_shortcut(s: &str) -> Option<Self> {
        let key = s.trim().to_ascii_lowercase();
        let bare = if key.starts_with("alt+") { &key[4..] } else { &key };
        match bare {
            "a" | "s" | "save" => Some(PanelSection::Save),
            "b" | "borders" => Some(PanelSection::Borders),
            "m" | "motion" => Some(PanelSection::Motion),
            "f" | "filters" => Some(PanelSection::Filters),
            "w" | "wallpaper" => Some(PanelSection::Wallpaper),
            "y" | "system" => Some(PanelSection::System),
            _ => None,
        }
    }
}

/// Direction of the panel mutation morph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutDir {
    Enter,
    Leave,
}

/// Panel mutation state machine (mutating-window R1).
/// `Closed` is HEAD behavior; `Mutating` holds the 350ms morph; `Open` holds the active section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelState {
    Closed,
    Mutating { direction: MutDir, target: PanelSection },
    Open(PanelSection),
}

/// A navigation command queued by the shell. Keyboard and mouse both emit
/// these — activation is identical regardless of input source (spec R4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum NavCommand {
    /// Expand into a target screen (grow the window, mount the slot).
    Expand(Screen),
    /// Collapse the current expansion and return to the root screen.
    Collapse,
    /// Back/collapse affordance. Equivalent to [`NavCommand::Collapse`]
    /// while no nested navigation exists (spec R2 back/collapse scenario).
    Back,
}

/// The observable effect of processing one nav command.
///
/// `mount`/`unmount` name the slots the shell must mount/unmount and
/// `expand` tells it whether the window grows (`true`) or shrinks (`false`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavTransition {
    pub mount: Option<Screen>,
    pub unmount: Option<Screen>,
    pub expand: bool,
}

/// Pure navigation state machine (nav-shell spec R1, R2).
///
/// `screen` is the currently active screen, `expansion` mirrors the window
/// geometry, `queue` holds rapid nav commands processed FIFO (final state
/// matches the last command) and `focused_card` is the keyboard-focused
/// card on Home. No Slint types — unit-testable headless.
#[derive(Debug)]
pub struct NavState {
    screen: Screen,
    expansion: ExpansionState,
    queue: VecDeque<NavCommand>,
    focused_card: usize,
    panel: PanelState,
    /// Reduced-motion flag mirrored from system (R8). When true all durations gate to 0.
    reduced_motion: bool,
}

impl NavState {
    /// New shell state: `Home` screen, `Collapsed` expansion, keyboard
    /// focus on card 0 and an empty command queue.
    pub fn new() -> Self {
        Self {
            screen: Screen::Home,
            expansion: ExpansionState::Collapsed,
            queue: VecDeque::new(),
            focused_card: 0,
            panel: PanelState::Closed,
            reduced_motion: false,
        }
    }

    /// The currently active screen.
    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// The current expansion state.
    pub fn expansion(&self) -> ExpansionState {
        self.expansion
    }

    /// Index of the keyboard-focused card on Home.
    pub fn focused_card(&self) -> usize {
        self.focused_card
    }

    /// Move the keyboard focus by `delta` cards, clamped to the card
    /// bounds `0..CARD_COUNT`. Returns the new focused index.
    pub fn move_focus(&mut self, delta: isize) -> usize {
        let next = self.focused_card as isize + delta;
        self.focused_card = next.clamp(0, CARD_COUNT as isize - 1) as usize;
        self.focused_card
    }

    /// Queue a nav command. Rapid commands are processed in order by
    /// [`Self::process_next`]; the final state matches the last command.
    pub fn push(&mut self, cmd: NavCommand) {
        self.queue.push_back(cmd);
    }

    /// Process the next queued command (FIFO) and return its observable
    /// effect, or `None` when the queue is drained or the command is a
    /// no-op (same/unknown target, collapse/back while already collapsed).
    pub fn process_next(&mut self) -> Option<NavTransition> {
        let cmd = self.queue.pop_front()?;
        match cmd {
            NavCommand::Expand(target) => self.expand(target),
            NavCommand::Collapse | NavCommand::Back => self.collapse(),
        }
    }

    /// Apply an expansion command. Returns `None` when the target is the
    /// root screen or already the active expansion (spec R2 no-op).
    fn expand(&mut self, target: Screen) -> Option<NavTransition> {
        if target == Screen::Home {
            // The root screen is not an expansion target; expanding into
            // it (or an otherwise unknown target) is a no-op. Unregistered
            // slots are additionally rejected by the SlotRegistry later.
            return None;
        }
        if self.expansion == ExpansionState::Expanded(target) {
            // Spec R2 "No-op on same target": already expanded into it.
            return None;
        }
        let unmount = match self.expansion {
            ExpansionState::Expanded(current) => Some(current),
            ExpansionState::Collapsed => None,
        };
        let transition = NavTransition {
            mount: Some(target),
            unmount,
            expand: true,
        };
        self.screen = target;
        self.expansion = ExpansionState::Expanded(target);
        Some(transition)
    }

    /// Collapse the current expansion and return to the root screen
    /// (spec R2 back/collapse scenario). No-op when already collapsed.
    fn collapse(&mut self) -> Option<NavTransition> {
        let current = match self.expansion {
            ExpansionState::Expanded(screen) => screen,
            ExpansionState::Collapsed => return None,
        };
        let transition = NavTransition {
            mount: Some(Screen::Home),
            unmount: Some(current),
            expand: false,
        };
        self.screen = Screen::Home;
        self.expansion = ExpansionState::Collapsed;
        Some(transition)
    }

    // ── Panel mutation (mutating-window R1, R2, R8, R9) ────────────────

    /// Enter the panel from Gallery only. Returns true on success and sets
    /// `Mutating{Enter,target}`; false when not Gallery or already mutating/open.
    pub fn enter_panel(&mut self, target: PanelSection) -> bool {
        if self.screen != Screen::Gallery {
            return false;
        }
        if self.panel != PanelState::Closed {
            return false;
        }
        self.panel = PanelState::Mutating { direction: MutDir::Enter, target };
        true
    }

    /// Complete the pending mutation after the 350ms Timer.
    /// `Mutating{Enter,target}` → `Open(target)`, `Mutating{Leave,_}` → `Closed`.
    pub fn complete_mutation(&mut self) {
        match self.panel {
            PanelState::Mutating { direction: MutDir::Enter, target } => {
                self.panel = PanelState::Open(target);
            }
            PanelState::Mutating { direction: MutDir::Leave, .. } => {
                self.panel = PanelState::Closed;
            }
            _ => {}
        }
    }

    /// Leave the panel (Esc reverses). From `Open(section)` or mid-enter `Mutating{Enter}`
    /// transitions to `Mutating{Leave}`; from `Mutating{Leave}` or `Closed` is no-op.
    pub fn leave_panel(&mut self) -> bool {
        match self.panel {
            PanelState::Open(section) => {
                self.panel = PanelState::Mutating { direction: MutDir::Leave, target: section };
                true
            }
            PanelState::Mutating { direction: MutDir::Enter, target } => {
                // Reverse mid-flight (R1 Esc reverses)
                self.panel = PanelState::Mutating { direction: MutDir::Leave, target };
                true
            }
            PanelState::Closed => false,
            PanelState::Mutating { direction: MutDir::Leave, .. } => false,
        }
    }

    /// Switch section without Gallery remount when `Open` (R2 internal nav). No-op otherwise.
    pub fn set_section(&mut self, section: PanelSection) {
        if let PanelState::Open(_) = self.panel {
            self.panel = PanelState::Open(section);
        }
    }

    /// Whether the window is currently mutating (input guard R9).
    pub fn is_mutating(&self) -> bool {
        matches!(self.panel, PanelState::Mutating { .. })
    }

    /// Current panel state (read-only).
    pub fn panel_state(&self) -> PanelState {
        self.panel
    }

    /// Current panel section if open/mutating, else None.
    pub fn panel_section(&self) -> Option<PanelSection> {
        match self.panel {
            PanelState::Open(s) => Some(s),
            PanelState::Mutating { target, .. } => Some(target),
            PanelState::Closed => None,
        }
    }

    /// Reduced-motion flag mutator (R8). Test harness sets it from env.
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
    }

    /// Whether reduced-motion is active.
    #[allow(dead_code)]
    pub fn is_reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    /// Gate a duration to 0 when reduced-motion is active (R8).
    pub fn effective_duration(&self, original_ms: u64) -> u64 {
        if self.reduced_motion { 0 } else { original_ms }
    }

    /// Stateless helper also gates an arbitrary duration (convenience for timers).
    #[allow(dead_code)]
    pub fn effective_duration_static(original_ms: u64, reduced: bool) -> u64 {
        if reduced { 0 } else { original_ms }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: a state machine expanded into the Gallery slot.
    fn expanded_gallery() -> NavState {
        let mut ns = NavState::new();
        ns.push(NavCommand::Expand(Screen::Gallery));
        assert!(ns.process_next().is_some());
        ns
    }

    // ── Initial state (spec R1) ────────────────────────────────────

    #[test]
    fn initial_state_is_home_collapsed_focus_zero() {
        let ns = NavState::new();
        assert_eq!(ns.screen(), Screen::Home);
        assert_eq!(ns.expansion(), ExpansionState::Collapsed);
        assert_eq!(ns.focused_card(), 0);
    }

    // ── Expansion (spec R2) ────────────────────────────────────────

    #[test]
    fn expand_happy_path_mounts_target_and_grows() {
        let mut ns = NavState::new();
        ns.push(NavCommand::Expand(Screen::Gallery));
        let transition = ns.process_next().expect("queued expand must produce a transition");

        assert_eq!(
            transition,
            NavTransition {
                mount: Some(Screen::Gallery),
                unmount: None,
                expand: true,
            }
        );
        assert_eq!(ns.expansion(), ExpansionState::Expanded(Screen::Gallery));
        assert_eq!(ns.screen(), Screen::Gallery);
    }

    #[test]
    fn expand_same_target_when_expanded_is_noop() {
        let mut ns = expanded_gallery();
        ns.push(NavCommand::Expand(Screen::Gallery));
        assert_eq!(ns.process_next(), None, "spec R2: no transition for the same target");
        assert_eq!(ns.expansion(), ExpansionState::Expanded(Screen::Gallery));
    }

    #[test]
    fn expand_root_screen_is_noop_for_unknown_target() {
        // Home is the root screen, not an expansion target: expanding into
        // it is a no-op (this also covers unknown-target handling at the
        // NavState level — unregistered slots are additionally rejected by
        // the SlotRegistry in delivery 2).
        let mut ns = NavState::new();
        ns.push(NavCommand::Expand(Screen::Home));
        assert_eq!(ns.process_next(), None);
        assert_eq!(ns.screen(), Screen::Home);
        assert_eq!(ns.expansion(), ExpansionState::Collapsed);
    }

    #[test]
    fn switching_expanded_target_unmounts_previous() {
        let mut ns = expanded_gallery();
        ns.push(NavCommand::Expand(Screen::Workshop));
        let transition = ns.process_next().expect("target switch must produce a transition");
        assert_eq!(
            transition,
            NavTransition {
                mount: Some(Screen::Workshop),
                unmount: Some(Screen::Gallery),
                expand: true,
            }
        );
        assert_eq!(ns.expansion(), ExpansionState::Expanded(Screen::Workshop));
        assert_eq!(ns.screen(), Screen::Workshop);
    }

    // ── Queue (spec R2 rapid navigation) ───────────────────────────

    #[test]
    fn queue_drains_fifo_and_final_matches_last_command() {
        let mut ns = NavState::new();
        ns.push(NavCommand::Expand(Screen::Gallery));
        ns.push(NavCommand::Expand(Screen::Workshop));

        let first = ns.process_next().expect("first queued command");
        assert_eq!(first.mount, Some(Screen::Gallery));

        let second = ns.process_next().expect("second queued command");
        assert_eq!(second.mount, Some(Screen::Workshop));

        assert_eq!(
            ns.expansion(),
            ExpansionState::Expanded(Screen::Workshop),
            "spec R2: final state matches the last command"
        );
        assert_eq!(ns.screen(), Screen::Workshop);
        assert_eq!(ns.process_next(), None, "queue drained");
    }

    #[test]
    fn noop_commands_are_consumed_in_order() {
        let mut ns = NavState::new();
        ns.push(NavCommand::Expand(Screen::Home)); // no-op, still consumed
        ns.push(NavCommand::Expand(Screen::Gallery));

        assert_eq!(ns.process_next(), None, "no-op consumed first");
        let transition = ns.process_next().expect("valid command after no-op");
        assert_eq!(transition.mount, Some(Screen::Gallery));
        assert_eq!(ns.expansion(), ExpansionState::Expanded(Screen::Gallery));
    }

    #[test]
    fn process_next_on_empty_queue_is_none() {
        let mut ns = NavState::new();
        assert_eq!(ns.process_next(), None, "drained queue yields no transition");
    }

    // ── Back / collapse (spec R2 back/collapse scenario) ───────────

    #[test]
    fn back_collapses_and_remounts_home() {
        let mut ns = expanded_gallery();
        ns.push(NavCommand::Back);
        let transition = ns.process_next().expect("back must produce a transition");
        assert_eq!(
            transition,
            NavTransition {
                mount: Some(Screen::Home),
                unmount: Some(Screen::Gallery),
                expand: false,
            }
        );
        assert_eq!(ns.expansion(), ExpansionState::Collapsed);
        assert_eq!(ns.screen(), Screen::Home);
    }

    #[test]
    fn collapse_returns_to_home() {
        let mut ns = expanded_gallery();
        ns.push(NavCommand::Collapse);
        let transition = ns.process_next().expect("collapse must produce a transition");
        assert_eq!(
            transition,
            NavTransition {
                mount: Some(Screen::Home),
                unmount: Some(Screen::Gallery),
                expand: false,
            }
        );
        assert_eq!(ns.expansion(), ExpansionState::Collapsed);
        assert_eq!(ns.screen(), Screen::Home);
    }

    #[test]
    fn collapse_when_already_collapsed_is_noop() {
        let mut ns = NavState::new();
        ns.push(NavCommand::Collapse);
        assert_eq!(ns.process_next(), None);
        assert_eq!(ns.expansion(), ExpansionState::Collapsed);
    }

    #[test]
    fn back_when_collapsed_is_noop() {
        let mut ns = NavState::new();
        ns.push(NavCommand::Back);
        assert_eq!(ns.process_next(), None);
    }

    // ── Keyboard focus (design: focused_card clamped to card bounds) ─

    #[test]
    fn move_focus_steps_and_clamps_at_upper_bound() {
        let mut ns = NavState::new();
        assert_eq!(ns.move_focus(1), 1);
        assert_eq!(ns.move_focus(1), 1, "clamped at the last card");
        assert_eq!(ns.focused_card(), 1);
    }

    #[test]
    fn move_focus_clamps_at_lower_bound() {
        let mut ns = NavState::new();
        ns.move_focus(1);
        assert_eq!(ns.move_focus(-5), 0, "clamped at the first card");
        assert_eq!(ns.focused_card(), 0);
    }

    // ── Screen ↔ card index mapping (delivery 4: callbacks ↔ NavCommand) ──

    #[test]
    fn from_card_index_maps_gallery_and_workshop() {
        // card-activated fires with the focused card index: 0 = Gallery,
        // 1 = Workshop (Home is the root screen, not a card).
        assert_eq!(Screen::from_card_index(0), Some(Screen::Gallery));
        assert_eq!(Screen::from_card_index(1), Some(Screen::Workshop));
        assert_eq!(Screen::from_card_index(2), None, "out of range → no target");
        assert_eq!(Screen::from_card_index(99), None);
    }

    #[test]
    fn mounted_index_round_trips_with_from_card_index() {
        // mounted_index is the UI mirror: Home=0, Gallery=1, Workshop=2.
        assert_eq!(Screen::Home.mounted_index(), 0);
        assert_eq!(Screen::Gallery.mounted_index(), 1);
        assert_eq!(Screen::Workshop.mounted_index(), 2);

        // Round trip: card index → screen → mounted index → same screen.
        for card in 0..CARD_COUNT {
            let screen = Screen::from_card_index(card).unwrap();
            // mounted_index is Home=0, but card indices skip Home (0=Gallery).
            // The mounted_index of a card target is card+1.
            assert_eq!(screen.mounted_index(), card + 1);
        }
    }

    // ── PanelState mutation (mutating-window R1, R9) ─────────────────

    #[test]
    fn test_enter_panel_only_from_gallery_sets_mutating() {
        let mut ns = NavState::new();
        // Not Gallery -> must refuse
        assert!(!ns.enter_panel(PanelSection::Save), "enter_panel must fail when not Gallery");
        assert!(!ns.is_mutating(), "not mutating after failed enter");
        // Expand Gallery
        ns.push(NavCommand::Expand(Screen::Gallery));
        assert!(ns.process_next().is_some());
        // Now Gallery -> enter_panel succeeds and sets mutating
        assert!(ns.enter_panel(PanelSection::Save), "enter_panel must succeed from Gallery");
        assert!(ns.is_mutating(), "is_mutating true while Mutating");
        // Complete mutation -> open, no longer mutating
        ns.complete_mutation();
        assert!(!ns.is_mutating(), "complete_mutation clears mutating");
        assert_eq!(ns.panel_state(), PanelState::Open(PanelSection::Save));
    }

    #[test]
    fn test_alt_a_enter_panel_maps() {
        // Alt-A shortcut maps to Save section (R9 keyboard-reachable)
        let target = PanelSection::from_shortcut("Alt+A").expect("Alt+A must map to a section");
        assert_eq!(target, PanelSection::Save);
        let mut ns = NavState::new();
        ns.push(NavCommand::Expand(Screen::Gallery));
        assert!(ns.process_next().is_some());
        assert!(ns.enter_panel(target), "Alt+A target must be enterable from Gallery");
        assert!(ns.is_mutating());
    }
}