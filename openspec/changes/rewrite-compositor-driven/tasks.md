# Tasks: Rewrite Compositor-Driven

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~250–350 |
| 400-line budget risk | Medium |
| Chained PRs recommended | No |
| Suggested split | Single PR (pure refactor, additive + move) |
| Delivery strategy | exception-ok |
| Chain strategy | pending |

Decision needed before apply: No
Chained PRs recommended: No
Chain strategy: pending
400-line budget risk: Medium

## Phase 1: Foundation (`src/composer/mod.rs`)

- [ ] 1.1 Crear `src/composer/mod.rs`: definir `Composer` trait (`Send + Sync`) con métodos `hide(&self, win: &slint::Window) -> bool`, `show(&self, win: &slint::Window) -> bool`, `focus(&self)`, `toggle_float(&self)`. Sin tipos de Hyprland.
- [ ] 1.2 Definir `enum HyprMode { V5, V4, None }` privado (mismo archivo, no público).
- [ ] 1.3 Definir struct `Controller { window_hidden: bool, tray_mode: bool, prev_workspace: Option<String>, composer: Box<dyn Composer> }` con `new()`, `toggle_tray()`, getters/setters, `composer()`. Migrar `TRAY_MODE` (main.rs:1434,1439) a `controller.tray_mode()`.
- [ ] 1.4 Crear `src/composer/hyprland.rs` (módulo vacío con `use crate::composer::Composer;` — el resto viene en Phase 2).
- [ ] 1.5 Actualizar `src/main.rs`: `mod composer;` en los mods del top.

## Phase 2: Core (`src/composer/hyprland.rs`)

- [ ] 2.1 Crear struct `HyprlandComposer { hypr_mode: OnceLock<HyprMode> }` — `OnceLock` para V4/V5 detection, NO global.
- [ ] 2.2 Implementar `hypr_mode()` en `HyprlandComposer` — V5 (`hl.dsp.no_op()` → "ok") → V4 (`hyprctl version`) → None. `get_or_init` una sola vez.
- [ ] 2.3 Implementar `hypr_dispatch_v5(script: &str)` y `hypr_dispatch_v4(args: &[&str])` — spawner ÚNICO de `hyprctl dispatch`.
- [ ] 2.4 Implementar `hve_in_special()` — parse JSON `hyprctl clients -j`, chequear title + workspace special.
- [ ] 2.5 Implementar `active_workspace()` — parse JSON `hyprctl activeworkspace -j`, extraer `name`.
- [ ] 2.6 Implementar `impl Composer for HyprlandComposer`: método `hide()` con V4/V5 branch (move to special:minimized + toggle_special / fallback hide()).
- [ ] 2.7 Implementar `impl Composer for HyprlandComposer`: método `show()` con V4/V5 branch (focus → move_to_workspace → Timer 150ms → focus + WindowActiveChanged), retornar bool fast path.
- [ ] 2.8 Implementar `impl Composer for HyprlandComposer`: `focus()` (V5 hl.dsp.focus / V4 focuswindow), `toggle_float()` (V5 hl.dsp.window.float / V4 togglefloating).

## Phase 3: Integration / Wiring

- [ ] 3.1 En `src/main.rs`: crear `Controller` en composition root con `HyprlandComposer` real. Reemplazar `ipc::toggle_float_hve()` y `ipc::focus_hve_window()` calls por `controller.composer().toggle_float()` / `.focus()`.
- [ ] 3.2 En `src/main.rs`: reemplazar `ipc::WINDOW_HIDDEN.store()` y `ipc::TRAY_MODE.store()` por `controller.set_window_hidden()` / `controller.window_hidden()`. Quitar `ipc::` en startup focus/floating calls.
- [ ] 3.3 En `src/ipc.rs`: convertir en transport delgado — `cmd_toggle_tray` delega a `controller.toggle_tray(win)`, lee `controller.window_hidden()`. Quitar `WINDOW_HIDDEN`/`TRAY_MODE` atomics.
- [ ] 3.4 En `src/ipc.rs`: mantener IPC server, debouncer `LAST_TOGGLE` local (Mutex<Instant>), `get_next_index`, `format_response` — sin cambios de lógica.
- [ ] 3.5 En `src/tray.rs`: reemplazar `crate::ipc::WINDOW_HIDDEN.load()` por lectura del Controller (pasar weak + callback o restructure menu para delegar a controller).
- [ ] 3.6 En `src/tray.rs`: actualizar tray menu para que hide/show delegate al controller y actualice su estado.
- [ ] 3.7 En `src/countdown.rs`: reemplazar las 4 referencias a `crate::ipc::WINDOW_HIDDEN` (líneas 26, 88, 158, 261) por acceso al state del Controller. CRÍTICO — sin esto no compila.

## Phase 4: Testing

- [ ] 4.1 Crear `FakeComposer` en tests (mismo archivo `src/composer/mod.rs` o `src/composer/hyprland.rs` bajo `#[cfg(test)]`): graba calls ordenados en `Vec<String>` o `Vec<Action>`.
- [ ] 4.2 Test hide contract: `Controller<FakeComposer>` → `toggle_tray()` → assert sequence `(record_workspace, move_to_special, close_special)`.
- [ ] 4.3 Test show contract (fast path): `Controller<FakeComposer>` → `toggle_tray()` desde hidden → assert `(focus, move_to_workspace)` + timer fire `(focus, WindowActiveChanged)`.
- [ ] 4.4 Test controller state: toggle_tray cuando hidden=false → show path; verificar `window_hidden` flip.
- [ ] 4.5 Test show slow path (hve no en special): assert solo `show()` sin focus/move.
- [ ] 4.6 Mantener 151 tests existentes verdes (verificar con `cargo test` al final).
- [ ] 4.7 Grep guard: `grep -r "hyprctl" src/` → solo `src/composer/hyprland.rs` (no main, no ipc, no tray).

## Phase 5: Cleanup

- [ ] 5.1 Eliminar de `src/ipc.rs`: enum `HyprMode`, `HYPR_MODE` OnceLock, `HYPR_TITLE`, `SPECIAL`, `PREV_WORKSPACE`, funciones `hypr_mode`, `hypr_dispatch_v5/v4`, `hve_in_special`, `focus_hve_window`, `toggle_float_hve`, `hide_window`, `show_window`, y los atomics `WINDOW_HIDDEN`/`TRAY_MODE` + imports `std::sync::atomic` (solo tras migrar countdown/tray/main en Phase 3).
- [ ] 5.2 Eliminar de `src/ipc.rs`: imports muertos (`serde_json`, `slint::platform::WindowEvent`).
- [ ] 5.3 Eliminar de `src/tray.rs`: referencia directa a `ipc::WINDOW_HIDDEN`.
- [ ] 5.4 Documentar módulo `composer` con doc comments en trait y Controller.
