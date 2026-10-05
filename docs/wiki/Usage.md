# Usage

## Graphical interface

```bash
hve
```

HVE has **one window that mutates** instead of several windows. On launch it mounts the **Gallery** directly — an immersive, fullscreen view of your saved themes, so the first painted frame is already the gallery and never a placeholder screen.

- **Gallery** — your themes as cards, in the **Slider** (carousel) or **Mosaic** (grid) style; the style switcher sits in the top bar. Right-click a card (or press `i`) for the theme info panel. Applying a card writes the theme and reloads Hyprland.
- **Panel** — opens from the Gallery's five section pills and floats over it. The left rail switches sections:

  | Section | What lives there |
  |---------|------------------|
  | **Save** | *Save Current Look* form and *My Themes* list (search, rename, delete, refresh, overwrite). |
  | **Borders** | Geometry (size, radius, gaps), corner angle, colour slots, glow/shadow, plus built-in and user border presets. |
  | **Motion** | Curve preview, bezier sliders, speed and style, plus animation presets. |
  | **Filters** | Shader cards; picking the active one again clears it. |
  | **System** | System on/off, hide delay, language, autostart, theme, tiling, the restart banner, and the **About HVE** block. |

- **Esc** or the close button **hides** the window (to the tray); it never quits the app. Quit from the tray menu, from `hve-ipc quit`, or by killing the process.

The shell underneath has three screens — **Home** (root), **Gallery** and **Workshop**. Gallery is mounted at startup; Workshop is a placeholder today.

## Command line

```text
hve [OPTIONS]

  --tray     Run in tray-only mode (start minimized to the system tray)
  -v         Verbose logging (DEBUG)
  -vv        Even more verbose logging (TRACE)
```

Without `-v` the log filter is `info`. `--help` and `--version` come from the argument parser itself.

## System tray

```bash
hve --tray
```

Starts with the window hidden; everything is reachable from the tray icon's menu. The tray itself is started on every launch, so `hve` also leaves you an icon to come back to — see [Tray and automation](Tray-and-Automation).

HVE allows a single instance: a second launch does not open a new window, it asks the running one to bring its window forward (the `show` IPC command) and exits. If that instance cannot be reached, the second launch fails with a message naming both ways out — `pkill -x hve` to close it, or `SUPER + H` to show it.

## Keyboard shortcuts (IPC)

The shortcuts need the `hve-ipc` client, offered as an option during [Installation](Installation):

```bash
hve-ipc show             # show the window (never hides it)
hve-ipc toggle-tray      # show / hide the window
hve-ipc pause-restart    # pause / resume the system
hve-ipc next-anim        # next animation
hve-ipc next-border      # next border
hve-ipc next-shader      # next shader
hve-ipc status           # current state as JSON
hve-ipc refresh-theme    # reload the colours
hve-ipc quit             # close HVE
```

With keybinds active (they are written on every launch) you get:

| Combination | Action |
|-------------|--------|
| `SUPER + H` | Show / hide the window |

Any other command can be bound the same way in `~/.cache/hve/hve-settings.lua`. The full command list and the socket details are in [IPC](IPC).

## See also

- [Configuration](Configuration) — every field of `~/.config/hve/config.json`.
- [Presets](Presets) — animation, border and shader presets.
- [Themes and colours](Themes-and-Colours) — where the colours come from.
