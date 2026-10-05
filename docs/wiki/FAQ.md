# FAQ

**Does HVE modify my Hyprland configuration files?**
No. It only appends an HVE block between the markers `-- >>> HYPRLAND VISUAL EDITOR START <<<` and `-- >>> HYPRLAND VISUAL EDITOR END <<<` in `hyprland.lua` (HVE manages `hyprland.lua` only); the block loads the overlay, loads the HVE settings and registers the watchdog. Disabling removes the whole block, and after an uninstall the watchdog removes it at the next Hyprland start, so your configuration ends up exactly as it was.

**Does it work with Noctalia Shell?**
Yes. HVE is built on Hyprland and ships with the Noctalia adapter today: with Noctalia it works out of the box. Another shell on Hyprland would need a new adapter — the core does not change — but none ships today. See [Architecture](Architecture).

**What happens if I delete HVE's files manually?**
The watchdog `hve_watchdog.sh`, which runs at every Hyprland start, detects that the HVE binary (`~/.local/bin/hve`) is gone, removes the markers and clears `~/.cache/hve/` automatically.

**Can I run two instances of HVE?**
No. HVE takes an exclusive lock on `~/.cache/hve/hve.lock`. A second launch opens no window: it asks the running instance to raise its window over IPC (`show`) and exits.

**Are animation, border and shader changes instant?**
Each change re-runs `assemble.sh`, which rewrites the overlay and queues a reload. Bursts of changes are folded into a single `hyprctl reload` by `reload_coalescer.sh`, so Hyprland picks the change up immediately instead of reloading once per click.

**Can I use HVE without a graphical window?**
Yes, with `hve --tray` (tray only) and the `hve-ipc` commands from a terminal or a keyboard shortcut.

**Does HVE use a lot of resources?**
No. The binary is compiled Rust and lightweight. `color_watcher.sh` sits on `inotifywait`, which uses no CPU while nothing changes; HVE only uses resources while you interact with the interface.

**What if I switch colour tools (pywal → matugen)?**
The colour sources are modules in `assets/scripts/color_sources.d/`, and every overlay assembly picks the best available one in the order Noctalia → pywal → matugen → manual. The files the watcher watches are chosen when it starts, so restart HVE after switching tools. See [Themes and colours](Themes-and-Colours).
