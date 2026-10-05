//! The shell-capability seam: what the CORE asks of whatever desktop shell
//! is running, phrased as capabilities instead of shell names, paths or CLI
//! calls.
//!
//! Hyprland is the base and stays the base; this seam is about the layer on
//! top of it (Noctalia today, anything else tomorrow). The core resolves the
//! running shell's adapter through [`crate::providers::active_shell`] and
//! calls only these methods. A shell that cannot provide a capability keeps
//! the declined default: the core falls back to its neutral path and nothing
//! breaks.

use std::path::Path;

/// Shell-specific operations the core needs, each with a declined default.
///
/// The core never learns a shell's name, file layout or command line: the
/// adapter that owns that knowledge answers these questions.
pub trait ShellCapabilities: Send + Sync {
    /// The wallpaper the running shell currently displays, if it can report
    /// one. `None` means "cannot tell" — never a fabricated value.
    fn live_wallpaper(&self) -> Option<String> {
        None
    }

    /// The wallpaper a saved theme recorded for THIS shell, read from the
    /// theme's own `providers/` tree. `None` when the shell records none.
    fn saved_theme_wallpaper(&self, themes_root: &Path, theme_name: &str) -> Option<String> {
        let _ = (themes_root, theme_name);
        None
    }

    /// Whether applying `theme_name` may trigger this shell's late palette
    /// re-assert. `false` = no late desktop mutation is expected.
    fn arms_palette_reassert(&self, themes_root: &Path, theme_name: &str) -> bool {
        let _ = (themes_root, theme_name);
        false
    }

    /// Current Do-Not-Disturb state ("on" / "off"), if the shell exposes one.
    fn dnd_status(&self) -> Option<String> {
        None
    }

    /// Ask the shell to set Do-Not-Disturb to `state` ("on" / "off").
    /// `true` when the shell accepted it. A shell without DND returns false.
    fn dnd_set(&self, state: &str) -> bool {
        let _ = state;
        false
    }
}

/// The declined shell: no capability is provided, every call returns the
/// neutral default. Used when no shipped shell is active.
///
/// Not constructed by any shipped configuration yet, but part of the seam's
/// vocabulary: the lint allowance keeps the release build warning-free.
#[allow(dead_code)]
pub struct NoShell;

impl ShellCapabilities for NoShell {}
