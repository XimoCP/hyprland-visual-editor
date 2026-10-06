# 🕹️ Usage

**You have HVE installed. Here is what to do with it.**

## 👀 What you see first

Launch it:

```bash
hve
```

HVE opens straight into the **gallery**: a fullscreen wall of your saved themes, each one a card. There is no menu bar and no second window to juggle — this is the app.

## 🎯 Three things you will do

1. **Look at your themes** — the gallery shows them as cards, and the active one is marked.
2. **Change the look** — open the panel and pick a section: borders, motion, filters or system.
3. **Save it** — give the current look a name and keep it as a theme you can come back to.

## 🖼️ The gallery

Your themes as cards, in two styles you switch from the pills in the top bar:

- **Slice** (the carousel): one card in the centre, its neighbours at the sides.
- **Mosaic** (the grid): a wall of tiles.

Right-click a card — or press `i` — to open the **theme info panel** for that theme: its colours, its saved date and what it was built from. Press Enter (or click) on a card to apply it: applying writes the theme and reloads Hyprland.

**Esc** or the close button **hides** the window (to the tray); it never quits the app. Quit from the tray menu, from `hve-ipc quit`, or by killing the process.

## 🎛️ The panel

The panel opens from the gallery's five section pills and floats over it. The left rail switches sections:

| Section | What lives there |
|---------|------------------|
| **Save** | *Save Current Look* form and *My Themes* list (search, rename, delete, refresh, overwrite). |
| **Borders** | Geometry (size, radius, gaps), corner angle, colour slots, glow/shadow, plus built-in and user border presets. |
| **Motion** | Curve preview, the sliders that shape it, speed and style, plus animation presets. |
| **Filters** | Shader cards; picking the active one again clears it. |
| **System** | System on/off, hide delay, language, autostart, theme, tiling, the restart banner, and the **About HVE** block. |

## 💾 Apply and save

Applying a card writes the theme to disk and reloads Hyprland, so the change lands immediately. To keep the look you have right now, open the **Save** section, name it in *Save Current Look*, and it joins *My Themes* — where you can search, apply, rename, delete, refresh or overwrite.

## 🖥️ The tray

```bash
hve --tray
```

Starts with the window hidden; everything is reachable from the tray icon's menu — toggle the system, step through animations, borders and shaders, show or hide the window, and quit. The tray itself is started on every launch, so `hve` also leaves you an icon to come back to — see [Tray and automation](Tray-and-Automation).

## ⌨️ Command line and shortcuts

```text
hve [OPTIONS]

  --tray     Run in tray-only mode (start minimized to the system tray)
  -v         Verbose logging (DEBUG)
  -vv        Even more verbose logging (TRACE)
```

Without `-v`, HVE logs ordinary messages and warnings. `-v` and `-vv` turn on debug and trace detail. `--help` and `--version` are handled by the command-line tool itself.

HVE allows a single instance: a second launch does not open a new window, it asks the running one to bring its window forward (the `show` command) and exits. If that instance cannot be reached, the second launch fails with a message naming both ways out — `pkill -x hve` to close it, or `SUPER + H` to show it.

The shortcuts need the `hve-ipc` tool, offered as an option during [Installation](Installation):

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

With the shortcuts active (they are set up on every launch) you get:

| Combination | Action |
|-------------|--------|
| `SUPER + H` | Show / hide the window |

Any other command can be bound the same way in `~/.cache/hve/hve-settings.lua`. The full command list is in [IPC](IPC).

## 🔧 Under the hood

HVE has **one window that mutates** instead of several windows. On launch it mounts the **Gallery** directly — an immersive, fullscreen view of your saved themes, so the first painted frame is already the gallery and never a placeholder screen.

The shell underneath has three screens — **Home** (root), **Gallery** and **Workshop**. Gallery is mounted at startup; Workshop is a placeholder today.

## 📚 See also

- [Configuration](Configuration) — every field of `~/.config/hve/config.json`.
- [Presets](Presets) — animation, border and shader presets.
- [Themes and colours](Themes-and-Colours) — where the colours come from.
