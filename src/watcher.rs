use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Owned handle to the spawned color watcher process.
///
/// Dropping a raw `std::process::Child` NEVER kills the child (its `Drop`
/// only closes the stdin handle), so an unguarded watcher is orphaned and
/// keeps running forever — the S5 defect of odd/hide-idempotency-and-singleton-watcher.
/// This guard terminates the watcher on drop AND reaps it (`wait`), so the
/// pid is fully released and no zombie is left under `/proc`.
pub struct ColorWatcher {
    child: std::process::Child,
    /// Process-group id, present only when this guard was created by
    /// `spawn_color_watcher` (its child is a group leader via
    /// `process_group(0)`). `None` means the group-kill must NEVER run:
    /// `kill(-pid)` on a non-leader could signal an unrelated group.
    pgid: Option<i32>,
}

impl ColorWatcher {
    /// Kill the watcher (best-effort) and reap it. Idempotent: a child that
    /// already exited makes `kill` fail harmlessly and `wait` still reaps it.
    fn terminate(&mut self) {
        // Kill the whole process group first: an orphaned long-lived child
        // (the inotifywait that holds the singleton lock) must die with the
        // watcher, not linger holding the lock for up to 30 s.
        if let Some(pgid) = self.pgid {
            let _ = unsafe { libc::kill(-pgid, libc::SIGKILL) };
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ColorWatcher {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// One spawn attempt: the child plus its process-group id. `pgid` is `Some`
/// only when the child was spawned as a group leader (the real spawn does
/// this via `process_group(0)`); teardown's group-kill must never run
/// against a non-leader pid — it could signal an unrelated process group.
struct SpawnedWatcher {
    child: std::process::Child,
    pgid: Option<i32>,
}

const MAX_START_ATTEMPTS: u32 = 5;
const START_GRACE: Duration = Duration::from_millis(250);
const START_GRACE_STEP: Duration = Duration::from_millis(20);
const START_RETRY_BACKOFF: Duration = Duration::from_millis(100);

/// Bounded, honestly-verified start of the colour watcher.
///
/// After each successful `spawn`, the child is watched through a short grace
/// period (~250 ms in ~20 ms steps): a child still alive at the end is a
/// REAL start; one that already exited is a failed attempt, retried up to
/// `MAX_START_ATTEMPTS` times. On exhaustion the caller gets `None` and a
/// real error log — never a "started" line for a dead child.
///
/// The normal path costs one ~250 ms grace; the failure path can delay
/// startup by a bounded few seconds. Alive-after-grace is the honest check;
/// verifying the watcher is FUNCTIONAL is out of scope by design.
fn start_watcher_core<F>(mut spawn: F, watcher_log: &std::path::Path) -> Option<ColorWatcher>
where
    F: FnMut() -> std::io::Result<SpawnedWatcher>,
{
    let mut last_exit: Option<std::process::ExitStatus> = None;

    for attempt in 1..=MAX_START_ATTEMPTS {
        let mut spawned = match spawn() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("[HVE] Could not start color watcher: {}", e);
                if attempt < MAX_START_ATTEMPTS {
                    std::thread::sleep(START_RETRY_BACKOFF);
                }
                continue;
            }
        };
        let pid = spawned.child.id();

        // Grace: poll for an early exit and leave the grace at the FIRST
        // observed exit instead of waiting out the full period.
        let grace_deadline = Instant::now() + START_GRACE;
        let mut status: Option<std::process::ExitStatus> = None;
        while status.is_none() && Instant::now() < grace_deadline {
            match spawned.child.try_wait() {
                Ok(Some(st)) => status = Some(st),
                Ok(None) => {}
                Err(_) => {}
            }
            if status.is_none() && Instant::now() < grace_deadline {
                std::thread::sleep(START_GRACE_STEP);
            }
        }

        match status {
            Some(st) => {
                last_exit = Some(st);
                if attempt < MAX_START_ATTEMPTS {
                    std::thread::sleep(START_RETRY_BACKOFF);
                }
            }
            None => {
                tracing::info!("[HVE] Color watcher started (pid: {})", pid);
                tracing::info!("[HVE] Watcher log: {}", watcher_log.display());
                return Some(ColorWatcher {
                    child: spawned.child,
                    pgid: spawned.pgid,
                });
            }
        }
    }

    tracing::error!(
        "[HVE] Color watcher failed to start after {} attempts \
         (last observed exit: {}) — no watcher is running, colour sync is \
         OFF for this session",
        MAX_START_ATTEMPTS,
        describe_exit(last_exit)
    );
    None
}

/// Human-readable rendering of the last observed exit status for the
/// honest-failure log (e.g. `exit code 7` or `signal 9`).
fn describe_exit(status: Option<std::process::ExitStatus>) -> String {
    use std::os::unix::process::ExitStatusExt;
    match status {
        None => "no completed attempt (spawn failed)".to_string(),
        Some(st) if st.success() => "success (unexpected here)".to_string(),
        Some(st) => match (st.code(), st.signal()) {
            (Some(code), _) => format!("exit code {code}"),
            (None, Some(sig)) => format!("signal {sig}"),
            _ => format!("{st:?}"),
        },
    }
}

/// Build the bash command that runs the watcher script, carrying the two
/// process-level guarantees that make the watcher die WITH HVE:
/// - the child starts its OWN process group (`process_group(0)`), so
///   teardown can kill the group — orphans included;
/// - on Linux, `PR_SET_PDEATHSIG = SIGKILL`: the kernel kills the watcher
///   the instant the parent thread dies, even on `kill -9`, when no guard
///   Drop can ever run (verified 2026-09-24: the signal survives `execve`).
///   The spawn site is HVE's main thread, so the parent thread IS the
///   process.
fn watcher_command(proj: &std::path::Path, stderr: std::process::Stdio) -> std::process::Command {
    let mut cmd = std::process::Command::new("bash");
    cmd.arg(proj.join("assets").join("scripts").join("color_watcher.sh"))
        .stdout(std::process::Stdio::null())
        .stderr(stderr);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        // Safety: this closure runs in the forked child before `execve`;
        // only async-signal-safe work is allowed, and prctl is exactly that.
        unsafe {
            cmd.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong, 0, 0, 0)
                    != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    cmd
}

/// Spawn the color_watcher.sh bash script and return the owned guard handle.
/// The start is verified honestly: the child must survive a ~250 ms grace
/// or the attempt is retried (bounded) — a dead child is never reported as
/// a started watcher. Returns `None` when the start never sticks.
pub fn spawn_color_watcher(proj: &std::path::Path) -> Option<ColorWatcher> {
    let watcher_script = proj.join("assets").join("scripts").join("color_watcher.sh");
    let watcher_log = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("hve")
        .join("color_watcher.log");

    if !watcher_script.exists() {
        tracing::warn!(
            "[HVE] Color watcher script not found: {}",
            watcher_script.display()
        );
        return None;
    }

    // Log to file so we can diagnose watcher issues. The handle is cloned
    // per attempt: Stdio consumes the file, and every retry needs its own.
    let log_file = std::fs::File::options()
        .create(true)
        .append(true)
        .open(&watcher_log)
        .ok()
        .and_then(|f| f.try_clone().ok());

    let spawn = move || {
        let stderr: std::process::Stdio = log_file
            .as_ref()
            .and_then(|f| f.try_clone().ok())
            .map(std::process::Stdio::from)
            .unwrap_or(std::process::Stdio::null());
        watcher_command(proj, stderr)
            .spawn()
            .map(|child| {
                #[cfg(target_os = "linux")]
                let pgid = Some(child.id() as i32);
                // The group kill must never be claimed where process_group(0)
                // was not set: kill(-pid) on a non-leader could signal an
                // unrelated group.
                #[cfg(not(target_os = "linux"))]
                let pgid = None;
                SpawnedWatcher { child, pgid }
            })
    };

    start_watcher_core(spawn, &watcher_log)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// Kills and reaps its child on drop, so a panicking test can never
    /// leave a watcher process behind. `std::process::Child`'s own `Drop`
    /// does NOT kill the process — the explicit kill + wait is required.
    struct KillOnDrop(Option<std::process::Child>);
    impl KillOnDrop {
        fn new(child: std::process::Child) -> Self {
            Self(Some(child))
        }
    }
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    #[test]
    fn test_spawn_color_watcher_script_not_found_returns_none() {
        // A non-existent project dir should cause the function to
        // return None because the script file doesn't exist.
        let proj = PathBuf::from("/tmp/hve_nonexistent_test");
        let result = spawn_color_watcher(&proj);
        assert!(result.is_none(), "should return None when script does not exist");
    }

    #[test]
    fn test_spawn_color_watcher_builds_correct_script_path() {
        // Verify the path logic (which is the core unit-testable part):
        // proj/assets/scripts/color_watcher.sh
        let proj = PathBuf::from("/some/project");
        let script = proj.join("assets").join("scripts").join("color_watcher.sh");
        assert_eq!(
            script.to_string_lossy(),
            "/some/project/assets/scripts/color_watcher.sh"
        );
    }

    #[test]
    fn test_spawn_color_watcher_builds_correct_log_path() {
        // Verify the log path resolution logic:
        // cache/hve/color_watcher.log (or fallback /tmp/hve/color_watcher.log)
        if let Some(cache) = dirs::cache_dir() {
            let log = cache.join("hve").join("color_watcher.log");
            assert!(log.to_string_lossy().contains("color_watcher.log"));
        }
    }

    #[test]
    fn color_watcher_guard_kills_and_reaps_the_child_on_drop() {
        // Dropping a raw std::process::Child NEVER kills the child: it would
        // be orphaned and keep running (the S5 defect). The owned guard must
        // kill AND reap, so no /proc/<pid> entry survives the drop — a zombie
        // (killed but unreaped) would still appear there.
        let child = Command::new("sleep").arg("86400").spawn().unwrap();
        let pid = child.id();
        assert!(
            PathBuf::from(format!("/proc/{pid}")).exists(),
            "precondition: the placeholder child is alive"
        );
        let guard = ColorWatcher { child, pgid: None };
        drop(guard);
        assert!(
            !PathBuf::from(format!("/proc/{pid}")).exists(),
            "guard drop must kill and reap the child (no /proc/{pid} entry)"
        );
    }

    /// Kills a raw pid on drop. For processes that are not
    /// `std::process::Child` — e.g. an orphaned stub whose parent bash is
    /// already dead and reaped — so a panicking test cannot leak it. Only
    /// kills when the pid is still alive (never signal a recycled pid).
    struct KillPidOnDrop(i32);
    impl Drop for KillPidOnDrop {
        fn drop(&mut self) {
            if PathBuf::from(format!("/proc/{}", self.0)).exists() {
                let _ = unsafe { libc::kill(self.0, libc::SIGKILL) };
            }
        }
    }

    #[test]
    fn sandboxed_lock_is_released_when_the_watcher_is_killed_with_a_live_child() {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::process::CommandExt;

        // The orphaning condition measured in odd/watcher-startup-resilience:
        // when the watcher's bash is killed, its long-lived inotifywait child
        // survives — and, before the fix, that orphan kept holding the
        // singleton lock (fd 9 is inherited, flock rides the open file
        // description), rejecting every new watcher. This test kills a watcher
        // with its long-lived stub child alive and demands that the lock is
        // released so the next instance starts.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let home = root.join("home");
        let cache = root.join("cache");
        std::fs::create_dir_all(home.join(".cache/wal")).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(
            home.join(".cache/wal/colors.json"),
            r#"{"wallpaper": "/dev/null"}"#,
        )
        .unwrap();

        let scripts = root.join("assets/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        let repo_scripts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/scripts");
        std::fs::copy(repo_scripts.join("color_watcher.sh"), scripts.join("color_watcher.sh"))
            .unwrap();
        std::fs::copy(repo_scripts.join("utils.sh"), scripts.join("utils.sh")).unwrap();
        std::fs::copy(repo_scripts.join("colors.sh"), scripts.join("colors.sh")).unwrap();
        copy_color_sources_dir(&repo_scripts, &scripts);

        let assemble_marker = cache.join("assemble_runs");
        std::fs::write(
            scripts.join("assemble.sh"),
            format!("#!/bin/bash\necho run >> {}\n", assemble_marker.display()),
        )
        .unwrap();
        std::fs::write(scripts.join("get_colors.sh"), "#!/bin/bash\nexit 0\n").unwrap();
        std::fs::write(scripts.join("hve-ipc"), "#!/bin/bash\nexit 0\n").unwrap();
        for name in ["assemble.sh", "get_colors.sh", "hve-ipc"] {
            let path = scripts.join(name);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // LONG-LIVED inotifywait stub: it must outlive the watcher's death —
        // the exact condition that kept the lock held. It records its own pid
        // so the test can prove the orphaning happened. The explicit `exec`
        // is REQUIRED: without it the stub bash forks `sleep` as a child and
        // the marker pid (the bash) dies while the sleep leaks on past a
        // by-pid kill.
        let stubs = root.join("stubs");
        std::fs::create_dir_all(&stubs).unwrap();
        let orphan_marker = cache.join("inotifywait_pid");
        std::fs::write(
            stubs.join("inotifywait"),
            format!(
                "#!/bin/bash\necho $$ > {}\nexec sleep 86400\n",
                orphan_marker.display()
            ),
        )
        .unwrap();
        std::fs::write(stubs.join("noctalia"), "#!/bin/bash\nexit 0\n").unwrap();
        for name in ["inotifywait", "noctalia"] {
            std::fs::set_permissions(&stubs.join(name), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let path_with_stubs =
            format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap_or_default());

        let script_path = scripts.join("color_watcher.sh");
        let spawn_instance = || {
            Command::new("bash")
                .arg(&script_path)
                .env("HOME", &home)
                .env("HVE_CACHE_DIR", &cache)
                .env("PATH", &path_with_stubs)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap()
        };

        // ── Instance 1 acquires the singleton lock and reaches the watch
        // loop (the orphan stub must already exist when we kill it). ──
        let mut first = KillOnDrop::new(spawn_instance());

        let log_file = cache.join("color_watcher.log");
        let lock_file = cache.join("color_watcher.lock");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let locked = lock_file.exists();
            let started = std::fs::read_to_string(&log_file)
                .map(|s| s.contains("Starting watcher"))
                .unwrap_or(false);
            let orphan_spawned = std::fs::read_to_string(&orphan_marker)
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            if locked && started && orphan_spawned {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "instance 1 never reached its watch loop within 30s"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let orphan_pid: i32 = std::fs::read_to_string(&orphan_marker)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // RAII once the pid exists: the orphan must never survive the test.
        let _orphan_guard = KillPidOnDrop(orphan_pid);
        assert!(
            PathBuf::from(format!("/proc/{orphan_pid}")).exists(),
            "precondition: the long-lived inotifywait stub is alive"
        );

        // ── Kill instance 1's bash while its long-lived child is alive. ──
        let _ = first.0.as_mut().unwrap().kill();
        let _ = first.0.as_mut().unwrap().wait();
        first.0 = None; // already reaped — dropping must not kill a recycled pid
        assert!(
            PathBuf::from(format!("/proc/{orphan_pid}")).exists(),
            "precondition: the orphan survived its dead parent (true orphaning)"
        );

        // ── Instance 2 must acquire the lock and start. Before the fix this
        // hangs: the orphan holds fd 9, flock refuses, and the log keeps a
        // single "Starting watcher" forever. ──
        // Instance 2 is spawned as its own process group so cleanup can kill
        // its own orphaned stub together with its bash.
        let mut second = KillOnDrop::new({
            let mut cmd = Command::new("bash");
            cmd.arg(&script_path)
                .env("HOME", &home)
                .env("HVE_CACHE_DIR", &cache)
                .env("PATH", &path_with_stubs)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .process_group(0);
            cmd.spawn().unwrap()
        });
        let second_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let log = std::fs::read_to_string(&log_file).unwrap_or_default();
            if log.matches("Starting watcher").count() >= 2 {
                break;
            }
            assert!(
                Instant::now() < second_deadline,
                "instance 2 was rejected while a dead watcher's orphan held the lock; \
                 log so far:\n{log}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        let log = std::fs::read_to_string(&log_file).unwrap();
        assert_eq!(
            log.matches("Starting watcher").count(),
            2,
            "the dead instance (1) and the new instance (2) must both have started, got:\n{log}"
        );
        // "Starting watcher" is logged BEFORE the initial refresh runs
        // (color_watcher.sh: the start line precedes the `assemble.sh` call), so
        // instance 2's marker may not exist yet when its start line appears.
        // Under load that gap widens, so POLL for the refresh with a bound
        // instead of asserting the instant the start line shows — the same
        // bounded wait the lock-of-instance-2 check above already uses.
        let refresh_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let runs = std::fs::read_to_string(&assemble_marker).unwrap_or_default();
            if runs.lines().count() >= 2 {
                break;
            }
            assert!(
                Instant::now() < refresh_deadline,
                "the new instance must perform its own initial refresh, got {runs:?}"
            );
            std::thread::sleep(Duration::from_millis(25));
        }

        // Cleanup: group-kill instance 2 (bash + its own stub), reap, then
        // the KillPidOnDrop guard kills instance 1's orphan.
        let pid2 = second.0.as_mut().unwrap().id() as i32;
        let _ = unsafe { libc::kill(-pid2, libc::SIGKILL) };
        let _ = second.0.as_mut().unwrap().wait();
        second.0 = None;
    }

    #[test]
    fn guard_termination_kills_the_whole_process_group_not_just_the_child() {
        use std::os::unix::process::CommandExt;

        // The watcher's long-lived child (inotifywait) shares its process
        // group. terminate() must kill the GROUP: killing only the direct
        // child would orphan the group member — exactly how the lock used to
        // stay held through teardown.
        let tmp = tempfile::TempDir::new().unwrap();
        let marker = tmp.path().join("grandchild_pid");
        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(format!("sleep 86400 & echo $! > {}; wait", marker.display()))
            .process_group(0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let child = cmd.spawn().unwrap();
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(10);
        let grandchild: i32 = loop {
            if let Ok(content) = std::fs::read_to_string(&marker) {
                if let Ok(p) = content.trim().parse() {
                    break p;
                }
            }
            assert!(
                Instant::now() < deadline,
                "child never started its background group member"
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        // RAII backstop: even when the guard fails to kill the group (RED
        // phase), the sleep must never leak.
        let _backstop = KillPidOnDrop(grandchild);
        let guard = ColorWatcher {
            child,
            pgid: Some(pid as i32),
        };
        drop(guard);

        let gone = Instant::now() + Duration::from_secs(5);
        loop {
            let leader_gone = !PathBuf::from(format!("/proc/{pid}")).exists();
            let member_gone = !PathBuf::from(format!("/proc/{grandchild}")).exists();
            if leader_gone && member_gone {
                break;
            }
            assert!(
                Instant::now() < gone,
                "guard drop must kill the WHOLE process group \
                 (leader {pid} gone={leader_gone}, member {grandchild} gone={member_gone})"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn watcher_child_dies_when_hve_dies_end_to_end() {
        use std::os::unix::fs::PermissionsExt;

        // ── Helper branch ──
        // The test re-invokes THIS binary (via current_exe) as a helper
        // process: it calls the real spawn_color_watcher against a sandboxed
        // tree, records the child pid, LEAKS the guard on purpose (its Drop
        // would kill the child — the kernel must do it instead), and exits.
        if std::env::var("HVE_WATCHER_PDEATH_HELPER").is_ok() {
            let proj = std::path::PathBuf::from(std::env::var("HVE_HELPER_ROOT").unwrap());
            let pid_file =
                std::path::PathBuf::from(std::env::var("HVE_PDEATH_PID_FILE").unwrap());
            match spawn_color_watcher(&proj) {
                Some(guard) => {
                    let pid = guard.child.id();
                    std::fs::write(&pid_file, pid.to_string()).unwrap();
                    // Guard intentionally forgotten: nothing must kill the
                    // child but the kernel (PR_SET_PDEATHSIG on parent death).
                    std::mem::forget(guard);
                    std::process::exit(0);
                }
                None => {
                    eprintln!("spawn_color_watcher returned None");
                    std::process::exit(2);
                }
            }
        }

        // ── Parent branch: sandboxed tree the helper will run against ──
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let home = root.join("home");
        let cache = root.join("cache");
        std::fs::create_dir_all(home.join(".cache/wal")).unwrap();
        std::fs::create_dir_all(home.join(".cache/hve")).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(
            home.join(".cache/wal/colors.json"),
            r#"{"wallpaper": "/dev/null"}"#,
        )
        .unwrap();

        let scripts = root.join("assets/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        let repo_scripts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/scripts");
        std::fs::copy(repo_scripts.join("color_watcher.sh"), scripts.join("color_watcher.sh"))
            .unwrap();
        std::fs::copy(repo_scripts.join("utils.sh"), scripts.join("utils.sh")).unwrap();
        std::fs::copy(repo_scripts.join("colors.sh"), scripts.join("colors.sh")).unwrap();
        copy_color_sources_dir(&repo_scripts, &scripts);

        let assemble_marker = cache.join("assemble_runs");
        std::fs::write(
            scripts.join("assemble.sh"),
            format!("#!/bin/bash\necho run >> {}\n", assemble_marker.display()),
        )
        .unwrap();
        std::fs::write(scripts.join("get_colors.sh"), "#!/bin/bash\nexit 0\n").unwrap();
        std::fs::write(scripts.join("hve-ipc"), "#!/bin/bash\nexit 0\n").unwrap();
        for name in ["assemble.sh", "get_colors.sh", "hve-ipc"] {
            let path = scripts.join(name);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Short-lived inotifywait stub: the watcher must stay alive across
        // its watch loop for the whole test (it respawns the stub each 2 s).
        let stubs = root.join("stubs");
        std::fs::create_dir_all(&stubs).unwrap();
        std::fs::write(stubs.join("inotifywait"), "#!/bin/bash\nexec sleep 2\n").unwrap();
        std::fs::write(stubs.join("noctalia"), "#!/bin/bash\nexit 0\n").unwrap();
        for name in ["inotifywait", "noctalia"] {
            std::fs::set_permissions(&stubs.join(name), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let path_with_stubs =
            format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap_or_default());

        let pid_file = root.join("watcher_pid");
        let helper = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("watcher::tests::watcher_child_dies_when_hve_dies_end_to_end")
            .env("HVE_WATCHER_PDEATH_HELPER", "1")
            .env("HVE_HELPER_ROOT", &root)
            .env("HVE_PDEATH_PID_FILE", &pid_file)
            .env("HOME", &home)
            .env("HVE_CACHE_DIR", &cache)
            // `dirs::cache_dir()` honours XDG_CACHE_HOME, so pinning HOME alone
            // is not enough: without this, a helper-run watcher would redirect
            // its stderr into the keeper's real ~/.cache/hve (verification
            // finding MINOR, W6). Keep the whole tree inside the sandbox.
            .env("XDG_CACHE_HOME", &cache)
            .env("PATH", &path_with_stubs)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let _helper_guard = KillOnDrop::new(helper);

        // The helper starts the watcher, records its pid, then exits. The
        // watcher must then disappear on its own: the kernel delivers
        // PR_SET_PDEATHSIG the instant the parent thread dies. Before the fix
        // the watcher survives → the death poll times out (RED).
        let deadline = Instant::now() + Duration::from_secs(20);
        let watcher_pid: i32 = loop {
            if let Ok(content) = std::fs::read_to_string(&pid_file) {
                if let Ok(pid) = content.trim().parse() {
                    break pid;
                }
            }
            assert!(
                Instant::now() < deadline,
                "helper never recorded the watcher pid"
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(watcher_pid > 0, "watcher pid must be positive");

        // RAII backstop: if the death poll below ever fails (RED phase, or a
        // genuinely broken PDEATHSIG), the leaked watcher must not survive
        // the test. On the green path the pid is already gone — the
        // existence check makes this a no-op, never a recycled-pid kill.
        let _watcher_backstop = KillPidOnDrop(watcher_pid);

        let death_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let alive = PathBuf::from(format!("/proc/{watcher_pid}")).exists();
            if !alive {
                break;
            }
            assert!(
                Instant::now() < death_deadline,
                "watcher {watcher_pid} still alive 10 s after its parent died — \
                 PR_SET_PDEATHSIG missing?"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Runs `f` under a tracing subscriber that records every formatted
    /// line, so tests can assert what the start core logs (e.g. never
    /// "started" for a dead child) without touching the real log sinks.
    fn with_captured_logs<T>(out: &mut Vec<String>, f: impl FnOnce() -> T) -> T {
        #[derive(Clone)]
        struct Sink(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        impl std::io::Write for Sink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let s = String::from_utf8_lossy(buf);
                let trimmed = s.trim_end_matches(['\n', '\r']).to_string();
                if !trimmed.is_empty() {
                    self.0.lock().unwrap().push(trimmed);
                }
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl tracing_subscriber::fmt::MakeWriter<'_> for Sink {
            type Writer = Sink;
            fn make_writer(&self) -> Self::Writer {
                self.clone()
            }
        }
        let sink = Sink(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())));
        let writer = sink.clone();
        let subscriber = tracing_subscriber::fmt().with_writer(writer).finish();
        let result = tracing::subscriber::with_default(subscriber, f);
        *out = std::mem::take(&mut *sink.0.lock().unwrap());
        result
    }

    #[test]
    fn core_retries_an_immediately_exiting_child_exactly_n_times_then_returns_none() {
        let mut spawn_count = 0u32;
        let mut logs = Vec::new();
        let result = with_captured_logs(&mut logs, || {
            start_watcher_core(
                || {
                    spawn_count += 1;
                    Command::new("sh")
                        .arg("-c")
                        .arg("exit 7")
                        .spawn()
                        .map(|child| SpawnedWatcher { child, pgid: None })
                },
                &PathBuf::from("/tmp/hve/no-such-log.log"),
            )
        });
        assert!(result.is_none(), "every attempt exited — the core must give up");
        assert_eq!(
            spawn_count, MAX_START_ATTEMPTS,
            "an immediately-exiting child must be retried exactly \
             MAX_START_ATTEMPTS times"
        );
        // Honest-failure contract: never "started" for a dead child, and a
        // real error naming the attempts and the last observed exit status.
        assert!(
            !logs.iter().any(|l| l.contains("Color watcher started")),
            "a dead child must never be logged as started, got:\n{logs:?}"
        );
        assert!(
            logs.iter().any(|l| {
                l.contains("failed to start after")
                    && l.contains(MAX_START_ATTEMPTS.to_string().as_str())
                    && l.contains("exit code 7")
            }),
            "the final error must name the attempts and the last exit status, \
             got:\n{logs:?}"
        );
    }

    #[test]
    fn core_succeeds_on_the_attempt_after_the_dead_ones() {
        let mut spawn_count = 0u32;
        let result = start_watcher_core(
            || {
                spawn_count += 1;
                let spawned = if spawn_count <= 2 {
                    Command::new("sh").arg("-c").arg("exit 1").spawn()
                } else {
                    Command::new("sleep").arg("10").spawn()
                };
                spawned.map(|child| SpawnedWatcher { child, pgid: None })
            },
            &PathBuf::from("/tmp/hve/no-such-log.log"),
        );
        let mut guard =
            result.expect("the k+1th attempt (a live child) must succeed");
        assert_eq!(spawn_count, 3, "died 2 times, then success on attempt 3");
        assert!(
            matches!(guard.child.try_wait().unwrap(), None),
            "the success guard must hold a LIVE child — no success outcome \
             for a dead one"
        );
        let pid = guard.child.id();
        drop(guard);
        assert!(
            !PathBuf::from(format!("/proc/{pid}")).exists(),
            "guard drop must reap the success child"
        );
    }

    #[test]
    fn core_succeeds_on_the_first_attempt_when_the_child_stays_alive_through_the_grace() {
        let mut spawn_count = 0u32;
        let result = start_watcher_core(
            || {
                spawn_count += 1;
                Command::new("sleep")
                    .arg("10")
                    .spawn()
                    .map(|child| SpawnedWatcher { child, pgid: None })
            },
            &PathBuf::from("/tmp/hve/no-such-log.log"),
        );
        let mut guard =
            result.expect("a child alive at the grace end is a real start");
        assert_eq!(spawn_count, 1, "success must land on the first attempt");
        assert!(
            matches!(guard.child.try_wait().unwrap(), None),
            "the success guard must hold a LIVE child"
        );
        let pid = guard.child.id();
        drop(guard);
        assert!(
            !PathBuf::from(format!("/proc/{pid}")).exists(),
            "guard drop must reap the success child"
        );
    }

    #[test]
    fn sandboxed_second_watcher_instance_is_rejected_while_first_holds_the_lock() {
        use std::os::unix::fs::PermissionsExt;

        // ── Sandboxed temp tree: HOME + HVE_CACHE_DIR point at it, so the
        // real script can never touch the keeper's live session. ──
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let home = root.join("home");
        let cache = root.join("cache");
        std::fs::create_dir_all(home.join(".cache/wal")).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        // Minimal watched file so `find_watch_files` is non-empty (pywal slot).
        std::fs::write(
            home.join(".cache/wal/colors.json"),
            r#"{"wallpaper": "/dev/null"}"#,
        )
        .unwrap();

        // Real scripts copied into the temp tree.
        let scripts = root.join("assets/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        let repo_scripts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/scripts");
        std::fs::copy(repo_scripts.join("color_watcher.sh"), scripts.join("color_watcher.sh"))
            .unwrap();
        std::fs::copy(repo_scripts.join("utils.sh"), scripts.join("utils.sh")).unwrap();
        std::fs::copy(repo_scripts.join("colors.sh"), scripts.join("colors.sh")).unwrap();
        copy_color_sources_dir(&repo_scripts, &scripts);

        // Stubs beside the real scripts so utils.sh resolves HVE_SCRIPTS_DIR
        // into the temp tree: assemble.sh (counts its own runs), get_colors.sh
        // and hve-ipc are harmless no-ops. hve-ipc needs +x for the `-x` guard.
        let assemble_marker = cache.join("assemble_runs");
        std::fs::write(
            scripts.join("assemble.sh"),
            format!("#!/bin/bash\necho run >> {}\n", assemble_marker.display()),
        )
        .unwrap();
        std::fs::write(scripts.join("get_colors.sh"), "#!/bin/bash\nexit 0\n").unwrap();
        std::fs::write(scripts.join("hve-ipc"), "#!/bin/bash\nexit 0\n").unwrap();
        for name in ["assemble.sh", "get_colors.sh", "hve-ipc"] {
            let path = scripts.join(name);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // inotifywait (sleeping stub — keeps the watch loop alive without
        // touching the real inotify) and a noctalia stub, both on PATH.
        let stubs = root.join("stubs");
        std::fs::create_dir_all(&stubs).unwrap();
        std::fs::write(stubs.join("inotifywait"), "#!/bin/bash\nexec sleep 2\n").unwrap();
        std::fs::write(stubs.join("noctalia"), "#!/bin/bash\nexit 0\n").unwrap();
        for name in ["inotifywait", "noctalia"] {
            std::fs::set_permissions(&stubs.join(name), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let path_with_stubs =
            format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap_or_default());

        let script_path = scripts.join("color_watcher.sh");
        let spawn_instance = || {
            Command::new("bash")
                .arg(&script_path)
                .env("HOME", &home)
                .env("HVE_CACHE_DIR", &cache)
                .env("PATH", &path_with_stubs)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap()
        };

        // ── Instance 1 must acquire the singleton lock and start ──
        let first = KillOnDrop::new(spawn_instance());

        let log_file = cache.join("color_watcher.log");
        let lock_file = cache.join("color_watcher.lock");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let locked = lock_file.exists();
            let started = std::fs::read_to_string(&log_file)
                .map(|s| s.contains("Starting watcher"))
                .unwrap_or(false);
            let refreshed = std::fs::read_to_string(&assemble_marker)
                .map(|s| s.lines().count() > 0)
                .unwrap_or(false);
            if locked && started && refreshed {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "first instance never acquired the singleton lock — flock guard missing?"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        // ── Instance 2 must be rejected: exit 0, guard log, no work ──
        let mut second = spawn_instance();
        let second_deadline = Instant::now() + Duration::from_secs(10);
        // Hard timeout: a missing guard would make instance 2 loop forever,
        // hanging the suite — bounded here and cleaned up on panic.
        let status = loop {
            match second.try_wait() {
                Ok(Some(st)) => break st,
                Ok(None) => {}
                Err(_) => {}
            }
            assert!(
                Instant::now() < second_deadline,
                "second instance did not exit — singleton guard missing?"
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(
            status.success(),
            "a rejected second instance must exit 0, got {status:?}"
        );

        // ── Assertions on the observable evidence ──
        let log = std::fs::read_to_string(&log_file).unwrap();
        assert!(
            log.contains("Starting watcher"),
            "the first instance logs its start"
        );
        assert_eq!(
            log.matches("Starting watcher").count(),
            1,
            "exactly one instance may start (the second must be rejected before doing work)"
        );
        assert!(
            log.contains("Singleton guard"),
            "the rejected instance must log the guard message, got:\n{log}"
        );
        assert!(
            lock_file.exists(),
            "the singleton lock file must exist under HVE_CACHE_DIR"
        );
        let runs = std::fs::read_to_string(&assemble_marker).unwrap_or_default();
        assert_eq!(
            runs.lines().count(),
            1,
            "exactly one instance performs assemble.sh (the first's initial refresh)"
        );

        // Cleanup: terminate + reap the first instance so the suite never hangs.
        drop(first);
    }

    /// Mirror every executable on the system PATH except `flock`, so a child
    /// can run the real script with `command -v flock` failing while every
    /// other command it needs (`bash`, `md5sum`, `sort`, `wc`, `cut`, `date`,
    /// `dirname`) keeps resolving. Mirrored targets are absolute symlinks, so
    /// the child's lookup never consults the original directories.
    fn system_path_without_flock(root: &std::path::Path, original: &str) -> String {
        let no_flock = root.join("no-flock-bin");
        std::fs::create_dir_all(&no_flock).unwrap();
        for dir in original.split(':') {
            if dir.is_empty() {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(dir) else { continue; };
            for entry in entries.flatten() {
                let name = entry.file_name();
                if name.to_string_lossy() == "flock" {
                    continue;
                }
                // Skip broken symlinks: a command that cannot resolve would
                // not work anyway, and its absence must not fail the test.
                let Ok(target) = std::fs::canonicalize(entry.path()) else {
                    continue;
                };
                let _ = std::os::unix::fs::symlink(target, no_flock.join(&name));
            }
        }
        no_flock.to_string_lossy().into_owned()
    }

    #[test]
    fn sandboxed_watcher_fails_open_when_flock_is_missing() {
        use std::os::unix::fs::PermissionsExt;

        // Same sandbox as the singleton test: HOME + HVE_CACHE_DIR point at
        // the temp tree so the real script never touches the keeper's live
        // session, and every external dependency is a stub.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let home = root.join("home");
        let cache = root.join("cache");
        std::fs::create_dir_all(home.join(".cache/wal")).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(
            home.join(".cache/wal/colors.json"),
            r#"{"wallpaper": "/dev/null"}"#,
        )
        .unwrap();

        let scripts = root.join("assets/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        let repo_scripts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/scripts");
        std::fs::copy(repo_scripts.join("color_watcher.sh"), scripts.join("color_watcher.sh"))
            .unwrap();
        std::fs::copy(repo_scripts.join("utils.sh"), scripts.join("utils.sh")).unwrap();
        std::fs::copy(repo_scripts.join("colors.sh"), scripts.join("colors.sh")).unwrap();
        copy_color_sources_dir(&repo_scripts, &scripts);

        let assemble_marker = cache.join("assemble_runs");
        std::fs::write(
            scripts.join("assemble.sh"),
            format!("#!/bin/bash\necho run >> {}\n", assemble_marker.display()),
        )
        .unwrap();
        std::fs::write(scripts.join("get_colors.sh"), "#!/bin/bash\nexit 0\n").unwrap();
        std::fs::write(scripts.join("hve-ipc"), "#!/bin/bash\nexit 0\n").unwrap();
        for name in ["assemble.sh", "get_colors.sh", "hve-ipc"] {
            let path = scripts.join(name);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let stubs = root.join("stubs");
        std::fs::create_dir_all(&stubs).unwrap();
        std::fs::write(stubs.join("inotifywait"), "#!/bin/bash\nexec sleep 2\n").unwrap();
        std::fs::write(stubs.join("noctalia"), "#!/bin/bash\nexit 0\n").unwrap();
        for name in ["inotifywait", "noctalia"] {
            std::fs::set_permissions(&stubs.join(name), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }

        // PATH WITHOUT flock: the stubs come first, then the mirrored system
        // binaries. This is the fail-open scenario — the singleton guarantee
        // is unavailable, so losing it must not disable the watcher.
        let path_without_flock = format!(
            "{}:{}",
            stubs.display(),
            system_path_without_flock(root, &std::env::var("PATH").unwrap_or_default())
        );

        let watcher = scripts.join("color_watcher.sh");
        let child = Command::new("bash")
            .arg(&watcher)
            .env("HOME", &home)
            .env("HVE_CACHE_DIR", &cache)
            .env("PATH", &path_without_flock)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // RAII cleanup: on panic the guard kills + reaps and the TempDir
        // removes the tree — a failure can never hang or leak a process.
        let _guard = KillOnDrop::new(child);

        // The unguarded watcher must reach normal operation: it logs its
        // start and performs the initial refresh (the assemble marker).
        let log_file = cache.join("color_watcher.log");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let log = std::fs::read_to_string(&log_file).unwrap_or_default();
            let refreshed = std::fs::read_to_string(&assemble_marker)
                .map(|s| s.lines().count() > 0)
                .unwrap_or(false);
            if refreshed && log.contains("Starting watcher") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "watcher did not continue unguarded (no start + initial refresh within 30s); \
                 missing flock must fail OPEN, log so far:\n{log}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        let log = std::fs::read_to_string(&log_file).unwrap();
        assert!(
            log.contains("Singleton guard unavailable"),
            "missing flock must log a distinct guard-unavailable warning, got:\n{log}"
        );
        assert!(
            log.contains("UNGUARDED"),
            "the warning must state the watcher continues unguarded, got:\n{log}"
        );
        assert!(
            !log.contains("another color watcher holds"),
            "no other instance is running here — the log must NOT claim one holds \
             the lock (that message means a false self-rejection), got:\n{log}"
        );
        let runs = std::fs::read_to_string(&assemble_marker).unwrap_or_default();
        assert!(
            runs.lines().count() > 0,
            "the unguarded watcher must still perform the initial refresh (work happened)"
        );
    }

    #[test]
    fn watch_list_still_contains_the_pywal_path_from_its_module() {
        // Unit 1d moved the pywal watch path out of color_watcher.sh into
        // the module's hve_colour_source_watch declaration (capability
        // routing: the list only reads declarations). With only the pywal
        // file present, find_watch_files must still list it.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let home = root.join("home");
        let cache = root.join("cache");
        std::fs::create_dir_all(home.join(".cache/wal")).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(
            home.join(".cache/wal/colors.json"),
            r#"{"wallpaper": "/dev/null"}"#,
        )
        .unwrap();

        let scripts = root.join("assets/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        let repo_scripts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/scripts");
        std::fs::copy(repo_scripts.join("color_watcher.sh"), scripts.join("color_watcher.sh"))
            .unwrap();
        std::fs::copy(repo_scripts.join("utils.sh"), scripts.join("utils.sh")).unwrap();
        std::fs::copy(repo_scripts.join("colors.sh"), scripts.join("colors.sh")).unwrap();
        copy_color_sources_dir(&repo_scripts, &scripts);

        // Source the sandbox layer and call only find_watch_files (sourcing
        // color_watcher.sh itself would start the daemon loop).
        let probe = format!(
            "source \"{s}/utils.sh\" >/dev/null 2>&1; \
             source \"{s}/colors.sh\" >/dev/null 2>&1; \
             eval \"$(sed -n '/^find_watch_files() {{/,/^}}/p' \"{s}/color_watcher.sh\")\"; \
             find_watch_files",
            s = scripts.display()
        );
        let out = Command::new("bash")
            .arg("-c")
            .arg(&probe)
            .env("HOME", &home)
            .env("HVE_CACHE_DIR", &cache)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "watch-list probe failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(".cache/wal/colors.json"),
            "the pywal watch path must still be listed (now from its module), got:\n{stdout}"
        );
    }

    /// Mirror the shipped colour-source modules into a sandbox scripts dir.
    /// The watcher reads watch paths from the modules (through colors.sh),
    /// so a sandbox without color_sources.d/ would silently lose them and
    /// misreport the watch list.
    fn copy_color_sources_dir(repo_scripts: &std::path::Path, scripts: &std::path::Path) {
        let dest = scripts.join("color_sources.d");
        std::fs::create_dir_all(&dest).unwrap();
        let src = repo_scripts.join("color_sources.d");
        if src.is_dir() {
            for entry in std::fs::read_dir(&src).unwrap().flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "sh") {
                    std::fs::copy(&path, dest.join(entry.file_name())).unwrap();
                }
            }
        }
    }

    /// Stateful recording `noctalia` stub for the theme-authority tests.
    ///
    /// Every invocation appends one line (`$*`) to the calls file. The stub
    /// keeps a scheme state file: `msg color-scheme-get` prints it (initially
    /// `custom skwd-wall`) and `msg color-scheme-set custom <name>` rewrites
    /// it, so the watcher observes its own re-assert like the live tool.
    fn write_stateful_noctalia_stub(stubs: &std::path::Path, cache: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let calls = cache.join("noctalia_calls");
        let state = cache.join("noctalia_scheme");
        std::fs::write(&state, "custom skwd-wall\n").unwrap();
        std::fs::write(
            stubs.join("noctalia"),
            format!(
                "#!/bin/bash\nCALLS=\"{}\"\nSTATE=\"{}\"\n\
                 echo \"$*\" >> \"$CALLS\"\n\
                 if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"color-scheme-get\" ]; then\n\
                 cat \"$STATE\" 2>/dev/null || echo \"custom skwd-wall\"\n\
                 elif [ \"$1\" = \"msg\" ] && [ \"$2\" = \"color-scheme-set\" ]; then\n\
                 printf '%s' \"$3 $4\" > \"$STATE\"\n\
                 fi\nexit 0\n",
                calls.display(),
                state.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&stubs.join("noctalia"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }

    /// Sandbox scripts for the theme-authority tests: the real watcher plus
    /// its `utils.sh`/`colors.sh` sources, counting `assemble.sh` stub and
    /// no-op `get_colors.sh`. The `hve-ipc` stub RECORDS every invocation
    /// (`$*` appended to `hve_ipc_calls`), built exactly like the stateful
    /// `noctalia` stub above, so the tests can count how often the watcher
    /// asked the app (`assert-color-authority`) — the capability-routing
    /// mechanism (`odd/tasks/hve-capability-routing.md`, unit 1c3).
    ///
    /// It also plants the colour-authority descriptor in the sandbox cache
    /// dir (the script's `HVE_SAFE_DIR`, via `HVE_CACHE_DIR`) because the
    /// script knows no backend layout and only reads that declaration (the
    /// capability-routing contract, `openspec/specs/capability-routing/spec.md`).
    /// The descriptor claims theme `Animation` owns palette `theme-blue` at
    /// its snapshot under the sandbox theme dir. Tests without that
    /// snapshot/config (no theme applied, wallpaper source) still observe no
    /// authority: the script validates the descriptor (theme match, existing
    /// regular file under `$HOME/.config/hve/themes/`) and falls through.
    fn write_theme_test_scripts(
        scripts: &std::path::Path,
        cache: &std::path::Path,
        home: &std::path::Path,
    ) {
        use std::os::unix::fs::PermissionsExt;
        let repo_scripts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/scripts");
        std::fs::copy(repo_scripts.join("color_watcher.sh"), scripts.join("color_watcher.sh"))
            .unwrap();
        std::fs::copy(repo_scripts.join("utils.sh"), scripts.join("utils.sh")).unwrap();
        std::fs::copy(repo_scripts.join("colors.sh"), scripts.join("colors.sh")).unwrap();
        copy_color_sources_dir(&repo_scripts, &scripts);
        let assemble_marker = cache.join("assemble_runs");
        std::fs::write(
            scripts.join("assemble.sh"),
            format!("#!/bin/bash\necho run >> {}\n", assemble_marker.display()),
        )
        .unwrap();
        std::fs::write(scripts.join("get_colors.sh"), "#!/bin/bash\nexit 0\n").unwrap();
        std::fs::write(
            scripts.join("hve-ipc"),
            format!(
                "#!/bin/bash\necho \"$*\" >> {}\nexit 0\n",
                cache.join("hve_ipc_calls").display()
            ),
        )
        .unwrap();
        for name in ["assemble.sh", "get_colors.sh", "hve-ipc"] {
            let path = scripts.join(name);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let palette_file =
            home.join(".config/hve/themes/Animation/providers/noctalia-v5/palette.json");
        std::fs::write(
            cache.join("color-authority.json"),
            format!(
                "{{\"backend\":\"noctalia-v5\",\"theme\":\"Animation\",\
                  \"palette_file\":\"{}\",\"palette_name\":\"theme-blue\"}}",
                palette_file.display()
            ),
        )
        .unwrap();
    }

    #[test]
    fn watcher_reasserts_the_applied_theme_palette_on_an_external_change() {
        use std::os::unix::fs::PermissionsExt;

        // Theme authority via capability routing
        // (odd/tasks/hve-capability-routing.md, unit 1c3): with a `custom`
        // theme applied, an external rewrite of a watched palette must make
        // the watcher ASK THE APP exactly once
        // (`hve-ipc assert-color-authority`). Bash itself must never run the
        // backend CLI (`color-scheme-set`) nor rewrite backend files — that
        // is now the app's business through the `assert-color-authority`
        // IPC verb (`src/ipc.rs`).
        const BLUE_SNAPSHOT: &str = r##"{"dark":{"mPrimary":"#67abe4","mSecondary":"#d6915c","mTertiary":"#9566cc","mSurface":"#11202c","mSurfaceVariant":"#1d3549"},"light":{"mPrimary":"#2279c3"}}"##;
        const STALE_THEME_BLUE: &str = r##"{"dark":{"mPrimary":"#224466","mSecondary":"#335577","mTertiary":"#446688","mSurface":"#101418","mSurfaceVariant":"#182028"}}"##;
        const AMBER_SKWALL: &str = r##"{"dark":{"mPrimary":"#e4aa67","mSecondary":"#d8dd55","mTertiary":"#6ba5ce","mSurface":"#291f14","mSurfaceVariant":"#372a1b"}}"##;
        const AMBER_HIJACK: &str = r##"{"dark":{"mPrimary":"#e4bb77","mSecondary":"#d8dd55","mTertiary":"#6ba5ce","mSurface":"#2a2015","mSurfaceVariant":"#382b1c"}}"##;

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let home = root.join("home");
        let cache = root.join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        // Applied theme BEFORE the watcher spawns: config + blue snapshot
        // whose source.txt is `custom theme-blue` with NO trailing newline.
        let theme_dir = home.join(".config/hve/themes/Animation/providers/noctalia-v5");
        std::fs::create_dir_all(&theme_dir).unwrap();
        std::fs::write(home.join(".config/hve/config.json"), r#"{"last_applied_theme":"Animation"}"#)
            .unwrap();
        std::fs::write(theme_dir.join("palette.json"), BLUE_SNAPSHOT).unwrap();
        std::fs::write(theme_dir.join("source.txt"), "custom theme-blue").unwrap();

        // Live state: a STALE theme-blue file plus the amber skwd-wall file
        // the external writer owns. The stub reports `custom skwd-wall`.
        let palettes = home.join(".config/noctalia/palettes");
        std::fs::create_dir_all(&palettes).unwrap();
        std::fs::write(palettes.join("theme-blue.json"), STALE_THEME_BLUE).unwrap();
        std::fs::write(palettes.join("skwd-wall.json"), AMBER_SKWALL).unwrap();

        let scripts = root.join("assets/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        write_theme_test_scripts(&scripts, &cache, &home);

        let stubs = root.join("stubs");
        std::fs::create_dir_all(&stubs).unwrap();
        std::fs::write(stubs.join("inotifywait"), "#!/bin/bash\nexec sleep 2\n").unwrap();
        std::fs::set_permissions(&stubs.join("inotifywait"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        write_stateful_noctalia_stub(&stubs, &cache);
        let path_with_stubs =
            format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap_or_default());

        let script_path = scripts.join("color_watcher.sh");
        let _watcher = KillOnDrop::new(
            Command::new("bash")
                .arg(&script_path)
                .env("HOME", &home)
                .env("HVE_CACHE_DIR", &cache)
                .env("PATH", &path_with_stubs)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );

        // Wait until the watcher is armed (lock + start line + initial refresh).
        let log_file = cache.join("color_watcher.log");
        let lock_file = cache.join("color_watcher.lock");
        let assemble_marker = cache.join("assemble_runs");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let armed = lock_file.exists()
                && std::fs::read_to_string(&log_file)
                    .map(|s| s.contains("Starting watcher"))
                    .unwrap_or(false)
                && std::fs::read_to_string(&assemble_marker)
                    .map(|s| s.lines().count() > 0)
                    .unwrap_or(false);
            if armed {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "watcher never armed within 30s"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        // Tell the start-side ask from the loop-side one apart: the `force`
        // ask at startup already fired. Clear both call logs and the ask
        // cooldown stamp, so the hijack below measures exactly the
        // loop-side reaction — one ask, zero backend CLI.
        std::fs::write(cache.join("hve_ipc_calls"), "").unwrap();
        std::fs::write(cache.join("noctalia_calls"), "").unwrap();
        let _ = std::fs::remove_file(cache.join("assert-color-authority.last"));

        // The hijack: an external writer rewrites the watched palette.
        std::fs::write(palettes.join("skwd-wall.json"), AMBER_HIJACK).unwrap();

        let ipc_calls_file = cache.join("hve_ipc_calls");
        let ask_count = || {
            std::fs::read_to_string(&ipc_calls_file)
                .unwrap_or_default()
                .lines()
                .filter(|l| l.contains("assert-color-authority"))
                .count()
        };
        let poll_deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if ask_count() == 1 {
                break;
            }
            assert!(
                Instant::now() < poll_deadline,
                "watcher never asked the app to re-assert within 20s \
                 (assert-color-authority calls: {}, log: {:?})",
                ask_count(),
                std::fs::read_to_string(&log_file).unwrap_or_default(),
            );
            std::thread::sleep(Duration::from_millis(100));
        }

        // Settle past the inotify cycle: a second ask (spam through the
        // socket) must never arrive — the bash-side cooldown forbids it.
        std::thread::sleep(Duration::from_secs(6));

        assert_eq!(
            ask_count(),
            1,
            "exactly one `assert-color-authority` ask must be recorded, got:\n{}",
            std::fs::read_to_string(&ipc_calls_file).unwrap_or_default()
        );
        let calls = std::fs::read_to_string(cache.join("noctalia_calls")).unwrap_or_default();
        assert!(
            !calls.contains("color-scheme-set"),
            "bash must never run the backend CLI itself — re-asserting is the \
             app's business, got:\n{calls}"
        );
        let live = std::fs::read_to_string(palettes.join("theme-blue.json")).unwrap_or_default();
        assert_eq!(
            live, STALE_THEME_BLUE,
            "bash must not rewrite backend files — the live theme-blue file \
             stays byte-identical until the app re-asserts, got: {live:?}"
        );
    }

    #[test]
    fn watcher_without_applied_theme_keeps_following_the_live_palette() {
        use std::os::unix::fs::PermissionsExt;

        // Regression: with no theme applied the watcher stays a follower —
        // an external palette rewrite regenerates the overlay and never
        // issues a `color-scheme-set`.
        const AMBER_SKWALL: &str = r##"{"dark":{"mPrimary":"#e4aa67","mSecondary":"#d8dd55","mTertiary":"#6ba5ce","mSurface":"#291f14","mSurfaceVariant":"#372a1b"}}"##;
        const AMBER_HIJACK: &str = r##"{"dark":{"mPrimary":"#e4bb77","mSecondary":"#d8dd55","mTertiary":"#6ba5ce","mSurface":"#2a2015","mSurfaceVariant":"#382b1c"}}"##;

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let home = root.join("home");
        let cache = root.join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        // No config.json on purpose: no theme authority.
        let palettes = home.join(".config/noctalia/palettes");
        std::fs::create_dir_all(&palettes).unwrap();
        std::fs::write(palettes.join("skwd-wall.json"), AMBER_SKWALL).unwrap();

        let scripts = root.join("assets/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        write_theme_test_scripts(&scripts, &cache, &home);

        let stubs = root.join("stubs");
        std::fs::create_dir_all(&stubs).unwrap();
        std::fs::write(stubs.join("inotifywait"), "#!/bin/bash\nexec sleep 2\n").unwrap();
        std::fs::set_permissions(&stubs.join("inotifywait"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        write_stateful_noctalia_stub(&stubs, &cache);
        let path_with_stubs =
            format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap_or_default());

        let script_path = scripts.join("color_watcher.sh");
        let _watcher = KillOnDrop::new(
            Command::new("bash")
                .arg(&script_path)
                .env("HOME", &home)
                .env("HVE_CACHE_DIR", &cache)
                .env("PATH", &path_with_stubs)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );

        let log_file = cache.join("color_watcher.log");
        let lock_file = cache.join("color_watcher.lock");
        let assemble_marker = cache.join("assemble_runs");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let armed = lock_file.exists()
                && std::fs::read_to_string(&log_file)
                    .map(|s| s.contains("Starting watcher"))
                    .unwrap_or(false)
                && std::fs::read_to_string(&assemble_marker)
                    .map(|s| s.lines().count() > 0)
                    .unwrap_or(false);
            if armed {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "watcher never armed within 30s"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        std::fs::write(palettes.join("skwd-wall.json"), AMBER_HIJACK).unwrap();

        let refresh_deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let runs = std::fs::read_to_string(&assemble_marker).unwrap_or_default();
            if runs.lines().count() >= 2 {
                break;
            }
            assert!(
                Instant::now() < refresh_deadline,
                "watcher never regenerated the overlay after the external change"
            );
            std::thread::sleep(Duration::from_millis(100));
        }

        std::thread::sleep(Duration::from_secs(3));
        let calls = std::fs::read_to_string(cache.join("noctalia_calls")).unwrap_or_default();
        assert!(
            !calls.contains("color-scheme-set"),
            "with no theme applied no `color-scheme-set` may be issued, got:\n{calls}"
        );
        let ipc_calls =
            std::fs::read_to_string(cache.join("hve_ipc_calls")).unwrap_or_default();
        assert!(
            !ipc_calls.contains("assert-color-authority"),
            "with no theme applied the watcher must never ask the app to re-assert, got:\n{ipc_calls}"
        );
    }

    #[test]
    fn watcher_with_wallpaper_source_theme_does_not_reassert() {
        use std::os::unix::fs::PermissionsExt;

        // Regression: a `wallpaper`-source theme owns no palette snapshot, so
        // the watcher keeps following the live palette.
        const AMBER_SKWALL: &str = r##"{"dark":{"mPrimary":"#e4aa67","mSecondary":"#d8dd55","mTertiary":"#6ba5ce","mSurface":"#291f14","mSurfaceVariant":"#372a1b"}}"##;
        const AMBER_HIJACK: &str = r##"{"dark":{"mPrimary":"#e4bb77","mSecondary":"#d8dd55","mTertiary":"#6ba5ce","mSurface":"#2a2015","mSurfaceVariant":"#382b1c"}}"##;

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let home = root.join("home");
        let cache = root.join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        // Wallpaper-source theme: source.txt says `wallpaper` and there is
        // deliberately NO palette.json.
        let theme_dir = home.join(".config/hve/themes/Animation/providers/noctalia-v5");
        std::fs::create_dir_all(&theme_dir).unwrap();
        std::fs::write(home.join(".config/hve/config.json"), r#"{"last_applied_theme":"Animation"}"#)
            .unwrap();
        std::fs::write(theme_dir.join("source.txt"), "wallpaper\n").unwrap();

        let palettes = home.join(".config/noctalia/palettes");
        std::fs::create_dir_all(&palettes).unwrap();
        std::fs::write(palettes.join("skwd-wall.json"), AMBER_SKWALL).unwrap();

        let scripts = root.join("assets/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        write_theme_test_scripts(&scripts, &cache, &home);

        let stubs = root.join("stubs");
        std::fs::create_dir_all(&stubs).unwrap();
        std::fs::write(stubs.join("inotifywait"), "#!/bin/bash\nexec sleep 2\n").unwrap();
        std::fs::set_permissions(&stubs.join("inotifywait"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        write_stateful_noctalia_stub(&stubs, &cache);
        let path_with_stubs =
            format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap_or_default());

        let script_path = scripts.join("color_watcher.sh");
        let _watcher = KillOnDrop::new(
            Command::new("bash")
                .arg(&script_path)
                .env("HOME", &home)
                .env("HVE_CACHE_DIR", &cache)
                .env("PATH", &path_with_stubs)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );

        let log_file = cache.join("color_watcher.log");
        let lock_file = cache.join("color_watcher.lock");
        let assemble_marker = cache.join("assemble_runs");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let armed = lock_file.exists()
                && std::fs::read_to_string(&log_file)
                    .map(|s| s.contains("Starting watcher"))
                    .unwrap_or(false)
                && std::fs::read_to_string(&assemble_marker)
                    .map(|s| s.lines().count() > 0)
                    .unwrap_or(false);
            if armed {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "watcher never armed within 30s"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        std::fs::write(palettes.join("skwd-wall.json"), AMBER_HIJACK).unwrap();

        let refresh_deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let runs = std::fs::read_to_string(&assemble_marker).unwrap_or_default();
            if runs.lines().count() >= 2 {
                break;
            }
            assert!(
                Instant::now() < refresh_deadline,
                "watcher never regenerated the overlay after the external change"
            );
            std::thread::sleep(Duration::from_millis(100));
        }

        std::thread::sleep(Duration::from_secs(3));
        let calls = std::fs::read_to_string(cache.join("noctalia_calls")).unwrap_or_default();
        assert!(
            !calls.contains("color-scheme-set"),
            "with a wallpaper-source theme no `color-scheme-set` may be issued, got:\n{calls}"
        );
        let ipc_calls =
            std::fs::read_to_string(cache.join("hve_ipc_calls")).unwrap_or_default();
        assert!(
            !ipc_calls.contains("assert-color-authority"),
            "with a wallpaper-source theme the watcher must never ask the app to re-assert, got:\n{ipc_calls}"
        );
    }

    #[test]
    fn colour_module_routing_declares_settings_watch_and_refresh() {
        use std::os::unix::fs::PermissionsExt;

        // Unit 1d4 (capability routing, odd/tasks/hve-capability-routing.md):
        // the settings.toml trigger and the templates-apply refresh belong
        // to the backend module. The loader routes a changed path to the
        // module that declared it; the module owns the refresh action and
        // the core only routes it.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let home = root.join("home");
        let cache = root.join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        let scripts = root.join("assets/scripts");
        std::fs::create_dir_all(scripts.join("color_sources.d")).unwrap();
        let repo_scripts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/scripts");
        std::fs::copy(repo_scripts.join("colors.sh"), scripts.join("colors.sh")).unwrap();
        std::fs::copy(repo_scripts.join("utils.sh"), scripts.join("utils.sh")).unwrap();
        copy_color_sources_dir(&repo_scripts, &scripts);

        // The module declares settings.toml only when the tool is
        // available: a `noctalia` stub on PATH plus the file itself.
        let stubs = root.join("stubs");
        std::fs::create_dir_all(&stubs).unwrap();
        std::fs::write(stubs.join("noctalia"), "#!/bin/bash\nexit 0\n").unwrap();
        std::fs::set_permissions(&stubs.join("noctalia"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let state_dir = home.join(".local/state/noctalia");
        std::fs::create_dir_all(&state_dir).unwrap();
        let settings = state_dir.join("settings.toml");
        std::fs::write(&settings, "[general]\n").unwrap();
        let path_with_stubs =
            format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap_or_default());

        let run_probe = |bash_body: &str| {
            Command::new("bash")
                .arg("-c")
                .arg(bash_body)
                .env("HOME", &home)
                .env("HVE_CACHE_DIR", &cache)
                .env("PATH", &path_with_stubs)
                .output()
                .unwrap()
        };

        // (i) The loader routes settings.toml to the owning module file.
        let probe = format!(
            "source \"{s}/utils.sh\" >/dev/null 2>&1; \
             source \"{s}/colors.sh\" >/dev/null 2>&1; \
             _hve_colour_module_for_path \"{p}\"",
            s = scripts.display(),
            p = settings.display()
        );
        let out = run_probe(&probe);
        assert!(
            out.status.success(),
            "the loader must route the settings path to a module: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.trim().ends_with("color_sources.d/noctalia_lua.sh"),
            "settings.toml must route to the owning module, got:\n{stdout}"
        );

        // (ii) That module defines the refresh action itself.
        let module = scripts.join("color_sources.d/noctalia_lua.sh");
        let probe = format!(
            "source \"{}\" >/dev/null 2>&1; declare -F hve_colour_source_refresh",
            module.display()
        );
        let out = run_probe(&probe);
        assert!(
            out.status.success(),
            "the owning module must define hve_colour_source_refresh"
        );

        // (iii) A path no module declares routes nowhere — no refresh.
        let probe = format!(
            "source \"{s}/utils.sh\" >/dev/null 2>&1; \
             source \"{s}/colors.sh\" >/dev/null 2>&1; \
             _hve_colour_module_for_path \"/definitely/not/watched-xyz.json\"",
            s = scripts.display()
        );
        let out = run_probe(&probe);
        assert!(
            !out.status.success(),
            "a path with no declaring module must route nowhere"
        );
    }

    #[test]
    fn watcher_routes_settings_change_through_declared_module_refresh() {
        use std::os::unix::fs::PermissionsExt;

        // Unit 1d4 end to end: rewriting the watched settings.toml must run
        // the OWNING MODULE's refresh exactly once (the stub records
        // `templates-apply`) and still regenerate the overlay — routed
        // generically, never through a hardcoded branch. A change in a path
        // whose module defines no refresh regenerates the overlay with no
        // refresh at all.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let home = root.join("home");
        let cache = root.join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        // Watched from the start: the backend settings file, one rendered
        // lua file (the refresh rewrites it — the re-baseline must swallow
        // that write), and a pywal file whose module defines no refresh.
        let state_dir = home.join(".local/state/noctalia");
        std::fs::create_dir_all(&state_dir).unwrap();
        let settings = state_dir.join("settings.toml");
        std::fs::write(&settings, "[general]\n").unwrap();
        let hypr_dir = home.join(".config/hypr");
        std::fs::create_dir_all(&hypr_dir).unwrap();
        let lua = hypr_dir.join("noctalia.lua");
        std::fs::write(&lua, "-- rendered v1\n").unwrap();
        std::fs::create_dir_all(home.join(".cache/wal")).unwrap();
        let wal = home.join(".cache/wal/colors.json");
        std::fs::write(&wal, r#"{"wallpaper": "/dev/null"}"#).unwrap();

        let scripts = root.join("assets/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        write_theme_test_scripts(&scripts, &cache, &home);

        // Recording `noctalia` stub: every invocation appends `$*` to the
        // calls file, and `templates-apply` rewrites the rendered lua —
        // exactly what the live tool does, so a missing re-baseline would
        // re-trigger the loop and the count below would grow past one.
        let stubs = root.join("stubs");
        std::fs::create_dir_all(&stubs).unwrap();
        std::fs::write(stubs.join("inotifywait"), "#!/bin/bash\nexec sleep 2\n").unwrap();
        std::fs::write(
            stubs.join("noctalia"),
            format!(
                "#!/bin/bash\nCALLS=\"{}\"\nLUA=\"{}\"\n\
                 echo \"$*\" >> \"$CALLS\"\n\
                 if [ \"$1\" = \"msg\" ] && [ \"$2\" = \"templates-apply\" ]; then\n\
                 echo \"-- rendered\" >> \"$LUA\"\n\
                 fi\nexit 0\n",
                cache.join("noctalia_calls").display(),
                lua.display()
            ),
        )
        .unwrap();
        for name in ["inotifywait", "noctalia"] {
            std::fs::set_permissions(&stubs.join(name), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let path_with_stubs =
            format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap_or_default());

        let script_path = scripts.join("color_watcher.sh");
        let _watcher = KillOnDrop::new(
            Command::new("bash")
                .arg(&script_path)
                .env("HOME", &home)
                .env("HVE_CACHE_DIR", &cache)
                .env("PATH", &path_with_stubs)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );

        let log_file = cache.join("color_watcher.log");
        let lock_file = cache.join("color_watcher.lock");
        let assemble_marker = cache.join("assemble_runs");
        let calls_file = cache.join("noctalia_calls");
        let assemble_runs = || {
            std::fs::read_to_string(&assemble_marker)
                .unwrap_or_default()
                .lines()
                .count()
        };
        let refresh_runs = || {
            std::fs::read_to_string(&calls_file)
                .unwrap_or_default()
                .lines()
                .filter(|l| l.contains("templates-apply"))
                .count()
        };

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let armed = lock_file.exists()
                && std::fs::read_to_string(&log_file)
                    .map(|s| s.contains("Starting watcher"))
                    .unwrap_or(false)
                && assemble_runs() > 0;
            if armed {
                break;
            }
            assert!(Instant::now() < deadline, "watcher never armed within 30s");
            std::thread::sleep(Duration::from_millis(50));
        }

        // Measure the loop-side reaction only: clear the calls recorded
        // during arming (none expected — the stub only records).
        std::fs::write(&calls_file, "").unwrap();
        let runs_before = assemble_runs();

        // Phase A: an external settings rewrite.
        std::fs::write(&settings, "[general]\ntheme = \"amber\"\n").unwrap();
        let poll = Instant::now() + Duration::from_secs(20);
        loop {
            if refresh_runs() == 1 && assemble_runs() >= runs_before + 1 {
                break;
            }
            assert!(
                Instant::now() < poll,
                "watcher never ran the declared refresh within 20s \
                 (refreshes: {}, log: {:?})",
                refresh_runs(),
                std::fs::read_to_string(&log_file).unwrap_or_default(),
            );
            std::thread::sleep(Duration::from_millis(100));
        }

        // The routing must be generic: the new mechanism logs its declared
        // refresh, and the old hardcoded branch (with its own log line) is
        // gone.
        let log = std::fs::read_to_string(&log_file).unwrap_or_default();
        assert!(
            log.contains("Declared refresh"),
            "the refresh must be routed as a declared module action, got:\n{log}"
        );
        assert!(
            !log.contains("applying templates"),
            "the hardcoded settings branch must be gone, got:\n{log}"
        );

        // Settle past several watch cycles: the refresh's own lua rewrite
        // was re-baselined, so no second refresh may fire.
        std::thread::sleep(Duration::from_secs(6));
        assert_eq!(
            refresh_runs(),
            1,
            "the module refresh must run exactly once, got:\n{}",
            std::fs::read_to_string(&calls_file).unwrap_or_default()
        );

        // Phase B: a path whose module defines no refresh.
        let regen_before = assemble_runs();
        std::fs::write(&wal, r#"{"wallpaper": "/other"}"#).unwrap();
        let poll = Instant::now() + Duration::from_secs(20);
        loop {
            if assemble_runs() >= regen_before + 1 {
                break;
            }
            assert!(
                Instant::now() < poll,
                "watcher never regenerated the overlay after the pywal change"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_secs(3));
        assert_eq!(
            refresh_runs(),
            1,
            "a change with no declaring refresh must run no refresh, got:\n{}",
            std::fs::read_to_string(&calls_file).unwrap_or_default()
        );
    }

    /// W3 (odd/tasks/palette-authority-mute-and-yield.md): the cooldown used
    /// to drop an ask with NO line at all, and the `assert-color-authority`
    /// stamp was written BEFORE the helper check — so an absent `hve-ipc`
    /// poisoned the following 5 s. The script must log the suppression and
    /// write the stamp only once the ask is really made. Static contract over
    /// the real script, comment-stripped, like the house precedent: a
    /// commented-out branch cannot satisfy it.
    #[test]
    fn the_watcher_logs_suppressed_asks_and_stamps_only_a_real_ask() {
        let src = std::fs::read_to_string("assets/scripts/color_watcher.sh")
            .expect("color_watcher.sh must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        let start = code
            .find("_reassert_theme_authority()")
            .expect("_reassert_theme_authority must exist");
        let body = &code[start..];
        let end = body.find("\n}").unwrap_or(body.len());
        let body = &body[..end];

        assert!(
            body.contains("suppressed"),
            "the cooldown drop must be logged, not silent: {body}"
        );
        let helper_check = body
            .find("-x \"$HVE_SCRIPTS_DIR/hve-ipc\"")
            .expect("the helper must be checked before asking");
        let stamp_write = body
            .find("> \"$stamp_file\"")
            .expect("the stamp must be written");
        assert!(
            helper_check < stamp_write,
            "the stamp must be written only after the helper is proven present"
        );
    }
}