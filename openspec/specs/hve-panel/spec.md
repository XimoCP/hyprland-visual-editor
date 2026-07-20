# hve-panel Specification

## Purpose

Panel replacing tray as primary HVE interaction. Auto-shows at screen edge via `hyprctl` polling. Quick controls: toggle-system, next-anim/border/shader. Shows preset names, settings shortcut. `ui_mode = "tray"` preserves existing behavior.

## Requirements

### R1: Config Migration v3→v4
Config MUST migrate v3→v4 adding `ui_mode` (`"tray"`|`"panel"`), `panel_edge`, `panel_width`. Unknown field rejection remains.

| # | GIVEN | WHEN | THEN |
|---|-------|------|------|
| 1 | v3 config, no panel fields | Load | v4 saved, ui_mode:"tray" |
| 2 | v4 config ui_mode:"panel", panel_edge:"left" | Save & reload | Values preserved |
| 3 | v4 config, unknown field | Load | Error returned |

### R2: Panel Thread Lifecycle
When ui_mode = "panel", HVE MUST spawn a panel thread on startup. ui_mode "tray" → no thread. HVE quit → thread stops, both windows close.

| # | GIVEN | WHEN | THEN |
|---|-------|------|------|
| 1 | ui_mode:"panel" | HVE starts | Thread spawns, polls every 200ms |
| 2 | ui_mode:"tray" | HVE starts | No panel thread, tray operates |
| 3 | Both windows open | HVE quits | Both close, thread joins |
| 4 | Panel thread running | Thread panics | HVE catches, logs, does NOT restart |

### R3: Edge Detection
Panel expands when cursor enters 10px trigger zone at configured edge. Collapses on cursor leaving or focus loss.

| # | GIVEN | WHEN | THEN |
|---|-------|------|------|
| 1 | Panel collapsed | Cursor <10px from edge | Panel expands spring-animated |
| 2 | Panel expanded | Cursor >100px, 500ms idle | Panel collapses animated |
| 3 | Panel expanded | Non-HVE window focused | Panel hides immediately |
| 4 | 2 monitors connected | Cursor reaches edge of either | Panel triggers per-monitor geometry |
| 5 | Panel polling | hyprctl fails | Skip cycle, log, retry |

### R4: Panel UI & Animations
Second Slint window sharing HVE's event loop. Collapsed: 48px wide. Expanded: 300px. Width: spring easing. Opacity: 200ms fade.

| # | GIVEN | WHEN | THEN |
|---|-------|------|------|
| 1 | Panel visible, no trigger | Steady state | Width 48px, thin strip |
| 2 | Trigger zone entered | Transition | Width 48→300px spring, opacity 0→1 |
| 3 | Cursor left zone | Transition | Width 300→48px spring, opacity fades |
| 4 | HVE main + panel open | User interacts | Separate focus, no event loop contention |

### R5: Quick Controls
Panel MUST provide: toggle-system, next-anim, next-border, next-shader, open-settings. Fires same actions as tray.

| # | GIVEN | WHEN | THEN |
|---|-------|------|------|
| 1 | Panel expanded, system active | Click toggle-system | System toggles, indicator updates |
| 2 | Panel expanded | Click next-anim | Animation preset advances |
| 3 | Panel expanded | Click next-border | Border preset advances |
| 4 | Panel expanded | Click next-shader | Shader preset advances |
| 5 | Panel expanded, HVE main hidden | Click settings | HVE main focused on settings tab |

### R6: Preset Indicator
Panel MUST show active animation, border & shader names when expanded. Updates in real time.

| # | GIVEN | WHEN | THEN |
|---|-------|------|------|
| 1 | Engine: anim "bounce", border "rounded", shader "rgb" | Panel expands | Shows "bounce / rounded / rgb" |
| 2 | Panel showing "bounce / rounded / rgb" | Click next-anim | Shows "slide / rounded / rgb" |

### R7: Tray Mode (Backward Compat)
When ui_mode = "tray", HVE behaves identically to pre-panel. Tray, IPC, window lifecycle unchanged.

| # | GIVEN | WHEN | THEN |
|---|-------|------|------|
| 1 | ui_mode:"tray", config v4 | HVE starts | Tray shows, all menu items work |
| 2 | ui_mode:"tray" saved | Config roundtrip | Remains "tray", panel inactive |