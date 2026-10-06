# ⚙️ Configuration

**The reference for `~/.config/hve/config.json`, field by field.**

This page is for people who edit HVE's configuration by hand. If you use the app and its panels, you do not need it — see [Usage](Usage).

HVE keeps one JSON file: `~/.config/hve/config.json` (it honours `$XDG_CONFIG_HOME`, so the file can live elsewhere if you set that variable). It is read and written by `src/config.rs`.

**Current `config_version`: 8.**

```json
{
  "config_version": 8,
  "is_system_active": false,
  "border_size": 2,
  "border_radius": 32,
  "gaps_in": 5,
  "gaps_out": 5,
  "active_anim_file": "",
  "active_border_file": "",
  "active_shader_file": "",
  "auto_start": false,
  "auto_minimize_enabled": true,
  "minimize_seconds": 4,
  "language": "",
  "tiling_mode": false,
  "gallery_style": 0,
  "theme": "system",
  "last_applied_theme": "",
  "keybinds_enabled": false,
  "disabled_providers": []
}
```

That block is what a **fresh** file looks like — the defaults from `Config::default()`. An existing file holds your current values instead; for example `"theme": "dark"` or `"active_anim_file": "01_relampago"`.

## 📋 Field reference

| Field | Type | Default | What it does |
|-------|------|---------|--------------|
| `config_version` | number | `8` | Schema version. Migrated forward on load (see below). |
| `is_system_active` | bool | `false` | Whether HVE's management is on. Toggled by the tray's *Toggle System* and by `hve-ipc pause-restart`. |
| `border_size` | number | `2` | Window border width, applied through the geometry fragment. |
| `border_radius` | number | `32` | Corner radius in pixels. The geometry fragment omits `rounding` when it is `0`. |
| `gaps_in` | number | `5` | Inner gaps, written as `general.gaps_in`. |
| `gaps_out` | number | `5` | Outer gaps, written as `general.gaps_out`. |
| `active_anim_file` | string | `""` | Name of the active animation preset; empty means none. |
| `active_border_file` | string | `""` | Name of the active border preset; empty means none. |
| `active_shader_file` | string | `""` | Name of the active shader; empty means none. |
| `auto_start` | bool | `false` | Start HVE in the tray at login; the autostart block itself lives in `~/.cache/hve/hve-settings.lua`. |
| `auto_minimize_enabled` | bool | `true` | Hide the window when it loses focus. |
| `minimize_seconds` | number | `4` | Seconds of countdown before the window hides. |
| `language` | string | `""` | `""` means auto-detect from `$LANG`. `en` and `es` are available; a change needs a restart. |
| `tiling_mode` | bool | `false` | `false` = floating, `true` = tiled windows. |
| `gallery_style` | number | `0` | `0` = the carousel (the **Slice** pill), `2` = the grid (the **Mosaic** pill). Any other value (a stale or hand-edited file) opens the carousel. |
| `theme` | string | `"system"` | `"dark"`, `"light"` or `"system"`; `system` asks the desktop preference. |
| `last_applied_theme` | string | `""` | Name of the last applied theme, used to re-assert its colours after a restart. |
| `keybinds_enabled` | bool | `false` | Saved toggle for HVE's shortcut block. Nothing outside `src/config.rs` reads it today: at startup HVE writes the vital `SUPER + H` keybind itself (`src/main.rs` → `src/settings.rs`). |
| `disabled_providers` | array | `[]` | Provider ids the user turned off, for example `"noctalia"` or `"hve-presets"`. |

## 🔄 Migrations

The system migrates **forward**: if a future version adds fields, HVE migrates automatically from any older version. The real chain:

| Step | Field(s) added |
|------|----------------|
| v0 → v1 | Version stamp (pre-versioning configs) |
| v1 → v2 | Settings panel: `auto_minimize_enabled`, `minimize_seconds`, `language`, `tiling_mode`, `theme` |
| v2 → v3 | `keybinds_enabled` |
| v3 → v4 | `last_applied_theme` |
| v4 → v5 | `disabled_providers` |
| v5 → v6 | `border_radius` |
| v6 → v7 | `gaps_in`, `gaps_out` |
| v7 → v8 | `gallery_style` |

Each step fills in the value **that version** introduced, so an upgraded file keeps the historical default of the step (for example `minimize_seconds: 5` from v1 → v2) while a fresh file starts at the current default (`4`). The migration prints `[hve] Info: migrating config from vN to v8.` to stderr and the file is rewritten at the current version.

## ⚠️ Unknown fields

The struct is declared with `deny_unknown_fields`. A key HVE does not know — a typo, or a key from a newer version — makes the parse fail: the warning goes to stderr and the **whole file** falls back to defaults, and the next save overwrites it. Editing this file by hand therefore means editing only keys that exist in the table above.

A corrupt or unreadable file behaves the same way: a warning on stderr, then defaults.

## 📚 See also

- [Architecture](Architecture) — how the configuration, fragment and colour systems fit together.
- [Project structure](Project-Structure) — where the config lives among the runtime files.
- [Uninstallation](Uninstallation) — what happens to this file when you remove HVE.
