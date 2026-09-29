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
//! is NEVER lost, only coalesced — the worst acceptable failure is that a
//! reload arrives LATER (or, per the documented trade, that a crash AFTER a
//! successful fire costs ONE redundant reload on the next drainer pass),
//! never that it never arrives. The corrected protocol (W5 cross-model
//! findings, see `odd/tasks/coalesce-config-reloads.md`) makes the durable
//! pending evidence outlive the drainer: `reload.requested` is CLAIMED
//! (`mv` → `reload.claimed`, atomic) before the drain window and consumed
//! (`rm`) only AFTER a successful fire. A drainer killed inside the window,
//! killed during a fire, or facing a down Hyprland therefore leaves evidence
//! the next drainer pass fires — an extra reload is safe, a lost one never is.
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

        // Stub bin: the ONLY hyprctl in the loop, timestamped; the pgrep and
        // hyprctl stubs report the live state from HVE_SIM_STATE_FILE when it
        // is present (so tests can flip Hyprland up/down under a live
        // drainer), else from HVE_SIM_HYPRLAND. HVE_SIM_HYPRCTL_DELAY_MS
        // makes the fire (the log line) land immediately and the call itself
        // block — placing the drainer INSIDE the fire with its cleanup
        // pending, for the post-fire-crash test.
        std::fs::write(
            stubs.join("hyprctl"),
            "#!/bin/bash\n\
             up=1\n\
             if [ -n \"${HVE_SIM_STATE_FILE:-}\" ] && [ -f \"$HVE_SIM_STATE_FILE\" ]; then\n\
             \x20 [ \"$(cat \"$HVE_SIM_STATE_FILE\")\" = \"1\" ] || up=0\n\
             else\n\
             \x20 [ \"${HVE_SIM_HYPRLAND:-1}\" = \"1\" ] || up=0\n\
             fi\n\
             if [ \"$up\" = \"1\" ]; then\n\
             \x20 echo \"$(date +%s.%N) hyprctl $*\" >> \"${HVE_SIM_LOG}\"\n\
             fi\n\
             if [ -n \"${HVE_SIM_HYPRCTL_DELAY_MS:-}\" ] && [ \"${HVE_SIM_HYPRCTL_DELAY_MS:-0}\" != \"0\" ]; then\n\
             \x20 sleep \"$(awk -v ms=\"$HVE_SIM_HYPRCTL_DELAY_MS\" 'BEGIN { printf \"%.3f\", ms / 1000 }')\"\n\
             fi\n\
             exit 0\n",
        )
        .unwrap();
        std::fs::write(
            stubs.join("pgrep"),
            "#!/bin/bash\n\
             if [ \"$1\" = \"-x\" ]; then\n\
             \x20 case \"$2\" in\n\
             \x20\x20 Hyprland)\n\
             \x20\x20\x20 if [ -n \"${HVE_SIM_STATE_FILE:-}\" ] && [ -f \"$HVE_SIM_STATE_FILE\" ]; then\n\
             \x20\x20\x20\x20 [ \"$(cat \"$HVE_SIM_STATE_FILE\")\" = \"1\" ] && exit 0 || exit 1\n\
             \x20\x20\x20 fi\n\
             \x20\x20\x20 [ \"${HVE_SIM_HYPRLAND:-1}\" = \"1\" ] && exit 0 || exit 1;;\n\
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
        run_apply_with_env(self, script, args, hyprland_up, coalesce, drain_ms, &[])
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

    /// The claimed pending bucket (`reload.claimed`): the durable memory of
    /// an owed reload after the drainer has atomically moved the marker.
    fn claim_path(&self) -> PathBuf {
        self.cache.join("reload.claimed")
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
        // poll keeps parallel tests from leaking sleeping drainers. A
        // drainer that exited on its own (the W7 idle exit) removes its
        // pidfile via the script's EXIT trap; a SIGKILLed one leaves a STALE
        // pidfile, so identity AND liveness are verified BEFORE signalling —
        // a pid file pointing at a dead (or recycled) pid must never be
        // signalled.
        if let Some(pid) = self.drainer_pid() {
            if is_live_drainer(pid) {
                let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status();
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline
                && std::path::Path::new(&format!("/proc/{pid}")).exists()
            {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

/// True only when `pid` is alive AND is the drainer we expect. `/proc/<pid>`
/// existence alone is not identity: a stale pidfile can name a pid the kernel
/// later recycled, and signalling a stranger is the hazard this guards
/// (cross-model verification finding MINOR, 2026-09-24). The drainer's final
/// process image is always `bash …/reload_coalescer.sh drain`.
fn is_live_drainer(pid: i32) -> bool {
    match std::fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(raw) => {
            let cmdline = String::from_utf8_lossy(&raw).replace('\0', " ");
            cmdline.contains("reload_coalescer.sh") && cmdline.contains("drain")
        }
        Err(_) => false,
    }
}

fn make_exec(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(path).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(path, perm).unwrap();
}

/// `run_apply` plus extra env vars (stub switches like the live-state file or
/// the fire delay), same bounded execution and the same loud-timeout failure.
fn run_apply_with_env(
    sb: &Sandbox,
    script: &str,
    args: &[&str],
    hyprland_up: bool,
    coalesce: &str,
    drain_ms: &str,
    extra: &[(&str, &str)],
) -> Output {
    use std::process::{Command, Stdio};
    let mut envs = sb.base_env(hyprland_up, coalesce, drain_ms);
    for (k, v) in extra {
        envs.push((*k, v.to_string()));
    }
    let mut child = Command::new("bash")
        .arg(sb.scripts().join(script))
        .args(args)
        .envs(envs)
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

/// SIGKILL the live drainer (via its pid file) and wait, bounded, until it is
/// gone. Probe-only: the drainer is never our direct child. Liveness is
/// verified BEFORE signalling, so a stale pid file can never lead to
/// signalling a dead (or recycled) pid.
fn kill_drainer(sb: &Sandbox) {
    let pid = sb.drainer_pid().expect("a drainer pid must exist");
    if is_live_drainer(pid) {
        let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status();
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && std::path::Path::new(&format!("/proc/{pid}")).exists() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "drainer {pid} must die on SIGKILL"
    );
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

// ── W5 MAJOR finding: a drainer killed INSIDE the window, after the ───────
// claim and before the fire, must lose nothing. The old protocol removed the
// marker BEFORE the drain window, so a kill in that ~600 ms gap destroyed the
// only memory of the owed reload. The claim (`reload.claimed`) is the durable
// ticket: it survives the kill and the next drainer pass fires it.

#[test]
fn sandboxed_killed_drainer_inside_window_recovers_reload() {
    let sb = Sandbox::build();
    const LONG_DRAIN: &str = "1500"; // a wide kill window for the probe
    sb.run_apply("apply_animation.sh", &["test"], true, "1", LONG_DRAIN);

    // Wait until the drainer has CLAIMED the marker (mv into reload.claimed),
    // then kill it: we are now inside the drain window, well before the fire
    // (1.5 s away). This is exactly the gap the old protocol lost.
    sb.wait_until(|| sb.claim_path().exists(), "drainer to claim the marker", Duration::from_secs(3));
    kill_drainer(&sb);

    // Dead inside the window -> it never fires: still zero reloads after a
    // full window plus margin.
    std::thread::sleep(Duration::from_millis(1800));
    assert_eq!(sb.reload_count(), 0, "a drainer killed inside the window must not fire");
    assert!(
        sb.claim_path().exists(),
        "the pending claim must survive the killed drainer (it IS the durable memory of the owed reload)"
    );
    assert!(
        !sb.marker_path().exists(),
        "the marker was claimed (mv'd), never deleted"
    );

    // The next request spawns a fresh drainer that folds claim + marker and
    // fires: the reload arrives LATER, never never.
    sb.run_apply("border.sh", &["test"], true, "1", DRAIN);
    sb.wait_until(|| sb.reload_count() == 1, "the recovered reload", Duration::from_secs(8));
    sb.assert_stable(1, Duration::from_millis(1200));
    assert!(!sb.claim_path().exists(), "the claim must be consumed by the fire");
    assert!(!sb.marker_path().exists(), "the marker must be consumed by the fire");
}

// ── W5 finding, trade side: a crash AFTER the fire but BEFORE the ─────────
// cleanup costs AT MOST one redundant reload, never a missing one. The claim
// is removed only after a successful `hyprctl reload`, so a kill inside the
// (stub-slow) fire leaves the claim pending; the next drainer pass folds it
// into its fire — an extra reload is safe, a lost one never is.

#[test]
fn sandboxed_post_fire_crash_costs_at_most_redundant_reload() {
    let sb = Sandbox::build();
    // A slow hyprctl stub: the fire (the log line) lands immediately, then the
    // call blocks — the drainer is INSIDE the fire, with the claim cleanup
    // pending, for the whole stub delay.
    run_apply_with_env(&sb, "apply_animation.sh", &["test"], true, "1", DRAIN, &[
        ("HVE_SIM_HYPRCTL_DELAY_MS", "3000"),
    ]);
    sb.wait_until(|| sb.reload_count() == 1, "the first fire to be logged", Duration::from_secs(8));
    kill_drainer(&sb);

    // The orphaned stub finishes its sleep (the in-flight fire completes); the
    // drainer is dead: cleanup never ran.
    std::thread::sleep(Duration::from_millis(3500));
    assert_eq!(sb.reload_count(), 1, "exactly the in-flight fire");
    assert!(
        sb.claim_path().exists(),
        "cleanup must not have run — the post-fire crash leaves the claim pending"
    );

    // The next request's drainer pass folds the leftover claim into its own
    // fire: ONE more fire (at most one redundant reload), never zero fires,
    // never more than one extra.
    sb.run_apply("border.sh", &["test"], true, "1", DRAIN);
    sb.wait_until(|| sb.reload_count() == 2, "the folded second fire", Duration::from_secs(8));
    sb.assert_stable(2, Duration::from_millis(1200));
    assert!(!sb.claim_path().exists(), "the claim must be consumed by the second fire");
    assert!(!sb.marker_path().exists(), "no marker may remain");
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

// ── Hyprland down at request time: no reload, exactly like today ─────────
// (assemble.sh's request-time pgrep guard swallows the queue before any
// marker is written — the preserved request-time semantics. The drainer-side
// case — up at request, down at fire time — is covered by
// `sandboxed_hyprland_down_keeps_pending_and_fires_when_back`.)

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

// ── W5 sibling finding: up at request, DOWN at fire time — the pending ────
// claim must survive and retry. Under the corrected protocol the drainer
// never consumes evidence without a fire: the reload waits for Hyprland and
// the SAME drainer's next pass fires it once the compositor is back — a
// delayed reload is acceptable, a lost one never is.

#[test]
fn sandboxed_hyprland_down_keeps_pending_and_fires_when_back() {
    let sb = Sandbox::build();
    const LONG_DRAIN: &str = "1500"; // wide window to park the drainer mid-flight
    let state_file = sb.root.join("hyprland.state");
    std::fs::write(&state_file, "1\n").unwrap();

    // Hyprland is UP at request time (the request-time guard passes and the
    // reload is queued)…
    run_apply_with_env(&sb, "apply_animation.sh", &["test"], true, "1", LONG_DRAIN, &[
        ("HVE_SIM_STATE_FILE", state_file.to_str().unwrap()),
    ]);
    sb.wait_until(|| sb.claim_path().exists(), "drainer to claim the marker", Duration::from_secs(3));

    // …then it DIES inside the drain window, before the fire-time guard.
    std::fs::write(&state_file, "0\n").unwrap();
    std::thread::sleep(Duration::from_millis(1800)); // ≥ one full window

    // Down at fire time: no reload, and the claim STAYS pending (consuming
    // it here is exactly the loss the finding flags).
    assert_eq!(sb.reload_count(), 0, "Hyprland down at fire time must fire nothing");
    assert!(
        sb.claim_path().exists(),
        "Hyprland down must keep the pending claim and retry, never consume it"
    );

    // Hyprland comes back: the SAME drainer's next pass fires the pending
    // reload — delayed, never lost.
    std::fs::write(&state_file, "1\n").unwrap();
    sb.wait_until(|| sb.reload_count() == 1, "the pending reload once Hyprland is back", Duration::from_secs(8));
    sb.assert_stable(1, Duration::from_millis(1200));
    assert!(!sb.claim_path().exists(), "the claim must be consumed by the fire");
    assert!(!sb.marker_path().exists(), "no marker may remain");
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

// ── W7: the drainer stops outliving its usefulness ────────────────────────
// (see odd/tasks/reload-drainer-idle-exit.md). The drainer must not run
// forever doing nothing, and it must never buy that with the one thing that
// is forbidden: losing a reload. The idle bound counts ONLY ticks in which
// NEITHER the marker NOR a claim exists (a claim is owed work — a
// Hyprland-down reload must never be abandoned), and the exit races marker
// writes via the `reload.request.lock` handshake.

// ── Idle exit: a drainer that owes nothing exits within the idle bound ───
// and removes its pidfile (the EXIT trap). The bound is a fixed TICK COUNT
// (HVE_RELOAD_IDLE_TICKS, independent of the drain window) so tests can set
// it small.

#[test]
fn sandboxed_idle_drainer_exits_and_removes_pidfile() {
    let sb = Sandbox::build();
    // Small drain window + a 2-tick idle bound: the drainer fires the first
    // request, then has ~2 ticks of quiet before it must exit on its own.
    run_apply_with_env(&sb, "apply_animation.sh", &["test"], true, "1", "100", &[
        ("HVE_RELOAD_IDLE_TICKS", "2"),
    ]);
    sb.wait_until(|| sb.reload_count() == 1, "the first request to fire", Duration::from_secs(8));
    assert!(!sb.claim_path().exists(), "the claim must be consumed by the fire");
    assert!(!sb.marker_path().exists(), "no marker may remain");

    // Capture the pid while the drainer is still alive, then watch it exit
    // and remove its pidfile — the process must not outlive its pidfile.
    let pid = sb.drainer_pid().expect("drainer must be alive after the fire");
    sb.wait_until(
        || std::fs::read_to_string(sb.cache.join("reload.drain.pid")).is_err(),
        "the idle drainer to exit and remove reload.drain.pid",
        Duration::from_secs(8),
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && std::path::Path::new(&format!("/proc/{pid}")).exists() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "the exited drainer's process must be gone, not just its pidfile"
    );

    // Drainer-first interleaving: a request AFTER the exit spawns a FRESH
    // drainer that wins the now-free drain lock and fires — nothing lost.
    sb.run_apply("border.sh", &["test"], true, "1", "100");
    sb.wait_until(|| sb.reload_count() == 2, "a fresh drainer to serve the new request", Duration::from_secs(8));
    sb.assert_stable(2, Duration::from_millis(1200));
    assert!(!sb.marker_path().exists(), "no marker may remain");
}

// ── A pending CLAIM is owed work: it must keep the drainer alive past the ─
// idle bound (counting it as idle would abandon a Hyprland-down reload), and
// only after the claim is fired and consumed does the drainer exit on its
// own idle bound.

#[test]
fn sandboxed_claim_pending_never_exits_and_fires_when_back() {
    let sb = Sandbox::build();
    const WIDE_DRAIN: &str = "1000"; // 1 s per tick — coarse, noise-proof walls
    let state_file = sb.root.join("hyprland.state");
    std::fs::write(&state_file, "1\n").unwrap();

    // Hyprland is UP at request time, so the reload is queued…
    run_apply_with_env(&sb, "apply_animation.sh", &["test"], true, "1", WIDE_DRAIN, &[
        ("HVE_SIM_STATE_FILE", state_file.to_str().unwrap()),
        ("HVE_RELOAD_IDLE_TICKS", "2"),
    ]);
    sb.wait_until(|| sb.claim_path().exists(), "drainer to claim the marker", Duration::from_secs(3));
    let pid = sb.drainer_pid().expect("a live drainer must exist");

    // …then Hyprland DIES inside the drain window: the pending claim is owed
    // work. The drainer must outlive far more than the 2-tick idle bound
    // (2 × 1 s) while it retries — a Hyprland-down reload waits, never lost.
    std::fs::write(&state_file, "0\n").unwrap();
    std::thread::sleep(Duration::from_millis(3200)); // > the whole idle bound
    assert!(
        std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "a pending claim (Hyprland down) must keep the drainer alive past the idle bound"
    );
    assert_eq!(sb.reload_count(), 0, "Hyprland down must fire nothing");

    // Hyprland returns: the SAME drainer fires exactly once and consumes the
    // claim, then exits only after a fresh idle bound.
    std::fs::write(&state_file, "1\n").unwrap();
    sb.wait_until(|| sb.reload_count() == 1, "the pending reload once Hyprland is back", Duration::from_secs(10));
    // The claim is consumed only AFTER a successful fire, so its absence is
    // POLLED, never asserted immediately: the stub's `rm` follows its
    // `hyprctl reload` return by a window a fast poller can straddle — an
    // immediate assert flaked once in three full-suite runs (cross-model
    // verification finding MINOR, 2026-09-24).
    sb.wait_until(
        || !sb.claim_path().exists(),
        "the claim to be consumed by the fire",
        Duration::from_secs(3),
    );
    sb.wait_until(
        || !sb.marker_path().exists(),
        "no marker left behind",
        Duration::from_secs(3),
    );
    sb.wait_until(
        || std::fs::read_to_string(sb.cache.join("reload.drain.pid")).is_err(),
        "the drainer to exit once idle after the fire",
        Duration::from_secs(10),
    );
}

// ── A pending REQUEST is owed work too: a request landing while the ───────
// drainer is idle must reset the idle budget, keep the drainer alive, and be
// fired by THE SAME drainer (its own spawn lost the drain lock). Only after
// the fresh budget does the drainer exit.

#[test]
fn sandboxed_request_pending_keeps_drainer_alive_and_fires() {
    let sb = Sandbox::build();
    const WIDE_DRAIN: &str = "1500"; // 1.5 s per tick — wide, noise-proof walls

    // Burst 1 fires; the drainer then goes idle with a tiny 2-tick bound.
    run_apply_with_env(&sb, "apply_animation.sh", &["test"], true, "1", WIDE_DRAIN, &[
        ("HVE_RELOAD_IDLE_TICKS", "2"),
    ]);
    sb.wait_until(|| sb.reload_count() == 1, "first burst reload", Duration::from_secs(10));

    // Burst 2 lands while the drainer is idle-but-alive (300 ms in, ~1.2 s
    // of bound left). Its spawn loses the drain lock, so the SAME drainer
    // must stay alive and fire it — exiting now would strand the request.
    std::thread::sleep(Duration::from_millis(300));
    let pid = sb.drainer_pid().expect("drainer must still be alive mid-idle");
    sb.run_apply("border.sh", &["test"], true, "1", WIDE_DRAIN);
    sb.wait_until(|| sb.reload_count() == 2, "the pending request to fire", Duration::from_secs(10));
    // The claim and marker are removed by the drainer AFTER the fire returns,
    // so their absence must be POLLED, never asserted immediately: the stub's
    // `rm` follows its `hyprctl reload` return by a window a fast poller can
    // straddle (the same discipline the sibling test above documents). An
    // immediate assert here failed at random under a loaded parallel suite.
    sb.wait_until(
        || !sb.marker_path().exists(),
        "no marker may remain",
        Duration::from_secs(3),
    );
    sb.wait_until(
        || !sb.claim_path().exists(),
        "no claim may remain",
        Duration::from_secs(3),
    );
    assert!(
        std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "the drainer that served burst 1 must be the one serving burst 2 (it may not exit while a request is owed)"
    );

    // …and only after a FRESH idle budget (the reset) does it exit: leftover
    // pre-fire ticks must not shorten the post-fire bound. Without the reset
    // the drainer exits the tick after the fire.
    std::thread::sleep(Duration::from_millis(300)); // well inside the fresh bound
    assert!(
        std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "after owed work the drainer must live out a full idle bound before exiting"
    );
    sb.wait_until(
        || std::fs::read_to_string(sb.cache.join("reload.drain.pid")).is_err(),
        "the drainer to exit once idle again",
        Duration::from_secs(10),
    );
}

// ── No loss under stress: with a one-or-two-tick idle bound the drainer is ─
// always close to exiting, so requests race exit decisions. Bursts are
// INDEPENDENT (spacing ≥ fraction of the drain window, so coalescing never
// legitimately folds two bursts into one fire) and the absolute invariant
// holds: every burst fires (reload_count == bursts), no marker or claim is
// left behind. The failure mode this test hunts is a LOST reload, never a
// folded one.

#[test]
fn sandboxed_no_loss_under_idle_exit_stress() {
    let sb = Sandbox::build();
    const BURSTS: usize = 12;
    for i in 0..BURSTS {
        // Alternate one/two-tick idle bounds so drainer lifecycles vary
        // while the queue keeps racing their exit decisions.
        let idle_ticks = if i % 2 == 0 { "1" } else { "2" };
        run_apply_with_env(&sb, "apply_animation.sh", &["test"], true, "1", "100", &[
            ("HVE_RELOAD_IDLE_TICKS", idle_ticks),
        ]);
        // ≥ 2 drain windows between bursts: independent bursts (each must
        // fire its own reload), with jitter so requests sweep across the
        // drainers' phases.
        std::thread::sleep(Duration::from_millis((220 + (i % 3) * 30) as u64));
    }
    sb.wait_until(|| sb.reload_count() == BURSTS, "every burst to fire", Duration::from_secs(25));
    sb.assert_stable(BURSTS, Duration::from_millis(1500));
    assert!(!sb.marker_path().exists(), "no marker may be left behind");
    assert!(!sb.claim_path().exists(), "no claim may be left behind");
}

// ── An apply script NEVER reloads the compositor beside the coalescer ───
// With `assemble.sh` missing, `shader.sh` used to fall back to a bare
// `hyprctl reload`: an uncoupled, uncoalesced fire that could not even apply
// what it had just written (the fragment only reaches the compositor through
// the overlay assemble.sh builds). The fallback is gone; the other three
// apply scripts already do nothing without the assembler.

#[test]
fn sandboxed_shader_without_assembler_fires_no_direct_reload() {
    let sb = Sandbox::build();
    std::fs::remove_file(sb.scripts().join("assemble.sh")).unwrap();

    sb.run_apply("shader.sh", &["test.frag"], true, "1", DRAIN);

    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(
        sb.reload_count(),
        0,
        "without assemble.sh no reload may fire: the coalescer is the only fire path"
    );
    assert!(!sb.marker_path().exists(), "no marker may be queued");
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