//! The GNOME settings key backend for `read-desktop-preference`.
//!
//! Moved here verbatim — behaviour and all — from `src/theme.rs`, where the
//! core shelled out to the probe directly (Phase 4 of
//! `odd/tasks/hve-capability-routing.md`). This module is the ONLY home of
//! that knowledge: `theme.rs` asks the capability and gets a preference
//! back.

use super::{DesktopPreference, DesktopPreferenceBackend};

/// Reads the desktop colour-scheme key through the `gsettings` CLI.
///
/// First in the router's declared precedence. Declared fields of the
/// backend contract: `id` below, capability `read-desktop-preference` by
/// placement, and nothing else — it owns, watches, deletes and refreshes
/// no file of HVE's.
pub struct GsettingsBackend;

impl DesktopPreferenceBackend for GsettingsBackend {
    fn id(&self) -> &'static str {
        "gsettings-color-scheme"
    }

    fn read(&self) -> Option<DesktopPreference> {
        let output = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output();
        match output {
            Ok(out) => {
                let s = String::from_utf8_lossy(&out.stdout);
                // "prefer-dark" → dark, anything else ("default", empty
                // stdout, garbage) → light. A spawn that SUCCEEDS always
                // answers, so the next backend is never consulted then.
                Some(if s.contains("prefer-dark") {
                    DesktopPreference::Dark
                } else {
                    DesktopPreference::Light
                })
            }
            // The binary does not exist → this backend cannot answer, so
            // the router falls through to the next declared one.
            Err(_) => None,
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempEnv;

    /// Hermetic stub for the probe binary: a fake `gsettings` prepended to
    /// `PATH` (the same shadow pattern as `providers::mod`'s `ShellSandbox`)
    /// so the parse is pinned without the real binary. The stub refuses the
    /// call — empty stdout — unless it received EXACTLY today's argv, so a
    /// changed command line fails this test. `TempEnv` holds the shared env
    /// lock for the whole struct, and `PATH` is restored in `Drop` BEFORE
    /// the lock (the last field) is released.
    struct StubGsettings {
        /// Restored first: `Drop` puts the original `PATH` back while the
        /// shared env lock is still held.
        old_path: Option<String>,
        /// The stub's directory — lives on `PATH` until `Drop` restores it.
        _bin: tempfile::TempDir,
        /// Shared env lock, released last.
        _env: TempEnv,
    }

    /// Everything before the answer: the stub exits 0 with NO stdout unless
    /// argv is exactly the read the core performs today.
    const ARGV_GUARD: &str = r#"#!/bin/bash
if [ "$1" != "get" ] || [ "$2" != "org.gnome.desktop.interface" ] || [ "$3" != "color-scheme" ]; then
    exit 0
fi
"#;

    impl StubGsettings {
        /// Install a stub whose stdout is `script`'s body after the guard.
        fn new(body: &str) -> Self {
            let env = TempEnv::new();
            let bin = tempfile::tempdir().expect("stub bin dir");
            let stub = bin.path().join("gsettings");
            std::fs::write(&stub, format!("{ARGV_GUARD}{body}")).expect("stub script");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
                    .expect("stub must be executable");
            }

            let old_path = std::env::var("PATH").ok();
            let new_path = match &old_path {
                Some(prev) => format!("{}:{}", bin.path().display(), prev),
                None => bin.path().display().to_string(),
            };
            std::env::set_var("PATH", &new_path);

            Self {
                old_path,
                _bin: bin,
                _env: env,
            }
        }

        /// Swap the stub's answer without touching the environment again.
        fn answer_with(&self, body: &str) {
            std::fs::write(self._bin.path().join("gsettings"), format!("{ARGV_GUARD}{body}"))
                .expect("rewrite stub");
        }
    }

    impl Drop for StubGsettings {
        fn drop(&mut self) {
            match &self.old_path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
        }
    }

    /// The parse, pinned exactly as the core used to do it — including the
    /// argv guard (a changed command line leaves stdout empty and would
    /// answer Light) and the empty-stdout case (a successful spawn always
    /// answers, so the indicator file is NOT consulted then).
    #[test]
    fn parses_the_colour_scheme_exactly_as_the_core_used_to() {
        let stub = StubGsettings::new("echo \"'prefer-dark'\"\n");
        assert_eq!(GsettingsBackend.read(), Some(DesktopPreference::Dark));

        stub.answer_with("echo \"'default'\"\n");
        assert_eq!(GsettingsBackend.read(), Some(DesktopPreference::Light));

        stub.answer_with("");
        assert_eq!(GsettingsBackend.read(), Some(DesktopPreference::Light));
    }
}
