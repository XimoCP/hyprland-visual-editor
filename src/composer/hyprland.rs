use crate::composer::{Composer, HyprMode};
use slint::ComponentHandle;
use std::sync::OnceLock;
use std::time::Duration;
use slint::Timer;

/// Deferred keyboard-focus re-assertion delay (ms) — the "mouse fixes it" bug.
const FOCUS_REASSERT_DELAY_MS: u64 = 150;

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
        let out = std::process::Command::new("hyprctl")
            .args(["clients", "-j"])
            .output()
            .ok()?;
        String::from_utf8(out.stdout).ok()
    }

    fn active_workspace(&self) -> Option<String> {
        let out = std::process::Command::new("hyprctl")
            .args(["activeworkspace", "-j"])
            .output()
            .ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        v.get("name").and_then(|n| n.as_str()).map(|s| s.to_string())
    }

    /// Re-afirma el foco de teclado tras un roundtrip de Wayland, con un
    /// retardo corto para que la ventana termine de re-mapearse.
    ///
    /// Hace DOS cosas: (a) re-dispatch de foco a Hyprland, y (b) re-emitir
    /// `WindowActiveChanged(true)` a Slint. Esta segunda parte es la
    /// decisiva: cuando la ventana vuelve a mostrarse sin pasar por un
    /// hide/show clásico (especial→real), Slint nunca recibe el evento de
    /// "ventana activa" y la FocusScope raíz no re-gana su `focus_item` —
    /// `key-pressed` queda sordo, exactamente lo que el usuario arreglaba
    /// con un click/hover del ratón. Al forzar este evento, Slint re-dispara
    /// FocusIn(WaylandActivation) y la navegación por teclado vuelve a
    /// funcionar sin tocar nada.
    ///
    /// Se usa en TODOS los caminos de `show()`: el rápido (ventana estaba en
    /// el special scratchpad) y el lento (ventana cerrada con SUPER+C, que
    /// no pasa por el special y por eso sufría el mismo bug del teclado mudo).
    fn schedule_focus_reassert(&self, win: &crate::MainWindow) {
        let mode = self.hypr_mode();
        let win_weak = win.as_weak();
        Timer::single_shot(Duration::from_millis(FOCUS_REASSERT_DELAY_MS), move || {
            // (a) Re-dispatch de foco a Hyprland, según versión detectada.
            match mode {
                HyprMode::V5 => {
                    let s = v5_focus();
                    let _ = hypr_dispatch_v5_standalone(&s);
                }
                // V4/conf fallback: no window targeting — focus by title is
                // the selector; keep this branch and its comment.
                HyprMode::V4 => {
                    let _ = hypr_dispatch_v4_standalone(&["focuswindow", HVE_TITLE]);
                }
                HyprMode::None => {}
            }
            // (b) Re-emitir activación a Slint para despertar la FocusScope.
            if let Some(win) = win_weak.upgrade() {
                use slint::platform::WindowEvent;
                win.window().dispatch_event(WindowEvent::WindowActiveChanged(true));
                win.window().request_redraw();
            }
        });
    }

    /// Despierta la FocusScope de Slint de forma INMEDIATA (síncrona).
    ///
    /// Esto es CRÍTICO para el camino lento de `show()` (SUPER+C → SUPER+H):
    /// cuando la superficie Wayland se re-crea, Slint no recibe
    /// `WindowActiveChanged` automáticamente. Sin este dispatch, la FocusScope
    /// raíz queda con `focus_item == None` y los eventos de teclado se
    /// rechazan inmediatamente. El Timer de 150ms de `schedule_focus_reassert`
    /// es un safety net para el foco de Hyprland, pero no llega a tiempo: la
    /// FocusScope necesita despertarse YA, en el mismo turno síncrono que
    /// `show()`.
    fn activate_focus_immediately(&self, win: &crate::MainWindow) {
        use slint::platform::WindowEvent;
        win.window().dispatch_event(WindowEvent::WindowActiveChanged(true));
        win.window().request_redraw();
    }

    fn show_and_sync(&self, win: &crate::MainWindow) {
        let _ = win.window().show();
        crate::shell::Shell::sync_global_after_show();
    }

    fn sync_after_show(&self) {
        crate::shell::Shell::sync_global_after_show();
    }
}

impl Composer for HyprlandComposer {
    fn hide(&self, win: &crate::MainWindow) -> bool {
        match self.hypr_mode() {
            HyprMode::V5 => {
                // Apuntar el move EXPLÍCITAMENTE a HVE por título. Sin `window=`,
                // `hl.dsp.window.move` actúa sobre la ventana con foco: si el
                // usuario está clickeando otra ventana en el mismo instante del
                // SUPER+H, el move secuestra ESA ventana y la manda al special.
                let s1 = v5_move_to_special();
                let did_move = self.hypr_dispatch_v5(&s1);
                if did_move {
                    // Mover al special lo "abre" como overlay visible. Hay que cerrar
                    // el scratchpad tras mover para que la ventana quede oculta, no
                    // flotando encima del workspace activo (era la regresión de "no
                    // minimiza").
                    let s2 = v5_toggle_special();
                    let _ = self.hypr_dispatch_v5(&s2);
                    true
                } else {
                    // Si el move por lua falló, intentamos mover con la sintaxis
                    // clásica (comunmente "special:minimized" acepta move).
                    // V4/conf fallback: Hyprland V4 dispatch has NO window targeting
                    // — must focus-by-title first; this pattern is required and
                    // forbidden to remove (movetoworkspacesilent acts on focused window).
                    let did_focus = self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
                    let did_move4 =
                        self.hypr_dispatch_v4(&["movetoworkspacesilent", &format!("special:{SPECIAL}")]);
                    if did_focus && did_move4 {
                        let _ = self.hypr_dispatch_v4(&["togglespecialworkspace", SPECIAL]);
                        true
                    } else {
                        let _ = win.window().hide();
                        false
                    }
                }
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
                    // Workspace donde devolver HVE: el activo o el previo.
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
                    // 1) Enfocar HVE PRIMERO. Imprescindible: `movetoworkspacesilent`
                    //    y `hl.dsp.window.move` sin `window=` actúan sobre la ventana
                    //    con foco. Como HVE estaba en el special scratchpad sin foco,
                    //    el move movía OTRA ventana e HVE quedaba atrapada. Al enfocar
                    //    primero, el move posterior la saca del special al workspace.
                    let s_focus = v5_focus();
                    let did_focus = self.hypr_dispatch_v5(&s_focus);
                    // 2) Devolverla al workspace real (el move actúa sobre la focada).
                    let s_move = v5_move_to_workspace(&target);
                    let did_move = self.hypr_dispatch_v5(&s_move);
                    if !(did_focus && did_move) {
                        self.show_and_sync(win);
                        return false;
                    }
                    // 3) Re-afirmar foco tras el roundtrip de Wayland (ver
                    //    schedule_focus_reassert: re-dispatch + WindowActiveChanged).
                    self.schedule_focus_reassert(win);
                    self.sync_after_show();
                    true
                } else {
                    // Ventana NO estaba en el special (p.ej. cerrada con SUPER+C,
                    // que oculta vía Slint sin pasar por el scratchpad). Al
                    // re-mostrarla, Slint re-mapea la superficie y la FocusScope
                    // puede quedar sorda — mismo bug del teclado mudo.
                    //
                    // FIX: despertar la FocusScope de forma INMEDIATA (antes de que
                    // el usuario pueda presionar una tecla) y también arrancar el
                    // Timer de 150ms que re-dispatcha foco a Hyprland como safety net.
                    self.show_and_sync(win);
                    self.activate_focus_immediately(win);
                    self.schedule_focus_reassert(win);
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
                    // 1) Enfocar HVE primero (movetoworkspacesilent acts on focused window — V4 has no window selector).
                    let did_focus = self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
                    // 2) Devolverla al workspace real.
                    let did_move =
                        self.hypr_dispatch_v4(&["movetoworkspacesilent", &target]);
                    if !(did_focus && did_move) {
                        self.show_and_sync(win);
                        return false;
                    }
                    // 3) Re-afirmar foco de teclado tras el roundtrip (mismo
                    //    motivo que en V5: re-activar Slint para que la FocusScope
                    //    raíz re-gane su focus_item y la navegación no quede sorda).
                    self.schedule_focus_reassert(win);
                    self.sync_after_show();
                    true
                } else {
                    // Ventana NO estaba en el special (p.ej. cerrada con SUPER+C).
                    // Re-mostrar + despertar inmediato de FocusScope + Timer de
                    // 150ms como safety net, igual que en V5.
                    self.show_and_sync(win);
                    self.activate_focus_immediately(win);
                    self.schedule_focus_reassert(win);
                    false
                }
            }
            HyprMode::None => {
                // Sin compositor Hyprland disponible. Solo mostrar y despertar
                // la FocusScope de Slint de inmediato.
                self.show_and_sync(win);
                self.activate_focus_immediately(win);
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
                did_focus && self.hypr_dispatch_v5(&s_fs)
            }
            // V4/conf fallback: no window targeting — must focus-by-title
            // first; this focus-then-act pattern is required and forbidden
            // to remove.
            HyprMode::V4 => {
                let did_focus = self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
                let mode = if on { "1" } else { "0" };
                did_focus && self.hypr_dispatch_v4(&["fullscreen", mode])
            }
            HyprMode::None => false,
        }
    }

    fn active_workspace(&self) -> Option<String> {
        self.active_workspace()
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

/// Standalone dispatch — used by Timer closures that can't capture &self.
fn hypr_dispatch_v5_standalone(script: &str) -> bool {
    std::process::Command::new("hyprctl")
        .args(["dispatch", script])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("ok"))
        .unwrap_or(false)
}

fn hypr_dispatch_v4_standalone(args: &[&str]) -> bool {
    std::process::Command::new("hyprctl")
        .arg("dispatch")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("ok"))
        .unwrap_or(false)
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
    fn sanity_healthy_or_absent_hve_needs_nothing() {
        let healthy = parse_stuck_state(
            r#"[{"title": "Hyprland Visual Editor", "fullscreen": 0, "fullscreenClient": 0, "floating": true}]"#,
        );
        assert!(healthy.present && !healthy.needs_repair());

        let absent = parse_stuck_state(r#"[]"#);
        assert!(!absent.present && !absent.needs_repair());

        let garbage = parse_stuck_state("not json");
        assert!(!garbage.present && !garbage.needs_repair());
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
    fn v5_move_to_workspace_preserves_workspace_param() {
        let s = v5_move_to_workspace("3");
        assert!(s.contains("workspace = \"3\""), "workspace param must be interpolated, got: {s}");
    }
}
