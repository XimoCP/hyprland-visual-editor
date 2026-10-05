# Installation

HVE is installed from a clone of the repository:

```bash
git clone https://github.com/tu-usuario/hve.git
cd hve
./install.sh
```

The installer is bilingual (Spanish/English), runs as your normal user and asks before doing anything optional. It asks for `sudo` only for system packages and for the optional `/usr/local/bin` symlink.

## What gets installed, and where

| Path | Contents | Type |
|------|----------|------|
| `~/.local/bin/hve` | Main binary (compiled Rust) | Required |
| `~/.local/bin/assets/scripts/` | 15 support scripts plus the `color_sources.d/` colour-source modules | Required |
| `~/.local/bin/assets/animations/` | 19 animation presets (`.lua`) | Required |
| `~/.local/bin/assets/borders/` | 14 border presets (`.lua`) | Required |
| `~/.local/bin/assets/shaders/` | 9 GLSL shaders (`.frag`) | Required |
| `~/.local/bin/assets/fragments/` | Dynamic fragments, rewritten at runtime | Required |
| `~/.local/share/applications/hve.desktop` | Desktop entry for application launchers; its `Exec=` line is rewritten to the absolute binary path | Required |
| `~/.local/bin/hve-ipc` | IPC client script (Python); required for the keyboard shortcuts | Optional |
| `/usr/local/bin/hve-ipc` | Symlink to the IPC script (requires `sudo`) | Optional |
| `~/.config/hve/config.json` with `auto_start: true` | Start HVE with the session; the autostart block itself is written to `~/.cache/hve/hve-settings.lua` on the next launch | Optional |

## System dependencies

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

## What the installer does

1. **Refuses to run as root** — `sudo` is requested only where a package or a system path is involved.
2. **Detects the distribution** from `ID` and `ID_LIKE` in `/etc/os-release`, with fallbacks to `/etc/arch-release`, `/etc/debian_version`, `/etc/fedora-release` and `/etc/gentoo-release`.
3. **Installs dependencies** — one batch install first; if it fails, package by package.
4. **Verifies Rust** — installs it through `rustup` when it is missing.
5. **Checks that Hyprland uses Lua** — HVE 2 manages `hyprland.lua` only. A legacy `hyprland.conf` is reported, and the configuration must be migrated before HVE can be enabled.
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

## After installing

- Launch with `hve`, or `hve --tray` to start in the system tray — see [Usage](Usage).
- Your settings live in `~/.config/hve/config.json` — see [Configuration](Configuration).
- To remove everything again — see [Uninstallation](Uninstallation).
