use crate::config::Config;
use crate::engine::Engine;
use crate::theme;
use slint::ComponentHandle;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

pub struct HyprIpc {
    socket_path: PathBuf,
}

/// Opaque handle to a background listener thread.
/// Dropping the handle sets an atomic shutdown flag so the loop exits cleanly
/// on its next reconnect iteration.
pub struct ListenerHandle {
    shutdown: std::sync::Arc<AtomicBool>,
    _thread: JoinHandle<()>,
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl HyprIpc {
    pub fn new() -> Option<Self> {
        let instance = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
        // Prefer the real XDG runtime dir (honors XDG_RUNTIME_DIR and the
        // per-UID /run/user/<uid> fallback) instead of a hardcoded /run/user/1000.
        let runtime_dir = dirs::runtime_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        let socket = runtime_dir
            .join("hypr")
            .join(&instance)
            .join(".socket2.sock");
        if socket.exists() {
            Some(Self { socket_path: socket })
        } else {
            None
        }
    }

    /// Spawn a thread that listens for Hyprland IPC events.
    /// Returns a `ListenerHandle` that, when dropped, signals the thread to
    /// stop on its next reconnect attempt.
    ///
    /// Calls `on_config_reload` when `configreloaded` event fires.
    pub fn spawn_listener<F>(self, on_config_reload: F) -> ListenerHandle
    where
        F: Fn() + Send + Sync + 'static,
    {
        let callback = std::sync::Arc::new(on_config_reload);
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));

        let shutdown_clone = shutdown.clone();
        let handle = thread::spawn(move || {
            loop {
                // Check shutdown flag before every reconnect attempt
                if shutdown_clone.load(Ordering::Relaxed) {
                    break;
                }
                match self.connect_and_listen(&callback) {
                    Ok(_) => break, // Clean exit from Hyprland
                    Err(e) => {
                        tracing::warn!("[HVE] IPC listener error: {}, reconnecting in 2s...", e);
                        // Respect shutdown during the sleep too
                        for _ in 0..20 {
                            if shutdown_clone.load(Ordering::Relaxed) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            }
        });

        ListenerHandle {
            shutdown,
            _thread: handle,
        }
    }

    fn connect_and_listen<F>(&self, on_config_reload: &std::sync::Arc<F>) -> Result<(), String>
    where
        F: Fn() + Send + Sync,
    {
        let stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| format!("Cannot connect to Hyprland IPC: {}", e))?;

        // Set read timeout to detect disconnects
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .map_err(|e| format!("Cannot set timeout: {}", e))?;

        // socket2.sock auto-subscribes — no handshake needed

        // Resume-detection baseline (see the resume-permission block below).
        // The listener is the process's long-lived thread, so a sample here
        // means a suspend that happens before the next palette change is
        // still measurable. A sample never grants on its own: only the
        // on-demand decision in `consume_reassert_permit` does.
        note_clock_sample();

        // Throttle config reload — assemble.sh triggers another reload event
        let last_reload: Mutex<Option<Instant>> = Mutex::new(None);

        let reader = BufReader::new(stream);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    // Resume-detection baseline: one sample per event line.
                    // Together with the timeout sample below this makes the
                    // baseline at most 30 s of RUNNING time stale, always:
                    // either a line arrived, or the read timed out. That
                    // bound is what keeps an edit made long after a resume
                    // from looking like a hijack that followed one.
                    note_clock_sample();
                    // Events come as: "eventname>>data"
                    if line.starts_with("configreloaded") {
                        // Line-level handling, BEFORE the throttle: masks +
                        // fullscreen restore always run; the heavy reload at
                        // most once per 3s. See on_configreloaded_line.
                        handle_configreloaded_line(
                            &last_reload,
                            Instant::now(),
                            on_configreloaded_line,
                            || {
                                tracing::info!("[HVE] Config reloaded, regenerating overlay...");
                                on_config_reload();
                            },
                        );
                    } else if let Some(payload) = line.strip_prefix("fullscreen") {
                        // W5: the compositor telling us HVE became immersive is
                        // the signal the transition waits for, instead of
                        // guessing with a timer. Listening is free: it takes no
                        // focus away (see the sentinel's root-cause note).
                        //
                        // Polarity matters: the event is `fullscreen>>1` on
                        // ENTER and `>>0` on LEAVE, and it is broadcast for ANY
                        // window. Only `>>1` means "HVE is immersive now"; a
                        // `>>0` (e.g. the un-float a reload triggers) would
                        // advance the finale mid-reload. The payload is
                        // `>>1`, so the first char after the name is `>`
                        // followed by `1`.
                        if payload.trim_start_matches('>').starts_with('1') {
                            crate::theme_fullscreen_signal();
                        }
                    } else if monitor_event_arms(&line) {
                        // A monitor coming or going makes the wallpaper engine
                        // re-apply the same wallpaper and recompute the theme a
                        // moment later; that is the permission for the palette
                        // defence. Arming is line-local and cheap.
                        let mut slot =
                            MONITOR_PERMIT.lock().unwrap_or_else(|e| e.into_inner());
                        arm_monitor_permit(&mut slot, Instant::now());
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // Timeout is normal — just continue listening. Re-take the
                    // resume baseline here (free: the read already woke us, no
                    // timer of our own) so the pre-suspend sample is never
                    // stale by more than 30 s of RUNNING time. That bound is
                    // what keeps a legitimate edit made long after a resume
                    // from being mistaken for a hijack that followed one.
                    note_clock_sample();
                    continue;
                }
                Err(e) => {
                    return Err(format!("Read error: {}", e));
                }
            }
        }

        Ok(())
    }
}

/// One `configreloaded` line, headless-testable: the line-level actions
/// (masks + fullscreen restore) ALWAYS run; the heavy regenerating reload
/// runs at most once per 3s window. Returns whether the throttled callback
/// ran.
fn handle_configreloaded_line<F, G>(
    last_reload: &Mutex<Option<Instant>>,
    now: Instant,
    mut on_line: F,
    mut throttled: G,
) -> bool
where
    F: FnMut(),
    G: FnMut(),
{
    // Masks + fullscreen restore run BEFORE the throttle: every reload
    // (assemble, noctalia, watcher) wipes the runtime animations/blur
    // overrides mid-fade and re-floats HVE, dropping its immersive
    // fullscreen. Both are cheap and idempotent, and a reload that never
    // reaches the throttled callback (assemble re-trigger) still needs its
    // masks back and its fullscreen back.
    on_line();
    let mut last = last_reload.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_none_or(|t| now.duration_since(t) > Duration::from_secs(3)) {
        *last = Some(now);
        throttled();
        true
    } else {
        false
    }
}

/// Line-level `configreloaded` actions: re-apply the transition masks and
/// consult the fullscreen-restore decision. Runs on the IPC listener
/// thread, so the restore hops to the UI thread — its step-2 timer uses
/// `slint::Timer`, and timers are thread-local (a timer created on the
/// listener thread never fires; verified in i-slint-core 1.17.0
/// `timers.rs::single_shot`). Best-effort and bounded: before the event
/// loop runs the hop errors and is ignored — the masks still apply
/// directly and the restore waits for the next reload line. The failure is
/// logged honestly (never silently dropped; nothing is aborted).
fn on_configreloaded_line() {
    crate::reapply_theme_masks();
    // W5b: a reload landed while the finale waits -> restart the settle window.
    // The burst is still going; running the finale now would land mid-reload.
    crate::theme_reload_signal();
    if let Err(e) = slint::invoke_from_event_loop(crate::reassert_fullscreen_after_reload) {
        tracing::debug!(
            "[reassert] fullscreen restore UI-thread hop failed (pre-loop): {}",
            e
        );
    }
}

// ─── Monitor-event colour-defence window ───────────────────────────
//
// odd/tasks/palette-defence-on-monitor-events.md: the palette FILE is the
// detector, the MONITOR event is the permission. A monitor hotplug makes the
// wallpaper engine re-apply the same wallpaper and recompute the theme a
// moment later, so HVE arms a short one-shot window; a palette change inside
// it may be corrected back. A change with no armed window is presumed
// legitimate (the keeper's own edit) and is never touched.

/// How long a monitor hotplug keeps the colour-defence window open.
///
/// Evidence, measured on this machine (`~/.cache/skwd-wall-v2/skwd-walld.log`,
/// 2026-09-30/10-01): every noctalia hotplug logged `theme apply` and its
/// palette write in the SAME second, and the slowest hotplug->theme chain
/// measured was 1198 ms (the render handoff). HVE's watcher then detects the
/// write sub-second (`~/.cache/hve/color_watcher.log`). The whole chain is
/// ~2 s end to end, so 5 s gives ~2.5x margin and matches the watcher's own
/// `HVE_THEME_ASSERT_COOLDOWN` (5 s): one burst, one window.
const MONITOR_REASSERT_WINDOW: Duration = Duration::from_secs(5);

/// One-shot permission to re-assert, armed by a monitor hotplug.
#[derive(Clone, Copy, Debug)]
struct MonitorPermit {
    until: Instant,
    consumed: bool,
}

/// The single armed permit, process-wide: the compositor listener thread
/// arms it, the IPC thread consumes it. `None` means no monitor event is
/// pending, so a palette change is presumed legitimate.
static MONITOR_PERMIT: Mutex<Option<MonitorPermit>> = Mutex::new(None);

/// Whether a socket2 line is a monitor hotplug. Only these arm the permit.
/// The prefix match also catches the `v2` spellings of both events.
fn monitor_event_arms(line: &str) -> bool {
    line.starts_with("monitoradded") || line.starts_with("monitorremoved")
}

/// Arm the one-shot permit at `now`, replacing any previous one.
fn arm_monitor_permit(slot: &mut Option<MonitorPermit>, now: Instant) {
    *slot = Some(MonitorPermit {
        until: now + MONITOR_REASSERT_WINDOW,
        consumed: false,
    });
}

/// Whether this palette change may re-assert. One-shot: the first permitted
/// change consumes the window, so a second change inside the same window
/// (the echo of HVE's own write) can never stack a second re-assert.
fn defence_permits_reassert(slot: &mut Option<MonitorPermit>, now: Instant) -> bool {
    match slot {
        Some(permit) if !permit.consumed && now < permit.until => {
            permit.consumed = true;
            true
        }
        _ => false,
    }
}

// ─── Resume-from-suspend colour-defence permission ─────────────────
//
// odd/tasks/palette-defence-covers-suspend-resume.md, decisions D1/D2:
// resume from suspend is a SECOND permission source, beside the monitor
// event. A suspend shows up as the wall clock advancing far more than
// monotonic time between two samples, because `SystemTime` keeps running
// while the machine sleeps and `Instant` does not.
//
// No D-Bus client and no timer. Samples are taken
//   - once when the listener connects, as the baseline;
//   - on every socket2 event line, and on the listener's OWN 30 s read
//     timeout. Either a line arrived or the read timed out, so the baseline
//     is never more than 30 s of RUNNING time stale — and that bound is what
//     keeps an edit made long after a resume from looking like one;
//   - on demand, when a re-assert is requested.
// Only the DECISION is on demand; nothing polls for a suspend.

/// How far the wall clock must outrun the monotonic clock, between two
/// samples, before the step counts as a suspend. Ordinary clock drift is
/// milliseconds and NTP slews instead of stepping, so 2 s leaves a wide
/// margin while still catching every real sleep. An NTP STEP larger than
/// this can open one false grace window; accepted, bounded and documented
/// in the task file (D1).
const SUSPEND_GAP_TOLERANCE: Duration = Duration::from_secs(2);

/// How long a detected resume keeps the colour defence open.
///
/// A GRACE WINDOW, not a one-shot (D2), because the measured chain needs
/// more than one pass: suspend ended 18:28:55 on 2026-10-01, the palette
/// hijack landed at 18:29:13 (18 s later) and a second burst pass at
/// 18:29:24 (11 s after the first) — both were refused under the one-shot
/// monitor rule, at 18:29:13 and 18:29:24 (`~/.cache/hve/color_watcher.log`).
/// 60 s covers both with margin. It cannot loop: the provider only writes
/// when the live palette differs from the snapshot
/// (`palette_needs_reassert`) and the watcher throttles its asks to one per
/// 5 s.
const RESUME_GRACE: Duration = Duration::from_secs(60);

/// Both clocks read at one instant. Plain values, so every decision below
/// is testable without touching a real clock.
#[derive(Clone, Copy, Debug)]
struct ClockSample {
    mono: Instant,
    wall: SystemTime,
}

/// Whether the step between two samples is a suspend: the wall clock
/// advanced `SUSPEND_GAP_TOLERANCE` more than the monotonic clock. A wall
/// clock that moved backwards (an NTP step) is not a suspend.
fn suspend_gap(wall_advanced: Duration, mono_advanced: Duration) -> bool {
    wall_advanced.saturating_sub(mono_advanced) > SUSPEND_GAP_TOLERANCE
}

/// Whether a resume grace ending at `until` is still open at `now`. The
/// boundary is exclusive: at `until` the grace is over.
fn resume_grace_open(until: Option<Instant>, now: Instant) -> bool {
    until.is_some_and(|end| now < end)
}

/// The resume permission's state: the previous sample (the baseline) and the
/// grace window a detected suspend opened.
#[derive(Debug, Default)]
struct ResumeState {
    last: Option<ClockSample>,
    until: Option<Instant>,
}

impl ResumeState {
    /// Fold one sample in and report whether the step was a suspend.
    ///
    /// The FIRST sample only establishes the baseline: one sample cannot
    /// show a gap, so it never detects and never grants. A sample that is NOT
    /// a suspend never closes an open grace window — the grace runs from the
    /// resume, not from the last sample.
    fn observe(&mut self, sample: ClockSample) -> bool {
        let suspended = self.last.is_some_and(|before| {
            suspend_gap(
                sample
                    .wall
                    .duration_since(before.wall)
                    .unwrap_or(Duration::ZERO),
                sample.mono.saturating_duration_since(before.mono),
            )
        });
        self.last = Some(sample);
        if suspended {
            self.until = Some(sample.mono + RESUME_GRACE);
        }
        suspended
    }
}

/// Which source permitted a re-assert, so the caller can log it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReassertPermit {
    Monitor,
    Resume,
    None,
}

impl ReassertPermit {
    /// The source's name as it appears in the log.
    pub fn as_str(self) -> &'static str {
        match self {
            ReassertPermit::Monitor => "monitor",
            ReassertPermit::Resume => "resume",
            ReassertPermit::None => "none",
        }
    }
}

/// One decision, with the detection kept so it can be logged once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReassertDecision {
    permit: ReassertPermit,
    suspend_detected: bool,
}

/// The single decision, over injected clock values.
///
/// Order matters for reporting only: an armed monitor window is the more
/// specific cause, so it is checked first and still consumes its one-shot
/// exactly as before; the resume grace is checked second and is never
/// consumed by asking.
fn decide_reassert(
    monitor: &mut Option<MonitorPermit>,
    resume: &mut ResumeState,
    sample: ClockSample,
) -> ReassertDecision {
    let suspend_detected = resume.observe(sample);
    let permit = if defence_permits_reassert(monitor, sample.mono) {
        ReassertPermit::Monitor
    } else if resume_grace_open(resume.until, sample.mono) {
        ReassertPermit::Resume
    } else {
        ReassertPermit::None
    };
    ReassertDecision {
        permit,
        suspend_detected,
    }
}

/// The single armed resume state, process-wide, beside `MONITOR_PERMIT`.
static RESUME_STATE: Mutex<ResumeState> = Mutex::new(ResumeState {
    last: None,
    until: None,
});

/// Read both clocks now.
fn clock_sample() -> ClockSample {
    ClockSample {
        mono: Instant::now(),
        wall: SystemTime::now(),
    }
}

/// One cheap line per detected resume (D4). This is the event whose absence
/// made the 18:29 refusal invisible; it is logged once per detection.
fn log_resume_detected() {
    tracing::info!(
        "[colour-defence] resume from suspend detected: re-asserts permitted for {}s",
        RESUME_GRACE.as_secs()
    );
}

/// Fold a sample in off the request path (the listener's baseline, its event
/// lines and its 30 s read timeout). It never grants by itself, except by
/// opening the grace a real resume deserves: the GRANT is taken on demand in
/// `consume_reassert_permit`.
fn note_clock_sample() {
    let mut resume = RESUME_STATE.lock().unwrap_or_else(|e| e.into_inner());
    if resume.observe(clock_sample()) {
        log_resume_detected();
    }
}

/// The one entry point the assert path uses: take an on-demand sample and
/// decide whether this palette change may re-assert, reporting which source
/// granted it.
///
/// Cheap and non-blocking on the change burst: two clock reads and two
/// mutexes, no I/O, no IPC, no sleep. A refusal is logged by the caller,
/// with its reason.
pub fn consume_reassert_permit() -> ReassertPermit {
    let sample = clock_sample();
    let decision = {
        let mut monitor = MONITOR_PERMIT.lock().unwrap_or_else(|e| e.into_inner());
        let mut resume = RESUME_STATE.lock().unwrap_or_else(|e| e.into_inner());
        decide_reassert(&mut monitor, &mut resume, sample)
    };
    if decision.suspend_detected {
        log_resume_detected();
    }
    decision.permit
}

/// Start the IPC listener that auto-refreshes colors on config reload.
/// Returns a handle so the caller can keep it alive for the app lifetime.
pub fn start_listener(window: &crate::MainWindow, proj: PathBuf) -> ListenerHandle {
    let weak = window.as_weak();
    let proj_for_ipc = proj;
    if let Some(ipc) = HyprIpc::new() {
        let handle = ipc.spawn_listener(move || {
            // 1. Re-run assemble.sh when config reloads
            let script = proj_for_ipc
                .join("assets")
                .join("scripts")
                .join("assemble.sh");
            if script.exists() {
                let _ = std::process::Command::new("bash")
                    .arg(script)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }

            // 2. Refresh UI colors from the current color source
            let eng = Engine::new(&proj_for_ipc);
            if let Ok(colors) = eng.get_colors() {
                let w = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(window) = w.upgrade() {
                        let cfg = Config::load();
                        let resolved = theme::resolve_scheme(&colors, &cfg.theme);
                        theme::apply_theme(&window, &resolved);
                    }
                });
            }

            // 3. Line-level masks + fullscreen restore only: the restore is
            //    consultation-gated and debounced, so it cannot re-arm the
            //    4-actor race the UNCONDITIONAL event-path cycle caused
            //    (event return vs step1/step2 timers, incl. an animated move
            //    AFTER fade-in). The single-owner finale still owns every
            //    cycle inside a theme transition; this path only re-applies
            //    the masks each reload wipes and restores the immersive
            //    fullscreen a reload dropped (visible + not fullscreen +
            //    outside a transition + outside the 1.5s debounce window).
            //    Line-level handling happens in connect_and_listen
            //    (bypasses the 3s throttle) via on_configreloaded_line.
            let _ = ();
        });
        tracing::info!("[HVE] Hyprland IPC listener started");
        handle
    } else {
        tracing::warn!("[HVE] Could not find Hyprland IPC socket");
        // Return a dummy handle that does nothing on drop (no listener to stop)
        let shutdown = std::sync::Arc::new(AtomicBool::new(true));
        ListenerHandle {
            shutdown,
            _thread: thread::spawn(move || {}),
        }
    }
}

// ─── Focus listener ────────────────────────────────────────────────
// Listens to Hyprland's `activewindow` event. Calls on_focus_lost
// when the active window stops being "Hyprland Visual Editor".
const HVE_WINDOW_TITLE: &str = "Hyprland Visual Editor";

/// Spawn a thread that listens for Hyprland `activewindow` events.
/// Returns a `ListenerHandle` that, when dropped, signals the thread to
/// stop on its next reconnect attempt.
///
/// Calls `on_focus_lost` when the active window is NOT HVE.
/// Calls `on_focus_gained` when the active window IS HVE.
pub fn spawn_focus_listener<F, G>(on_focus_lost: F, on_focus_gained: G) -> ListenerHandle
where
    F: Fn() + Send + Sync + 'static,
    G: Fn() + Send + Sync + 'static,
{
    let ipc = match HyprIpc::new() {
        Some(i) => i,
        None => {
            tracing::warn!("[HVE] Could not find Hyprland IPC socket (focus listener skipped)");
            // Return a dummy handle
            let shutdown = std::sync::Arc::new(AtomicBool::new(true));
            return ListenerHandle {
                shutdown,
                _thread: thread::spawn(move || {}),
            };
        }
    };

    tracing::info!("[focus] Focus listener connecting to {:?}", ipc.socket_path);

    let lost = std::sync::Arc::new(on_focus_lost);
    let gained = std::sync::Arc::new(on_focus_gained);
    let shutdown = std::sync::Arc::new(AtomicBool::new(false));

    let shutdown_clone = shutdown.clone();
    let handle = thread::spawn(move || {
        loop {
            // Check shutdown flag before every reconnect attempt
            if shutdown_clone.load(Ordering::Relaxed) {
                break;
            }
            match connect_and_listen_focus(&ipc, &lost, &gained) {
                Ok(_) => break,
                Err(e) => {
                    tracing::warn!("[HVE] Focus listener error: {}, reconnecting in 2s...", e);
                    // Respect shutdown during the sleep too
                    for _ in 0..20 {
                        if shutdown_clone.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        }
    });

    tracing::info!("[HVE] Hyprland focus listener started");
    ListenerHandle {
        shutdown,
        _thread: handle,
    }
}

fn connect_and_listen_focus<F, G>(
    ipc: &HyprIpc,
    on_focus_lost: &std::sync::Arc<F>,
    on_focus_gained: &std::sync::Arc<G>,
) -> Result<(), String>
where
    F: Fn() + Send + Sync,
    G: Fn() + Send + Sync,
{
    let stream = UnixStream::connect(&ipc.socket_path)
        .map_err(|e| format!("Cannot connect to Hyprland IPC: {}", e))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|e| format!("Cannot set timeout: {}", e))?;

    // socket2.sock auto-subscribes — no handshake needed

    tracing::info!("[focus] Connected and subscribed to Hyprland events");

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        match line {
            Ok(line) => {
                // `activewindow` event → "activewindow>>CLASS,TITLE"
                if line.starts_with("activewindow>>") {
                    tracing::debug!("[focus] activewindow event: {}", line);
                    let title = line.trim_start_matches("activewindow>>");
                    if title.contains(HVE_WINDOW_TITLE) {
                        tracing::debug!("[focus] Foco GANADO (HVE)");
                        on_focus_gained();
                    } else if !title.is_empty() {
                        tracing::debug!("[focus] Foco PERDIDO -> '{}'", title);
                        on_focus_lost();
                    }
                    // If title is empty, do nothing (transition between workspaces)
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => {
                return Err(format!("Read error: {}", e));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── configreloaded line handling (fullscreen survives a reload) ────

    /// The line-level handler must run its actions (masks + fullscreen
    /// restore) on EVERY `configreloaded` line, while the heavy throttled
    /// reload runs at most once per 3s window.
    #[test]
    fn configreloaded_line_always_runs_restore_before_the_throttle() {
        let last_reload: Mutex<Option<Instant>> = Mutex::new(None);
        let mut on_line = 0;
        let mut heavy = 0;

        let ran = handle_configreloaded_line(
            &last_reload,
            Instant::now(),
            || on_line += 1,
            || heavy += 1,
        );
        assert!(ran, "the first line inside a 3s window runs the throttled reload");
        assert_eq!(on_line, 1, "line actions (masks + restore) run on EVERY line");
        assert_eq!(heavy, 1);

        let ran2 = handle_configreloaded_line(
            &last_reload,
            Instant::now(),
            || on_line += 1,
            || heavy += 1,
        );
        assert!(!ran2, "a second line inside the 3s window is throttled");
        assert_eq!(on_line, 2, "line actions still run on the throttled line");
        assert_eq!(heavy, 1, "the heavy reload does not run again inside the window");

        let ran3 = handle_configreloaded_line(
            &last_reload,
            Instant::now() + Duration::from_secs(4),
            || on_line += 1,
            || heavy += 1,
        );
        assert!(ran3, "a line after the 3s window reloads again");
        assert_eq!(on_line, 3);
        assert_eq!(heavy, 2);
    }

    /// The line-level `configreloaded` handler must log a failed UI-thread
    /// hop honestly (bounded and best-effort) instead of silently
    /// discarding the error — the unit's logging contract. The check is
    /// scoped to the `on_configreloaded_line` body (comment-stripped), so
    /// neither the test's own assertion text nor a commented-out call can
    /// satisfy it (house precedent).
    #[test]
    fn configreloaded_hop_failure_is_logged_not_silently_dropped() {
        let src = std::fs::read_to_string("src/hypr_ipc.rs").expect("src/hypr_ipc.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // Scope to the on_configreloaded_line body (up to the next fn).
        let start = code
            .find("fn on_configreloaded_line")
            .expect("on_configreloaded_line must exist");
        let body = &code[start..];
        let end = body
            .find("\npub fn ")
            .or_else(|| body.find("\nfn "))
            .unwrap_or(body.len());
        let body = &body[..end];
        // The restore must still hop to the UI thread.
        assert!(
            body.contains("invoke_from_event_loop(crate::reassert_fullscreen_after_reload)"),
            "the fullscreen restore must hop to the UI thread"
        );
        // The hop result must be inspected and logged — never `let _ =`.
        assert!(
            !body.contains("let _ = slint::invoke_from_event_loop"),
            "a silent hop failure contradicts the unit's logging contract"
        );
        assert!(
            body.contains("tracing::debug!") || body.contains("tracing::warn!"),
            "the hop failure must be logged at debug or warn"
        );
    }

    // ── Monitor-event colour-defence window ─────────────────────────
    //
    // odd/tasks/palette-defence-on-monitor-events.md: the palette file stays
    // the detector, the monitor event is the permission. These pin the line
    // classifier and the one-shot window so a change with no window is a
    // no-op, a change inside the window re-asserts exactly once, and a second
    // change in the same window never stacks a second re-assert.

    #[test]
    fn monitor_events_arm_the_window_and_other_events_do_not() {
        for line in [
            "monitoradded>>DP-1",
            "monitoraddedv2>>1,DP-1,Some Monitor",
            "monitorremoved>>DP-1",
            "monitorremovedv2>>1,DP-1",
        ] {
            assert!(monitor_event_arms(line), "{line:?} must arm the window");
        }
        for line in [
            "activewindow>>kitty,term",
            "configreloaded>>",
            "fullscreen>>1",
            "workspace>>2",
            "openwindow>>123,1,kitty,term",
            "monitor",
            "",
            "garbage line",
        ] {
            assert!(!monitor_event_arms(line), "{line:?} must NOT arm the window");
        }
    }

    #[test]
    fn a_palette_change_with_no_armed_window_does_not_reassert() {
        let mut slot: Option<MonitorPermit> = None;
        assert!(
            !defence_permits_reassert(&mut slot, Instant::now()),
            "with no monitor event the palette change is presumed legitimate"
        );
    }

    #[test]
    fn a_palette_change_inside_the_window_reasserts_exactly_once() {
        let t0 = Instant::now();
        let mut slot = None;
        arm_monitor_permit(&mut slot, t0);
        let mut reasserts = 0;
        for step in 1..=3u64 {
            if defence_permits_reassert(&mut slot, t0 + Duration::from_millis(step)) {
                reasserts += 1;
            }
        }
        assert_eq!(
            reasserts, 1,
            "the window must permit exactly one re-assert (no ping-pong)"
        );
    }

    #[test]
    fn a_palette_change_after_the_window_expires_does_not_reassert() {
        let t0 = Instant::now();
        let mut slot = None;
        arm_monitor_permit(&mut slot, t0);
        assert!(
            !defence_permits_reassert(
                &mut slot,
                t0 + MONITOR_REASSERT_WINDOW + Duration::from_millis(1)
            ),
            "a change past the window must not re-assert"
        );
    }

    #[test]
    fn each_monitor_event_arms_a_fresh_one_shot_window() {
        let t0 = Instant::now();
        let mut slot = None;
        arm_monitor_permit(&mut slot, t0);
        assert!(defence_permits_reassert(&mut slot, t0 + Duration::from_millis(1)));
        assert!(
            !defence_permits_reassert(&mut slot, t0 + Duration::from_millis(2)),
            "the first window is spent"
        );
        arm_monitor_permit(&mut slot, t0 + Duration::from_millis(3));
        assert!(
            defence_permits_reassert(&mut slot, t0 + Duration::from_millis(4)),
            "a second monitor event permits one more re-assert"
        );
    }

    // ── Resume-from-suspend colour-defence permission ───────────────
    //
    // odd/tasks/palette-defence-covers-suspend-resume.md: resume is a SECOND
    // permission source. A suspend shows up as the wall clock advancing far
    // more than monotonic time between two samples, because `SystemTime`
    // keeps running while the machine sleeps and `Instant` does not.
    // Detecting one opens a GRACE window, not a one-shot: the measured burst
    // needs two passes. Every sample below is synthesized, so no test sleeps.

    /// A long sleep for the synthesized samples: three hours. The measured
    /// chain's suspend was shorter, but the detector only sees the gap.
    const SUSPENDED_MS: u64 = 3 * 3600 * 1000;

    /// A synthesized sample: absolute monotonic/wall offsets from the test's
    /// own base. Never reads a real clock.
    fn sample_at(base: Instant, mono_ms: u64, wall_ms: u64) -> ClockSample {
        ClockSample {
            mono: base + Duration::from_millis(mono_ms),
            wall: SystemTime::UNIX_EPOCH + Duration::from_millis(wall_ms),
        }
    }

    /// (a) The gap itself: wall ≫ monotonic is a suspend, ordinary drift and a
    /// small skew are not.
    #[test]
    fn a_suspend_is_detected_when_wall_advances_far_more_than_monotonic() {
        assert!(
            suspend_gap(
                Duration::from_millis(SUSPENDED_MS + 18_000),
                Duration::from_millis(18_000)
            ),
            "the wall clock carrying the whole sleep, monotonic only the awake part, is a resume"
        );
    }

    #[test]
    fn normal_drift_and_a_small_skew_are_not_a_suspend() {
        for (wall_ms, mono_ms) in [
            (30_000u64, 30_000u64), // both clocks advance together
            (30_000, 28_000),       // 2 s of skew, inside the tolerance
            (1_000, 0),             // a sub-tolerance step
        ] {
            assert!(
                !suspend_gap(
                    Duration::from_millis(wall_ms),
                    Duration::from_millis(mono_ms)
                ),
                "wall +{wall_ms}ms vs monotonic +{mono_ms}ms must not look like a suspend"
            );
        }
        assert!(
            !suspend_gap(Duration::ZERO, Duration::from_secs(30)),
            "a wall clock stepped backwards (NTP) is not a suspend"
        );
    }

    #[test]
    fn the_suspend_tolerance_is_two_seconds() {
        assert!(
            !suspend_gap(SUSPEND_GAP_TOLERANCE, Duration::ZERO),
            "a gap exactly at the tolerance is ordinary drift"
        );
        assert!(
            suspend_gap(
                SUSPEND_GAP_TOLERANCE + Duration::from_millis(1),
                Duration::ZERO
            ),
            "a gap just past the tolerance is a suspend"
        );
    }

    #[test]
    fn the_resume_grace_is_sixty_seconds() {
        assert_eq!(
            RESUME_GRACE,
            Duration::from_secs(60),
            "the measured chain needs ~29 s (18 s to the first hijack + an 11 s second pass)"
        );
    }

    #[test]
    fn the_grace_boundary_is_exclusive() {
        let base = Instant::now();
        assert!(
            !resume_grace_open(Some(base + RESUME_GRACE), base + RESUME_GRACE),
            "at the boundary the grace is over"
        );
        assert!(
            resume_grace_open(
                Some(base + RESUME_GRACE),
                base + RESUME_GRACE - Duration::from_millis(1)
            ),
            "just inside the boundary it is open"
        );
        assert!(!resume_grace_open(None, base), "no window is never open");
    }

    /// (b) The measured chain: first sample is only a baseline, the detected
    /// suspend grants (source = resume), and the burst's second pass is still
    /// inside the grace.
    #[test]
    fn a_detected_suspend_permits_the_reassert_and_the_second_burst_pass() {
        let base = Instant::now();
        let mut monitor: Option<MonitorPermit> = None;
        let mut resume = ResumeState::default();

        // The FIRST sample only establishes the baseline: one sample cannot
        // show a gap, so it grants nothing.
        let first = decide_reassert(&mut monitor, &mut resume, sample_at(base, 0, 0));
        assert!(!first.suspend_detected, "one sample cannot show a gap");
        assert_eq!(first.permit, ReassertPermit::None);

        // 18 s awake after the suspend: the measured first hijack (18:29:13).
        let hijack = decide_reassert(
            &mut monitor,
            &mut resume,
            sample_at(base, 18_000, SUSPENDED_MS + 18_000),
        );
        assert!(hijack.suspend_detected, "the gap is detected on demand");
        assert_eq!(
            hijack.permit,
            ReassertPermit::Resume,
            "a detected resume permits the re-assert"
        );

        // The measured second pass, 11 s later. The monitor permit is absent
        // and one-shot, so only the grace can grant it.
        let second = decide_reassert(
            &mut monitor,
            &mut resume,
            sample_at(base, 29_000, SUSPENDED_MS + 29_000),
        );
        assert!(
            !second.suspend_detected,
            "the machine is awake: both clocks advance together now"
        );
        assert_eq!(
            second.permit,
            ReassertPermit::Resume,
            "the grace permits the burst's second pass"
        );
    }

    /// (c) No monitor event, no suspend, no permanent permission.
    #[test]
    fn with_no_monitor_event_and_no_suspend_the_reassert_is_refused() {
        let base = Instant::now();
        let mut monitor: Option<MonitorPermit> = None;
        let mut resume = ResumeState::default();
        let _ = decide_reassert(&mut monitor, &mut resume, sample_at(base, 0, 0));

        let quiet = decide_reassert(&mut monitor, &mut resume, sample_at(base, 5_000, 5_000));
        assert!(!quiet.suspend_detected);
        assert_eq!(
            quiet.permit,
            ReassertPermit::None,
            "a quiet machine grants nothing"
        );

        let later = decide_reassert(&mut monitor, &mut resume, sample_at(base, 600_000, 600_000));
        assert_eq!(
            later.permit,
            ReassertPermit::None,
            "and it stays refused: there is no permanent permission"
        );
    }

    /// (d) The grace expires: far beyond it the change is refused again.
    #[test]
    fn a_request_far_beyond_the_grace_is_refused() {
        let base = Instant::now();
        let mut monitor: Option<MonitorPermit> = None;
        let mut resume = ResumeState::default();
        let _ = decide_reassert(&mut monitor, &mut resume, sample_at(base, 0, 0));
        let hijack = decide_reassert(
            &mut monitor,
            &mut resume,
            sample_at(base, 18_000, SUSPENDED_MS + 18_000),
        );
        assert_eq!(hijack.permit, ReassertPermit::Resume);

        let late_mono = 18_000 + RESUME_GRACE.as_millis() as u64 + 1;
        let late = decide_reassert(
            &mut monitor,
            &mut resume,
            sample_at(base, late_mono, SUSPENDED_MS + late_mono),
        );
        assert!(
            !late.suspend_detected,
            "an awake machine shows no gap, however long it has been awake"
        );
        assert_eq!(
            late.permit,
            ReassertPermit::None,
            "the grace expires; the change is refused again"
        );
    }

    /// (e) The monitor path is unchanged and reachable through the same entry
    /// point, reported with its own source.
    #[test]
    fn a_monitor_event_grants_through_the_same_entry_point_and_is_reported() {
        let base = Instant::now();
        let mut monitor: Option<MonitorPermit> = None;
        let mut resume = ResumeState::default();
        arm_monitor_permit(&mut monitor, base);

        let granted = decide_reassert(&mut monitor, &mut resume, sample_at(base, 1_000, 1_000));
        assert_eq!(granted.permit, ReassertPermit::Monitor);
        assert!(!granted.suspend_detected);

        // One-shot, exactly as before: the next change in the same window is
        // refused.
        let again = decide_reassert(&mut monitor, &mut resume, sample_at(base, 2_000, 2_000));
        assert_eq!(again.permit, ReassertPermit::None, "the window is spent");
    }

    #[test]
    fn a_spent_monitor_window_does_not_block_the_resume_grace() {
        let base = Instant::now();
        let mut monitor: Option<MonitorPermit> = None;
        let mut resume = ResumeState::default();
        arm_monitor_permit(&mut monitor, base);
        // Spend the monitor one-shot.
        let spent = decide_reassert(&mut monitor, &mut resume, sample_at(base, 1_000, 1_000));
        assert_eq!(spent.permit, ReassertPermit::Monitor);

        // The resume grace is a separate source and is not consumed by it.
        let hijack = decide_reassert(
            &mut monitor,
            &mut resume,
            sample_at(base, 18_000, SUSPENDED_MS + 18_000),
        );
        assert_eq!(
            hijack.permit,
            ReassertPermit::Resume,
            "a spent monitor window must not block the resume permission"
        );
    }

    /// The production entry point grants nothing on a quiet machine. No
    /// listener runs in a test process, so nothing armed a monitor window and
    /// the machine did not suspend. The first call only sets the baseline.
    #[test]
    fn the_production_entry_point_refuses_without_a_permission() {
        assert_eq!(consume_reassert_permit(), ReassertPermit::None);
        assert_eq!(
            consume_reassert_permit(),
            ReassertPermit::None,
            "still refused: the detector must not leak a permanent permission"
        );
    }
}
