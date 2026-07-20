# Design: hve-panel-phase0

## Technical Approach

Thread within HVE (exploration Approach 2). Panel polls `hyprctl cursorpos` + `monitors -j` every 200ms. Second Slint window sharing HVE's event loop. Communication: `Arc<AtomicBool>` + `invoke_from_event_loop()` — same pattern as tray→window callbacks. Config v4 migration adds `ui_mode`, `panel_edge`, `panel_width`. Default `ui_mode: "tray"` → no thread spawned.

## Architecture Decisions

| Decision | Choice | Alternatives | Rationale |
|----------|--------|-------------|-----------|
| Process model | Thread within HVE | Separate binary (Phase 1), sidebar | Shares engine/config/callbacks. No IPC needed. Phase 1 extraction safe. |
| Edge detection | `hyprctl` polling | wayle-hyprland events | Avoids tokio conflict. 200ms is lightweight. Phase 1 can migrate. |
| IPC method | AtomicBool + invoke | mpsc channel | Matches existing tray pattern. State is boolean — no complex msgs. |
| Event loop | Single loop, 2 windows | Separate loop thread | Slint 1.17 supports multi-window. Avoids thread-safety issues. |

## Data Flow

```
main.rs ──spawn──→ panel::thread (poll 200ms)
                    ├── hyprctl cursorpos + monitors -j
                    ├── threshold check → AtomicBool flags
                    └── invoke_from_event_loop()
                         └── PanelWindow (Slint)
                              ├── width anim (48↔300 spring)
                              ├── quick controls → invoke_toggle_system etc.
                              └── callbacks (reuse tray actions)
```

## Thread Model

```rust
pub struct PanelHandle { shutdown: Arc<AtomicBool>, thread: Option<JoinHandle<()>> }
impl Drop for PanelHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}
```

- Spawn when `ui_mode == "panel"` after config load
- Shutdown via Drop → atomic flag → thread join
- Panic: `catch_unwind`, log, do NOT restart (spec R2.4)

## Edge Detection

1. `hyprctl cursorpos` → `"x, y"`; `hyprctl monitors -j` → JSON array
2. Per monitor, distance to configured `panel_edge`:
   - `left`: `cx - mx` < 10 | `right`: `(mx + mw) - cx` < 10
   - `top`: `cy - my` < 10 | `bottom`: `(my + mh) - cy` < 10
3. In zone → expand (48→300 spring). >100px + 500ms idle → collapse (300→48)
4. Focus loss via `activewindow` event → hide immediately
5. hyprctl fail → skip cycle, log, retry

## Panel UI

**New**: `ui/panel.slint` — PanelWindow with animated width/opacity, quick control buttons, preset name labels. **New**: `src/panel/ui.rs` — binding, callback wiring to invoke_toggle_system/invoke_apply_animation etc.

## Config Changes (v3→v4)

```rust
enum UiMode { Tray, Panel }         // default: Tray
enum PanelEdge { BottomRight, Left, Top, Bottom }  // default: BottomRight
struct PanelWidth { collapsed: 48, expanded: 300 }

// Migration step
if cfg.config_version < 4 {
    cfg.ui_mode = UiMode::Tray; cfg.panel_edge = PanelEdge::BottomRight;
    cfg.panel_width = PanelWidth { collapsed: 48, expanded: 300 };
    cfg.config_version = 4;
}
```

## File Changes

| File | Action | Description |
|------|--------|-------------|
| `src/config.rs` | Modify | New enums/structs, v3→v4 migration, CONFIG_VERSION→4 |
| `src/main.rs` | Modify | `mod panel`, spawn PanelHandle, pass weak refs |
| `src/panel/mod.rs` | Create | Module root |
| `src/panel/thread.rs` | Create | Poll loop, edge detection, shutdown |
| `src/panel/ui.rs` | Create | PanelWindow binding, callback wiring |
| `ui/panel.slint` | Create | Panel window with spring width anim |
| `src/tray.rs` | Modify | Actions gated at spawn, no structural change |

## Testing Strategy

| Layer | What | Approach |
|-------|------|----------|
| Unit | Config migration v3→v4 | TempEnv + JSON roundtrip |
| Unit | Edge threshold | Pure fn: `is_in_trigger_zone(cursor, monitors, edge) → bool` |
| Unit | Thread lifecycle | Shutdown atomic signal, mock hyprctl fail |
| Integration | Slint window | PanelWindow create+show, property roundtrip |
| Manual | Full flow | Run with ui_mode="panel", visual verification |

## Risks

| Risk | Mitigation |
|------|------------|
| Thread panic kills HVE | catch_unwind, log, no restart |
| hyprctl latency | 200ms tolerates occasional delay |
| Panel focus war | Separate PANEL_VISIBLE flag, skip countdown |
| Two-window Slint | Same event loop, test on Hyprland |
| Config deny_unknown | Already handled — defaults on parse error |

## Open Questions

- [ ] Panel window geometry: position at edge of trigger monitor via `hyprctl` or Slint window.set? API exploration needed.
- [ ] Do we need a `--panel` CLI flag to override ui_mode, or is config-only sufficient?
