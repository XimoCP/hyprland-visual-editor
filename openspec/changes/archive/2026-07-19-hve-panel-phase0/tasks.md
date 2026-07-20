# Tasks: hve-panel-phase0

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~480 (config 60, panel 200, ui 100, wiring 20, tests 100) |
| 400-line budget risk | High |
| Chained PRs recommended | No |
| Suggested split | Single PR (change is tightly coupled) |
| Delivery strategy | exception-ok |
| Chain strategy | pending |

Decision needed before apply: No
Chained PRs recommended: No
Chain strategy: pending
400-line budget risk: High

### Suggested Work Units

| Unit | Goal | Likely PR | Notes |
|------|------|-----------|-------|
| 1 | Config v4 + Panel thread + UI + wiring | PR 1 | Single PR; all parts depend on each other |

**Gate review resolutions incorporated:**
1. **BottomRight → Right**: `PanelEdge::BottomRight` renamed to `PanelEdge::Right`. Detection formula for right edge is unambiguous: `(monitor.x + width) - cursor.x < 10`.
2. **Focus loss**: Uses existing `hypr_ipc::spawn_focus_listener()` + `activewindow` event (already wired for countdown). Panel gets its own listener or hooks into the existing countdown `on_focus_lost` callback to set `PANEL_VISIBLE=false` immediately.
3. **Window positioning**: `hyprctl monitors -j` per poll cycle → get trigger monitor geometry → `slint::Window::set_position(x, y)` for edge placement.
4. **AtomicBool flags** (explicit):
   - `SHUTDOWN` — stop thread loop
   - `PANEL_VISIBLE` — Slint window show/hide
   - `PANEL_EXPANDED` — collapsed vs expanded width
5. **Monitor hotplug**: Re-read `hyprctl monitors -j` on EVERY poll cycle (200ms). No caching. If monitors changed between cycles, detection recalculates naturally.

## Phase 1: Config Foundation

- [x] 1.1 Add `UiMode`, `PanelEdge` (rename `BottomRight` → `Right`), `PanelWidth` to `src/config.rs`. Default `ui_mode: UiMode::Tray`, `panel_edge: PanelEdge::Right`, `panel_width: PanelWidth { collapsed: 48, expanded: 300 }`. Bump `CONFIG_VERSION` to 4.
- [x] 1.2 Add v3→v4 migration step in `migrate()`: set defaults for new fields, increment version.
- [x] 1.3 Add test: v3 config saved → reloaded as v4 with `ui_mode: "tray"`. Add test: v4 config roundtrip preserves `panel_edge` and `ui_mode`.

## Phase 2: Panel Module & Thread

- [x] 2.1 Create `src/panel/mod.rs` — re-export `thread::PanelHandle`, `thread::run`.
- [x] 2.2 Create `src/panel/thread.rs` — `PanelHandle { shutdown: Arc<AtomicBool>, thread: Option<JoinHandle> }` with `Drop` impl. `run()` function: poll loop (200ms), edge detection, `catch_unwind` for panic safety.
- [x] 2.3 Define AtomicBool flags in thread scope: `SHUTDOWN`, `PANEL_VISIBLE`, `PANEL_EXPANDED`. Expose as `Arc<AtomicBool>`.
- [x] 2.4 Implement `is_in_trigger_zone(cursor: (i32,i32), monitors: &[Monitor], edge: PanelEdge) -> bool` as pure function in `src/panel/thread.rs`. Test with: each edge, both monitors, out of zone.
- [x] 2.5 Edge detection loop: per cycle, re-read `hyprctl monitors -j` (hotplug-safe) + `hyprctl cursorpos`. Parse, run `is_in_trigger_zone()`, set `PANEL_EXPANDED`. If monitor list changes between cycles, detection adapts naturally.
- [x] 2.6 Wire focus loss: on `activewindow` event → `PANEL_VISIBLE = false` → `invoke_from_event_loop` to hide panel. Use existing `spawn_focus_listener()` pattern or shared callback.

## Phase 3: Panel UI

- [x] 3.1 Create `ui/panel.slint` — `PanelWindow` inherits `Window`. Properties: `panel-expanded` (bool), `active-anim-name`, `active-border-name`, `active-shader-name` (string). Width spring-animated 48↔300. Opacity 200ms fade. Callbacks: `toggle-system`, `next-anim`, `next-border`, `next-shader`, `open-settings`.
- [x] 3.2 Create `src/panel/ui.rs` — `PanelWindow::new()` binding. Wire `invoke_from_event_loop` to set Slint properties from thread flags.
- [x] 3.3 Callback wiring: map panel button clicks to `invoke_toggle_system(next_active)`, `invoke_apply_animation(next, file)`, etc. Reuse same `slint::Weak<crate::MainWindow>` pattern from tray.
- [x] 3.4 Window positioning: in `src/panel/ui.rs`, after show(), call `hyprctl monitors -j` to get trigger monitor geometry, then `slint::Window::set_position(x, y)` to place panel at the right edge of that monitor.

## Phase 4: Integration & Spawn

- [x] 4.1 In `main.rs`: add `mod panel;` and `pub mod panel;`. After config load, if `ui_mode == Panel`, call `panel::thread::run()` with config values and `window.as_weak()`.
- [x] 4.2 Connect panel `toggle-system` callback to update tray's `system_active` atomic so tray icon stays in sync. Extract shared action dispatch between tray and panel.
- [x] 4.3 Verify lifecycle: HVE quit → `PanelHandle` Drop → `SHUTDOWN=true` → thread join. Both windows close. Test with `catch_unwind` for panic path (spec R2.4).
- [x] 4.4 Existing tray tests pass with v4 config (`ui_mode: "tray"`). Verify backward compat: tray menu, IPC commands, window lifecycle unchanged.

## Dependencies

```
Phase 1 (Config) → Phase 2 (Thread) → Phase 3 (UI) → Phase 4 (Integration)
```
Phase 1 has no dependencies. Phase 2 depends on config types from Phase 1. Phase 3 depends on thread flags from Phase 2. Phase 4 integrates all previous phases.

## Ready for Apply

Yes — all gate review issues resolved, single PR with exception-ok on the 400-line budget (user stated 1000-line budget).
