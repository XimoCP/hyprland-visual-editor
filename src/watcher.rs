use std::path::PathBuf;

/// Owned handle to the spawned color watcher process.
///
/// Dropping a raw `std::process::Child` NEVER kills the child (its `Drop`
/// only closes the stdin handle), so an unguarded watcher is orphaned and
/// keeps running forever — the S5 defect of odd/hide-idempotency-and-singleton-watcher.
/// This guard terminates the watcher on drop AND reaps it (`wait`), so the
/// pid is fully released and no zombie is left under `/proc`.
pub struct ColorWatcher {
    child: std::process::Child,
}

impl ColorWatcher {
    /// Kill the watcher (best-effort) and reap it. Idempotent: a child that
    /// already exited makes `kill` fail harmlessly and `wait` still reaps it.
    fn terminate(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ColorWatcher {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// Spawn the color_watcher.sh bash script and return the owned guard handle.
pub fn spawn_color_watcher(proj: &std::path::Path) -> Option<ColorWatcher> {
    let watcher_script = proj.join("assets").join("scripts").join("color_watcher.sh");
    let watcher_log = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("hve")
        .join("color_watcher.log");

    if watcher_script.exists() {
        // Log to file so we can diagnose watcher issues
        let log_file = std::fs::File::options()
            .create(true)
            .append(true)
            .open(&watcher_log)
            .ok()
            .and_then(|f| f.try_clone().ok());
        let stderr: std::process::Stdio = log_file
            .map(std::process::Stdio::from)
            .unwrap_or(std::process::Stdio::null());

        match std::process::Command::new("bash")
            .arg(watcher_script)
            .stdout(std::process::Stdio::null())
            .stderr(stderr)
            .spawn()
        {
            Ok(child) => {
                tracing::info!("[HVE] Color watcher started (pid: {})", child.id());
                tracing::info!("[HVE] Watcher log: {}", watcher_log.display());
                Some(ColorWatcher { child })
            }
            Err(e) => {
                tracing::error!("[HVE] Could not start color watcher: {}", e);
                None
            }
        }
    } else {
        tracing::warn!(
            "[HVE] Color watcher script not found: {}",
            watcher_script.display()
        );
        None
    }
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
        let guard = ColorWatcher { child };
        drop(guard);
        assert!(
            !PathBuf::from(format!("/proc/{pid}")).exists(),
            "guard drop must kill and reap the child (no /proc/{pid} entry)"
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
        std::fs::write(stubs.join("inotifywait"), "#!/bin/bash\nsleep 2\n").unwrap();
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

        let log_file = home.join(".cache/hve/color_watcher.log");
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
        std::fs::write(stubs.join("inotifywait"), "#!/bin/bash\nsleep 2\n").unwrap();
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
        let log_file = home.join(".cache/hve/color_watcher.log");
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
}