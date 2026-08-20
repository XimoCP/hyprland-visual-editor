//! Pure navigation state machine for the HVE 2 shell.
//!
//! `NavState` owns ALL shell navigation state (nav-shell spec R1): the
//! current screen, the expansion state, the FIFO command queue and the
//! focused card index. The UI is a read-only view of this state — it never
//! holds navigation state of its own. The module is intentionally free of
//! Slint types so it stays unit-testable headless (R2: deterministic
//! transitions).

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

/// Whether the window is collapsed (base size, Home visible) or expanded
/// into a target screen (window grown, target slot mounted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpansionState {
    Collapsed,
    Expanded(Screen),
}

/// A navigation command queued by the shell. Keyboard and mouse both emit
/// these — activation is identical regardless of input source (spec R4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}