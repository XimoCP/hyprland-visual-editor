//! The `read-desktop-preference` capability
//! (`openspec/specs/capability-routing/spec.md`).
//!
//! The core states WHAT it wants — a system-wide desktop preference such as
//! the light/dark theme — and gets a [`DesktopPreference`] back. It never
//! names the probe that produced the answer: every backend declared here
//! says what it reads, answers `None` when its source is unavailable, and
//! the router walks the DECLARED ORDER until one answers. Backends are
//! siblings (spec: "Backends Are Siblings, Never a Chain"): one never calls
//! another; only this router sequences them.
//!
//! The backend contract as it applies to a read-only probe:
//!
//! - `id` — the stable declared name, returned by
//!   [`DesktopPreferenceBackend::id`].
//! - `capabilities` — declared by placement: a backend declared in this
//!   module implements `read-desktop-preference` and nothing else.
//! - `files-owned` / `watch-paths` / `deletable-artefacts` / `refresh` —
//!   none: these backends only READ their source. The file a probe reads
//!   belongs to the tool that writes it, never to HVE.
//!
//! The order in `Default for DesktopPreferenceRouter` IS the precedence —
//! exactly the fallback order the core used before this seam existed,
//! ending in [`DEFAULT_PREFERENCE`]. Selection is by declared order, not by
//! config: whether `disabled_providers` ever gates this router is an OPEN
//! QUESTION in the spec, so this seam invents no switch.

pub mod darkman;
pub mod gsettings;

/// One answer of the system-wide desktop preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPreference {
    Light,
    Dark,
}

impl DesktopPreference {
    /// The core's only consumption question: should the surface go dark?
    pub fn is_dark(self) -> bool {
        matches!(self, DesktopPreference::Dark)
    }
}

/// Backend contract for one source of the preference: a backend is
/// declarative — it states what it is (`id`) and answers when asked; the
/// core reads those declarations and never executes a probe itself.
pub trait DesktopPreferenceBackend: Send + Sync {
    /// Stable declared name of the source (backend contract `id`).
    ///
    /// It is a declaration for review and contract tests, not part of the
    /// pick: the router picks by declared order (see `Default`). Production
    /// code never reads it — same shape as `ThemeProvider`'s declaration
    /// methods that only the suite exercises.
    #[allow(dead_code)]
    fn id(&self) -> &'static str;

    /// Read the preference. `None` = this backend cannot answer (its source
    /// is unavailable); the router then tries the next one in declared
    /// order. A backend that CAN answer always answers: an empty or
    /// unexpected payload maps to a real preference, never to "keep falling
    /// through" — that is the behaviour the core had before the seam
    /// existed, preserved here.
    fn read(&self) -> Option<DesktopPreference>;
}

/// The answer applied when NO backend can answer: dark — the core's
/// historical default before the seam existed.
pub const DEFAULT_PREFERENCE: DesktopPreference = DesktopPreference::Dark;

/// The capability's router: walks its backends in declared order and takes
/// the first answer.
pub struct DesktopPreferenceRouter {
    backends: Vec<Box<dyn DesktopPreferenceBackend>>,
}

impl DesktopPreferenceRouter {
    /// Router over an explicit ordered backend list — the list IS the
    /// declared precedence.
    pub fn new(backends: Vec<Box<dyn DesktopPreferenceBackend>>) -> Self {
        Self { backends }
    }

    /// First backend that answers decides; when none answers,
    /// [`DEFAULT_PREFERENCE`] applies.
    pub fn read(&self) -> DesktopPreference {
        self.backends
            .iter()
            .find_map(|backend| backend.read())
            .unwrap_or(DEFAULT_PREFERENCE)
    }
}

impl Default for DesktopPreferenceRouter {
    /// The declared precedence — byte-for-byte the core's former fallback
    /// order: the GNOME settings key first, the darkman indicator file
    /// second, [`DEFAULT_PREFERENCE`] last. This `Vec` is the one
    /// registration point for the capability, exactly like
    /// `providers::register_default_providers` for the provider list.
    fn default() -> Self {
        Self::new(vec![
            Box::new(gsettings::GsettingsBackend),
            Box::new(darkman::DarkmanBackend),
        ])
    }
}

/// What the core calls: read the preference through the default router.
/// Every call reads fresh — the seam adds no caching, because the code it
/// replaced had none.
pub fn read_desktop_preference() -> DesktopPreference {
    DesktopPreferenceRouter::default().read()
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend whose id and answer are both declared by the test — the
    /// core cannot tell it from a real probe.
    struct Answer(&'static str, Option<DesktopPreference>);

    impl DesktopPreferenceBackend for Answer {
        fn id(&self) -> &'static str {
            self.0
        }

        fn read(&self) -> Option<DesktopPreference> {
            self.1
        }
    }

    /// Precedence, fallback and default, all driven through fake backends:
    /// the first backend that answers decides; an unavailable backend hands
    /// over to the next one in declared order; when nothing answers the
    /// historical default (dark) applies.
    #[test]
    fn first_answer_wins_then_falls_back_then_defaults_to_dark() {
        let first_decides = DesktopPreferenceRouter::new(vec![
            Box::new(Answer("first", Some(DesktopPreference::Dark))),
            Box::new(Answer("second", Some(DesktopPreference::Light))),
        ]);
        assert_eq!(first_decides.read(), DesktopPreference::Dark);

        let hands_over = DesktopPreferenceRouter::new(vec![
            Box::new(Answer("first", None)),
            Box::new(Answer("second", Some(DesktopPreference::Light))),
        ]);
        assert_eq!(hands_over.read(), DesktopPreference::Light);

        let nothing_answers = DesktopPreferenceRouter::new(vec![
            Box::new(Answer("first", None)),
            Box::new(Answer("second", None)),
        ]);
        assert_eq!(nothing_answers.read(), DEFAULT_PREFERENCE);
        assert_eq!(nothing_answers.read(), DesktopPreference::Dark);
    }

    /// The DEFAULT router's declared precedence — read through each
    /// backend's own `id()` so the order is pinned WITHOUT executing a
    /// single probe. Swapping the registration order fails here.
    #[test]
    fn the_default_router_declares_the_historical_precedence() {
        let ids: Vec<&str> = DesktopPreferenceRouter::default()
            .backends
            .iter()
            .map(|backend| backend.id())
            .collect();
        assert_eq!(
            ids,
            vec![
                gsettings::GsettingsBackend.id(),
                darkman::DarkmanBackend.id(),
            ],
            "declared precedence: the settings key first, the indicator file second"
        );
    }
}
