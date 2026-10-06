# 🚀 Installation

**Get HVE onto your machine, in one command.**

HVE installs from a clone of the repository. It builds the app, puts the binary and everything it needs in your home folder, and adds a launcher. It never rewrites your Hyprland configuration: the app adds one block between its own markers, and it can remove that block completely.

## 📋 What you need first

- A Hyprland desktop — Hyprland is the program that draws your desktop and manages your windows — that uses the Lua configuration (`hyprland.lua`). HVE manages only that file: if your Hyprland still runs the older `hyprland.conf`, migrate it first.
- A normal user account. Do not run the installer with `sudo` or as root — it asks for `sudo` by itself when a package or a system path is involved.
- Rust. The installer checks for it and installs it through `rustup` when it is missing.
- `python3`. Without it, colour detection and the IPC shortcuts may not work.

The installer knows Arch, Manjaro, EndeavourOS, CachyOS, Artix, Debian, Ubuntu, Pop, Linux Mint, Elementary, Fedora, openSUSE, Void and Gentoo, and installs the system packages for you. On any other distribution it stops with an "unsupported" message and asks you to install the dependencies by hand.

## ⚡ Install

From a clone of the repository, at its root:

```bash
cd hve
./install.sh
```

The installer is bilingual (Spanish/English), runs as your normal user and asks before doing anything optional. It asks for `sudo` only for system packages and for the optional `/usr/local/bin` symlink.

## 📍 What lands where

Everything goes into your home folder — the binary, the assets it needs, a desktop entry and one config file. The installer writes nothing into your Hyprland configuration; the app adds its own marked block there when you enable it. The exact paths are in the table below.

## 🏁 Launch it

```bash
hve
```

The gallery opens fullscreen with your saved themes. To start in the system tray instead (the small icon in your panel where background apps live):

```bash
hve --tray
```

Your settings live in `~/.config/hve/config.json`.

## ✅ How to know it worked

- The installer finishes with **HVE installed!** and prints the path of the binary.
- `hve --version` answers with a version.
- Running `hve` opens the fullscreen gallery.
- If `~/.local/bin` is not in your `PATH`, the installer warns you and prints the line to add to your shell configuration.
- Your application launcher shows a **Hyprland Visual Editor** entry.

## 🔧 Under the hood

### 📦 What gets installed, and where

| Path | Contents | Type |
|------|----------|------|
| `~/.local/bin/hve` | Main binary (compiled Rust) | Required |
| `~/.local/bin/assets/scripts/` | 16 support scripts (including `hve-ipc`, listed below) plus the `color_sources.d/` colour-source modules | Required |
| `~/.local/bin/assets/animations/` | 19 animation presets (`.lua`) | Required |
| `~/.local/bin/assets/borders/` | 14 border presets (`.lua`) | Required |
| `~/.local/bin/assets/shaders/` | 9 GLSL shaders (`.frag`) | Required |
| `~/.local/bin/assets/fragments/` | Dynamic fragments, rewritten at runtime | Required |
| `~/.config/hve/themes/` | Reference themes shipped with HVE, copied without overwriting a theme you already have | Required |
| `~/.local/share/applications/hve.desktop` | Desktop entry for application launchers; its `Exec=` line is rewritten to the absolute binary path | Required |
| `~/.local/bin/hve-ipc` | IPC client script (Python); required for the keyboard shortcuts | Optional |
| `/usr/local/bin/hve-ipc` | Symlink to the IPC script (requires `sudo`) | Optional |
| `~/.config/hve/config.json` with `auto_start: true` | Start HVE with the session; the autostart block itself is written to `~/.cache/hve/hve-settings.lua` on the next launch | Optional |

### 🧰 System dependencies

The installer detects your distribution and installs them for you:

| Family | Manager | Packages |
|--------|---------|----------|
| Arch / Manjaro / EndeavourOS / CachyOS / Artix | `pacman` | `inotify-tools pkg-config gtk3 glib2 cairo pango python` |
| Debian / Ubuntu / Pop / Linux Mint / Elementary | `apt` | `inotify-tools pkg-config libgtk-3-dev libglib2.0-dev libcairo2-dev libpango1.0-dev python3` |
| Fedora | `dnf` | `inotify-tools pkgconfig gtk3-devel glib2-devel cairo-devel pango-devel python3` |
| openSUSE | `zypper` | `inotify-tools pkg-config gtk3-devel glib2-devel cairo-devel pango-devel python3` |
| Void | `xbps-install` | `inotify-tools pkg-config gtk+3-devel glib-devel cairo-devel pango-devel python3` |
| Gentoo | `emerge` | `inotify-tools pkg-config gtk3 glib2 cairo pango dev-lang/python` |

A distribution outside that list stops the installer with an "unsupported" message. After the attempt, the installer checks that `inotifywait`, `pkg-config` and `python3` are available.

### 🛠️ What the installer does

1. **Refuses to run as root** — `sudo` is requested only where a package or a system path is involved.
2. **Detects the distribution** from `ID` and `ID_LIKE` in `/etc/os-release`, with fallbacks to `/etc/arch-release`, `/etc/debian_version`, `/etc/fedora-release` and `/etc/gentoo-release`.
3. **Installs dependencies** — one batch install first; if it fails, package by package.
4. **Verifies Rust** — installs it through `rustup` when it is missing.
5. **Checks that Hyprland uses Lua** — HVE manages `hyprland.lua` only. A legacy `hyprland.conf` is reported, and the configuration must be migrated before HVE can be enabled.
6. **Builds** (`cargo build --release`).
7. **Copies the binary** to `~/.local/bin/hve`.
8. **Copies the assets** (scripts, animations, borders, shaders, fragments).
9. **Installs the desktop entry**, rewriting `Exec=` to the absolute binary path so the launcher does not depend on `PATH`.
10. **Detects your colour tool** (Noctalia, pywal, matugen or a manual source) — informational only; runtime detection belongs to `color_watcher.sh`.
11. **Asks for the optional pieces**:
    - Install `hve-ipc`? (keyboard shortcuts in Hyprland)
    - Create a symlink in `/usr/local/bin`? (fixed paths in Hyprland configs; needs `sudo`, offered only if `hve-ipc` was installed)
    - Start HVE with the session? (tray at login)
    - Start it now?
12. **Warns if `~/.local/bin` is not in your `PATH`.**

## 📚 See also

- [Usage](Usage) — the first ten minutes with the app.
- [Configuration](Configuration) — every field of `~/.config/hve/config.json`.
- [Uninstallation](Uninstallation) — remove everything again.
