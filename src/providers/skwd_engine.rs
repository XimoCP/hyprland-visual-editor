//! The skwd engine adapter: the one place HVE hands a wallpaper path to
//! the skwd-wall-v2 / skwd-helm engine pair.
//!
//! Moved verbatim out of `noctalia.rs` for the capability-routing
//! contract (`openspec/specs/capability-routing/spec.md`): the engine is
//! an independent backend, so no provider names it — the
//! `apply-background` router (`crate::providers::background`) selects the
//! engine and calls in here. Semantics, log lines, error strings, the
//! `SKWD_WALL_V2_SOCK` env seam and the measured comments are unchanged.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Best-effort delegation to skwd-walld (now skwd-wall-v2 / skwd-helm).
///
/// HVE remains agnostic: if the daemon's socket is absent we do nothing.
/// If it is present we try `skwd-helm apply <path>` (and `skwd-wall-v2 apply` as fallback).
///
/// D1: returns whether the wallpaper was actually applied. Only a real
/// success reports success — every other outcome (absent socket, invalid
/// path, failing or unreachable daemon) returns false so the caller keeps
/// the fallback routes (manifest, static restore) instead of claiming a
/// background that was never painted. Failures are logged (warn), never
/// silent and never fatal to the theme apply.
fn skwd_wall_socket_path() -> PathBuf {
    if let Ok(custom) = std::env::var("SKWD_WALL_V2_SOCK") {
        let p = PathBuf::from(&custom);
        if p.exists() {
            return p;
        }
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/user/1000"));
    runtime.join("skwd-wall-v2").join("wall.sock")
}

/// Whether the engine's socket exists AND really is a socket — its gate for
/// painting a wallpaper. Symlinks are FOLLOWED (the delegation gate uses
/// `Path::exists()`, which follows), so a symlink to a live socket counts as
/// available. A stale regular FILE at the path — or a symlink to one — is not
/// a listening socket, so it must not report the engine as available; that
/// wrong "available" silenced the video-backend notice exactly where it was
/// needed. Exposed so the `apply-background` router can decide, without naming
/// the engine backend, whether a video backend is available at all.
pub(crate) fn socket_available() -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(skwd_wall_socket_path())
        .map(|meta| meta.file_type().is_socket())
        .unwrap_or(false)
}

/// The gate the delegation itself uses (`Path::exists`, symlinks followed):
/// with it closed `delegate_to_skwd_walld` never spawns, so a route to this
/// painter would be a route to nothing. Kept separate from
/// [`socket_available`] on purpose — that one answers "is a live socket
/// there?" for the video-backend notice, this one mirrors the delegation.
pub(crate) fn engine_reachable() -> bool {
    skwd_wall_socket_path().exists()
}

pub(crate) fn delegate_to_skwd_walld(wallpaper_path: &Path) -> bool {
    let socket = skwd_wall_socket_path();
    if !socket.exists() {
        tracing::debug!(
            "[skwd-wall] socket absent at {:?} — skipping delegation",
            socket
        );
        return false;
    }
    let path_str = match wallpaper_path.to_str() {
        Some(s) if !s.is_empty() => s,
        _ => {
            tracing::warn!("[skwd-wall] invalid wallpaper path {:?}", wallpaper_path);
            return false;
        }
    };
    // W6 (theme-package trust): this path can come from a THEME PACKAGE
    // (`wallpaper.txt`, a packaged poster), so its value is
    // ATTACKER-CONTROLLED. A value that STARTS WITH `-` would be parsed by
    // `skwd-helm` / `skwd-wall-v2` as an OPTION instead of as the path to
    // paint (argument injection). Refuse it HERE, before any spawn: no
    // `--` end-of-options marker is added because neither engine's support
    // for it can be confirmed from this repo, and an unsupported `--` would
    // break every real apply. A legitimate wallpaper path — absolute or
    // relative — never starts with `-`, so this never touches the normal
    // path.
    if path_str.starts_with('-') {
        tracing::warn!(
            "[skwd-wall] refusing wallpaper path {:?}: a leading '-' could be \
             read as an option by the engine, not as the path to paint",
            path_str
        );
        return false;
    }
    // PICK THE RIGHT ENGINE FOR THE JOB, the other as fallback.
    //
    // Measured on this machine (2026-09-26), and the numbers are NOT a
    // fastest-first ordering — each engine is fast at its own thing:
    //   still image: skwd-wall-v2 ~3ms   | skwd-helm ~616ms
    //   video:       skwd-wall-v2 ~472ms | skwd-helm ~90ms
    // Both accept both, so either order "works"; the order decides who does the
    // work. Sending a video to the still-image path wastes ~380ms, and sending a
    // still to the video engine wastes ~525ms — and the theme apply runs on the
    // UI thread, so those are milliseconds the window cannot repaint.
    //
    // The old code always tried `skwd-helm` first under a 500ms cap, so the
    // still-image case logged "timed out after 500ms" and killed the helper
    // mid-work before falling through. The log lied and the path was wasteful.
    let order: [&str; 2] = if crate::theme_media::is_video_path(wallpaper_path) {
        ["skwd-helm", "skwd-wall-v2"]
    } else {
        ["skwd-wall-v2", "skwd-helm"]
    };
    for (attempt, bin) in order.iter().enumerate() {
        match run_delegate_apply(bin, path_str, delegate_timeout(attempt)) {
            Ok(()) => {
                tracing::info!("[skwd-wall] delegated wallpaper to {}: {}", bin, path_str);
                return true;
            }
            Err(reason) => {
                tracing::warn!("[skwd-wall] {} apply failed: {}", bin, reason);
                // try next bin
            }
        }
    }
    tracing::warn!(
        "[skwd-wall] delegation failed for {} (daemon may be unreachable)",
        path_str
    );
    false
}

/// Bounded wait for ONE delegation `apply` call, so a hung daemon helper can
/// never freeze the theme apply (never held across I/O locks).
///
/// This runs on the UI thread, so the number is milliseconds the window cannot
/// repaint. The FIRST attempt is always the engine this machine measured as fast
/// for that role, so a short cap is honest there: an engine that slow means the
/// daemon is busy, and falling through to the other one is the cheaper path. The
/// FALLBACK keeps a long cap, because it may be the engine that is slow for the
/// role (616 ms measured for `skwd-helm` on a still image).
///
/// Measured on this machine (2026-09-26), healthy single-engine times:
///   still image: `skwd-wall-v2` ~3 ms   | `skwd-helm` ~616 ms
///   video:       `skwd-wall-v2` ~472 ms | `skwd-helm` ~90 ms
///
/// The failure this split fixes (keeper's log, 2026-10-11): `skwd-wall-v2` timed
/// out at the single 700 ms cap on still images six times while the fallback
/// answered ~150 ms later — 700 ms of frozen window spent waiting for an engine
/// that was never going to answer.
const FIRST_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(300);
const FALLBACK_TIMEOUT: Duration = Duration::from_millis(800);

/// The cap for `attempt`: 0 is the preferred engine, every later one is the
/// fallback.
fn delegate_timeout(attempt: usize) -> Duration {
    if attempt == 0 {
        FIRST_ATTEMPT_TIMEOUT
    } else {
        FALLBACK_TIMEOUT
    }
}

fn run_delegate_apply(bin: &str, path_str: &str, timeout: Duration) -> Result<(), String> {
    let mut child = std::process::Command::new(bin)
        .arg("apply")
        .arg(path_str)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("not available: {}", e))?;
    let start = Instant::now();
    loop {
        match child
            .try_wait()
            .map_err(|e| format!("wait failed: {}", e))?
        {
            Some(_) => {
                let out = child
                    .wait_with_output()
                    .map_err(|e| format!("output read failed: {}", e))?;
                if out.status.success() {
                    return Ok(());
                }
                return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
            }
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("timed out after {:?}", timeout));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

// ── The engine's own picker program: presence probe (W2/W6 of
// `odd/tasks/palette-authority-mute-and-yield.md`) ─────────────────────
//
// The engine's own picker/window program is `skwd-wall-v2` — a DIFFERENT
// process name from the `skwd-walld` daemon — so its presence means the
// keeper is driving the engine by hand. That is engine knowledge, so it lives
// HERE with the engine adapter; the `apply-background` router only asks this
// capability and never names the program itself.

/// The engine's own picker/window program.
const ENGINE_PICKER_BIN: &str = "skwd-wall-v2";

/// Whether the engine's own picker program is running right now, by scanning a
/// procfs-like directory for a numeric pid whose `comm` is the picker.
/// `Some(true)` = found, `Some(false)` = the table was readable and it is not
/// there, `None` = the table could not be read (the caller then holds,
/// conservatively). Cheap: one directory scan and one small file read per pid,
/// no subprocess and no new dependency.
pub(crate) fn picker_running_in(proc_dir: &Path) -> Option<bool> {
    let entries = std::fs::read_dir(proc_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(comm) = std::fs::read_to_string(proc_dir.join(name).join("comm")) {
            if comm.trim() == ENGINE_PICKER_BIN {
                return Some(true);
            }
        }
    }
    Some(false)
}

/// The production picker probe: the real `/proc`.
pub(crate) fn picker_is_running() -> Option<bool> {
    #[cfg(test)]
    {
        if let Some(forced) = test_picker_override() {
            return Some(forced);
        }
    }
    picker_running_in(Path::new("/proc"))
}

/// The Wayland layer namespace of the engine's own picker surface, as
/// Hyprland reports it in `openlayer>>` / `closelayer>>` (captured live
/// 16-oct-2026 with the picker opened and closed by hand). Engine knowledge,
/// so it lives here next to the comm name: the picker is a layer-shell
/// surface — no window event exists for it — and these two events are its
/// open/close signal (keeper's directive: events instead of timers).
pub(crate) const ENGINE_PICKER_LAYER: &str = "skwd-wall";

/// The palette NAME the engine publishes for its own wallpaper-derived
/// colours, and therefore the file it lands in under the backend's palettes
/// directory. Engine knowledge: it is how a palette change can say "the
/// engine did this" instead of guessing the keeper's hand.
pub(crate) const ENGINE_PALETTE_NAME: &str = "skwd-wall";

/// Whether a Hyprland layer namespace is the engine's own picker surface.
/// `skwd-paper` — the still renderer HVE itself launches — is a DIFFERENT
/// namespace, present all the time; it must never read as "the keeper opened
/// the picker".
pub(crate) fn is_picker_layer(namespace: &str) -> bool {
    namespace == ENGINE_PICKER_LAYER
}

/// Test-only override of [`picker_is_running`]: `Some(v)` forces the answer so
/// a hermetic test never depends on what runs on the box; `None` uses the real
/// scan. Compiled only under `cfg(test)`, so production always reads `/proc`.
#[cfg(test)]
static TEST_PICKER_RUNNING: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(0);

#[cfg(test)]
fn test_picker_override() -> Option<bool> {
    match TEST_PICKER_RUNNING.load(std::sync::atomic::Ordering::SeqCst) {
        1 => Some(true),
        -1 => Some(false),
        _ => None,
    }
}

/// Test-only: force the picker answer (`Some`), or clear it (`None`).
#[cfg(test)]
pub(crate) fn set_picker_running_for_test(value: Option<bool>) {
    let raw = match value {
        Some(true) => 1,
        Some(false) => -1,
        None => 0,
    };
    TEST_PICKER_RUNNING.store(raw, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    /// The measured healthy times this machine recorded (2026-09-26, see the
    /// comment above the caps). The test pins the RELATIONSHIP the design needs:
    /// the first attempt gets a short cap because it is always the engine that
    /// is fast for the role, and the fallback keeps a long one because it may be
    /// the slow-for-the-role engine (616 ms measured on a still image).
    const MEASURED_FASTEST_FIRST_ATTEMPT_MS: u128 = 90; // skwd-helm on a video
    const MEASURED_SLOWEST_FALLBACK_MS: u128 = 616; // skwd-helm on a still image

    #[test]
    fn delegate_timeout_keeps_the_first_attempt_short_and_the_fallback_long() {
        let first = delegate_timeout(0).as_millis();
        let fallback = delegate_timeout(1).as_millis();

        assert!(
            first >= MEASURED_FASTEST_FIRST_ATTEMPT_MS,
            "the first attempt must still cover the fastest healthy engine (got {first} ms)"
        );
        assert!(
            fallback >= MEASURED_SLOWEST_FALLBACK_MS,
            "the fallback must still cover the slowest healthy engine (got {fallback} ms)"
        );
        assert!(
            first < fallback,
            "a hung preferred engine must cost less than a hung fallback ({first} vs {fallback} ms)"
        );
    }

    #[test]
    fn delegate_timeout_treats_every_later_attempt_as_the_fallback() {
        assert_eq!(delegate_timeout(1), delegate_timeout(2));
        assert_eq!(delegate_timeout(1), delegate_timeout(7));
    }

    #[test]
    #[serial]
    fn delegate_noop_when_socket_absent() {
        let _env = crate::test_utils::env_guard();
        // Task 2: delegation must be no-op when socket absent and never panic
        let tmp_runtime = TempDir::new().unwrap();
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        std::env::set_var("XDG_RUNTIME_DIR", tmp_runtime.path());
        std::env::remove_var("SKWD_WALL_V2_SOCK");
        // No socket file exists in tmp_runtime/skwd-wall-v2/wall.sock
        delegate_to_skwd_walld(Path::new("/tmp/fake-wallpaper.jpg"));
        // Should not panic and socket path should be absent
        assert!(!skwd_wall_socket_path().exists());
        if let Some(v) = orig_runtime {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        if let Some(v) = orig_sock {
            std::env::set_var("SKWD_WALL_V2_SOCK", v);
        } else {
            std::env::remove_var("SKWD_WALL_V2_SOCK");
        }
    }

    #[test]
    #[serial]
    fn delegate_swallow_failure_when_socket_is_not_socket() {
        let _env = crate::test_utils::env_guard();
        // Task 2: failure must be swallowed, not propagated. D1: it must
        // also REPORT the failure. Stub binaries on PATH (both fail) keep
        // this hermetic: no real `skwd-helm apply` may run during tests.
        let tmp_runtime = TempDir::new().unwrap();
        let sock_dir = tmp_runtime.path().join("skwd-wall-v2");
        std::fs::create_dir_all(&sock_dir).unwrap();
        let sock_path = sock_dir.join("wall.sock");
        // Create a regular file where socket should be — connect will fail
        std::fs::write(&sock_path, b"not a socket").unwrap();
        let bin_dir = tmp_runtime.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        for bin in ["skwd-helm", "skwd-wall-v2"] {
            let stub = bin_dir.join(bin);
            std::fs::write(&stub, "#!/bin/sh\nexit 1\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        let orig_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("XDG_RUNTIME_DIR", tmp_runtime.path());
        std::env::remove_var("SKWD_WALL_V2_SOCK");
        std::env::set_var("PATH", format!("{}:{}", bin_dir.display(), orig_path));
        // Must not panic, must swallow the error — and must report failure
        // so the caller keeps its fallback routes.
        assert!(!delegate_to_skwd_walld(Path::new("/tmp/another.jpg")));
        // restore
        std::env::set_var("PATH", &orig_path);
        if let Some(v) = orig_runtime {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        if let Some(v) = orig_sock {
            std::env::set_var("SKWD_WALL_V2_SOCK", v);
        } else {
            std::env::remove_var("SKWD_WALL_V2_SOCK");
        }
    }

    /// D1: the delegation must report whether it actually applied. An
    /// absent socket (daemon unreachable) is a failed delegation, never a
    /// silent success — the caller gates the fallback routes on this.
    #[test]
    #[serial]
    fn delegate_reports_failure_when_socket_absent() {
        let _env = crate::test_utils::env_guard();
        let tmp_runtime = TempDir::new().unwrap();
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        std::env::set_var("XDG_RUNTIME_DIR", tmp_runtime.path());
        std::env::remove_var("SKWD_WALL_V2_SOCK");
        let applied = delegate_to_skwd_walld(Path::new("/tmp/fake-wallpaper.jpg"));
        assert!(
            !applied,
            "absent socket must report failure so the caller keeps the fallback routes"
        );
        if let Some(v) = orig_runtime {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        if let Some(v) = orig_sock {
            std::env::set_var("SKWD_WALL_V2_SOCK", v);
        } else {
            std::env::remove_var("SKWD_WALL_V2_SOCK");
        }
    }

    /// Defect 2 (verifier): the socket probe must be a SOCKET probe. A stale
    /// regular FILE at the socket path is not a listening socket, so it must
    /// not report the engine as available — that wrong "available" silenced
    /// the video-backend notice exactly where it was needed.
    #[test]
    #[serial]
    fn socket_available_is_false_for_a_regular_file_at_the_socket_path() {
        let _env = crate::test_utils::env_guard();
        let tmp_runtime = TempDir::new().unwrap();
        let sock = tmp_runtime.path().join("wall.sock");
        std::fs::write(&sock, b"not a socket").unwrap();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        std::env::set_var("SKWD_WALL_V2_SOCK", &sock);
        std::env::remove_var("XDG_RUNTIME_DIR");
        let available = socket_available();
        if let Some(v) = orig_sock {
            std::env::set_var("SKWD_WALL_V2_SOCK", v);
        } else {
            std::env::remove_var("SKWD_WALL_V2_SOCK");
        }
        if let Some(v) = orig_runtime {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        assert!(
            !available,
            "a regular file at the socket path is not a listening socket"
        );
    }

    /// The positive half of the same probe: a REAL unix socket at the path must
    /// still report available, so the probe rejects only non-sockets.
    #[test]
    #[serial]
    fn socket_available_is_true_for_a_real_unix_socket() {
        let _env = crate::test_utils::env_guard();
        let tmp_runtime = TempDir::new().unwrap();
        let sock = tmp_runtime.path().join("wall.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        std::env::set_var("SKWD_WALL_V2_SOCK", &sock);
        std::env::remove_var("XDG_RUNTIME_DIR");
        let available = socket_available();
        if let Some(v) = orig_sock {
            std::env::set_var("SKWD_WALL_V2_SOCK", v);
        } else {
            std::env::remove_var("SKWD_WALL_V2_SOCK");
        }
        if let Some(v) = orig_runtime {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        assert!(
            available,
            "a real unix socket at the socket path is an available engine"
        );
    }

    /// Point 2 correction: a symlink at the socket path whose target is a live
    /// socket must count as available. The delegation gate follows symlinks
    /// (`Path::exists()`), so a probe that does not would fire the notice
    /// although the engine can paint.
    #[test]
    #[serial]
    fn socket_available_is_true_for_a_symlink_to_a_real_unix_socket() {
        let _env = crate::test_utils::env_guard();
        let tmp_runtime = TempDir::new().unwrap();
        let target = tmp_runtime.path().join("real.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&target).unwrap();
        let link = tmp_runtime.path().join("wall.sock");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        std::env::set_var("SKWD_WALL_V2_SOCK", &link);
        std::env::remove_var("XDG_RUNTIME_DIR");
        let available = socket_available();
        if let Some(v) = orig_sock {
            std::env::set_var("SKWD_WALL_V2_SOCK", v);
        } else {
            std::env::remove_var("SKWD_WALL_V2_SOCK");
        }
        if let Some(v) = orig_runtime {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        assert!(
            available,
            "a symlink to a live socket is the engine's socket: it must report available"
        );
    }

    /// Following the symlink must not weaken the socket requirement: a symlink
    /// whose target is a regular file is still not a listening socket.
    #[test]
    #[serial]
    fn socket_available_is_false_for_a_symlink_to_a_regular_file() {
        let _env = crate::test_utils::env_guard();
        let tmp_runtime = TempDir::new().unwrap();
        let target = tmp_runtime.path().join("real.txt");
        std::fs::write(&target, b"not a socket").unwrap();
        let link = tmp_runtime.path().join("wall.sock");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let orig_sock = std::env::var("SKWD_WALL_V2_SOCK").ok();
        let orig_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        std::env::set_var("SKWD_WALL_V2_SOCK", &link);
        std::env::remove_var("XDG_RUNTIME_DIR");
        let available = socket_available();
        if let Some(v) = orig_sock {
            std::env::set_var("SKWD_WALL_V2_SOCK", v);
        } else {
            std::env::remove_var("SKWD_WALL_V2_SOCK");
        }
        if let Some(v) = orig_runtime {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        assert!(
            !available,
            "a symlink to a regular file is not a listening socket"
        );
    }

    /// W2/W6: the picker probe reads a procfs-like table — the engine's own
    /// program counts, the daemon's different name does not, an empty table is
    /// "not running", and an unreadable table degrades to "unknown". It lives
    /// here because the probe is ENGINE knowledge (moved from `background`).
    #[test]
    fn the_keeper_program_signal_reads_the_process_table() {
        let tmp = tempfile::tempdir().unwrap();
        let pid = tmp.path().join("4242");
        std::fs::create_dir_all(&pid).unwrap();
        std::fs::write(pid.join("comm"), "skwd-wall-v2\n").unwrap();
        assert_eq!(
            picker_running_in(tmp.path()),
            Some(true),
            "the engine's own picker program must be recognised"
        );

        // The daemon's name is different: it must NOT count as the keeper's UI.
        std::fs::write(pid.join("comm"), "skwd-walld\n").unwrap();
        assert_eq!(
            picker_running_in(tmp.path()),
            Some(false),
            "the daemon is not the keeper's picker"
        );

        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            picker_running_in(empty.path()),
            Some(false),
            "a readable table with no picker is 'not running'"
        );

        assert_eq!(
            picker_running_in(&tmp.path().join("missing")),
            None,
            "an unreadable table degrades to unknown, never a guess"
        );
    }

    /// W6 (16-oct-2026, keeper's directive): detection is EVENT-DRIVEN — the
    /// picker is a layer-shell surface and its Hyprland namespace is ENGINE
    /// knowledge, pinned here like the comm name. The still renderer HVE
    /// itself launches reports `skwd-paper`; it is always present and must
    /// never read as "the keeper opened the picker".
    #[test]
    fn the_picker_layer_namespace_is_pinned_and_nothing_else_matches() {
        assert!(
            is_picker_layer("skwd-wall"),
            "the engine's own picker surface namespace must match"
        );
        for namespace in [
            "skwd-paper", // HVE's own poster renderer (always present)
            "dev.noctalia.Noctalia",
            "kitty",
            "",
        ] {
            assert!(
                !is_picker_layer(namespace),
                "{namespace:?} must never read as the picker"
            );
        }
    }

    // ── W6 (`odd/tasks/theme-package-trust.md`): a theme value that could
    // be read as an option never reaches the engine's argv ──────────────

    /// Stub `skwd-helm` / `skwd-wall-v2` binaries that RECORD every argv
    /// element they receive (one per line) and succeed — so a test can
    /// assert the exact argv the delegation constructed, without a real
    /// engine ever running.
    fn recording_engine_stubs(bin_dir: &Path, log: &Path) {
        for bin in ["skwd-helm", "skwd-wall-v2"] {
            let stub = bin_dir.join(bin);
            std::fs::write(
                &stub,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\nexit 0\n",
                    log.display()
                ),
            )
            .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    }

    /// Env sandbox for a delegation test: an EXISTING socket file (the
    /// delegation gate) plus the recording stubs on `PATH`. Holds the
    /// shared env lock for its whole life and restores every variable on
    /// drop, so a parallel test never observes the mutation.
    struct DelegationSandbox {
        /// Held for the whole test; released LAST on drop.
        _env: std::sync::MutexGuard<'static, ()>,
        /// Removed AFTER the variables are restored.
        _tmp: TempDir,
        log: PathBuf,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl DelegationSandbox {
        fn new() -> Self {
            let env = crate::test_utils::env_guard();
            let tmp = TempDir::new().unwrap();
            let log = tmp.path().join("argv.log");
            let sock_dir = tmp.path().join("skwd-wall-v2");
            std::fs::create_dir_all(&sock_dir).unwrap();
            // A regular file at the socket path is enough for the gate
            // (`socket.exists()`), and it can never be a live daemon.
            std::fs::write(sock_dir.join("wall.sock"), b"stub socket").unwrap();
            let bin_dir = tmp.path().join("bin");
            std::fs::create_dir_all(&bin_dir).unwrap();
            recording_engine_stubs(&bin_dir, &log);
            let saved: Vec<(&'static str, Option<String>)> =
                ["XDG_RUNTIME_DIR", "SKWD_WALL_V2_SOCK", "PATH"]
                    .iter()
                    .map(|k| (*k, std::env::var(k).ok()))
                    .collect();
            let orig_path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var("XDG_RUNTIME_DIR", tmp.path());
            std::env::remove_var("SKWD_WALL_V2_SOCK");
            std::env::set_var("PATH", format!("{}:{}", bin_dir.display(), orig_path));
            Self {
                _env: env,
                _tmp: tmp,
                log,
                saved,
            }
        }

        /// Every argv element the stubs received, in order, one per line.
        fn recorded(&self) -> Vec<String> {
            std::fs::read_to_string(&self.log)
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    impl Drop for DelegationSandbox {
        fn drop(&mut self) {
            for (k, v) in &self.saved {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    /// A LEGITIMATE path must still reach the engine byte for byte: the
    /// refusal below must never reshape, quote or drop a good value (the
    /// space in the path proves no re-quoting happens).
    #[test]
    #[serial]
    fn delegate_hands_a_legitimate_path_to_the_engine_byte_identically() {
        let sandbox = DelegationSandbox::new();
        let applied = delegate_to_skwd_walld(Path::new("/tmp/legit wall.png"));
        assert!(applied, "a legitimate path must still be applied");
        assert_eq!(
            sandbox.recorded(),
            vec!["apply", "/tmp/legit wall.png"],
            "the argv must be exactly `apply <path>`, byte for byte"
        );
    }

    /// W6, the defect: the wallpaper path can come from a THEME PACKAGE,
    /// so it is attacker-controlled. A value that STARTS WITH `-` could be
    /// parsed by `skwd-helm` / `skwd-wall-v2` as an OPTION instead of as
    /// the path to paint, so it must be refused BEFORE any spawn — the
    /// stubs must not have recorded a single argv element.
    #[test]
    #[serial]
    fn delegate_refuses_a_path_that_could_be_read_as_an_option() {
        let sandbox = DelegationSandbox::new();
        let applied = delegate_to_skwd_walld(Path::new("-o/tmp/pwned.png"));
        assert!(!applied, "an option-looking path must not be applied");
        assert!(
            sandbox.recorded().is_empty(),
            "the dangerous argument must never be handed to the engine, argv was: {:?}",
            sandbox.recorded()
        );
    }

    /// An empty path is refused today; pinned so the new leading-dash rule
    /// keeps that behaviour and never widens into an empty argv element.
    #[test]
    #[serial]
    fn delegate_refuses_an_empty_path_without_spawning() {
        let sandbox = DelegationSandbox::new();
        let applied = delegate_to_skwd_walld(Path::new(""));
        assert!(!applied, "an empty path must not be applied");
        assert!(
            sandbox.recorded().is_empty(),
            "an empty path must never be handed to the engine, argv was: {:?}",
            sandbox.recorded()
        );
    }
}
