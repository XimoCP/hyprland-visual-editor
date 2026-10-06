# 🏠 HVE — Hyprland Visual Editor

**Shape your Hyprland desktop. Live.**

HVE is a graphical application for managing the look of a Hyprland desktop from one place: animations, borders, rounded corners, gaps, shaders, wallpapers and colours and themes. It is aimed at anyone running a Hyprland setup — plain Hyprland or Noctalia Shell today — who wants to change how the desktop looks without editing configuration files by hand.

Change something, see it straight away, keep what you like. HVE never rewrites your personal Hyprland configuration, and uninstalling leaves the system as it was before HVE was installed.

## 🚀 Quick start

1. From a clone of the repository, run `./install.sh` and follow the prompts.
2. Launch the app with `hve`, or `hve --tray` to start in the system tray.
3. Your settings are stored in `~/.config/hve/config.json`.

## 📚 What's in this manual

- [🚀 Installation](Installation) — get HVE onto your machine: its system dependencies and where the installer places each file.
- [🕹️ Usage](Usage) — the first ten minutes: the graphical interface, the command line, the tray and the keyboard shortcuts.
- [🎨 Themes and colours](Themes-and-Colours) — choose where your colours come from (Noctalia, pywal, matugen or a manual source), and how complete themes are saved and applied.
- [🧩 Presets](Presets) — try a ready-made look: animation, border and shader presets, their metadata, how they are scanned, and the toggle behaviour.
- [🖼️ Backgrounds](Backgrounds) — set your wallpaper: static and animated (video) wallpapers, and which piece owns the one on screen.
- [❓ FAQ](FAQ) — short answers to common questions.
- [🧹 Uninstallation](Uninstallation) — remove HVE: what is deleted, what is kept, and why nothing is left behind.
- [🧠 Architecture](Architecture) — how fragments are assembled into the overlay, the Lua-only configuration, and the configuration, colour, preset, background, IPC, tray and watchdog systems.
- [🔌 IPC](IPC) — drive HVE from scripts and keys: the `hve-ipc` client, its commands and the default keyboard shortcuts.
- [🗂️ Project structure](Project-Structure) — find your way around: the repository layout and the runtime files HVE creates.
- [⚙️ Configuration](Configuration) — tune HVE by hand: a reference for `~/.config/hve/config.json`, its fields and its migrations.
- [💻 Development](Development) — build HVE from source and work on its code.
- [🖥️ Tray and automation](Tray-and-Automation) — the tray icon and its menu, the colour watcher, auto-minimise with countdown, and the safety watchdog.

## 🔧 Under the hood

Your own Hyprland files stay untouched. HVE writes one generated file that Hyprland reads alongside them:

```
your Hyprland files  ←  immutable, HVE never modifies them
      ┃
      ┃ (source / dofile)
      ┃
  ~/.cache/hve/overlay.lua  ←  the single file HVE generates
      ┃
      ┃ (assembles)
      ┃
  assets/fragments/*.lua    ←  individual fragments (animation, border, shader, geometry)
  + auto-detected colours
```

HVE never rewrites your personal Hyprland configuration. Instead it keeps small snippets called **fragments** — one for the animation, one for the border, one for the geometry, one for the shader — and **assembles** them, together with the colours detected on your system, into a single `overlay` file that Hyprland loads alongside your own configuration. It injects one small block between markers it can fully revert, every change is written to a temporary file and moved into place, and uninstalling leaves the system as it was before HVE was installed.
