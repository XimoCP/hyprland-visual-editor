//! W3 of `odd/tasks/palette-authority-mute-and-yield.md`: the guard's
//! decision log.
//!
//! WHY: in production HVE is started by the session/tray, so its `tracing`
//! output goes to `/dev/null`. Every palette-authority decision (mute set,
//! mute held, released, step-aside, engine absent) was invisible, which is
//! why the 07-oct failure took three rounds of questions to read. This
//! module writes one timestamped line per decision to a small file under
//! HVE's cache dir, which the keeper can read directly.
//!
//! Format (same shape as the watcher's own `color_watcher.log`): one line
//! per decision, `[HVE Authority] <UTC timestamp> <DECISION> <detail>`,
//! append-only and best-effort. A failed write is reported through
//! `tracing` and NEVER aborts the decision it records.
//!
//! Bounded growth: the file is capped at [`MAX_BYTES`]; before an append
//! that would push it past the cap it is rotated to `<log>.1` (one previous
//! generation, replacing any older one). Disk use is therefore bounded by
//! `2 * MAX_BYTES`, and the most recent history is always available.

use crate::config::hve_cache_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// File name of the decision log inside HVE's cache dir.
pub(crate) const LOG_FILE: &str = "color-authority.log";

/// Rotation threshold: the live log is rotated to `.1` before any append
/// that would push it past this size.
pub(crate) const MAX_BYTES: u64 = 256 * 1024;

/// Absolute path of the live decision log on this machine.
pub(crate) fn log_path() -> PathBuf {
    hve_cache_dir().join(LOG_FILE)
}

/// Absolute path of the single previous generation.
pub(crate) fn rotated_path() -> PathBuf {
    hve_cache_dir().join(format!("{LOG_FILE}.1"))
}

/// Record one decision. Best-effort: any failure is reported via `tracing`
/// and swallowed, so a decision is never aborted by a logging problem.
pub(crate) fn record(decision: &str, detail: &str) {
    let line = format!("[HVE Authority] {} {} {}\n", timestamp(), decision, detail);
    if let Err(e) = append_bounded(&line) {
        tracing::warn!(
            "[decision-log] could not write {}: {}",
            log_path().display(),
            e
        );
    }
}

fn append_bounded(line: &str) -> std::io::Result<()> {
    append_bounded_with(line, crate::open_private_log)
}

/// The opener is injected so a FAILED tighten (a chmod that cannot be
/// performed) is observable in a test: as the file's owner a real failure
/// cannot be produced without root. `record` still swallows the error — this
/// module never aborts the decision it records.
fn append_bounded_with(
    line: &str,
    open: impl FnOnce(&Path) -> std::io::Result<fs::File>,
) -> std::io::Result<()> {
    use std::io::Write;
    let path = log_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    rotate_if_needed(&path)?;
    let mut file = open(&path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// Rotate the live log to its single previous generation when it has reached
/// the cap. A missing file is a no-op; an older `.1` is replaced.
fn rotate_if_needed(path: &Path) -> std::io::Result<()> {
    let size = match fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if size < MAX_BYTES {
        return Ok(());
    }
    let rotated = rotated_path();
    let _ = fs::remove_file(&rotated);
    fs::rename(path, &rotated)?;
    // The rotated copy is the OLD file: created before this hardening it is
    // world-readable, which is the same leak this unit closes. Best-effort —
    // a failure never blocks the decision being recorded.
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(&rotated, fs::Permissions::from_mode(0o600));
    Ok(())
}

fn timestamp() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let (y, mo, d, h, mi, s) = epoch_to_utc(dur.as_secs());
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Convert epoch seconds to UTC date/time components (no `chrono` dep).
fn epoch_to_utc(epoch: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = epoch / 86400;
    let time_secs = epoch % 86400;
    let h = time_secs / 3600;
    let mi = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d, h, mi, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempEnv;

    #[test]
    fn records_one_timestamped_decision_line() {
        let _env = TempEnv::new();
        record("MUTE SET", "theme.policy \"wallpaper\" -> \"off\"");
        let text = fs::read_to_string(log_path()).expect("the decision log must exist");
        let line = text.lines().next().expect("one line");
        assert!(
            line.starts_with("[HVE Authority] "),
            "the log must carry its tag, got: {line:?}"
        );
        let rest = line.trim_start_matches("[HVE Authority] ");
        assert_eq!(
            rest.as_bytes().get(19),
            Some(&b'Z'),
            "the timestamp must be UTC with a Z, got: {rest:?}"
        );
        assert!(
            rest.contains("MUTE SET theme.policy"),
            "the decision and its detail must be on the line, got: {rest:?}"
        );
    }

    #[test]
    fn a_full_log_rotates_to_one_previous_generation() {
        let _env = TempEnv::new();
        let path = log_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let filler = "x".repeat(MAX_BYTES as usize);
        fs::write(&path, &filler).unwrap();

        record("MUTE SET", "after rotation");

        let rotated = rotated_path();
        assert!(rotated.exists(), "a full log must rotate to .1");
        assert_eq!(
            fs::read(&rotated).unwrap().len(),
            MAX_BYTES as usize,
            "the previous generation must keep the old bytes"
        );
        let fresh = fs::read_to_string(&path).unwrap();
        assert!(
            fresh.contains("MUTE SET after rotation"),
            "the live log must hold the new decision, got: {fresh:?}"
        );
        assert!(
            (fresh.len() as u64) < MAX_BYTES,
            "the live log must restart below the cap, got {} bytes",
            fresh.len()
        );
    }

    #[test]
    fn a_failed_write_never_panics_and_never_aborts_the_decision() {
        let _env = TempEnv::new();
        // Make the cache dir itself a FILE: creating or writing the log must
        // fail, and `record` must swallow it instead of panicking.
        let cache = hve_cache_dir();
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, b"not a directory").unwrap();

        record("MUTE SET", "must not panic");

        assert!(
            cache.is_file(),
            "the unwritable cache path must be left as-is"
        );
        assert!(
            !log_path().exists(),
            "a failed write must not create a log file"
        );
    }

    // ── W8: the decision log is private (mode 0600) ──────────────────
    //
    // It records which palette decisions the guard took, line by line. It is
    // a log of the session, so it is created 0600 AND an existing file from
    // before this hardening is tightened on the next append — that old
    // world-readable file is the leak.

    /// File mode (permission bits only) of `path`.
    fn file_mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .expect("path must be readable")
            .permissions()
            .mode()
            & 0o777
    }

    /// Set the permission bits of `path` (fixture setup).
    fn set_file_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod the fixture");
    }

    #[test]
    fn the_decision_log_is_created_with_mode_0600() {
        let _env = TempEnv::new();

        record("MUTE SET", "the log must be private");

        assert_eq!(
            file_mode(&log_path()),
            0o600,
            "color-authority.log must be private to its owner"
        );
    }

    #[test]
    fn a_preexisting_world_readable_decision_log_is_tightened_and_never_truncated() {
        let _env = TempEnv::new();
        let path = log_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[HVE Authority] older line\n").unwrap();
        set_file_mode(&path, 0o644);

        record("MUTE SET", "after the tighten");

        assert_eq!(
            file_mode(&path),
            0o600,
            "a decision log created before this change must be tightened"
        );
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("[HVE Authority] older line\n"),
            "the existing lines must survive, got: {text:?}"
        );
        assert!(
            text.contains("MUTE SET after the tighten"),
            "the new decision must be appended, got: {text:?}"
        );
    }

    #[test]
    fn a_failed_tighten_returns_the_error_and_keeps_the_existing_lines() {
        let _env = TempEnv::new();
        let path = log_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[HVE Authority] older line\n").unwrap();
        set_file_mode(&path, 0o644);

        let err = append_bounded_with("[HVE Authority] must not land\n", |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "chmod denied",
            ))
        })
        .expect_err("a failed chmod on the create path must be returned");

        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[HVE Authority] older line\n",
            "a failed tighten must not destroy or truncate the decision log"
        );
        assert_eq!(file_mode(&path), 0o644, "and must not pretend it succeeded");
    }
}
