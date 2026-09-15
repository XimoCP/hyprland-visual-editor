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
/// Preferred floating Settings size: 1000×640. Comfortable for the
/// two-column panel (list + tune) without being oversized on a large
/// display: a form does not improve by being wider.
const FLOAT_PREFERRED: (f32, f32) = (1000.0, 640.0);
/// Gap kept between the floating Settings window and the monitor edge, on
/// every side. The compositor draws HVE's own border around that window, so
/// this margin is what keeps that border fully visible — and therefore what
/// makes the border a live preview of the values being tuned.
const FLOAT_MARGIN: f32 = 48.0;

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

    /// Floating Settings window size for a monitor whose logical size is
    /// `monitor`.
    ///
    /// The Settings window leaves fullscreen and floats while the panel is
    /// open, so that the compositor-drawn border around HVE itself becomes
    /// the live preview of the border values being tuned. Two consequences
    /// shape the answer:
    ///
    /// * The size is a property of the panel's CONTENT, not of the display.
    ///   A 65" 4K panel and a 32" 1440p panel get the same window: a form
    ///   does not improve by being wider, and the border stays visible
    ///   either way because it wraps a window, not the desktop.
    /// * The only hard constraint is the margin: the window must not touch
    ///   the monitor edge, or the border would sit flush against it.
    ///
    /// On a display too small to hold `MIN` plus the margins, the monitor
    /// wins and the panel degrades to the stacked layout — the caller never
    /// rejects a size for being small.
    pub fn settings_float_size(&self, monitor: (f32, f32)) -> (f32, f32) {
        let max_w = (monitor.0 - FLOAT_MARGIN * 2.0).max(MIN.0);
        let max_h = (monitor.1 - FLOAT_MARGIN * 2.0).max(MIN.1);
        (FLOAT_PREFERRED.0.min(max_w), FLOAT_PREFERRED.1.min(max_h))
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

    // ── Floating Settings size (self-preview border) ──────────────────

    #[test]
    fn float_size_is_the_same_on_every_roomy_monitor() {
        let policy = SizePolicy::new();
        // The keeper's 14" panel (1920x1200 @ scale 1.333 → 1440x900 logical),
        // the 32" 1440p (scale 1) and a 65" 4K TV all get the same window.
        assert_eq!(policy.settings_float_size((1440.0, 900.0)), (1000.0, 640.0));
        assert_eq!(policy.settings_float_size((2560.0, 1440.0)), (1000.0, 640.0));
        assert_eq!(policy.settings_float_size((3840.0, 2160.0)), (1000.0, 640.0));
    }

    #[test]
    fn float_size_never_touches_the_monitor_edge() {
        let policy = SizePolicy::new();
        for monitor in [
            (1024.0, 768.0),
            (1280.0, 720.0),
            (1440.0, 900.0),
            (2560.0, 1440.0),
            (3840.0, 2160.0),
        ] {
            let (w, h) = policy.settings_float_size(monitor);
            assert!(
                w + FLOAT_MARGIN <= monitor.0,
                "width {w} leaves no margin on {monitor:?}"
            );
            assert!(
                h + FLOAT_MARGIN <= monitor.1,
                "height {h} leaves no margin on {monitor:?}"
            );
        }
    }

    #[test]
    fn float_size_shrinks_on_a_small_monitor_and_keeps_two_columns() {
        let policy = SizePolicy::new();
        // 1024x768 leaves 928x672 after the margins: wider than the 852
        // two-column threshold, so the panel keeps both panes side by side.
        assert_eq!(policy.settings_float_size((1024.0, 768.0)), (928.0, 640.0));
    }

    #[test]
    fn float_size_shrinks_the_margin_before_the_minimum() {
        let policy = SizePolicy::new();
        // 900x600 cannot hold MIN plus both margins. Order of precedence:
        // the margin gives way first (10px per side instead of 48), the
        // panel stays usable, and no display is rejected for being small.
        let (w, h) = policy.settings_float_size((900.0, 600.0));
        assert_eq!((w, h), (804.0, 580.0), "margin yields, MIN holds");
        assert!(w <= 900.0 && h <= 600.0, "still inside the display");
    }

    #[test]
    fn float_size_holds_min_when_the_monitor_is_smaller_than_min() {
        let policy = SizePolicy::new();
        // MIN (800x580) is the app's hard floor and predates this function:
        // on a display narrower than MIN the window matches MIN, which is
        // the floor SizePolicy::clamp already enforces everywhere.
        assert_eq!(policy.settings_float_size((700.0, 500.0)), MIN);
    }
}
