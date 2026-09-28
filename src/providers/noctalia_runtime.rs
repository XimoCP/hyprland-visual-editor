//! Shared access to the Noctalia runtime: IPC (`noctalia msg`) and on-disk
//! path resolution (config dir, state dir).
//!
//! This is the neutral shared piece used by the `noctalia` and `mpvpaper`
//! providers, so neither depends on the other provider's internals.
//!
//! Plugin supervision lives here too: the plugin is Noctalia's
//! (`noctalia/mpvpaper`), so knowing how to probe, bounce and command it is
//! Noctalia runtime knowledge — not the media backend's. Every supervisor
//! call takes an explicit plugin id, so this seam is not shaped around one
//! plugin.

use std::io::Read;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

/// Hard wall-clock bound for one `noctalia msg` call.
///
/// A healthy call is an IPC round-trip against the running daemon and
/// answers in milliseconds; the bound exists for the unhealthy case: a
/// wedged `noctalia` binary must never hold its caller — the IPC thread
/// serving `assert-color-authority`, the apply path, the watcher — forever.
pub(crate) const NOCTALIA_MSG_TIMEOUT: Duration = Duration::from_secs(10);

/// Poll cadence while waiting for the child: the same 25 ms step the other
/// bounded helpers in this codebase already use. Worst case it adds 25 ms
/// to a healthy call.
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Run `noctalia msg <args...>` and return stdout on success, bounded by
/// [`NOCTALIA_MSG_TIMEOUT`].
pub(crate) fn noctalia_msg(args: &[&str]) -> Result<String, String> {
    noctalia_msg_bounded(args, NOCTALIA_MSG_TIMEOUT)
}

/// Bounded variant of [`noctalia_msg`] — also the seam the tests drive
/// with a stub script and a short bound.
///
/// Contract, identical to the old `Command::output()` for every
/// non-timeout case: stdout on success, the `noctalia {args} failed: …`
/// shape on a non-zero exit, `Failed to run noctalia: …` when the binary
/// cannot be spawned. On timeout the child is killed AND reaped (no
/// zombie) and the error names the bound that fired.
///
/// Both pipes are drained on worker threads, so the parent never blocks on
/// a pipe the killed child left open — an orphaned grandchild holding the
/// write end delays those threads, never this call.
fn noctalia_msg_bounded(args: &[&str], timeout: Duration) -> Result<String, String> {
    let label = args.join(" ");
    let mut child = std::process::Command::new("noctalia")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run noctalia: {}", e))?;

    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let (err_tx, err_rx) = mpsc::channel::<Vec<u8>>();
    drain_on_thread(child.stdout.take(), out_tx);
    drain_on_thread(child.stderr.take(), err_tx);

    let start = Instant::now();
    let mut stdout: Option<Vec<u8>> = None;
    let mut stderr: Option<Vec<u8>> = None;
    let status = loop {
        take_pipe(&out_rx, &mut stdout);
        take_pipe(&err_rx, &mut stderr);
        let exited = match child.try_wait() {
            Ok(Some(status)) => Some(status),
            Ok(None) => None,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("noctalia {} wait failed: {}", label, e));
            }
        };
        if let Some(status) = exited {
            if stdout.is_some() && stderr.is_some() {
                break status;
            }
        }
        if start.elapsed() >= timeout {
            // Kill + reap: the child leaves no zombie behind. The pipe
            // readers are deliberately NOT joined — they finish on their
            // own when the inherited write ends finally close.
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("noctalia {} timed out after {:?}", label, timeout));
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    let stdout = stdout.unwrap_or_default();
    let stderr = stderr.unwrap_or_default();
    if status.success() {
        Ok(String::from_utf8_lossy(&stdout).to_string())
    } else {
        Err(format!(
            "noctalia {} failed: {}",
            label,
            String::from_utf8_lossy(&stderr).trim()
        ))
    }
}

/// Drain one child pipe on its own thread. The bytes are handed to the
/// channel only at EOF; a parent that has already given up drops the
/// receiver, the send fails, and the thread exits without a hand-off.
fn drain_on_thread<R>(pipe: Option<R>, tx: mpsc::Sender<Vec<u8>>)
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut reader) = pipe {
            let _ = reader.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
}

/// Non-blocking hand-off of one drained pipe into its slot. A closed
/// channel with no bytes left means the reader thread died before it
/// finished: treat that as empty output instead of stalling to the bound.
fn take_pipe(rx: &mpsc::Receiver<Vec<u8>>, slot: &mut Option<Vec<u8>>) {
    if slot.is_some() {
        return;
    }
    match rx.try_recv() {
        Ok(bytes) => *slot = Some(bytes),
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => *slot = Some(Vec::new()),
    }
}

/// Resolve Noctalia config dir (~/.config/noctalia).
pub(crate) fn noctalia_config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("HVE_NOCTALIA_CONFIG") {
        let p = PathBuf::from(dir);
        if p.exists() {
            return Some(p);
        }
    }
    dirs::config_dir().map(|d| d.join("noctalia"))
}

/// Resolve Noctalia state dir (~/.local/state/noctalia).
///
/// v5 stores its runtime settings in `settings.toml` inside this directory,
/// NOT in `~/.config/noctalia/settings.json` (which is a v4 artifact).
pub(crate) fn noctalia_state_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".local").join("state").join("noctalia"))
}

// ── Plugin supervision ─────────────────────────────────────────────────

/// The plugin id HVE supervises by default: `noctalia/mpvpaper`. One
/// `pub(crate)` constant, so the id is never spelled out twice in the
/// backend that feeds the plugin.
pub(crate) const MPVPAPER_PLUGIN_ID: &str = "noctalia/mpvpaper";

/// Tri-state probe behind [`plugin_enabled`]: `Ok` carries the parsed
/// answer (line suffix `enabled`), `Err` carries the IPC failure verbatim
/// (missing binary, non-zero exit). This is the ONE `plugins list` parse
/// in the codebase: callers that must keep "IPC failed" (unknown) apart
/// from "disabled" — the apply-time plugin snapshot — go through here;
/// everyone else reads failure as NOT enabled via [`plugin_enabled`].
pub(crate) fn plugin_enabled_probe(plugin_id: &str) -> Result<bool, String> {
    let out = noctalia_msg(&["msg", "plugins", "list"])?;
    Ok(out
        .lines()
        .any(|l| l.trim_start().starts_with(plugin_id) && l.trim_end().ends_with("enabled")))
}

/// Check whether the plugin is installed AND enabled, through the
/// tri-state probe [`plugin_enabled_probe`].
///
/// Semantics preserved from the old `mpvpaper::mpvpaper_enabled`: a failed
/// IPC (missing binary, non-zero exit) reads as NOT enabled — the caller
/// then refuses to act, which is always the safe side.
pub(crate) fn plugin_enabled(plugin_id: &str) -> bool {
    plugin_enabled_probe(plugin_id).unwrap_or(false)
}

/// Bounce the plugin so it re-reads its assignments at boot and relaunches
/// what it supervises (the programmatic "click").
///
/// G3: refused while the plugin is currently disabled — the enable step
/// would RE-ENABLE a plugin the user turned off and resurrect its old video
/// on the next theme apply. Callers check first; this guard is the backstop
/// so no path can bounce a disabled plugin by accident.
pub(crate) fn plugin_bounce(plugin_id: &str) -> Result<(), String> {
    if !plugin_enabled(plugin_id) {
        return Err(format!(
            "{plugin_id} plugin is not enabled; refusing to bounce \
             (would re-enable a plugin the user disabled)"
        ));
    }
    noctalia_msg(&["msg", "plugins", "disable", plugin_id])?;
    noctalia_msg(&["msg", "plugins", "enable", plugin_id])?;
    Ok(())
}

/// Send `clear-all` to the plugin's own service — the IPC that stops every
/// running instance the plugin supervises. The enabled check and any
/// settle sleep belong to the caller: this is the bare IPC, nothing else.
pub(crate) fn plugin_service_clear_all(plugin_id: &str) -> Result<(), String> {
    let service = format!("{plugin_id}:service");
    noctalia_msg(&["msg", "plugin", &service, "all", "clear-all"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::time::{Duration, Instant};

    /// PATH-shadowing stub for the `noctalia` CLI — the SAME mechanism the
    /// `ColorStub` harness in `providers/noctalia.rs` uses: `TempEnv` takes
    /// the shared environment lock (so no parallel test observes the
    /// mutation), the stub bin dir is prepended to `PATH`, and `Drop`
    /// restores the original `PATH` while that lock is still held.
    struct StubNoctalia {
        /// Restored first: `Drop` puts this back before any field goes.
        old_path: Option<String>,
        /// Lives on `PATH` until `old_path` is restored above.
        _bin: tempfile::TempDir,
        /// Shared env lock, released last.
        _env: crate::test_utils::TempEnv,
    }

    impl StubNoctalia {
        /// `Some(script)` shadows `noctalia` with that script; `None` leaves
        /// no `noctalia` resolvable, so the spawn itself must fail.
        fn new(script: Option<&str>) -> Self {
            let env = crate::test_utils::TempEnv::new();
            let bin = tempfile::tempdir().expect("stub bin dir");
            if let Some(body) = script {
                let stub = bin.path().join("noctalia");
                std::fs::write(&stub, body).expect("stub script");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
                        .expect("stub must be executable");
                }
            }
            let old_path = std::env::var("PATH").ok();
            let new_path = match (&script, &old_path) {
                (Some(_), Some(prev)) => format!("{}:{}", bin.path().display(), prev),
                (Some(_), None) => bin.path().display().to_string(),
                // Empty dir only: nothing can resolve `noctalia`.
                (None, _) => bin.path().display().to_string(),
            };
            std::env::set_var("PATH", &new_path);
            Self {
                old_path,
                _bin: bin,
                _env: env,
            }
        }
    }

    impl Drop for StubNoctalia {
        fn drop(&mut self) {
            match &self.old_path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
        }
    }

    /// Read the stub child's pid, retrying briefly: the script records it
    /// right after spawn and the exact moment of that write is up to the OS.
    fn stub_child_pid(pid_file: &std::path::Path) -> u32 {
        let start = Instant::now();
        loop {
            if let Ok(raw) = std::fs::read_to_string(pid_file) {
                if let Ok(pid) = raw.trim().parse::<u32>() {
                    return pid;
                }
            }
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "the stub never recorded its pid"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// A wedged CLI must be killed, reaped and reported within the bound —
    /// and the caller must NOT block on the pipes the killed child left
    /// open. The stub records its pid, then parks behind a background
    /// `sleep 5` (so bash cannot `exec` it away): killing bash orphans that
    /// sleeper, which keeps stdout/stderr open for the remaining 5 s. A
    /// naive "kill, then join the pipe readers" therefore returns only
    /// after 5 s — this test's elapsed bound is what catches it.
    #[test]
    #[serial]
    fn wedged_noctalia_is_bounded_killed_and_reaped() {
        let state = tempfile::tempdir().expect("state dir");
        let pid_file = state.path().join("child.pid");
        let script = format!(
            "#!/bin/bash\nprintf '%s' \"$$\" > {}\nsleep 5 &\nwait\n",
            pid_file.display()
        );
        let _stub = StubNoctalia::new(Some(&script));

        let started = Instant::now();
        let err = noctalia_msg_bounded(&["msg", "color-scheme-get"], Duration::from_millis(300))
            .expect_err("a wedged CLI cannot be allowed to succeed");
        let elapsed = started.elapsed();

        assert!(
            err.contains("timed out"),
            "the error must name the timeout, got: {err}"
        );
        assert!(
            err.contains("300ms"),
            "the error must state the bound that fired, got: {err}"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "a wedged CLI must not hold the caller hostage: took {elapsed:?}"
        );

        let pid = stub_child_pid(&pid_file);
        assert!(
            !std::path::PathBuf::from(format!("/proc/{pid}")).exists(),
            "the killed child must be reaped, not left as a zombie (pid {pid})"
        );
    }

    #[test]
    #[serial]
    fn bounded_run_returns_stdout_on_success() {
        let _stub = StubNoctalia::new(Some(
            "#!/bin/bash\nprintf 'color-scheme-get: custom demo'\nexit 0\n",
        ));
        assert_eq!(
            noctalia_msg_bounded(&["msg", "color-scheme-get"], Duration::from_secs(5))
                .expect("a healthy CLI must keep its Ok path"),
            "color-scheme-get: custom demo"
        );
    }

    /// The non-zero-exit shape is load-bearing: callers match on it and the
    /// wording predates this unit, so it must survive byte for byte.
    #[test]
    #[serial]
    fn bounded_run_reports_stderr_on_non_zero_exit() {
        let _stub = StubNoctalia::new(Some("#!/bin/bash\necho 'palette not found' >&2\nexit 7\n"));
        let err = noctalia_msg_bounded(
            &["msg", "color-scheme-set", "custom", "demo"],
            Duration::from_secs(5),
        )
        .expect_err("a failing CLI must stay an Err");
        assert_eq!(
            err,
            "noctalia msg color-scheme-set custom demo failed: palette not found"
        );
    }

    #[test]
    #[serial]
    fn bounded_run_reports_a_missing_binary_verbatim() {
        let _stub = StubNoctalia::new(None);
        let err = noctalia_msg_bounded(&["msg", "color-scheme-get"], Duration::from_secs(5))
            .expect_err("an unspawnable CLI must stay an Err");
        assert!(
            err.starts_with("Failed to run noctalia: "),
            "the spawn-error shape must not drift, got: {err}"
        );
    }

    #[test]
    fn the_wall_clock_bound_is_ten_seconds() {
        assert_eq!(NOCTALIA_MSG_TIMEOUT, Duration::from_secs(10));
    }

    /// The production entry point must run THROUGH the bounded seam: an
    /// unbounded `Command::output()` creeping back in would reopen exactly
    /// the stall this unit closes.
    #[test]
    fn noctalia_msg_runs_through_the_bounded_seam_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia_runtime.rs")
            .expect("the runtime module must exist");
        let start = src
            .find("pub(crate) fn noctalia_msg(")
            .expect("the public entry point must exist");
        let brace_at = start + src[start..].find('{').expect("it must have a body");
        let rest = &src[brace_at..];
        let body = &rest[..rest.find("\n}").expect("the body must close")];
        assert!(
            body.contains("noctalia_msg_bounded(args, NOCTALIA_MSG_TIMEOUT)"),
            "noctalia_msg must delegate to the bounded runner with the named const; body was:\n{body}"
        );
    }

    // ── Plugin supervision seam ───────────────────────────────────────

    /// Stub that logs every invocation and answers `msg plugins list` with
    /// `<plugin_id> enabled` when `lists_enabled` (nothing otherwise), so
    /// the supervisor tests assert the exact IPC sequence the old
    /// `mpvpaper` code sent, in the same order.
    fn supervisor_stub(log: &std::path::Path, plugin_id: &str, lists_enabled: bool) -> String {
        let answer = if lists_enabled {
            format!("printf '{} enabled\\n'\n", plugin_id)
        } else {
            String::new()
        };
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\n\
             if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"plugins\" ] && [ \"$3\" = \"list\" ]; then\n\
             :\n{answer}fi\nexit 0\n",
            log = log.display(),
            answer = answer
        )
    }

    /// The recorded invocation lines, in order.
    fn stub_log(log: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The probe: an `enabled` line answers enabled, and the id PARAMETER
    /// decides — the seam never hardcodes which plugin it is asked about.
    #[test]
    #[serial]
    fn plugin_enabled_reflects_the_plugins_list_answer() {
        let state = tempfile::tempdir().expect("stub state");
        let log = state.path().join("log");
        let _stub = StubNoctalia::new(Some(&supervisor_stub(&log, "noctalia/mpvpaper", true)));
        assert!(
            plugin_enabled(MPVPAPER_PLUGIN_ID),
            "an `enabled` line for the plugin id must probe as enabled"
        );
        assert!(
            !plugin_enabled("other/plugin"),
            "the id parameter must filter the list, not a hardcoded name"
        );
    }

    /// Exactly ONE `plugins list` parse exists behind these helpers: the
    /// tri-state probe. `plugin_enabled` must reach it THROUGH the probe —
    /// a second parse here would fork the semantics that keep "IPC failed"
    /// and "disabled" apart. Comment lines are stripped first so a
    /// commented-out parse cannot satisfy it. The needle is built with
    /// concat() so this test's own source — it lives in the file it
    /// inspects — can never contain it.
    #[test]
    fn plugin_enabled_delegates_to_the_tri_state_probe_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia_runtime.rs")
            .expect("the runtime module must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let start = code
            .find("pub(crate) fn plugin_enabled(")
            .expect("the boolean wrapper must exist");
        let brace_at = start + code[start..].find('{').expect("it must have a body");
        let rest = &code[brace_at..];
        let body = &rest[..rest.find("\n}").expect("the body must close")];
        assert!(
            body.contains("plugin_enabled_probe("),
            "plugin_enabled must delegate to the tri-state probe; body was:\n{body}"
        );
        assert!(
            !body.contains("noctalia_msg("),
            "plugin_enabled must not parse `plugins list` itself — the parse lives \
             only in the probe; body was:\n{body}"
        );
    }

    /// The tri-state probe is what the apply-time snapshot needs:
    /// `Ok(true)` / `Ok(false)` from the answer, `Err` carrying the IPC
    /// failure verbatim (the caller's warning keeps its cause) — the
    /// distinction `plugin_enabled` deliberately collapses to `false`.
    #[test]
    #[serial]
    fn plugin_enabled_probe_is_tri_state() {
        let state = tempfile::tempdir().expect("stub state");
        let log = state.path().join("log");
        {
            let _stub = StubNoctalia::new(Some(&supervisor_stub(&log, "noctalia/mpvpaper", true)));
            assert_eq!(plugin_enabled_probe(MPVPAPER_PLUGIN_ID), Ok(true));
        }
        {
            let _stub = StubNoctalia::new(Some(&supervisor_stub(&log, "noctalia/mpvpaper", false)));
            assert_eq!(plugin_enabled_probe(MPVPAPER_PLUGIN_ID), Ok(false));
        }
        {
            let _stub = StubNoctalia::new(Some("#!/bin/sh\necho 'no daemon' >&2\nexit 4\n"));
            assert_eq!(
                plugin_enabled_probe(MPVPAPER_PLUGIN_ID),
                Err("noctalia msg plugins list failed: no daemon".to_string()),
                "the failure must reach the caller verbatim, not collapse to false"
            );
        }
    }

    /// A failed IPC means "not enabled" — never a panic, never `true`.
    #[test]
    #[serial]
    fn plugin_enabled_is_false_when_the_cli_fails() {
        let _stub = StubNoctalia::new(Some("#!/bin/sh\necho 'no daemon' >&2\nexit 4\n"));
        assert!(
            !plugin_enabled(MPVPAPER_PLUGIN_ID),
            "a failing CLI must read as not-enabled"
        );
    }

    /// No resolvable `noctalia` at all: the spawn itself fails and still
    /// reads as not-enabled (the same semantics the old code had).
    #[test]
    #[serial]
    fn plugin_enabled_is_false_when_the_cli_is_missing() {
        let _stub = StubNoctalia::new(None);
        assert!(
            !plugin_enabled(MPVPAPER_PLUGIN_ID),
            "an unspawnable CLI must read as not-enabled"
        );
    }

    /// The bounce: probe first, then `disable`, then `enable` — exactly the
    /// IPC the old `mpvpaper::bounce_plugin` sent, in the same order.
    #[test]
    #[serial]
    fn plugin_bounce_disables_then_enables_in_order() {
        let state = tempfile::tempdir().expect("stub state");
        let log = state.path().join("log");
        let _stub = StubNoctalia::new(Some(&supervisor_stub(&log, "noctalia/mpvpaper", true)));
        plugin_bounce(MPVPAPER_PLUGIN_ID).expect("an enabled plugin must bounce");
        assert_eq!(
            stub_log(&log),
            vec![
                "msg plugins list",
                "msg plugins disable noctalia/mpvpaper",
                "msg plugins enable noctalia/mpvpaper",
            ],
            "the bounce must probe, disable and enable in that exact order"
        );
    }

    /// G3, moved here verbatim from `mpvpaper.rs`: a disabled plugin must
    /// never be bounced, because the enable step would RE-ENABLE a plugin
    /// the user turned off — so the refusal fires at the probe and the stub
    /// must record no disable and no enable at all.
    #[test]
    #[serial]
    fn plugin_bounce_refuses_a_disabled_plugin_and_sends_no_ipc() {
        let state = tempfile::tempdir().expect("stub state");
        let log = state.path().join("log");
        let _stub = StubNoctalia::new(Some(&supervisor_stub(&log, "noctalia/mpvpaper", false)));
        let err =
            plugin_bounce(MPVPAPER_PLUGIN_ID).expect_err("a disabled plugin must never bounce");
        assert_eq!(
            err,
            "noctalia/mpvpaper plugin is not enabled; refusing to bounce (would re-enable a plugin the user disabled)"
        );
        assert_eq!(
            stub_log(&log),
            vec!["msg plugins list"],
            "the refusal must stop at the probe: no disable, no enable"
        );
    }

    /// `clear-all` is ONE service IPC: no probe and no bounce around it
    /// (the enabled check and the settle sleep belong to the caller).
    #[test]
    #[serial]
    fn plugin_service_clear_all_sends_the_service_ipc() {
        let state = tempfile::tempdir().expect("stub state");
        let log = state.path().join("log");
        let _stub = StubNoctalia::new(Some(&supervisor_stub(&log, "noctalia/mpvpaper", false)));
        plugin_service_clear_all(MPVPAPER_PLUGIN_ID).expect("the service call must succeed");
        assert_eq!(
            stub_log(&log),
            vec!["msg plugin noctalia/mpvpaper:service all clear-all"],
            "clear-all must reach the plugin service verbatim, once"
        );
    }

    /// The non-zero-exit shape callers match on must not drift.
    #[test]
    #[serial]
    fn plugin_service_clear_all_reports_a_failing_cli_verbatim() {
        let _stub = StubNoctalia::new(Some(
            "#!/bin/sh\n\
             if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"plugin\" ]; then\n\
             echo 'service unavailable' >&2\nexit 9\nfi\nexit 0\n",
        ));
        let err = plugin_service_clear_all(MPVPAPER_PLUGIN_ID)
            .expect_err("a failing service call must stay an Err");
        assert_eq!(
            err,
            "noctalia msg plugin noctalia/mpvpaper:service all clear-all failed: service unavailable"
        );
    }

    /// The G3 guard moved here from `mpvpaper.rs`'s source-inspection test:
    /// inside `plugin_bounce` the enabled check must DOMINATE the enable
    /// step. Comment lines are stripped first so a commented-out guard
    /// cannot satisfy it.
    #[test]
    fn plugin_bounce_guard_dominates_the_enable_step_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia_runtime.rs")
            .expect("the runtime module must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let bounce_at = code
            .find("fn plugin_bounce")
            .expect("plugin_bounce must exist");
        let after_bounce = &code[bounce_at..];
        let guard_at = after_bounce
            .find("plugin_enabled")
            .expect("plugin_bounce must consult the live enabled check");
        let enable_at = after_bounce
            .find("\"enable\"")
            .expect("plugin_bounce must still contain its enable step");
        assert!(
            guard_at < enable_at,
            "the enabled check must dominate the enable step inside plugin_bounce"
        );
    }

    #[test]
    fn test_noctalia_state_dir() {
        let path = noctalia_state_dir();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.ends_with(".local/state/noctalia"));
        assert!(path.is_absolute());
    }

    #[test]
    fn test_noctalia_state_dir_exists() {
        let path = noctalia_state_dir().unwrap();
        // This should exist on a real system when noctalia v5 is installed
        // We just verify it doesn't crash — existence depends on the system
        let _ = path.exists();
    }
}