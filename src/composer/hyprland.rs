use crate::composer::{Composer, CompositorQuery, HyprMode};
use slint::ComponentHandle;
use std::sync::OnceLock;

pub struct HyprlandComposer {
    hypr_mode: OnceLock<HyprMode>,
}

impl HyprlandComposer {
    pub fn new() -> Self {
        Self {
            hypr_mode: OnceLock::new(),
        }
    }

    fn hypr_mode(&self) -> HyprMode {
        *self.hypr_mode.get_or_init(|| {
            if let Ok(out) = std::process::Command::new("hyprctl")
                .args(["dispatch", "hl.dsp.no_op()"])
                .output()
            {
                if String::from_utf8_lossy(&out.stdout).contains("ok") {
                    return HyprMode::V5;
                }
            }
            if let Ok(out) = std::process::Command::new("hyprctl").arg("version").output() {
                if String::from_utf8_lossy(&out.stdout).contains("Hyprland") {
                    return HyprMode::V4;
                }
            }
            HyprMode::None
        })
    }

    /// Startup sanity check (spec scenario: crash left HVE stuck fullscreen).
    /// Parses the same `hyprctl clients -j` query path as `hve_in_special`
    /// and, before the first show, repairs a stuck state: exit fullscreen,
    /// then restore floating. Focus-by-title precedes every dispatch.
    /// Best-effort by design — any failure is swallowed; HVE simply starts.
    pub fn startup_sanity(&self) {
        let Some(text) = self.clients_json() else {
            return;
        };
        let state = parse_stuck_state(&text);
        if !state.needs_repair() {
            return;
        }
        tracing::info!(
            fullscreen = state.fullscreen,
            tiled = state.tiled,
            "startup sanity: repairing stuck gallery fullscreen"
        );
        // Dispatches act on the focused window — focus HVE by title first.
        if !self.focus_by_title() {
            return;
        }
        for step in repair_plan(&state) {
            match step {
                "fullscreen_off" => match self.hypr_mode() {
                    HyprMode::V5 => {
                        let s = v5_set_fullscreen(false);
                        self.hypr_dispatch_v5(&s);
                    }
                    // V4/conf fallback: Hyprland V4 dispatch has NO window
                    // targeting — must rely on the prior focus_by_title; this
                    // focus-then-act pattern is required and forbidden to remove.
                    HyprMode::V4 => {
                        self.hypr_dispatch_v4(&["fullscreen", "0"]);
                    }
                    HyprMode::None => {}
                },
                "refloat" => match self.hypr_mode() {
                    HyprMode::V5 => {
                        let s = v5_float_on();
                        self.hypr_dispatch_v5(&s);
                    }
                    // Tiled per the parsed snapshot, so toggle == set float.
                    // V4/conf fallback: no window targeting — relies on prior
                    // focus_by_title; do not remove the focus step.
                    HyprMode::V4 => {
                        self.hypr_dispatch_v4(&["togglefloating", HVE_TITLE]);
                    }
                    HyprMode::None => {}
                },
                _ => {}
            }
        }
    }

    /// Focus HVE by title (constant argv) — prerequisite for every
    /// subsequent window-targeted dispatch. Returns dispatch success.
    fn focus_by_title(&self) -> bool {
        match self.hypr_mode() {
            HyprMode::V5 => {
                let s = v5_focus();
                self.hypr_dispatch_v5(&s)
            }
            HyprMode::V4 => self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]),
            HyprMode::None => false,
        }
    }

    fn hypr_dispatch_v5(&self, script: &str) -> bool {
        std::process::Command::new("hyprctl")
            .args(["dispatch", script])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("ok"))
            .unwrap_or(false)
    }

    fn hypr_dispatch_v4(&self, args: &[&str]) -> bool {
        std::process::Command::new("hyprctl")
            .arg("dispatch")
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("ok"))
            .unwrap_or(false)
    }

    fn hve_in_special(&self) -> bool {
        let Some(text) = self.clients_json() else {
            return false;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            return false;
        };
        v.as_array().into_iter().flatten().any(|w| {
            w.get("title")
                .and_then(|t| t.as_str())
                .is_some_and(|t| t.contains("Hyprland Visual Editor"))
                && w.get("workspace")
                    .and_then(|ws| ws.get("name"))
                    .and_then(|n| n.as_str())
                    .is_some_and(|n| n.contains("special"))
        })
    }

    /// Fetch `hyprctl clients -j` stdout. Shared query path for
    /// `hve_in_special` and the startup sanity check.
    fn clients_json(&self) -> Option<String> {
        hyprctl_json(&["clients", "-j"])
    }

    /// Fetch `hyprctl monitors -j` stdout. Query path for the verified-hide
    /// scratchpad-open check (odd/hide-idempotency-and-singleton-watcher W2).
    fn monitors_json(&self) -> Option<String> {
        hyprctl_json(&["monitors", "-j"])
    }

    fn active_workspace(&self) -> Option<String> {
        let text = hyprctl_json(&["activeworkspace", "-j"])?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        v.get("name").and_then(|n| n.as_str()).map(|s| s.to_string())
    }

    fn show_and_sync(&self, win: &crate::MainWindow) {
        let _ = win.window().show();
        crate::shell::Shell::sync_global_after_show();
    }

    fn sync_after_show(&self) {
        crate::shell::Shell::sync_global_after_show();
    }
}

// ── Read-only query helpers (one spawn path for every JSON verb) ──────

/// Spawn one read-only `hyprctl … -j` query and return its raw stdout.
/// `None` when the process cannot run or its output is not UTF-8 — every
/// query verb in this module routes through here, so there is exactly ONE
/// `Command::new("hyprctl")` per query shape instead of one per verb.
fn hyprctl_json(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("hyprctl").args(args).output().ok()?;
    String::from_utf8(out.stdout).ok()
}

/// argv (after the `hyprctl` binary) for one query selector. Pure, so the
/// selector → query mapping is asserted without spawning anything.
fn query_args(query: CompositorQuery) -> &'static [&'static str] {
    match query {
        CompositorQuery::Workspaces => &["workspaces", "-j"],
    }
}

/// argv (after the `hyprctl` binary) for the raw live-option read. Pure,
/// so the moved `main.rs` call site's argv (`getoption <name> -j`) is
/// pinned by test.
fn option_args(name: &str) -> [&str; 3] {
    ["getoption", name, "-j"]
}

impl Composer for HyprlandComposer {
    fn hide(&self, win: &crate::MainWindow) -> bool {
        match self.hypr_mode() {
            HyprMode::V5 => {
                // Idempotent verified hide (odd/hide-idempotency-and-singleton-watcher
                // W2): query the live state, derive the plan purely, dispatch it,
                // then VERIFY the scratchpad ended closed. The plan never emits a
                // toggle while the scratchpad is closed — the old blind toggle
                // re-opened the overlay and HVE became visible inside it.
                let parked = self.hve_in_special();
                let open = self
                    .monitors_json()
                    .is_some_and(|text| scratchpad_open(&text));
                let plan = hide_plan(parked, open);
                if plan.is_empty() {
                    // Already parked with the scratchpad closed: dispatch NOTHING
                    // (no toggle, no move) and report success — the regression case.
                    return true;
                }
                let mut abandoned_v5 = false;
                for step in &plan {
                    match step {
                        HideStep::MoveToSpecial => {
                            // Target the move EXPLICITLY at HVE by title. Without `window=`,
                            // `hl.dsp.window.move` acts on the focused window: if the
                            // user is clicking another window at the very instant of
                            // SUPER+H, the move hijacks THAT window and sends it to the special.
                            if !self.hypr_dispatch_v5(&v5_move_to_special()) {
                                // Abandon the v5 plan before any close: the move is the
                                // foundation the close builds on. The v4 fallback below
                                // re-runs move + close with the classic syntax.
                                abandoned_v5 = true;
                                break;
                            }
                        }
                        HideStep::CloseScratchpad => {
                            // Moving to the special "opens" it as a visible overlay: closing
                            // after moving leaves the window hidden, not floating above the
                            // active workspace (that was the "does not minimize" regression).
                            let _ = self.hypr_dispatch_v5(&v5_toggle_special());
                        }
                    }
                }
                if abandoned_v5 {
                    // If the lua move failed, we try moving with the classic
                    // syntax (commonly "special:minimized" accepts move).
                    // V4/conf fallback: Hyprland V4 dispatch has NO window targeting
                    // — must focus-by-title first; this pattern is required and
                    // forbidden to remove (movetoworkspacesilent acts on focused window).
                    let did_focus = self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
                    let did_move4 =
                        self.hypr_dispatch_v4(&["movetoworkspacesilent", &format!("special:{SPECIAL}")]);
                    if did_focus && did_move4 {
                        let _ = self.hypr_dispatch_v4(&["togglespecialworkspace", SPECIAL]);
                        return true;
                    }
                    let _ = win.window().hide();
                    return false;
                }
                // Verify with a FRESH query that the scratchpad ended closed — never
                // trust the toggle blindly. Still open: exactly ONE bounded corrective
                // close (never a loop), then log the outcome.
                if self.monitors_json().is_some_and(|text| scratchpad_open(&text)) {
                    let _ = self.hypr_dispatch_v5(&v5_toggle_special());
                    let still_open = self.monitors_json().is_some_and(|text| scratchpad_open(&text));
                    tracing::warn!(
                        still_open,
                        "[hide] scratchpad still open after planned close — corrective close dispatched"
                    );
                }
                true
            }
            HyprMode::V4 => {
                // V4/conf fallback: Hyprland V4 dispatch has NO window targeting
                // — must focus-by-title first; this focus-then-act pattern is
                // required and forbidden to remove. Without it a concurrent
                // click in another window would hijack that window to special.
                let did_focus = self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
                let did_move =
                    self.hypr_dispatch_v4(&["movetoworkspacesilent", &format!("special:{SPECIAL}")]);
                if did_focus && did_move {
                    let _ = self.hypr_dispatch_v4(&["togglespecialworkspace", SPECIAL]);
                    true
                } else {
                    let _ = win.window().hide();
                    false
                }
            }
            HyprMode::None => {
                let _ = win.window().hide();
                false
            }
        }
    }

    fn show(&self, win: &crate::MainWindow, prev_workspace: Option<&str>) -> bool {
        let fast = match self.hypr_mode() {
            HyprMode::V5 => {
                if self.hve_in_special() {
                    // Workspace to bring HVE back to: the active one or the previous one.
                    let target = self
                        .active_workspace()
                        .or_else(|| prev_workspace.map(|s| s.to_string()))
                        .unwrap_or_else(|| "1".to_string());
                    // SECURITY: reject workspace names that could break out of
                    // the Lua string below (see safe_workspace_target).
                    if !safe_workspace_target(&target) {
                        self.show_and_sync(win);
                        return false;
                    }
                    // 1) Move HVE to the target workspace FIRST — the V5 move
                    //    is explicitly targeted with `window="title:Hyprland Visual Editor"`
                    //    (unit 2), so it works without focus. Moving before focusing
                    //    avoids focusing inside the special overlay, which would open
                    //    the overlay and emit blur on close -> instant re-hide loop.
                    let s_move = v5_move_to_workspace(&target);
                    let did_move = self.hypr_dispatch_v5(&s_move);
                    // 2) Focus HVE after it is back in the real workspace.
                    let s_focus = v5_focus();
                    let did_focus = self.hypr_dispatch_v5(&s_focus);
                    if !(did_focus && did_move) {
                        self.show_and_sync(win);
                        return false;
                    }
                    // 3) Sync size after show; focus + fullscreen are deferred
                    // to the SETTLE path (focus confirm or 1s timeout) — this
                    // kills the 150ms/400ms reassert races and the
                    // auto-minimize-on-open focus-lost race.
                    self.sync_after_show();
                    true
                } else {
                    // Window not in special (e.g. closed with SUPER+C) — slow path.
                    // Show via Slint and sync size; settle will wake focus.
                    self.show_and_sync(win);
                    false
                }
            }
            HyprMode::V4 => {
                // V4/conf fallback: Hyprland V4 dispatch has NO window targeting
                // — must focus-by-title first; this focus-then-act pattern is
                // required and forbidden to remove.
                if self.hve_in_special() {
                    let target = self
                        .active_workspace()
                        .or_else(|| prev_workspace.map(|s| s.to_string()))
                        .unwrap_or_else(|| "1".to_string());
                    // SECURITY: same allowlist as the v5 path.
                    if !safe_workspace_target(&target) {
                        self.show_and_sync(win);
                        return false;
                    }
                    // 1) Focus HVE first (movetoworkspacesilent acts on focused window — V4 has no window selector).
                    let did_focus = self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
                    // 2) Bring it back to the real workspace.
                    let did_move =
                        self.hypr_dispatch_v4(&["movetoworkspacesilent", &target]);
                    if !(did_focus && did_move) {
                        self.show_and_sync(win);
                        return false;
                    }
                    // 3) Sync size; focus is deferred to settle.
                    self.sync_after_show();
                    true
                } else {
                    // Window not in special — slow path, settle will wake focus.
                    self.show_and_sync(win);
                    false
                }
            }
            HyprMode::None => {
                // No Hyprland — just show via Slint; settle will wake focus.
                self.show_and_sync(win);
                false
            }
        };
        win.window().request_redraw();
        fast
    }

    fn focus(&self) {
        match self.hypr_mode() {
            HyprMode::V5 => {
                let s = v5_focus();
                let _ = self.hypr_dispatch_v5(&s);
            }
            // V4/conf fallback: no window targeting — focus by title is the
            // only selector; keep this path and its comment.
            HyprMode::V4 => {
                let _ = self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
            }
            HyprMode::None => {}
        }
    }

    fn toggle_float(&self) {
        match self.hypr_mode() {
            HyprMode::V5 => {
                let s = v5_float_toggle();
                let _ = self.hypr_dispatch_v5(&s);
            }
            // V4/conf fallback: no window targeting — focus semantics are
            // carried by the title arg; keep and do not remove.
            HyprMode::V4 => {
                let _ = self.hypr_dispatch_v4(&["togglefloating", HVE_TITLE]);
            }
            HyprMode::None => {}
        }
    }

    /// Fullscreen lifecycle for immersive Gallery sessions (design D1).
    ///
    /// Dispatches act on the focused window, so HVE is focused BY TITLE
    /// first (proven pattern from hide/show). All arguments are compile-time
    /// constants — no dynamic interpolation, no new injection surface.
    ///
    /// V5 spelling verified against the installed Hyprland 0.56.2 source
    /// (`src/config/lua/bindings/LuaBindingsDispatchers.cpp`): the dispatcher
    /// is nested under `window.` and takes `{ mode, action, window }` where
    /// mode `"fullscreen"` is borderless fullscreen (`"0"` literal) and
    /// `"maximized"` is `"1"`. Enter uses `action = "set"`, exit
    /// `action = "unset"` — both idempotent, so a lost event can never wedge
    /// the state machine.
    ///
    /// V4 keeps the classic fallback contract: `focuswindow` +
    /// `fullscreen 1|0`.
    ///
    /// Any failed focus or fullscreen dispatch returns false: the caller
    /// continues windowed and the gallery stays fully functional.
    fn set_fullscreen(&self, on: bool) -> bool {
        match self.hypr_mode() {
            HyprMode::V5 => {
                let s_focus = v5_focus();
                let did_focus = self.hypr_dispatch_v5(&s_focus);
                let s_fs = v5_set_fullscreen(on);
                if did_focus && self.hypr_dispatch_v5(&s_fs) {
                    return true;
                }
                // Fallback to legacy real fullscreen (2, covers layer-shell)
                if did_focus {
                    let mode = if on { "2" } else { "0" };
                    if self.hypr_dispatch_v4(&["fullscreen", mode]) {
                        return true;
                    }
                    let fb = if on { "1" } else { "0" };
                    return self.hypr_dispatch_v4(&["fullscreen", fb]);
                }
                false
            }
            // V4/conf fallback: no window targeting — must focus-by-title
            // first; this focus-then-act pattern is required and forbidden
            // to remove. Try real fullscreen (2) first.
            HyprMode::V4 => {
                let did_focus = self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
                if !did_focus {
                    return false;
                }
                let mode = if on { "2" } else { "0" };
                if self.hypr_dispatch_v4(&["fullscreen", mode]) {
                    return true;
                }
                let fb = if on { "1" } else { "0" };
                self.hypr_dispatch_v4(&["fullscreen", fb])
            }
            HyprMode::None => false,
        }
    }

    fn active_workspace(&self) -> Option<String> {
        self.active_workspace()
    }

    /// Read-only query by explicit selector (see the trait doc): the raw
    /// JSON text, `None` when the compositor does not answer.
    fn query_json(&self, query: CompositorQuery) -> Option<String> {
        hyprctl_json(query_args(query))
    }

    /// Raw live-option read (see the trait doc): the caller parses the
    /// JSON text and owns the option-name normalisation.
    fn read_option(&self, name: &str) -> Option<String> {
        hyprctl_json(&option_args(name))
    }

    /// Floating Settings presentation (see the trait doc): float and center
    /// HVE so the border the compositor draws around it becomes the live
    /// preview of the border being tuned.
    ///
    /// Dispatch order matters: float FIRST, then center. A floating window
    /// keeps its last position, so centering is what places it with the
    /// margin visible all around — and centering a tiled window is a no-op.
    ///
    /// Both float actions are the idempotent `on`/`off` forms, never
    /// `toggle`, so a lost event cannot wedge the state (same reasoning as
    /// the fullscreen lifecycle above).
    fn set_settings_float(&self, on: bool) -> bool {
        match self.hypr_mode() {
            HyprMode::V5 => {
                let did_focus = self.hypr_dispatch_v5(&v5_focus());
                if !did_focus {
                    return false;
                }
                let s_float = if on { v5_float_on() } else { v5_float_off() };
                let floated = self.hypr_dispatch_v5(&s_float);
                if !on {
                    return floated;
                }
                let centered = self.hypr_dispatch_v5(&v5_center());
                floated && centered
            }
            // V4/conf fallback: no window targeting, so focus-by-title first
            // (the same required pattern as set_fullscreen above). Legacy
            // Hyprland has no on/off float form — `togglefloating` is the
            // only dispatcher, so this branch is best-effort: a lost event
            // can leave the float state inverted, which is why the V5 path
            // is the contract and this one is the fallback.
            HyprMode::V4 => {
                if !self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]) {
                    return false;
                }
                let floated = self.hypr_dispatch_v4(&["togglefloating", HVE_TITLE]);
                if !on {
                    return floated;
                }
                let centered = self.hypr_dispatch_v4(&["centerwindow"]);
                floated && centered
            }
            HyprMode::None => false,
        }
    }

    /// Explicit resize (see the trait doc): the client-side size request is
    /// not honoured for a floating window here, so the resolved Settings size
    /// is dispatched instead.
    fn resize_window(&self, size: (f32, f32)) -> bool {
        match self.hypr_mode() {
            HyprMode::V5 => self.hypr_dispatch_v5(&v5_resize(size)),
            // V4/conf fallback: no window targeting, so focus-by-title first.
            HyprMode::V4 => {
                if !self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]) {
                    return false;
                }
                self.hypr_dispatch_v4(&[
                    "resizeactive",
                    &format!("exact {} {}", size.0.round() as i32, size.1.round() as i32),
                ])
            }
            HyprMode::None => false,
        }
    }
}

const HVE_TITLE: &str = "Hyprland Visual Editor";
const SPECIAL: &str = "minimized";

// ── V5 command builders (pure, no I/O — testable targeting seam) ──────
// Extracted for unit 2 scope guard: every HVE-window-acting V5 dispatch
// must carry an explicit `window="title:..."` selector. Builders are pure
// so targeting can be asserted without hyprctl or sleeps.

pub(crate) fn v5_focus() -> String {
    format!("hl.dsp.focus({{ window = \"title:{HVE_TITLE}\" }})")
}

pub(crate) fn v5_move_to_special() -> String {
    format!(
        "hl.dsp.window.move({{ window = \"title:{HVE_TITLE}\", workspace = \"special:{SPECIAL}\" }})"
    )
}

pub(crate) fn v5_move_to_workspace(workspace: &str) -> String {
    format!(
        "hl.dsp.window.move({{ window = \"title:{HVE_TITLE}\", workspace = \"{workspace}\" }})"
    )
}

pub(crate) fn v5_toggle_special() -> String {
    format!("hl.dsp.workspace.toggle_special(\"{SPECIAL}\")")
}

pub(crate) fn v5_set_fullscreen(on: bool) -> String {
    if on {
        format!(
            "hl.dsp.window.fullscreen({{ mode = \"fullscreen\", action = \"set\", \
             window = \"title:{HVE_TITLE}\" }})"
        )
    } else {
        format!(
            "hl.dsp.window.fullscreen({{ action = \"unset\", \
             window = \"title:{HVE_TITLE}\" }})"
        )
    }
}

pub(crate) fn v5_float_on() -> String {
    format!("hl.dsp.window.float({{ action = \"on\", window = \"title:{HVE_TITLE}\" }})")
}

pub(crate) fn v5_float_toggle() -> String {
    format!("hl.dsp.window.float({{ action = \"toggle\", window = \"title:{HVE_TITLE}\" }})")
}

pub(crate) fn v5_float_off() -> String {
    format!("hl.dsp.window.float({{ action = \"off\", window = \"title:{HVE_TITLE}\" }})")
}

pub(crate) fn v5_center() -> String {
    format!("hl.dsp.window.center({{ window = \"title:{HVE_TITLE}\" }})")
}

/// Explicit resize for the floating Settings presentation.
///
/// The only builder that carries numbers. They are rounded to `i32` here, so
/// the dispatch string can only ever hold digits and a sign — no text from
/// any other source reaches it (pinned by `v5_resize_is_integer_only`).
pub(crate) fn v5_resize(size: (f32, f32)) -> String {
    format!(
        "hl.dsp.window.resize({{ x = {}, y = {}, relative = false, window = \"title:{HVE_TITLE}\" }})",
        size.0.round() as i32,
        size.1.round() as i32
    )
}

/// Mapped/fullscreen snapshot of the HVE client, parsed from a
/// `hyprctl clients -j` response. Pure data — headless-testable.
/// Unlike `StuckState` (crash repair), this answers the startup settle
/// question: "is HVE mapped yet, and is it already fullscreen?"
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub(crate) struct HveSeen {
    /// An HVE-titled client exists in the compositor's client list.
    pub mapped: bool,
    /// That client sits in a fullscreen/maximized state.
    pub fullscreen: bool,
}

/// Parse a `hyprctl clients -j` snapshot for the HVE client. Malformed
/// JSON or a missing HVE client yields the default (unmapped) state —
/// never panic on compositor output.
///
/// JSON field shapes verified against Hyprland 0.56.2 live output:
/// `fullscreen`/`fullscreenClient` are ints (0 none, 1 fullscreen, 2
/// maximized).
pub(crate) fn parse_hve_seen(clients_json: &str) -> HveSeen {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(clients_json) else {
        return HveSeen::default();
    };
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let title = w.get("title")?.as_str()?;
            if !title.contains(HVE_TITLE) {
                return None;
            }
            let flag = |key: &str| w.get(key).and_then(|f| f.as_i64()).unwrap_or(0) > 0;
            Some(HveSeen {
                mapped: true,
                fullscreen: flag("fullscreen") || flag("fullscreenClient"),
            })
        })
        .next()
        .unwrap_or_default()
}

/// Best-effort live query: current mapped/fullscreen snapshot of HVE.
/// No compositor interaction beyond a read-only `clients -j`; unmapped or
/// unreachable sessions yield the default (unmapped) state.
pub(crate) fn query_hve_seen() -> HveSeen {
    std::process::Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|text| parse_hve_seen(&text))
        .unwrap_or_default()
}

// ── Startup sanity (gallery-immersive-redesign 1.4/1.5) ────────────────

/// Stuck-fullscreen state of the HVE window, parsed from a
/// `hyprctl clients -j` snapshot. Pure data — headless-testable.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct StuckState {
    /// An HVE-titled client was found at all.
    present: bool,
    /// Window sits in a fullscreen/maximized state (internal or client flag).
    fullscreen: bool,
    /// Window is tiled (`floating == false`).
    tiled: bool,
}

impl StuckState {
    /// Whether any repair dispatch is required.
    fn needs_repair(&self) -> bool {
        self.fullscreen || self.tiled
    }
}

/// Parse a `hyprctl clients -j` snapshot and locate HVE by title substring,
/// reporting its stuck flags. Malformed JSON or a missing HVE client yields
/// the default (nothing stuck) state — never panic on compositor output.
///
/// JSON field shapes verified against Hyprland 0.56.2 live output:
/// `fullscreen`/`fullscreenClient` are ints (0 none, 1 fullscreen, 2
/// maximized), `floating` is a bool.
fn parse_stuck_state(clients_json: &str) -> StuckState {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(clients_json) else {
        return StuckState::default();
    };
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let title = w.get("title")?.as_str()?;
            if !title.contains(HVE_TITLE) {
                return None;
            }
            let flag = |key: &str| w.get(key).and_then(|f| f.as_i64()).unwrap_or(0) > 0;
            Some(StuckState {
                present: true,
                fullscreen: flag("fullscreen") || flag("fullscreenClient"),
                tiled: w.get("floating").and_then(|f| f.as_bool()) == Some(false),
            })
        })
        .next()
        .unwrap_or_default()
}

/// Symbolic repair dispatches derived from a stuck state, in execution
/// order: exit fullscreen first, then restore float.
fn repair_plan(state: &StuckState) -> Vec<&'static str> {
    let mut plan = Vec::new();
    if state.fullscreen {
        plan.push("fullscreen_off");
    }
    if state.tiled {
        plan.push("refloat");
    }
    plan
}

/// SECURITY: workspace names are interpolated into a Lua string sent to
/// hyprctl dispatch (`hl.dsp.window.move({ workspace = "..." })`). Validate
/// against a strict allowlist so a crafted name (quotes, backslashes, `;`,
/// control chars) cannot break out of the Lua string and inject arbitrary
/// Lua (which `hl.exec_cmd` would run). Applied to both v5 and v4 paths.
fn safe_workspace_target(target: &str) -> bool {
    !target.is_empty()
        && target.len() <= 128
        && target
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
}

// ── Idempotent verified hide (odd/hide-idempotency-and-singleton-watcher W2) ──

/// Hide steps for the V5 arm, in execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HideStep {
    /// Move HVE (by title) into the special workspace. Opens the overlay if
    /// it was closed, which is why a close always follows in the same plan.
    MoveToSpecial,
    /// Close the scratchpad. Only ever dispatched when the state says OPEN —
    /// a blind toggle is the regression this change removes.
    CloseScratchpad,
}

/// Whether the scratchpad (`special:minimized`) is OPEN on any monitor, parsed
/// from a `hyprctl monitors -j` snapshot. `{id: 0, name: ""}` is the CLOSED
/// state (verified live 2026-09-22); a non-empty `name` or a non-zero `id`
/// means open. Malformed or absent monitor data parses as CLOSED, so hide
/// degrades to the legacy path instead of panicking on compositor output.
pub(crate) fn scratchpad_open(monitors_json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(monitors_json) else {
        return false;
    };
    v.as_array().into_iter().flatten().any(|m| {
        let Some(sw) = m.get("specialWorkspace") else {
            return false;
        };
        let name_open = sw
            .get("name")
            .and_then(|n| n.as_str())
            .is_some_and(|n| !n.is_empty());
        let id_open = sw.get("id").and_then(|i| i.as_i64()).unwrap_or(0) != 0;
        name_open || id_open
    })
}

/// Pure hide plan for the V5 arm.
///
/// - `parked`: HVE already lives in the special workspace (the existing
///   `hve_in_special` semantics).
/// - `scratchpad_open`: live `hyprctl monitors -j` state (see `scratchpad_open`).
///
/// The regression case — parked with the scratchpad closed — yields an EMPTY
/// plan: the old code blind-toggled there, re-opening the overlay so HVE
/// became visible inside it (the 2nd/3rd apply bug). A toggle is only ever
/// emitted when the state says the scratchpad is open.
pub(crate) fn hide_plan(parked: bool, scratchpad_open: bool) -> Vec<HideStep> {
    if parked && !scratchpad_open {
        return Vec::new();
    }
    let mut plan = Vec::new();
    if !parked {
        // Moving to the special workspace opens the overlay, so the close
        // must follow in the same plan (comment at `hyprland.rs` hide, V5).
        plan.push(HideStep::MoveToSpecial);
    }
    plan.push(HideStep::CloseScratchpad);
    plan
}



// ── Tests (gallery-immersive-redesign 1.4) ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A crash left HVE stuck fullscreen while still floating: the repair
    /// plan must demand the fullscreen-off dispatch only.
    #[test]
    fn sanity_stuck_fullscreen_demands_fullscreen_off() {
        let st = parse_stuck_state(
            r#"[
            {"title": "other app", "fullscreen": 2, "fullscreenClient": 2, "floating": false},
            {"title": "Hyprland Visual Editor", "fullscreen": 1, "fullscreenClient": 0, "floating": true}
        ]"#,
        );
        assert!(st.present, "HVE must be located by title");
        assert!(st.fullscreen, "internal fullscreen flag must be detected");
        assert!(!st.tiled, "a floating window is not tiled");
        assert_eq!(
            repair_plan(&st),
            vec!["fullscreen_off"],
            "stuck fullscreen must yield the fullscreen 0 repair"
        );
    }

    /// Fullscreen OTHER windows (games, video players) must never trigger
    /// a repair: only the HVE-titled client counts.
    #[test]
    fn sanity_ignores_non_hve_fullscreen_clients() {
        let st = parse_stuck_state(
            r#"[{"title": "Some Game", "fullscreen": 2, "fullscreenClient": 2, "floating": false}]"#,
        );
        assert!(!st.present);
        assert!(repair_plan(&st).is_empty());
    }

    /// A stuck fullscreen that ALSO lost float (tiled) adds the float
    /// restore step after the fullscreen-off step.
    #[test]
    fn sanity_tiled_stuck_adds_refloat_after_fullscreen_off() {
        let st = parse_stuck_state(
            r#"[{"title": "Hyprland Visual Editor", "fullscreen": 2, "fullscreenClient": 1, "floating": false}]"#,
        );
        assert!(st.fullscreen && st.tiled);
        assert_eq!(
            repair_plan(&st),
            vec!["fullscreen_off", "refloat"],
            "float restore follows the fullscreen 0 repair"
        );
    }

    /// Healthy (floating, windowed) or absent HVE needs no repair at all.
    #[test]
    fn sanity_healthy_or_absent_hve_needs_nothing() {        let healthy = parse_stuck_state(
            r#"[{"title": "Hyprland Visual Editor", "fullscreen": 0, "fullscreenClient": 0, "floating": true}]"#,
        );
        assert!(healthy.present && !healthy.needs_repair());

        let absent = parse_stuck_state(r#"[]"#);
        assert!(!absent.present && !absent.needs_repair());

        let garbage = parse_stuck_state("not json");
        assert!(!garbage.present && !garbage.needs_repair());
    }

    // ── Startup fullscreen confirmation (fresh-launch settle) ──────────
    // Fixed 200ms/400ms timers fire before Hyprland maps/focuses HVE, so
    // the fullscreen dispatch lands on an unknown client. The startup path
    // now gates retries on `hyprctl clients -j`: only dispatch once HVE is
    // mapped, stop once it reports fullscreen.

    #[test]
    fn seen_reports_mapped_windowed_hve() {
        let seen = parse_hve_seen(
            r#"[{"title": "Hyprland Visual Editor", "fullscreen": 0, "fullscreenClient": 0, "floating": true}]"#,
        );
        assert!(seen.mapped, "HVE client must count as mapped");
        assert!(!seen.fullscreen, "windowed HVE is not fullscreen");
    }

    #[test]
    fn seen_reports_mapped_fullscreen_hve() {
        let seen = parse_hve_seen(
            r#"[{"title": "Hyprland Visual Editor", "fullscreen": 2, "fullscreenClient": 0, "floating": true}]"#,
        );
        assert!(seen.mapped);
        assert!(seen.fullscreen, "internal fullscreen flag must be detected");
    }

    #[test]
    fn seen_ignores_other_fullscreen_clients() {
        let seen = parse_hve_seen(
            r#"[{"title": "Some Game", "fullscreen": 2, "fullscreenClient": 2, "floating": false}]"#,
        );
        assert!(!seen.mapped, "only the HVE-titled client counts as mapped");
        assert!(!seen.fullscreen);
    }

    #[test]
    fn seen_defaults_on_absent_or_garbage() {
        assert_eq!(parse_hve_seen(r#"[]"#), HveSeen::default());
        assert_eq!(parse_hve_seen("not json"), HveSeen::default());
        assert_eq!(parse_hve_seen(r#"{"not": "an array"}"#), HveSeen::default());
    }

    // ── Idempotent verified hide (odd/hide-idempotency-and-singleton-watcher W2) ──

    /// The healthy live state (verified 2026-09-22): `{id: 0, name: ""}` on
    /// every monitor means the scratchpad is CLOSED.
    #[test]
    fn scratchpad_closed_state_parses_to_false() {
        let closed = r#"[
            {"name": "DP-1", "specialWorkspace": {"id": 0, "name": ""}},
            {"name": "HDMI-A-1", "specialWorkspace": {"id": 0, "name": ""}}
        ]"#;
        assert!(
            !scratchpad_open(closed),
            "both monitors closed ({{id:0, name:\"\"}}) must parse as closed"
        );
    }

    /// An open scratchpad (`special:minimized`) on a monitor means OPEN.
    #[test]
    fn scratchpad_open_state_parses_to_true() {
        let open = r#"[
            {"name": "DP-1", "specialWorkspace": {"id": 2, "name": "special:minimized"}}
        ]"#;
        assert!(
            scratchpad_open(open),
            "a monitor carrying special:minimized must parse as open"
        );
    }

    /// The scratchpad is open when ANY monitor reports it open.
    #[test]
    fn scratchpad_open_on_any_monitor_parses_to_true() {
        let mixed = r#"[
            {"name": "DP-1", "specialWorkspace": {"id": 0, "name": ""}},
            {"name": "HDMI-A-1", "specialWorkspace": {"id": 4, "name": "special:minimized"}}
        ]"#;
        assert!(
            scratchpad_open(mixed),
            "one open monitor is enough — hiding must not blind-toggle while visible"
        );
    }

    /// Malformed or absent monitor data parses as CLOSED: hide degrades to the
    /// legacy move+close path instead of panicking on compositor output.
    #[test]
    fn scratchpad_garbage_or_absent_parses_to_closed() {
        assert!(!scratchpad_open("not json"));
        assert!(!scratchpad_open(r#"[]"#));
        assert!(!scratchpad_open(r#"{"not": "an array"}"#));
        assert!(!scratchpad_open(r#"[{"name": "DP-1"}]"#));
    }

    /// THE regression case: already parked with the scratchpad closed, a
    /// repeated hide() must dispatch NOTHING — the old blind toggle re-opened
    /// the overlay and HVE became visible inside it (the 2nd/3rd apply bug).
    #[test]
    fn hide_plan_parked_with_closed_scratchpad_is_empty() {
        let plan = hide_plan(true, false);
        assert!(
            plan.is_empty(),
            "parked + closed must never emit a toggle or a move, got: {:?}",
            plan
        );
    }

    /// Parked with the scratchpad open: close only — no move, no toggle while
    /// the window is already in the special workspace.
    #[test]
    fn hide_plan_parked_with_open_scratchpad_closes_only() {
        assert_eq!(
            hide_plan(true, true),
            vec![HideStep::CloseScratchpad],
            "parked + open must emit exactly one close"
        );
    }

    /// Not parked: move to special FIRST (the move opens the overlay), then
    /// close — regardless of whether the scratchpad was open or closed before.
    #[test]
    fn hide_plan_not_parked_moves_then_closes() {
        assert_eq!(
            hide_plan(false, true),
            vec![HideStep::MoveToSpecial, HideStep::CloseScratchpad],
            "not parked + open must move then close"
        );
        assert_eq!(
            hide_plan(false, false),
            vec![HideStep::MoveToSpecial, HideStep::CloseScratchpad],
            "not parked + closed: the move opens the overlay, so the close must follow"
        );
    }

    // ── Read-only query verbs (capability-routing Phase 3) ────────────

    /// Each selector maps to EXACTLY the argv the moved `main.rs` call
    /// site used — same verb, same `-j`, no speculative queries.
    #[test]
    fn query_args_map_each_selector_to_its_json_query() {
        assert_eq!(
            query_args(CompositorQuery::Workspaces),
            &["workspaces", "-j"],
            "the workspaces query must stay `hyprctl workspaces -j`"
        );
    }

    /// The raw option read keeps the moved call site's argv:
    /// `hyprctl getoption <name> -j`, with the name passed verbatim.
    #[test]
    fn option_args_keep_the_getoption_argv_shape() {
        assert_eq!(
            option_args("decoration:blur:enabled"),
            ["getoption", "decoration:blur:enabled", "-j"],
            "getoption must take the caller's name verbatim, unnormalised"
        );
    }
}

#[cfg(test)]
mod targeting_tests {
    use super::*;

    const WINDOW_SELECTOR: &str = "window = \"title:Hyprland Visual Editor\"";

    fn assert_window_targeted(s: &str) {
        assert!(
            s.contains(WINDOW_SELECTOR),
            "V5 window-acting dispatch must carry explicit window selector `{WINDOW_SELECTOR}`, got: {s}"
        );
    }

    #[test]
    fn v5_focus_contains_window_selector() {
        assert_window_targeted(&v5_focus());
    }

    #[test]
    fn v5_move_to_special_contains_window_selector() {
        assert_window_targeted(&v5_move_to_special());
    }

    #[test]
    fn v5_move_to_workspace_contains_window_selector() {
        // Hijack regression: without `window=`, hl.dsp.window.move acts on
        // focused window and hijacks the user's window.
        assert_window_targeted(&v5_move_to_workspace("1"));
        assert_window_targeted(&v5_move_to_workspace("special:minimized"));
    }

    #[test]
    fn v5_set_fullscreen_on_contains_window_selector() {
        assert_window_targeted(&v5_set_fullscreen(true));
    }

    #[test]
    fn v5_set_fullscreen_off_contains_window_selector() {
        assert_window_targeted(&v5_set_fullscreen(false));
    }

    #[test]
    fn v5_float_on_contains_window_selector() {
        assert_window_targeted(&v5_float_on());
    }

    #[test]
    fn v5_float_toggle_contains_window_selector() {
        assert_window_targeted(&v5_float_toggle());
    }

    #[test]
    fn v5_float_off_contains_window_selector() {
        assert_window_targeted(&v5_float_off());
    }

    #[test]
    fn v5_center_contains_window_selector() {
        assert_window_targeted(&v5_center());
    }

    /// `v5_resize` is the only builder carrying numbers. It must round to
    /// integers, so the dispatch string can never hold text from any other
    /// source.
    #[test]
    fn v5_resize_is_integer_only() {
        let s = v5_resize((1000.4, 640.6));
        assert_eq!(
            s,
            "hl.dsp.window.resize({ x = 1000, y = 641, relative = false, \
             window = \"title:Hyprland Visual Editor\" })"
        );
        assert_window_targeted(&s);
        // No fractional digits survive: the alphabet reaching the dispatch is
        // exactly the fixed literal plus integer digits.
        assert!(!s.contains("1000.4"), "value must be rounded, got: {s}");
        assert!(!s.contains("640.6"), "value must be rounded, got: {s}");
    }

    /// The Settings presentation must be idempotent: `on`/`off`, never
    /// `toggle`, or a lost event inverts the float state instead of being a
    /// no-op.
    #[test]
    fn v5_settings_float_actions_are_not_toggle() {
        assert!(!v5_float_on().contains("toggle"), "float on must be `on`, got: {}", v5_float_on());
        assert!(!v5_float_off().contains("toggle"), "float off must be `off`, got: {}", v5_float_off());
        assert!(v5_float_off().contains("action = \"off\""), "float off spelling");
    }

    #[test]
    fn v5_move_to_workspace_preserves_workspace_param() {
        let s = v5_move_to_workspace("3");
        assert!(s.contains("workspace = \"3\""), "workspace param must be interpolated, got: {s}");
    }
}
