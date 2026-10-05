# HVE — Hyprland Visual Editor

HVE is a graphical app that manages the look of a Hyprland desktop from one place.

## What it is

A desktop application for changing how your Hyprland setup looks: animations, borders, shaders, window gaps, wallpapers, colours and themes — all from one interface, with the change applied live as you make it.

## Philosophy

HVE never rewrites your personal Hyprland configuration. It keeps small **fragments** — one for the animation, one for the border, one for the geometry, one for the shader — and **assembles** them, together with the colours detected on your system, into a single overlay file that Hyprland loads alongside your own configuration. It injects one small block between markers HVE can fully revert, and every write is atomic. Uninstalling leaves the system as it was before HVE was installed.

## Hyprland is the base; the shell is a separate axis

Hyprland is the base HVE builds on, not a swappable backend. The desktop shell that sits on top of it is a separate axis: what the core needs from a shell (active wallpaper, Do-Not-Disturb, palette re-assert) goes through a capability seam where every method has a declined default, and an architecture test fails the build if a shell name appears in core production code. Today the shipped adapter is **Noctalia**; another shell would need its own adapter, without changes to the core.

## Install

```bash
./install.sh
```

Full details, dependencies and what the installer places where: [Installation](docs/wiki/Installation.md).

## Quick start

```bash
hve          # open the window
hve --tray   # start minimized in the system tray
```

Your settings live in `~/.config/hve/config.json`.

## Manual

The full manual is the wiki under [`docs/wiki/`](docs/wiki/Home.md), starting at [Home](docs/wiki/Home.md): installation, architecture, themes and colours, presets, backgrounds, IPC, tray and automation, project structure, usage, configuration, FAQ and development.

## Licence

MIT — see [LICENSE](LICENSE).
