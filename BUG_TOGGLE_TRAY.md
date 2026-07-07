# Bug: toggle-tray requires 3 presses on Wayland

## Project

**HVE** (Hyprland Visual Editor) — A system tray app written in **Rust + Slint 1.17** (winit backend, Wayland). The app runs as a tray icon and toggles its window via a global keyboard shortcut (`SUPER+H`).

## Bug

When the app starts in `--tray` mode, the user must press `SUPER+H` **3 times** before the window appears. After that, normal toggling (single press = show, single press = hide) works fine until the app is restarted.

This only happens on **Wayland** (Hyprland compositor). The 3-press sequence is:

1. `SUPER+H` → nothing happens (window doesn't appear)
2. `SUPER+H` → nothing happens (window still doesn't appear)
3. `SUPER+H` → window finally appears and works normally

## Architecture

- The app starts a **Unix socket IPC server** on a background thread
- The global shortcut triggers a **`toggle-tray`** command sent to the IPC socket
- `cmd_toggle_tray` calls `invoke_on_main()` which uses `slint::invoke_from_event_loop()` to run a closure on the Slint event loop thread
- The closure checks `win.window().is_visible()`:
  - If visible → calls `hide()` (destroys xdg_toplevel on Wayland)
  - If not visible → calls `show()` (creates new xdg_toplevel)
- Slint uses `run_event_loop_until_quit()` so `hide()` doesn't kill the process

## Root Cause Analysis

### What we know for sure:

1. **`is_visible()` is unreliable.** In Slint, `win.window().is_visible()` is an internal software flag set by `show()`/`hide()`. It does NOT reflect the actual compositor state. When the first `show()` "fails" (compositor never maps the window), `is_visible()` still returns `true`. This causes the toggle logic to misbehave.

2. **Wayland xdg_toplevel roundtrip is async.** When `show()` is called, Slint/winit:
   - Creates an xdg_surface + xdg_toplevel
   - The compositor sends a `configure` event with dimensions
   - The client must `ack_configure` and commit a **buffer** (render a frame)
   - Only after the buffer is committed does the compositor map the window

3. **First `show()` after idle is slow.** The first `show()` after startup (or after a long idle period) requires initializing the GPU rendering context, allocating framebuffers, compiling shaders, and rendering the first frame. If this takes too long, the compositor may discard the surface.

### The 3-press sequence explained:

- **Press 1:** `show()` → xdg_toplevel negotiation starts → renderer cold → buffer commit takes too long → compositor drops the surface → window never appears → `is_visible()` = `true` (lying)
- **Press 2:** `is_visible()` = `true` → `hide()` → destroys the failed xdg_toplevel → `is_visible()` = `false`
- **Press 3:** `show()` → renderer now warm (primed by the failed attempt) → buffer commits fast → compositor accepts it → window appears ✓

## Attempts

### Attempt 1: External script (workaround)

`hve-first-toggle.sh` — external script that sent 2 toggles on startup to "prime" the Wayland connection.

**Result:** Race condition on cold boot. Window appeared but was sometimes stale. Rejected as non-idiomatic.

### Attempt 2: Internal minimize via winit

`window.show()` at startup + `Timer::single_shot(300ms)` → `winit_window.set_minimized(true)`.

**Result:** `set_minimized(true)` works to minimize, but `set_minimized(false)` (un-minimize) is **not supported on Wayland** by design. The compositor is the only one that can restore a minimized window. So after startup minimize, `SUPER+H` couldn't restore the window (required 2 presses: first hide destroys the toplevel, second show recreates it). Went from 3 presses to 2 — improvement but not a fix.

### Attempt 3: Lazy load + IPC debounce (CURRENT)

No `window.show()` at startup. The first `SUPER+H` triggers `show()` directly, with a **400ms debounce** in `cmd_toggle_tray` to prevent rapid toggles from sabotaging the roundtrip.

**Result:** Still requires 3 presses. The debounce prevents rapid toggles (within 400ms), but the user is pressing slowly (waiting ~1s between presses). The real problem is that the first `show()` fails silently (cold renderer), and `is_visible()` lies about it.

## Current State (commit 4dd241e)

### `src/main.rs` — tray_mode block:
```rust
if tray_mode {
    ipc::WINDOW_HIDDEN.store(true, std::sync::atomic::Ordering::Relaxed);
    ipc::TRAY_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
    tracing::info!("Starting in tray mode (lazy load...)");
}
```

### `src/ipc.rs` — `cmd_toggle_tray`:
```rust
fn cmd_toggle_tray(window: &slint::Weak<crate::MainWindow>) -> String {
    // Debounce: ignore toggles within 400ms
    {
        let mut last = LAST_TOGGLE.lock().unwrap();
        let now = Instant::now();
        if let Some(prev) = *last {
            if now.duration_since(prev).as_millis() < 400 {
                return "debounced\n".to_string();
            }
        }
        *last = Some(now);
    }

    format_response(invoke_on_main(window, |win| {
        if win.window().is_visible() {
            let _ = win.window().hide();
            WINDOW_HIDDEN.store(true, Ordering::Relaxed);
        } else {
            let _ = win.window().show();
            WINDOW_HIDDEN.store(false, Ordering::Relaxed);
        }
        "ok".to_string()
    }))
}
```

### `src/tray.rs` — tray menu toggle (bypasses IPC debounce):
```rust
// Inside the tray menu activation:
if crate::ipc::WINDOW_HIDDEN.load(Ordering::Relaxed) {
    let _ = win.window().show();
    crate::ipc::WINDOW_HIDDEN.store(false, Ordering::Relaxed);
} else {
    let _ = win.window().hide();
    crate::ipc::WINDOW_HIDDEN.store(true, Ordering::Relaxed);
}
```

Note: the tray menu uses `WINDOW_HIDDEN` directly (not `is_visible()`), and it also toggles via `slint::invoke_from_event_loop`. The keyboard shortcut goes through `cmd_toggle_tray` which uses `is_visible()`.

## Constraints

- **Must be WM-agnostic** (no Hyprland-specific hacks like `hyprctl`)
- **No external scripts** (everything must be in Rust)
- **Slint 1.17**, winit backend, Wayland
- **No flash** on startup — tray mode should start silently
- `set_minimized(false)` is **not available** on Wayland in winit

## Technical Details

- Slint version: 1.17
- winit dependency: winit 0.30 (bundled via Slint 1.17)
- GPU: NVIDIA RTX 5070 Ti
- WM: Hyprland (Wayland)
- OS: CachyOS (Arch derivative)
- Renderer: winit backend, likely OpenGL via glow

## Key Files

- `src/main.rs` — startup logic, event loop
- `src/ipc.rs` — IPC server, `cmd_toggle_tray`
- `src/tray.rs` — tray icon with show/hide menu item
- `Cargo.toml` — dependencies

## Question

What is the correct fix? How do we handle the Wayland cold-start problem where the first `show()` takes too long to render its first frame, the compositor drops the surface, and `is_visible()` reports incorrect state?

Our next candidate fix (NOT implemented yet) is to **prime the renderer at startup** by calling `show()` + `hide()` after a very short delay (50ms), so the GPU context is initialized before the first real toggle.

```rust
if tray_mode {
    window.show()?;
    // Prime renderer, then hide immediately
    let weak = window.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        if let Some(win) = weak.upgrade() {
            let _ = win.window().hide();
            crate::ipc::WINDOW_HIDDEN.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
    crate::ipc::WINDOW_HIDDEN.store(false, std::sync::atomic::Ordering::Relaxed);
    crate::ipc::TRAY_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
}
```

Is this approach correct? Is there a better way? Alternatives considered:
- Using `WINDOW_HIDDEN` instead of `is_visible()` as toggle state?
- Detecting WindowEvent::Occluded or similar to know when mapping completed?
- Double-show pattern (show → delay → show again) on first toggle?
