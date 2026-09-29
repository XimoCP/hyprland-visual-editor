//! The darkman indicator-file backend for `read-desktop-preference`.
//!
//! Second in the router's declared precedence: consulted only when the
//! settings key backend cannot even start (its spawn fails), exactly as
//! the core did before the seam existed.

use super::{DesktopPreference, DesktopPreferenceBackend};
use std::path::PathBuf;

/// Reads darkman's mode indicator file under `$HOME/.cache`.
///
/// Declared fields of the backend contract: `id` below, capability
/// `read-desktop-preference` by placement, and nothing else — the indicator
/// file is darkman's own artefact: this backend reads it, never writes,
/// watches or deletes it.
pub struct DarkmanBackend;

impl DesktopPreferenceBackend for DarkmanBackend {
    fn id(&self) -> &'static str {
        "darkman-mode-file"
    }

    fn read(&self) -> Option<DesktopPreference> {
        let mode_path = PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".cache")
            .join("darkman")
            .join("mode");
        match std::fs::read_to_string(&mode_path) {
            // Only the literal `dark` (trimmed) answers dark; any other
            // readable payload answers light — verbatim from the old core.
            Ok(mode) => Some(if mode.trim() == "dark" {
                DesktopPreference::Dark
            } else {
                DesktopPreference::Light
            }),
            // No indicator file → this backend cannot answer; the router
            // applies the historical default instead.
            Err(_) => None,
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempEnv;

    /// Hermetic: `TempEnv` redirects `HOME` to a throwaway directory, so
    /// the probe never reads the keeper's own indicator file.
    #[test]
    fn reads_the_mode_file_and_reports_a_missing_one_as_unavailable() {
        let _env = TempEnv::new();
        let mode_path = PathBuf::from(std::env::var("HOME").expect("TempEnv sets HOME"))
            .join(".cache")
            .join("darkman")
            .join("mode");

        // Missing file → no answer (the router moves on / applies default).
        assert_eq!(DarkmanBackend.read(), None);

        std::fs::create_dir_all(mode_path.parent().expect("cache dir")).expect("mkdir");
        std::fs::write(&mode_path, "  dark  \n").expect("write mode");
        assert_eq!(DarkmanBackend.read(), Some(DesktopPreference::Dark));

        std::fs::write(&mode_path, "light\n").expect("rewrite mode");
        assert_eq!(DarkmanBackend.read(), Some(DesktopPreference::Light));

        // Any other readable payload answers light — only the literal
        // `dark` answers dark.
        std::fs::write(&mode_path, "auto").expect("rewrite mode");
        assert_eq!(DarkmanBackend.read(), Some(DesktopPreference::Light));
    }
}
