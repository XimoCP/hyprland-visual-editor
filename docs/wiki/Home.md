# HVE — Hyprland Visual Editor

**HVE** is a graphical application for managing the look of a Hyprland desktop from one place: animations, borders, rounded corners, gaps, shaders, wallpapers and colours and themes. It is aimed at anyone running a Hyprland setup — plain Hyprland or Noctalia Shell today — who wants to change how the desktop looks without editing configuration files by hand.

## How it works

HVE never rewrites your personal Hyprland configuration. Instead it keeps small snippets called **fragments** — one for the animation, one for the border, one for the geometry, one for the shader — and **assembles** them, together with the colours detected on your system, into a single `overlay` file that Hyprland loads alongside your own configuration. It injects one small block between markers it can fully revert, every change is written to a temporary file and moved into place, and uninstalling leaves the system as it was before HVE was installed.

```
your Hyprland files  ←  immutable, HVE never modifies them
      ┃
      ┃ (source / dofile)
      ┃
  ~/.cache/hve/overlay.{lua,conf}  ←  the single file HVE generates
      ┃
      ┃ (assembles)
      ┃
  assets/fragments/*.{lua,conf}    ←  individual fragments (animation, border, shader, geometry)
  + auto-detected colours
```

## Quick start

1. From a clone of the repository, run `./install.sh` and follow the prompts.
2. Launch the app with `hve`, or `hve --tray` to start in the system tray.
3. Your settings are stored in `~/.config/hve/config.json`.

## What's in this manual

- [Installation](Installation) — installing HVE, its system dependencies, and where the installer places each file.
- [Uninstallation](Uninstallation) — removing HVE, what is deleted, what is kept, and why nothing is left behind.
- [Architecture](Architecture) — how fragments are assembled into the overlay, Lua vs Conf detection, and the configuration, colour, preset, background, IPC, tray and watchdog systems.
- [Themes and colours](Themes-and-Colours) — how HVE reads colours from Noctalia, pywal, matugen or a manual source, and how complete themes are saved and applied.
- [Presets](Presets) — animation, border and shader presets: their metadata, how they are scanned, and the toggle behaviour.
- [Backgrounds](Backgrounds) — static and animated (video) wallpapers, and which piece owns the one on screen.
- [IPC](IPC) — the `hve-ipc` client, its commands and the default keyboard shortcuts.
- [Tray and automation](Tray-and-Automation) — the tray icon and its menu, the colour watcher, auto-minimise with countdown, and the safety watchdog.
- [Project structure](Project-Structure) — the repository layout and the runtime files HVE creates.
- [Usage](Usage) — the graphical interface, the command line, the tray and the keyboard shortcuts.
- [Configuration](Configuration) — a reference for `~/.config/hve/config.json`, its fields and its migrations.
- [FAQ](FAQ) — short answers to common questions.
- [Development](Development) — building HVE from source and working on its code.
