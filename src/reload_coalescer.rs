//! Sandboxed behavioural tests for the reload coalescer
//! (`assets/scripts/reload_coalescer.sh` + the `assemble.sh` wiring).
//!
//! House pattern from `hide-idempotency-and-singleton-watcher.md` and
//! `scripts_contract.rs`: the REAL scripts are copied into a temp tree,
//! a stub `bin/` is prepended to `PATH` (`hyprctl`, `pgrep`, `notify-send`),
//! `HOME` and `HVE_CACHE_DIR` point into the temp tree, and the ONLY
//! `hyprctl` in the loop is a timestamped recording stub — the real
//! compositor is never invoked and the keeper's live session is never
//! touched.
//!
//! What is pinned here is the unit's primary acceptance criterion: a reload
//! is never lost, only coalesced — the worst acceptable failure is that a
//! reload arrives LATER, never that it never arrives.
#![cfg(test)]

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_scripts() -> PathBuf {
    repo_root().join("assets").join("scripts")
}

fn now_epoch_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

/// Hermetic temp tree holding the real scripts + stubs + fake HOME.
struct Sandbox {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    cache: PathBuf,
    stubs: PathBuf,
    hyprctl_log: PathBuf,
}

impl Sandbox {
    fn build() -> Sandbox {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let home = root.join("home");
        let cache = root.join("cache");
        let scripts = root.join("assets/scripts");
        let stubs = root.join("stubs");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::create_dir_all(&stubs).unwrap();
        std::fs::create_dir_all(home.join(".config/hypr")).unwrap();
        std::fs::create_dir_all(home.join(".cache/wal")).unwrap();
        std::fs::create_dir_all(&cache).unwrap();

        // Real scripts (unmodified) + the coalescer under test.
        for name in [
            "assemble.sh",
            "apply_animation.sh",
            "border.sh",
            "shader.sh",
            "geometry.sh",
            "utils.sh",
            "colors.sh",
            "get_colors.sh",
            "reload_coalescer.sh",
        ] {
            std::fs::copy(repo_scripts().join(name), scripts.join(name)).unwrap_or_else(|e| {
                panic!("cannot stage {name}: {e}")
            });
        }
        for dir in std::fs::read_dir(&scripts).unwrap() {
            let p = dir.unwrap().path();
            if p.extension().and_then(|e| e.to_str()) == Some("sh") {
                make_exec(&p);
            }
        }

        // Preset assets the four apply scripts resolve.
        std::fs::create_dir_all(root.join("assets/animations")).unwrap();
        std::fs::create_dir_all(root.join("assets/borders")).unwrap();
        std::fs::create_dir_all(root.join("assets/shaders")).unwrap();
        std::fs::write(
            root.join("assets/animations/test.lua"),
            "hl.config({ animations = { enabled = true } })\n",
        )
        .unwrap();
        std::fs::write(
            root.join("assets/borders/test.lua"),
            "hl.config({ general = { [\"col.active_border\"] = primary } })\n",
        )
        .unwrap();
        std::fs::write(root.join("assets/shaders/test.frag"), "// dummy\n").unwrap();

        // Colour source for colors.sh (Noctalia v5 style).
        std::fs::write(
            home.join(".config/hypr/noctalia.lua"),
            "local primary = \"rgb(7a5cff)\"\nlocal secondary = \"rgb(2ec436)\"\n",
        )
        .unwrap();

        // Stub bin: the ONLY hyprctl in the loop, timestamped; pgrep honours
        // HVE_SIM_HYPRLAND.
        std::fs::write(
            stubs.join("hyprctl"),
            "#!/bin/bash\n\
             if [ \"${HVE_SIM_HYPRLAND:-1}\" = \"1\" ]; then\n\
             \x20 echo \"$(date +%s.%N) hyprctl $*\" >> \"${HVE_SIM_LOG}\"\n\
             fi\n\
             exit 0\n",
        )
        .unwrap();
        std::fs::write(
            stubs.join("pgrep"),
            "#!/bin/bash\n\
             if [ \"$1\" = \"-x\" ]; then\n\
             \x20 case \"$2\" in\n\
             \x20\x20 Hyprland) [ \"${HVE_SIM_HYPRLAND:-1}\" = \"1\" ] && exit 0 || exit 1;;\n\
             \x20 esac\n\
             fi\n\
             exit 1\n",
        )
        .unwrap();
        std::fs::write(stubs.join("notify-send"), "#!/bin/bash\nexit 0\n").unwrap();
        for name in ["hyprctl", "pgrep", "notify-send"] {
            make_exec(&stubs.join(name));
        }

        Sandbox {
            _tmp: tmp,
            root: root.clone(),
            home,
            cache,
            stubs,
            hyprctl_log: root.join("hyprctl.log"),
        }
    }

    fn scripts(&self) -> PathBuf {
        self.root.join("assets/scripts")
    }

    fn path_with_stubs(&self) -> String {
        format!("{}:{}", self.stubs.display(), std::env::var("PATH").unwrap_or_default())
    }

    fn base_env(&self, hyprland_up: bool, coalesce: &str, drain_ms: &str) -> Vec<(&str, String)> {
        vec![
            ("HOME", self.home.to_string_lossy().into_owned()),
            ("HVE_CACHE_DIR", self.cache.to_string_lossy().into_owned()),
            ("PATH", self.path_with_stubs()),
            ("HVE_SIM_LOG", self.hyprctl_log.to_string_lossy().into_owned()),
            ("HVE_SIM_HYPRLAND", if hyprland_up { "1" } else { "0" }.to_string()),
            ("HVE_RELOAD_COALESCE", coalesce.to_string()),
            ("HVE_RELOAD_DRAIN_MS", drain_ms.to_string()),
        ]
    }

    /// Run one real apply script the way `Engine::run_script` does (bash,
    /// captured stdout/stderr), bounded: a drainer that wrongly inherits the
    /// capture pipes would block `.output()` forever, so every run is polled
    /// with a deadline and killed on timeout — a hang is a loud RED, never a
    /// hung suite.
    fn run_apply(&self, script: &str, args: &[&str], hyprland_up: bool, coalesce: &str, drain_ms: &str) -> Output {
        use std::process::{Command, Stdio};
        let mut child = Command::new("bash")
            .arg(self.scripts().join(script))
            .args(args)
            .envs(self.base_env(hyprland_up, coalesce, drain_ms))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("bash must spawn");
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut exited = false;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        if !exited {
            let _ = child.kill();
            panic!("{script} did not exit within 15 s — the coalescer drainer may be holding the capture pipes");
        }
        let out = child.wait_with_output().expect("wait_for_output");
        assert!(
            out.status.success(),
            "{script} failed (args {args:?}): {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn reload_count(&self) -> usize {
        std::fs::read_to_string(&self.hyprctl_log)
            .map(|s| s.lines().filter(|l| l.contains(" hyprctl reload")).count())
            .unwrap_or(0)
    }

    fn first_reload_ts(&self) -> Option<f64> {
        let log = std::fs::read_to_string(&self.hyprctl_log).unwrap_or_default();
        log.lines()
            .find(|l| l.contains(" hyprctl reload"))
            .and_then(|l| l.split_whitespace().next())
            .and_then(|t| t.parse().ok())
    }

    fn marker_path(&self) -> PathBuf {
        self.cache.join("reload.requested")
    }

    fn drainer_pid(&self) -> Option<i32> {
        std::fs::read_to_string(self.cache.join("reload.drain.pid"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
    }

    /// Poll until `pred` holds, panicking with `desc` on timeout.
    fn wait_until(&self, mut pred: impl FnMut() -> bool, desc: &str, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if pred() {
                return;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        panic!("timed out waiting for {desc}");
    }

    /// Assert the reload count is `expected` AND stays so for `quiet`.
    fn assert_stable(&self, expected: usize, quiet: Duration) {
        assert_eq!(self.reload_count(), expected, "reload count");
        std::thread::sleep(quiet);
        assert_eq!(
            self.reload_count(),
            expected,
            "reload count must stay stable (a late duplicate is a loss-of-coalescing bug)"
        );
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // The drainer is not our direct child (it was spawned by the apply
        // script's bash), so a plain reap is impossible — a bounded SIGKILL
        // poll keeps parallel tests from leaking sleeping drainers.
        if let Some(pid) = self.drainer_pid() {
            let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status();
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline
                && std::path::Path::new(&format!("/proc/{pid}")).exists()
            {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

fn make_exec(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(path).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(path, perm).unwrap();
}

/// The four-fragment theme apply, in `hve_presets::apply` order (measured:
/// `odd/tasks/coalesce-config-reloads.md`).
fn apply_four_fragments(sb: &Sandbox, hyprland_up: bool, coalesce: &str, drain_ms: &str) {
    sb.run_apply("apply_animation.sh", &["test"], hyprland_up, coalesce, drain_ms);
    sb.run_apply("border.sh", &["test"], hyprland_up, coalesce, drain_ms);
    sb.run_apply("shader.sh", &["test.frag"], hyprland_up, coalesce, drain_ms);
    sb.run_apply("geometry.sh", &["4", "4", "5", "5"], hyprland_up, coalesce, drain_ms);
}

const DRAIN: &str = "500"; // 500 ms — fast tests, still ≥ the apply burst.

// ── The four fragments of one theme apply fire EXACTLY one reload ────────

#[test]
fn sandboxed_burst_fires_exactly_one_reload() {
    let sb = Sandbox::build();
    let before = now_epoch_secs();
    apply_four_fragments(&sb, true, "1", DRAIN);

    sb.wait_until(|| sb.reload_count() == 1, "one reload for the burst", Duration::from_secs(8));
    sb.assert_stable(1, Duration::from_millis(1200));

    assert!(
        sb.cache.join("overlay.lua").exists(),
        "assemble must still write the overlay"
    );
    let fire_ts = sb.first_reload_ts().expect("one reload exists");
    assert!(
        fire_ts >= before - 0.05,
        "the coalesced reload must arrive AFTER the burst (later, never lost): {fire_ts} vs burst start {before}"
    );
}

// ── Never lost: a drainer killed between marker and fire loses nothing ───

#[test]
fn sandboxed_killed_drainer_loses_nothing() {
    let sb = Sandbox::build();
    sb.run_apply("apply_animation.sh", &["test"], true, "1", DRAIN);

    // Kill the drainer as soon as its pid file appears — well inside the
    // drain window, before it can fire.
    sb.wait_until(|| sb.drainer_pid().is_some(), "drainer pid file", Duration::from_secs(3));
    let pid = sb.drainer_pid().unwrap();
    let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && std::path::Path::new(&format!("/proc/{pid}")).exists() {
        std::thread::sleep(Duration::from_millis(20));
    }

    // The killed drainer never fired: still zero reloads after a full window.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(sb.reload_count(), 0, "the killed drainer must not have fired");

    // The next request spawns a fresh drainer that consumes the leftover
    // marker: the reload arrives LATER, never never.
    sb.run_apply("border.sh", &["test"], true, "1", DRAIN);
    sb.wait_until(|| sb.reload_count() == 1, "the delayed reload", Duration::from_secs(8));
    sb.assert_stable(1, Duration::from_millis(1200));
    assert!(
        !sb.marker_path().exists(),
        "the marker must be consumed by the fire"
    );
}

// ── A stray marker (orphaned by a dead drainer) is consumed next request ─

#[test]
fn sandboxed_stray_marker_is_consumed_by_the_next_request() {
    let sb = Sandbox::build();
    std::fs::write(sb.marker_path(), "stray\n").unwrap();

    sb.run_apply("geometry.sh", &["4", "4", "5", "5"], true, "1", DRAIN);
    sb.wait_until(|| sb.reload_count() == 1, "the stray marker to be fired", Duration::from_secs(8));
    sb.assert_stable(1, Duration::from_millis(1200));
    assert!(!sb.marker_path().exists(), "marker consumed");
}

// ── Independent changes (outside the drain window) each fire their own ───

#[test]
fn sandboxed_distinct_bursts_each_fire() {
    let sb = Sandbox::build();
    sb.run_apply("apply_animation.sh", &["test"], true, "1", DRAIN);
    sb.wait_until(|| sb.reload_count() == 1, "first burst reload", Duration::from_secs(8));
    sb.assert_stable(1, Duration::from_millis(1200));

    sb.run_apply("border.sh", &["test"], true, "1", DRAIN);
    sb.wait_until(|| sb.reload_count() == 2, "second burst reload", Duration::from_secs(8));
    sb.assert_stable(2, Duration::from_millis(1200));
}

// ── Requests inside the drain window fold into one reload ────────────────

#[test]
fn sandboxed_requests_within_drain_join() {
    let sb = Sandbox::build();
    sb.run_apply("apply_animation.sh", &["test"], true, "1", DRAIN);
    std::thread::sleep(Duration::from_millis(250)); // still inside the window
    sb.run_apply("border.sh", &["test"], true, "1", DRAIN);

    sb.wait_until(|| sb.reload_count() == 1, "joined reload", Duration::from_secs(8));
    sb.assert_stable(1, Duration::from_millis(1200));
}

// ── Hyprland down: no reload, exactly like today ─────────────────────────

#[test]
fn sandboxed_hyprland_down_fires_nothing() {
    let sb = Sandbox::build();
    apply_four_fragments(&sb, false, "1", DRAIN);

    std::thread::sleep(Duration::from_millis(2000));
    assert_eq!(sb.reload_count(), 0, "Hyprland down must fire nothing");
    assert!(
        !sb.marker_path().exists(),
        "Hyprland down must not leave a pending marker (same as today's skip)"
    );
}

// ── The disable switch restores today's immediate reload-per-assemble ────

#[test]
fn sandboxed_disable_switch_keeps_immediate_reload() {
    let sb = Sandbox::build();
    apply_four_fragments(&sb, true, "0", DRAIN);

    // Every assemble reloads synchronously: after the four applies the count
    // is already 4 (the disable path never delays, exactly like today).
    sb.wait_until(|| sb.reload_count() == 4, "four immediate reloads", Duration::from_secs(8));
    sb.assert_stable(4, Duration::from_millis(1200));
}

// ── Missing flock fails open: reloads still happen, warningly ────────────

#[test]
fn sandboxed_missing_flock_fails_open() {
    // PATH mirror without flock (precedent: watcher.rs's fail-open test).
    let tmp = tempfile::TempDir::new().unwrap();
    let no_flock = tmp.path().join("no-flock-bin");
    std::fs::create_dir_all(&no_flock).unwrap();
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        if dir.is_empty() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { continue; };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy() == "flock" {
                continue;
            }
            let Ok(target) = std::fs::canonicalize(entry.path()) else { continue; };
            let _ = std::os::unix::fs::symlink(target, no_flock.join(&name));
        }
    }

    let sb = Sandbox::build();
    let pathed = format!("{}:{}", sb.stubs.display(), no_flock.display());
    let warns = apply_four_fragments_with_path(&sb, &pathed);
    assert!(
        warns.iter().any(|s| s.contains("flock") && s.contains("reloading immediately")),
        "a missing flock must warn once and reload immediately"
    );
    sb.wait_until(|| sb.reload_count() >= 1, "at least one reload without flock", Duration::from_secs(8));
    // Fail-open fires per assemble (immediate path) — four scripts, four reloads.
    sb.assert_stable(4, Duration::from_millis(1200));
}

fn apply_four_fragments_with_path(sb: &Sandbox, path: &str) -> Vec<String> {
    use std::process::{Command, Stdio};
    let mut warns = Vec::new();
    for (script, args) in [
        ("apply_animation.sh", vec!["test"]),
        ("border.sh", vec!["test"]),
        ("shader.sh", vec!["test.frag"]),
        ("geometry.sh", vec!["4", "4", "5", "5"]),
    ] {
        let merged = format!("{}:{}", sb.stubs.display(), path);
        let mut child = Command::new("bash")
            .arg(sb.scripts().join(script))
            .args(&args)
            .envs(sb.base_env(true, "1", DRAIN))
            .env("PATH", merged)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(Some(_)) = child.try_wait() {
                break;
            }
            assert!(Instant::now() < deadline, "{script} hung without flock");
            std::thread::sleep(Duration::from_millis(25));
        }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{script} failed: {}", String::from_utf8_lossy(&out.stderr));
        warns.push(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    warns
}