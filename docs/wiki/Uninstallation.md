# 🧹 Uninstallation

**Get HVE off your machine, and know exactly what happens.**

```bash
./uninstall.sh
```

The uninstaller is bilingual (Spanish/English), prints a summary of what it removed and what it did not find, and asks before deleting anything that is yours. It removes HVE's own files, keeps your settings unless you say otherwise, and never rewrites your Hyprland configuration.

## 🗑️ What it removes

| Item | Path | Notes |
|------|------|-------|
| Program | `~/.local/bin/hve` | — |
| Support files | `~/.local/bin/assets/` | Scripts, animations, borders, shaders and the pieces of the current look |
| Launcher entry | `~/.local/share/applications/hve.desktop` | — |
| Autostart entry | The `-- >>> HVE AUTOSTART <<<` block inside `~/.cache/hve/hve-settings.lua` | Removed even if you keep the temporary files, so no `hve --tray` entry outlives the program |
| Shortcut helper | `~/.local/bin/hve-ipc` | Only if you installed it |
| System shortcut | `/usr/local/bin/hve-ipc` | Requires `sudo`; only if it exists |

## ❓ What it asks about first

| Item | Path | Why it asks |
|------|------|-------------|
| Temporary files | `~/.cache/hve/` | Temporary data (logs, the assembled look, lock files). Recreated the next time you run HVE. |
| User configuration | `~/.config/hve/` | **Your data**: `config.json` with your settings, `themes/` with saved themes, `presets/` with your saved presets. |

Both questions default to **No**, so answering nothing keeps everything.

## 🔧 Under the hood

### 🛡️ Why uninstalling leaves nothing behind

1. **Your Hyprland configuration is only appended to, never rewritten.** HVE appends one block between the markers `-- >>> HYPRLAND VISUAL EDITOR START <<<` and `-- >>> HYPRLAND VISUAL EDITOR END <<<` in `hyprland.lua` (HVE manages `hyprland.lua` only). Disabling HVE removes the whole block, so the file is exactly as it was. The uninstaller itself never edits your Hyprland files.

2. **The watchdog (`hve_watchdog.sh`) finishes the job.** It runs at every Hyprland start (registered inside that same block). If the HVE binary (`~/.local/bin/hve`) is gone — uninstall, manual deletion — it detects the absence, removes the markers from `hyprland.lua` (and a leftover `#`-style block from an older `hyprland.conf`) and clears `~/.cache/hve/`.

3. **The uninstaller removes every file the installer created.** No hidden files, no system records, no residual configuration.

4. **Your own configuration (`~/.config/hve/config.json`, `themes/`, `presets/`) is kept** unless you explicitly choose to delete it. If you reinstall later, your previous settings work again.

5. **`~/.local/bin` is never deleted** (you may have other programs there). Only HVE's files are removed.

In short: **you can install and uninstall HVE as many times as you like without leaving rubbish behind or breaking your Hyprland configuration.**

## 📚 See also

- [Installation](Installation) — install again.
- [Configuration](Configuration) — what the kept configuration contains.
