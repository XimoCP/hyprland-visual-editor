//! Pure window sizing policy for the HVE 2 shell (base-window spec R8).
//!
//! `SizePolicy` is a pure math helper (design D4): it maps the shell
//! expansion state to a target window size and clamps any size to the
//! configured minimum on each axis. It owns no window and performs no I/O —
//! the stepped `Timer`/`set_size` animation lives in the Shell root
//! (task 4.3). Sizes are skwd-wall geometry scaled to match the current HVE
//! minimum window.
//!
//! MIT credit: visual language, geometry and tokens are translated from
//! skwd-wall (MIT, © liixini, https://github.com/liixini/skwd-wall). No GPL
//! code from hyprmod is used.

/// Base (collapsed) window size: 900×680.
const BASE: (f32, f32) = (900.0, 680.0);
/// Expanded window size: 1200×800.
const EXPANDED: (f32, f32) = (1200.0, 800.0);
/// Settings panel size (Gallery card → Settings expansion, S9, 4.4): 1300×900.
/// Same window mutates to settings via stepped animator (22 steps 350ms OutCubic).
const SETTINGS_EXPANDED: (f32, f32) = (1300.0, 900.0);
/// Minimum window size on any axis: 800×580.
const MIN: (f32, f32) = (800.0, 580.0);

/// Pure window sizing policy (base-window spec R8, design D4).
///
/// `target` resolves the desired window size for the shell expansion state
/// and `clamp` guarantees the window never shrinks below the configured
/// minimum on either axis (user resize or programmatic `set_size`).
pub struct SizePolicy;

impl SizePolicy {
    /// A policy with the trunk defaults: base 900×680, expanded 1200×800,
    /// minimum 800×580, settings 1300×900 (S9ExpandToSettings).
    pub fn new() -> Self {
        Self
    }

    /// The target window size: `BASE` when collapsed, `EXPANDED` when
    /// expanded (base-window spec R8 growth scenarios).
    pub fn target(&self, expanded: bool) -> (f32, f32) {
        if expanded { EXPANDED } else { BASE }
    }

    /// Settings panel target (S9): when Gallery card expands to settings
    /// the same window mutates to SETTINGS_EXPANDED (1300×900) via the
    /// same 22-step 350ms OutCubic animator (task 4.4).
    pub fn target_for_settings(&self, settings: bool) -> (f32, f32) {
        if settings { SETTINGS_EXPANDED } else { EXPANDED }
    }

    /// Combined target helper: collapsed → BASE, expanded gallery → EXPANDED,
    /// expanded + settings → SETTINGS_EXPANDED.
    pub fn target_with_settings(&self, expanded: bool, settings_open: bool) -> (f32, f32) {
        if !expanded { BASE } else if settings_open { SETTINGS_EXPANDED } else { EXPANDED }
    }

    /// `size` raised to the minimum on every axis that falls below it;
    /// axes already at or above the minimum pass through unchanged
    /// (spec R8 edge cases).
    pub fn clamp(&self, size: (f32, f32)) -> (f32, f32) {
        (size.0.max(MIN.0), size.1.max(MIN.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Target sizing (design D4) ───────────────────────────────────

    #[test]
    fn target_collapsed_is_the_base_size() {
        let policy = SizePolicy::new();
        assert_eq!(policy.target(false), (900.0, 680.0));
    }

    #[test]
    fn target_expanded_is_the_expanded_size() {
        let policy = SizePolicy::new();
        assert_eq!(policy.target(true), (1200.0, 800.0));
    }

    // ── Clamp (spec R8 edge cases) ──────────────────────────────────

    #[test]
    fn clamp_raises_only_the_axis_below_min() {
        let policy = SizePolicy::new();
        assert_eq!(policy.clamp((700.0, 600.0)), (800.0, 600.0));
    }

    #[test]
    fn clamp_raises_both_axes_to_min() {
        let policy = SizePolicy::new();
        assert_eq!(policy.clamp((500.0, 400.0)), (800.0, 580.0));
    }

    #[test]
    fn clamp_preserves_sizes_above_min() {
        let policy = SizePolicy::new();
        assert_eq!(policy.clamp((1000.0, 700.0)), (1000.0, 700.0));
    }

    #[test]
    fn clamp_accepts_the_exact_minimum() {
        let policy = SizePolicy::new();
        assert_eq!(policy.clamp((800.0, 580.0)), (800.0, 580.0));
    }

    // ── 4.4 ExpandToSettings mutation S9 (PR4) ────────────────────────

    #[test]
    fn apply_noop_settings_target_expands_beyond_gallery() {
        let policy = SizePolicy::new();
        assert_eq!(policy.target_for_settings(false), (1200.0, 800.0), "gallery expanded");
        assert_eq!(policy.target_for_settings(true), (1300.0, 900.0), "settings panel larger than gallery");
        assert_eq!(policy.target_with_settings(false, false), (900.0, 680.0), "collapsed base");
        assert_eq!(policy.target_with_settings(true, false), (1200.0, 800.0), "expanded gallery");
        assert_eq!(policy.target_with_settings(true, true), (1300.0, 900.0), "expanded settings S9");
        assert_eq!(policy.target_with_settings(false, true), (900.0, 680.0), "collapsed ignores settings flag");
    }
}