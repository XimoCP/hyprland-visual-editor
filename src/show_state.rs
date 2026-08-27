// ShowStateMachine — pure window show/hide/focus core.
//
// Three states: Hidden / Entering / Visible
//
// - Hidden: window is not shown (tray or never shown). No focus to lose;
//   `begin_show` is the only valid outbound transition.
// - Entering: transient state after `begin_show` until the compositor
//   confirms the window actually has focus. This window exists because
//   Hyprland's mapping/focus is asynchronous — the compositor may emit
//   focus-lost or stale events while the window is still arriving.
// - Visible: window is shown and focus has been confirmed. This is the
//   only state where a focus-lost event should be honored as a hide
//   request (auto-minimize on blur).
//
// Why focus-lost is only honored in Visible:
// Focus-lost events observed during Entering are the compositor's own
// arrival noise (e.g. the previous window still reporting blur, or the
// new window not yet reported as focused). Honoring them would instantly
// re-hide the window — the gallery auto-minimize-on-open race. By
// ignoring focus-lost in Hidden and Entering, the machine guarantees
// that only a deliberate blur after the window is settled can hide it.
//
// Why the entry timeout fallback exists:
// The machine must never stick in Entering. Under normal conditions
// `confirm_focus` moves Entering -> Visible exactly once and the
// fullscreen/session code runs there. If that confirmation never arrives
// (compositor quirk, missed event), the timeout fallback promotes
// Entering -> Visible after a bounded duration so fullscreen is still
// applied exactly once — either via `confirm_focus` OR via the timeout,
// never both.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShowState {
    Hidden,
    Entering,
    Visible,
}

pub struct ShowStateMachine {
    state: ShowState,
    entered_at: Option<Instant>,
}

impl ShowStateMachine {
    pub fn new() -> Self {
        Self {
            state: ShowState::Hidden,
            entered_at: None,
        }
    }

    pub fn begin_show(&mut self) -> bool {
        let _ = &self.entered_at;
        false
    }

    pub fn confirm_focus(&mut self) -> bool {
        false
    }

    pub fn entry_timed_out(&mut self, _timeout: Duration) -> bool {
        false
    }

    pub fn on_focus_lost(&self) -> bool {
        false
    }

    pub fn on_hide(&mut self) -> bool {
        false
    }

    pub fn state(&self) -> ShowState {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn new_starts_hidden() {
        let m = ShowStateMachine::new();
        assert_eq!(m.state(), ShowState::Hidden);
    }

    #[test]
    fn begin_show_hidden_to_entering_returns_true() {
        let mut m = ShowStateMachine::new();
        assert!(m.begin_show());
        assert_eq!(m.state(), ShowState::Entering);
    }

    #[test]
    fn begin_show_double_returns_false() {
        let mut m = ShowStateMachine::new();
        assert!(m.begin_show());
        assert!(!m.begin_show());
        assert_eq!(m.state(), ShowState::Entering);
    }

    #[test]
    fn begin_show_from_visible_returns_false() {
        let mut m = ShowStateMachine::new();
        assert!(m.begin_show());
        assert!(m.confirm_focus());
        assert_eq!(m.state(), ShowState::Visible);
        assert!(!m.begin_show());
        assert_eq!(m.state(), ShowState::Visible);
    }

    #[test]
    fn confirm_focus_from_entering_goes_visible() {
        let mut m = ShowStateMachine::new();
        m.begin_show();
        assert!(m.confirm_focus());
        assert_eq!(m.state(), ShowState::Visible);
    }

    #[test]
    fn confirm_focus_from_visible_noop_returns_true() {
        let mut m = ShowStateMachine::new();
        m.begin_show();
        m.confirm_focus();
        assert!(m.confirm_focus());
        assert_eq!(m.state(), ShowState::Visible);
    }

    #[test]
    fn confirm_focus_from_hidden_returns_false_stays_hidden() {
        let mut m = ShowStateMachine::new();
        assert!(!m.confirm_focus());
        assert_eq!(m.state(), ShowState::Hidden);
    }

    #[test]
    fn entry_timed_out_before_timeout_stays_entering() {
        let mut m = ShowStateMachine::new();
        m.begin_show();
        // Large timeout vs tiny elapsed -> must NOT time out.
        assert!(!m.entry_timed_out(Duration::from_secs(10)));
        assert_eq!(m.state(), ShowState::Entering);
    }

    #[test]
    fn entry_timed_out_after_timeout_goes_visible() {
        let mut m = ShowStateMachine::new();
        m.begin_show();
        // Zero timeout guarantees elapsed >= timeout immediately.
        assert!(m.entry_timed_out(Duration::from_millis(0)));
        assert_eq!(m.state(), ShowState::Visible);
    }

    #[test]
    fn entry_timed_out_from_hidden_returns_false() {
        let mut m = ShowStateMachine::new();
        assert!(!m.entry_timed_out(Duration::from_millis(0)));
        assert_eq!(m.state(), ShowState::Hidden);
    }

    #[test]
    fn entry_timed_out_from_visible_returns_false() {
        let mut m = ShowStateMachine::new();
        m.begin_show();
        m.confirm_focus();
        assert!(!m.entry_timed_out(Duration::from_millis(0)));
        assert_eq!(m.state(), ShowState::Visible);
    }

    #[test]
    fn on_focus_lost_visible_returns_true() {
        let mut m = ShowStateMachine::new();
        m.begin_show();
        m.confirm_focus();
        assert!(m.on_focus_lost());
    }

    #[test]
    fn on_focus_lost_entering_returns_false_regression() {
        let mut m = ShowStateMachine::new();
        m.begin_show();
        assert_eq!(m.state(), ShowState::Entering);
        assert!(!m.on_focus_lost());
        assert_eq!(m.state(), ShowState::Entering);
    }

    #[test]
    fn on_focus_lost_hidden_returns_false() {
        let m = ShowStateMachine::new();
        assert!(!m.on_focus_lost());
    }

    #[test]
    fn on_hide_from_visible_goes_hidden() {
        let mut m = ShowStateMachine::new();
        m.begin_show();
        m.confirm_focus();
        assert!(m.on_hide());
        assert_eq!(m.state(), ShowState::Hidden);
    }

    #[test]
    fn on_hide_from_hidden_returns_false() {
        let mut m = ShowStateMachine::new();
        assert!(!m.on_hide());
        assert_eq!(m.state(), ShowState::Hidden);
    }

    #[test]
    fn full_cycle_via_confirm_focus() {
        let mut m = ShowStateMachine::new();
        assert_eq!(m.state(), ShowState::Hidden);
        assert!(m.begin_show());
        assert!(m.confirm_focus());
        assert_eq!(m.state(), ShowState::Visible);
        assert!(m.on_hide());
        assert_eq!(m.state(), ShowState::Hidden);
    }

    #[test]
    fn full_cycle_via_timeout() {
        let mut m = ShowStateMachine::new();
        assert!(m.begin_show());
        assert!(m.entry_timed_out(Duration::from_millis(0)));
        assert_eq!(m.state(), ShowState::Visible);
        assert!(m.on_hide());
        assert_eq!(m.state(), ShowState::Hidden);
    }
}
