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
        let out = match std::process::Command::new("hyprctl")
            .args(["clients", "-j"])
            .output() {
            Ok(o) => o,
            Err(_) => return false,
        };
        let text = match String::from_utf8(out.stdout) {
            Ok(t) => t,
            Err(_) => return false,
        };
        let v: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => return false,
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
                    let s = format!("hl.dsp.focus({{ window = \"title:{HVE_TITLE}\" }})");
                    let _ = hypr_dispatch_v5_standalone(&s);
                }
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
}

impl Composer for HyprlandComposer {
    fn hide(&self, win: &crate::MainWindow) -> bool {
        match self.hypr_mode() {
            HyprMode::V5 => {
                // Apuntar el move EXPLÍCITAMENTE a HVE por título. Sin `window=`,
                // `hl.dsp.window.move` actúa sobre la ventana con foco: si el
                // usuario está clickeando otra ventana en el mismo instante del
                // SUPER+H, el move secuestra ESA ventana y la manda al special.
                let s1 = format!(
                    "hl.dsp.window.move({{ window = \"title:{HVE_TITLE}\", workspace = \"special:{SPECIAL}\" }})"
                );
                let did_move = self.hypr_dispatch_v5(&s1);
                if did_move {
                    // Mover al special lo "abre" como overlay visible. Hay que cerrar
                    // el scratchpad tras mover para que la ventana quede oculta, no
                    // flotando encima del workspace activo (era la regresión de "no
                    // minimiza").
                    let s2 = format!("hl.dsp.workspace.toggle_special(\"{SPECIAL}\")");
                    let _ = self.hypr_dispatch_v5(&s2);
                    true
                } else {
                    // Si el move por lua falló, intentamos mover con la sintaxis
                    // clásica (comunmente "special:minimized" acepta move).
                    // Enfocar HVE primero como en show(): `movetoworkspacesilent`
                    // actúa sobre la ventana con foco y no debe atrapar otra.
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
                // Enfocar HVE primero: `movetoworkspacesilent` actúa sobre la
                // ventana con foco. Sin esto, un click simultáneo del ratón en
                // otra ventana la atraparía al moverla al special.
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
                        let _ = win.window().show();
                        return false;
                    }
                    // 1) Enfocar HVE PRIMERO. Imprescindible: `movetoworkspacesilent`
                    //    y `hl.dsp.window.move` sin `window=` actúan sobre la ventana
                    //    con foco. Como HVE estaba en el special scratchpad sin foco,
                    //    el move movía OTRA ventana e HVE quedaba atrapada. Al enfocar
                    //    primero, el move posterior la saca del special al workspace.
                    let s_focus = format!("hl.dsp.focus({{ window = \"title:{HVE_TITLE}\" }})");
                    let did_focus = self.hypr_dispatch_v5(&s_focus);
                    // 2) Devolverla al workspace real (el move actúa sobre la focada).
                    let s_move = format!("hl.dsp.window.move({{ workspace = \"{target}\" }})");
                    let did_move = self.hypr_dispatch_v5(&s_move);
                    if !(did_focus && did_move) {
                        let _ = win.window().show();
                        return false;
                    }
                    // 3) Re-afirmar foco tras el roundtrip de Wayland (ver
                    //    schedule_focus_reassert: re-dispatch + WindowActiveChanged).
                    self.schedule_focus_reassert(win);
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
                    let _ = win.window().show();
                    self.activate_focus_immediately(win);
                    self.schedule_focus_reassert(win);
                    false
                }
            }
            HyprMode::V4 => {
                if self.hve_in_special() {
                    let target = self
                        .active_workspace()
                        .or_else(|| prev_workspace.map(|s| s.to_string()))
                        .unwrap_or_else(|| "1".to_string());
                    // SECURITY: same allowlist as the v5 path.
                    if !safe_workspace_target(&target) {
                        let _ = win.window().show();
                        return false;
                    }
                    // 1) Enfocar HVE primero (movetoworkspacesilent actúa sobre la focada).
                    let did_focus = self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
                    // 2) Devolverla al workspace real.
                    let did_move =
                        self.hypr_dispatch_v4(&["movetoworkspacesilent", &target]);
                    if !(did_focus && did_move) {
                        let _ = win.window().show();
                        return false;
                    }
                    // 3) Re-afirmar foco de teclado tras el roundtrip (mismo
                    //    motivo que en V5: re-activar Slint para que la FocusScope
                    //    raíz re-gane su focus_item y la navegación no quede sorda).
                    self.schedule_focus_reassert(win);
                    true
                } else {
                    // Ventana NO estaba en el special (p.ej. cerrada con SUPER+C).
                    // Re-mostrar + despertar inmediato de FocusScope + Timer de
                    // 150ms como safety net, igual que en V5.
                    let _ = win.window().show();
                    self.activate_focus_immediately(win);
                    self.schedule_focus_reassert(win);
                    false
                }
            }
            HyprMode::None => {
                // Sin compositor Hyprland disponible. Solo mostrar y despertar
                // la FocusScope de Slint de inmediato.
                let _ = win.window().show();
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
                let s = format!("hl.dsp.focus({{ window = \"title:{HVE_TITLE}\" }})");
                let _ = self.hypr_dispatch_v5(&s);
            }
            HyprMode::V4 => {
                let _ = self.hypr_dispatch_v4(&["focuswindow", HVE_TITLE]);
            }
            HyprMode::None => {}
        }
    }

    fn toggle_float(&self) {
        match self.hypr_mode() {
            HyprMode::V5 => {
                let s = format!("hl.dsp.window.float({{ action = \"toggle\", window = \"title:{HVE_TITLE}\" }})");
                let _ = self.hypr_dispatch_v5(&s);
            }
            HyprMode::V4 => {
                let _ = self.hypr_dispatch_v4(&["togglefloating", HVE_TITLE]);
            }
            HyprMode::None => {}
        }
    }

    fn active_workspace(&self) -> Option<String> {
        self.active_workspace()
    }
}

const HVE_TITLE: &str = "Hyprland Visual Editor";
const SPECIAL: &str = "minimized";

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
