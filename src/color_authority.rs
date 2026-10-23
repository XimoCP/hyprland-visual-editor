//! Declarative colour-authority descriptor (`odd/tasks/hve-capability-routing.md`
/// unit 1a, serving the rule in `odd/tasks/theme-owns-the-palette.md`).
///
/// Meaning: when the file exists, the applied theme's backend owns the
/// colours — it names the backend, the theme, the theme's saved
/// `palette.json` snapshot and the palette name that snapshot restores as.
/// When the file is ABSENT, no theme claims authority and the colour
/// pipeline follows the live source, exactly as today.
///
/// Write-only for now: providers create, update and remove the descriptor;
/// NOTHING reads it yet (no behaviour change), until the central colour
/// scripts learn to consult it as a declaration they only read.
use crate::config::hve_cache_dir;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// File name of the descriptor inside HVE's cache dir.
pub const DESCRIPTOR_FILE: &str = "color-authority.json";

/// File name of the keeper-palette marker inside HVE's cache dir (W5 of
/// `odd/tasks/palette-authority-mute-and-yield.md`).
///
/// Meaning: the keeper changed the palette by hand (normally in Noctalia), so
/// the theme's frozen snapshot must stop painting HVE's own overlay and the
/// LIVE palette rules it. The descriptor is deliberately KEPT: the engine
/// mute is a skwd-side concern, and only a foreign BACKGROUND change or a
/// theme re-apply touches it. Written by the app on the legitimate-change
/// path; cleared when a theme claims the colours again (`write_descriptor`)
/// or the claim goes away (`clear_descriptor`).
pub const KEEPER_PALETTE_FILE: &str = "keeper-palette.json";

/// Which backend owns the colours and where its snapshot lives.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ColorAuthority {
    pub backend: String,
    pub theme: String,
    pub palette_file: String,
    pub palette_name: String,
}

/// Absolute path of the descriptor on this machine.
pub fn descriptor_path() -> PathBuf {
    hve_cache_dir().join(DESCRIPTOR_FILE)
}

/// Absolute path of the keeper-palette marker on this machine.
pub fn keeper_palette_path() -> PathBuf {
    hve_cache_dir().join(KEEPER_PALETTE_FILE)
}

/// Unique temp-file sibling of a cache file: same directory (same filesystem,
/// so the rename stays an atomic swap), distinct per invocation (pid +
/// monotonic counter) so concurrent writers never collide.
fn temp_sibling(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = hve_cache_dir();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".{}.{}.{}.tmp", tag, std::process::id(), seq))
}

/// Write `data` to `path` atomically (temp file + rename in the same
/// directory), mode 0600 on unix. Creates the cache dir when missing. A
/// failed write leaves no temp file behind.
fn write_atomic(path: &std::path::Path, data: &[u8], tag: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = temp_sibling(tag);
    let result = (|| {
        fs::write(&tmp, data)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Write the descriptor atomically, mode 0600 on unix. A theme claiming the
/// colours also CLEARS any keeper-palette takeover (W5): the applied theme's
/// own palette rules again.
pub fn write_descriptor(authority: &ColorAuthority) -> std::io::Result<()> {
    let path = descriptor_path();
    let data = serde_json::to_string_pretty(authority)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let result = write_atomic(&path, data.as_bytes(), "color-authority");
    if result.is_ok() {
        let _ = clear_keeper_palette();
        crate::decision_log::record(
            "CLAIMED",
            &format!(
                "theme \"{}\" owns the palette (backend {})",
                authority.theme, authority.backend
            ),
        );
    }
    result
}

/// Read the descriptor. Returns `None` on any error — missing file,
/// unreadable file, malformed JSON or unknown fields. Never panics, never
/// propagates.
pub fn read_descriptor() -> Option<ColorAuthority> {
    let raw = fs::read_to_string(descriptor_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Remove the descriptor. Removing an absent file is Ok. Losing the claim
/// also clears the keeper-palette marker: a takeover has nothing to mean
/// without a theme claiming the colours (W5).
pub fn clear_descriptor() -> std::io::Result<()> {
    let result = match fs::remove_file(descriptor_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    };
    let _ = clear_keeper_palette();
    result
}

/// Mark that the keeper changed the palette by hand: the theme's frozen
/// snapshot stops painting HVE's own overlay (the live palette rules it)
/// while the engine mute — a skwd-side concern — stays held. Idempotent;
/// the body is for the keeper's eyes, everything reads it by existence.
pub fn mark_keeper_palette(backend: &str, theme: &str) -> std::io::Result<()> {
    let data = serde_json::json!({ "backend": backend, "theme": theme }).to_string();
    write_atomic(&keeper_palette_path(), data.as_bytes(), "keeper-palette")
}

/// Whether the keeper owns the palette by hand right now (W5).
pub fn keeper_owns_palette() -> bool {
    keeper_palette_path().exists()
}

/// The keeper took the colours over through the engine's own picker (W2/W6
/// step-aside of `odd/tasks/palette-authority-survives-restart.md`, S2): the
/// engine paints the background and derives the palette from it, so nothing in
/// HVE owns the colours any more. Recorded in the SAME marker as a hand-made
/// palette change on purpose — one state, one predicate
/// ([`keeper_owns_palette`]), one clear point ([`write_descriptor`] on the next
/// theme claim) — so that a restart cannot rebuild the theme's claim over the
/// keeper's own choice.
pub fn mark_keeper_background(engine: &str) -> std::io::Result<()> {
    let data = serde_json::json!({ "engine": engine, "origin": "keeper-background" }).to_string();
    write_atomic(&keeper_palette_path(), data.as_bytes(), "keeper-palette")
}

/// File name of the self-write stamp inside HVE's cache dir.
///
/// Meaning: HVE itself just wrote a palette into the backend (a repair, a
/// granted re-assert, the picker-voice close). That write comes back through
/// the colour watcher as an EXTERNAL change, and reading it as the keeper's
/// hand disarmed the guard for good (measured 2026-10-23 17:31:44: the repair
/// repainted the theme's palette and the refusal marked the keeper one second
/// later). The stamp lets the refusal tell its own write from a foreign one.
pub const SELF_WRITE_FILE: &str = "color-write.last";

/// How long a self-write stamp may explain a palette change. Short on
/// purpose: it must cover the watcher's debounce and inner burst loop, and
/// nothing more, so a genuine foreign write right after ours is still judged
/// on its own evidence (the engine's palette mtime settles that tie).
const SELF_WRITE_WINDOW_SECS: u64 = 10;

/// Absolute path of the self-write stamp on this machine.
pub fn self_write_path() -> PathBuf {
    hve_cache_dir().join(SELF_WRITE_FILE)
}

/// Record "HVE wrote the palette now". Best-effort by contract: a failed
/// stamp must never block a repair.
pub fn stamp_self_write() -> std::io::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    write_atomic(&self_write_path(), now.to_string().as_bytes(), "color-write")
}

/// Epoch seconds of the last self-write, when the stamp exists and parses.
pub fn self_write_epoch() -> Option<u64> {
    fs::read_to_string(self_write_path())
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// Whether HVE wrote a palette within [`SELF_WRITE_WINDOW_SECS`].
pub fn self_write_recent() -> bool {
    let Some(stamp) = self_write_epoch() else {
        return false;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(stamp) <= SELF_WRITE_WINDOW_SECS
}

/// Drop a marker the engine has since overwritten.
///
/// The marker means "the keeper chose this palette by hand". If the engine's
/// own wallpaper-derived palette was written AFTER the marker, that sentence
/// is no longer true: the last hand that touched the palette was the machine's
/// (measured 2026-10-22 04:48:27 marker vs 2026-10-23 16:54:11 engine write).
/// Called on resume from suspend, where the keeper's own logic holds: waking
/// up is not a palette choice.
///
/// Returns `true` when a marker was dropped. Absent marker or absent engine
/// palette → `false`, silently: nothing to judge.
pub fn drop_marker_the_engine_overwrote() -> bool {
    let marker = match fs::metadata(keeper_palette_path()).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let Some(engine) = crate::providers::engine_palette_modified() else {
        return false;
    };
    if engine > marker && clear_keeper_palette().is_ok() {
        crate::decision_log::record(
            "KEEPER PALETTE DROPPED",
            "the engine wrote its own palette after the marker; waking up is not a palette choice",
        );
        return true;
    }
    false
}

/// Drop the keeper-palette marker. Removing an absent marker is Ok.
pub fn clear_keeper_palette() -> std::io::Result<()> {
    match fs::remove_file(keeper_palette_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempEnv;

    fn sample() -> ColorAuthority {
        ColorAuthority {
            backend: "noctalia-v5".to_string(),
            theme: "Animation".to_string(),
            palette_file: "/tmp/themes/Animation/providers/noctalia-v5/palette.json".to_string(),
            palette_name: "Animation-vib".to_string(),
        }
    }

    #[test]
    fn round_trip_write_read() {
        let _env = TempEnv::new();
        let authority = sample();
        write_descriptor(&authority).expect("write must succeed");
        assert_eq!(read_descriptor(), Some(authority));
    }

    /// W3 of `odd/tasks/palette-authority-mute-and-yield.md`: writing the
    /// descriptor is the moment a theme CLAIMS the palette, so the keeper
    /// must find it in the decision log. Read inside the `TempEnv` sandbox.
    #[test]
    fn writing_the_descriptor_records_the_claim_in_the_decision_log() {
        let _env = TempEnv::new();
        write_descriptor(&sample()).expect("write must succeed");
        let text = fs::read_to_string(crate::decision_log::log_path()).unwrap_or_default();
        assert!(
            text.contains("CLAIMED") && text.contains("Animation"),
            "the claim must name the theme, got: {text}"
        );
    }

    #[test]
    fn missing_file_reads_as_none() {
        let _env = TempEnv::new();
        assert!(!descriptor_path().exists());
        assert_eq!(read_descriptor(), None);
    }

    #[test]
    fn malformed_json_reads_as_none() {
        let _env = TempEnv::new();
        let path = descriptor_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(read_descriptor(), None);
    }

    #[test]
    fn unknown_field_is_refused() {
        let _env = TempEnv::new();
        let path = descriptor_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"backend":"noctalia-v5","theme":"Animation","palette_file":"p","palette_name":"n","extra":"nope"}"#,
        )
        .unwrap();
        assert_eq!(
            read_descriptor(),
            None,
            "deny_unknown_fields must refuse extra keys"
        );
    }

    #[test]
    fn clear_on_missing_file_is_ok() {
        let _env = TempEnv::new();
        assert!(!descriptor_path().exists());
        clear_descriptor().expect("clearing an absent descriptor must be Ok");
    }

    #[test]
    fn clear_removes_an_existing_descriptor() {
        let _env = TempEnv::new();
        write_descriptor(&sample()).expect("write must succeed");
        assert!(descriptor_path().exists());
        clear_descriptor().expect("clear must succeed");
        assert_eq!(read_descriptor(), None);
    }

    #[test]
    fn successful_write_leaves_no_temp_file_behind() {
        let _env = TempEnv::new();
        write_descriptor(&sample()).expect("write must succeed");
        let leftovers: Vec<_> = fs::read_dir(hve_cache_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temp file may survive a write: {:?}",
            leftovers
        );
    }

    #[test]
    #[cfg(unix)]
    fn descriptor_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let _env = TempEnv::new();
        write_descriptor(&sample()).expect("write must succeed");
        let mode = fs::metadata(descriptor_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "descriptor must be owner-only");
    }

    // ── W5: the keeper-palette marker (skwd and Noctalia are separate) ────
    //
    // A hand-made palette change releases the theme's COLOURS for HVE's own
    // overlay; it must never touch the engine mute, which is what the
    // descriptor (kept) holds. The marker's whole lifecycle is pinned here.

    #[test]
    fn keeper_palette_marker_round_trip_and_clear() {
        let _env = TempEnv::new();
        assert!(!keeper_owns_palette(), "nothing is marked at the start");
        mark_keeper_palette("noctalia-v5", "Work").expect("mark must succeed");
        assert!(keeper_owns_palette(), "the marker must be visible");
        assert!(keeper_palette_path().exists());
        clear_keeper_palette().expect("clear must succeed");
        assert!(!keeper_owns_palette());
        clear_keeper_palette().expect("clearing an absent marker must be Ok");
    }

    /// A theme apply writes the descriptor to claim the colours; that same
    /// moment must drop any keeper takeover, so the applied theme's own
    /// colours rule again.
    #[test]
    fn a_theme_claim_clears_the_keeper_palette_marker() {
        let _env = TempEnv::new();
        mark_keeper_palette("noctalia-v5", "Old").expect("mark must succeed");
        write_descriptor(&sample()).expect("write must succeed");
        assert!(
            !keeper_owns_palette(),
            "a theme re-claim must clear the keeper's takeover"
        );
    }

    /// Losing the claim (step-aside, theme removal) must take the keeper's
    /// takeover with it: with no theme claiming, the marker has nothing to
    /// mean and must never outlive its descriptor.
    #[test]
    fn clearing_the_descriptor_clears_the_keeper_palette_marker() {
        let _env = TempEnv::new();
        write_descriptor(&sample()).expect("write must succeed");
        mark_keeper_palette("noctalia-v5", "Animation").expect("mark must succeed");
        clear_descriptor().expect("clear must succeed");
        assert!(
            !keeper_owns_palette(),
            "clearing the claim must clear the keeper's takeover"
        );
    }

    /// A marker the engine has since overwritten was never the keeper's hand,
    /// so waking up drops it and the guard comes back armed. Measured case:
    /// marker 2026-10-22 04:48:27, engine write 2026-10-23 16:54:11.
    #[test]
    fn a_marker_the_engine_overwrote_is_dropped() {
        let _env = TempEnv::new();
        mark_keeper_palette("noctalia-v5", "Work").expect("mark must succeed");
        backdate(&keeper_palette_path(), "2 hours ago");
        crate::providers::publish_engine_palette_for_test()
            .expect("the engine palette must be publishable");

        assert!(
            drop_marker_the_engine_overwrote(),
            "a marker older than the engine's own write must be dropped"
        );
        assert!(!keeper_owns_palette());
    }

    /// The keeper's own choice survives: a marker NEWER than the engine's last
    /// publish is a real hand-made choice, and resume must leave it alone.
    #[test]
    fn a_marker_newer_than_the_engine_palette_survives_on_resume() {
        let _env = TempEnv::new();
        crate::providers::publish_engine_palette_for_test()
            .expect("the engine palette must be publishable");
        let engine = crate::providers::engine_palette_path()
            .expect("the engine palette path must resolve");
        backdate(&engine, "2 hours ago");
        mark_keeper_palette("noctalia-v5", "Work").expect("mark must succeed");

        assert!(
            !drop_marker_the_engine_overwrote(),
            "the keeper's later choice must not be dropped"
        );
        assert!(keeper_owns_palette());
    }

    /// Backdate a file so the marker/engine order is unambiguous without
    /// sleeping a real second in the suite.
    fn backdate(path: &std::path::Path, when: &str) {
        let status = std::process::Command::new("touch")
            .arg("-d")
            .arg(when)
            .arg(path)
            .status()
            .expect("touch must run");
        assert!(status.success(), "touch must succeed on {path:?}");
    }
}
