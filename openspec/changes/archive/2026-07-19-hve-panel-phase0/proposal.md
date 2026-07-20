# Proposal: hve-panel-phase0

## Intent

Replace tray as primary HVE interaction. Users need persistent quick controls for preset cycling (animations, borders, shaders) and system toggling without the HVE window visible. Tray is limited: hidden in many Hyprland setups, single-click only, no visual status feedback.

## Scope

### In Scope
- Slint panel window (48px collapsed, 300px expanded)
- Quick controls: toggle-system, next-anim, next-border, next-shader
- Active preset indicator (shows current animation/border/shader name)
- Settings shortcut (opens HVE window to settings tab)
- Edge auto-show via `hyprctl cursorpos` + `hyprctl monitors -j`
- Panel thread lifecycle (start/stop coordinated with HVE)
- Config v4 migration: `ui_mode` enum, `panel_edge`, `panel_width`
- `ui_mode` default: `tray` (opt-in to panel, zero breakage)
- Spring-animated width transitions and opacity fade

### Out of Scope
- Layer-shell support (Phase 1)
- Themes/wallpaper controls
- wayle-hyprland dependency
- Drag-and-drop panel customization
- Separate panel process (Phase 1)

## Capabilities

### New Capabilities
- `hve-panel`: Panel window with quick controls, edge auto-show/hide, active preset display, and settings launcher

### Modified Capabilities
None — no existing specs to modify.

## Approach

Thread within HVE (exploration Approach 2). Panel thread polls cursor position every 200ms. Panel is a second Slint window sharing HVE's event loop. Communication via `Arc<AtomicBool>` + `invoke_from_event_loop()` — same pattern as tray.

Edge detection: `hyprctl cursorpos` + `hyprctl monitors -j` via `std::process::Command`. Show panel when cursor within trigger zone. Hide on focus loss or timeout.

Config: v4 migration adds `ui_mode` (`tray`|`panel`), `panel_edge` (`bottom-right`|`left`|`top`|`bottom`), `panel_width` (default 48/300). Old tray behavior preserved when `ui_mode: "tray"`.

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `src/config.rs` | Modified | v3→v4 migration, new fields |
| `src/tray.rs` | Modified | Actions shared with panel, gated by ui_mode |
| `src/main.rs` | Modified | Panel thread spawn, second window init |
| `src/panel/` | New | Panel module: thread, edge detection, UI binding |
| `ui/panel.slint` | New | Panel Slint window with animations |
| `Cargo.toml` | Modified | No new deps for Phase 0 |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Thread crash kills HVE | Low | Minimal logic; catch panics; restart thread |
| Auto-minimize conflicts | Medium | Panel skips countdown; separate focus tracking |
| hyprctl polling overhead | Low | 200ms interval, lightweight syscalls |
| Dual-window lifecycle | Medium | Quit closes both; panel hide ≠ quit |
| Slint second window quirks | Medium | Same event loop; geometry via hyprctl |

## Rollback Plan

Set `ui_mode: "tray"` — panel thread not spawned, tray resumes. Config revert via git. Old binaries ignore unknown config fields on deserialize.

## Dependencies

None — no new crate dependencies for Phase 0.

## Success Criteria

- [ ] Panel shows when cursor reaches configured edge trigger zone
- [ ] Quick controls fire same actions as tray (toggle-system, next-anim, next-border, next-shader)
- [ ] Active preset name visible in expanded state
- [ ] Config migration preserves tray behavior with `ui_mode: "tray"`
- [ ] Panel hides on focus-out or timeout
- [ ] Quit from panel closes both windows cleanly
- [ ] Width animation smooth (spring easing) on collapse/expand

## Delivery Plan

1. **PR #1 — Config**: v4 migration, `ui_mode` enum, `panel_edge`/`panel_width` fields
2. **PR #2 — Panel thread**: Edge detection loop, panel start/stop lifecycle
3. **PR #3 — Panel UI**: Slint window with quick controls, preset indicator, settings shortcut, animations
4. **PR #4 — Integration**: Action bridge (tray→panel), ui_mode gating, `--panel` CLI flag
