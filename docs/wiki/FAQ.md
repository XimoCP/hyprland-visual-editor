# ❓ FAQ

**Short answers to the questions that come up most.**

## 🔧 Does HVE modify my Hyprland configuration files?

No. It only appends an HVE block between the markers `-- >>> HYPRLAND VISUAL EDITOR START <<<` and `-- >>> HYPRLAND VISUAL EDITOR END <<<` in `hyprland.lua` (HVE manages `hyprland.lua` only); the block loads the overlay, loads the HVE settings and registers the watchdog. Disabling removes the whole block, and after an uninstall the watchdog removes it at the next Hyprland start, so your configuration ends up exactly as it was.

## 🖥️ Does it work with Noctalia Shell?

Yes. HVE is built on Hyprland and ships with support for Noctalia today: with Noctalia it works out of the box. Another shell on Hyprland would need its own support built in — HVE's main code would not change — but none ships today. See [Architecture](Architecture).

## 🗑️ What happens if I delete HVE's files manually?

A small cleanup script (`hve_watchdog.sh`) runs at every Hyprland start. If the HVE program (`~/.local/bin/hve`) is gone, it removes HVE's block from your configuration and clears `~/.cache/hve/` automatically.

## 🔒 Can I run two instances of HVE?

No. HVE marks itself as the only copy allowed to run, using a lock file (`~/.cache/hve/hve.lock`). A second launch opens no window: it asks the running copy to raise its window (the `show` command) and exits.

## ⚡ Are animation, border and shader changes instant?

Each change rebuilds the look and asks Hyprland to reload. Several changes in quick succession are folded into a single reload, so Hyprland picks the change up immediately instead of reloading once per click.

## 🖱️ Can I use HVE without a graphical window?

Yes, with `hve --tray` (tray only) and the `hve-ipc` commands from a terminal or a keyboard shortcut.

## 🐘 Does HVE use a lot of resources?

No. HVE is a small, lightweight program. A background helper (`color_watcher.sh`) only wakes up when a colour file changes, so it uses no CPU while nothing happens; HVE only uses resources while you interact with the interface.

## 🎨 What if I switch colour tools (pywal → matugen)?

HVE reads the colours from the tool you use. Every time it rebuilds the look it picks the best available source — first the applied theme's own saved colours, then the first that works in the order Noctalia → pywal → matugen → manual. Which files it follows is decided when it starts, so restart HVE after switching tools. See [Themes and colours](Themes-and-Colours).
