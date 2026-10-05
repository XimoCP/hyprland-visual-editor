# Tray and automation

Four pieces keep HVE working while you are not looking at it: the tray icon, the colour watcher, the focus countdown and the safety watchdog.

## System tray

HVE publishes a tray icon through the **KDE Status Notifier Item** protocol using the `ksni` crate (`src/tray.rs`). The tray starts on every launch, with or without `--tray`; the flag only decides whether the window opens as well.

- The icon is drawn in Rust from the HVE logo as RGBA pixel data and handed to the tray as a **48×48** image — no system icon theme is involved.
- Its tint follows the current theme accent (lightened for dark panels); the on/off state of HVE is shown by the menu, not by an icon colour swap.
- The tray title is `HVE — Hyprland Visual Editor`.

### Context menu

| Entry | Action |
|-------|--------|
| *Hyprland Visual Editor* | Read-only title. |
| — separator — | |
| **Toggle System** | Turn all of HVE's visual management on or off. |
| **Next Animation** | Cycle to the next animation preset. |
| **Next Border** | Cycle to the next border preset. |
| **Next Shader** | Cycle to the next shader. |
| — separator — | |
| **Show Window** / **Hide Window** | Shows or hides the window; the label follows the current visibility. |
| — separator — | |
| **Quit** | Close HVE. |

Every label goes through the translation table (`tray.*` keys) with English as the fallback.

## Colour watcher (`color_watcher.sh`)

`color_watcher.sh` is a background process spawned by HVE on every launch (`src/watcher.rs`) that watches your colour files with `inotifywait` and keeps the overlay in sync.

- **Colour sources**: watch paths are declared by the modules in `assets/scripts/color_sources.d/` — Noctalia (Lua config and palette), pywal, matugen, and a manual scan of your Hyprland config, which is watched only when nothing else was found.
- **Events**: `inotifywait -q -e close_write -t 30` — `close_write` fires after a file has been fully written, and the 30 s timeout groups a burst such as a pywal regeneration.
- **Real changes only**: each watched file is hashed with `md5sum`, so a touch that did not change the bytes does not trigger a refresh.
- **On a change**: the applied theme's colour authority is re-asserted once per burst (`hve-ipc assert-color-authority`, with a 5 s cooldown), the owning colour source runs its declared refresh, `assemble.sh` regenerates the overlay, the signal file is written, and HVE is told to reload with `hve-ipc refresh-theme`.
- **On startup**: it repairs a palette that was taken over while HVE was off (`hve-ipc repair-color-authority`), then runs one initial overlay refresh.
- **Singleton**: a non-blocking `flock` on `~/.cache/hve/color_watcher.lock` allows only one watcher; a second instance logs one line and exits. If `flock` is missing the guard is skipped and the watcher continues unguarded rather than silently losing colour sync.
- **Log**: `~/.cache/hve/color_watcher.log` (XDG-aware).

## Auto-minimise and countdown

When the window loses focus — detected from Hyprland's `activewindow` events (`src/hypr_ipc.rs`) — and auto-minimise is enabled:

1. A circular countdown appears over the close button, filling from the bottom as the seconds tick down.
2. Regaining focus cancels it immediately.
3. When it reaches 0 the window is hidden and moved to the `special:minimized` scratchpad.
4. Clicking the close button skips the countdown and minimises at once.

The countdown is suppressed while a theme is being applied (a reload briefly moves focus) and during a theme transition. If the fullscreen Gallery has the focus, losing it hides the window immediately instead of counting down.

Defaults and the toggle live in `~/.config/hve/config.json`: `auto_minimize_enabled: true`, `minimize_seconds: 4`. Both are editable from the **System** panel section — see [Configuration](Configuration).

## Safety watchdog (`hve_watchdog.sh`)

`init.sh enable` copies the watchdog to the cache directory (`~/.cache/hve/hve_watchdog.sh`) and registers it in `hyprland.lua` so it runs at every Hyprland start:

```lua
hl.on("hyprland.start", function()
    hl.exec_cmd("<cache>/hve/hve_watchdog.sh")
end)
```

The script resolves the directory it treats as HVE's home from its own location — two levels above the script's own directory — and tests that it still exists. (With the registered copy living in `~/.cache/hve`, that resolved directory is normally `$HOME`, so the cleanup below only runs when that directory is gone.)

If the directory is missing — a manual uninstall or an accidental deletion — the script:

1. strips HVE's marker block from `~/.config/hypr/hyprland.conf` (`# >>> HYPRLAND VISUAL EDITOR START <<<` … `# >>> HYPRLAND VISUAL EDITOR END <<<`) and from `~/.config/hypr/hyprland.lua` (the same markers with `--` comments);
2. deletes the cache directory, but only when the resolved path is absolute and ends in `/hve` — a relative `HVE_CACHE_DIR` or a broken `$HOME` can never send `rm -rf` after the wrong directory.

If the directory exists, the script does nothing.

## See also

- [IPC](IPC) — the commands these scripts send.
- [Uninstallation](Uninstallation) — what is removed when you take HVE out.
