# 🎨 Themes and colours

**Where your colours come from, and what applying a theme does.**

HVE takes the colours from whatever colour tool you already use and applies them to two places: the look of your Hyprland window borders, gradients and rounded corners, and HVE's own interface. You do not pick a palette by hand — HVE finds one. A complete desktop look — colours, presets, backgrounds — is saved and put back as a **theme**.

## 🌈 Where your colours come from

A *colour source* is the program or file HVE reads a palette from. HVE tries the sources it knows in order and stops at the first one that works: Noctalia, then pywal, then matugen, then a plain scan of your Hyprland files. If the theme you applied owns its own colours, that saved snapshot wins over all of them. You never choose a source; switching colour tools just means restarting HVE.

## 💾 What a theme is, and what applying one does

A theme is a saved look: the colours, plus the presets, borders, shaders and background that go with it. It lives in its own folder under `~/.config/hve/themes/<name>/`, so copying the folder carries the whole look. Applying a theme writes its files to disk and reloads Hyprland, so the change lands immediately. The gallery lists a theme only when every part it recorded is available on this machine, so a theme is never half-applied.

## 🔧 Under the hood

### 🔍 How a colour source is chosen

`assets/scripts/colors.sh` builds one palette for the overlay. It resets
every colour role, then tries the available sources **in priority order** and
stops at the first one that succeeds; a failing source leaves no partial
state behind for the next.

| Priority | Source | What it reads | Where it lives |
|----------|--------|---------------|----------------|
| 10 | Applied theme snapshot | `~/.cache/hve/color-authority.json` → the theme's saved `palette.json` under `~/.config/hve/themes/` | inline in `colors.sh` |
| 20 | Noctalia palette | `noctalia msg color-scheme-get` plus the scheme's own JSON | `color_sources.d/noctalia_palette.sh` |
| 30 | Noctalia rendered template | `~/.config/hypr/noctalia.lua` (v5), else `~/.config/hypr/noctalia/noctalia-colors.lua` (v4) | `color_sources.d/noctalia_lua.sh` |
| 40 | pywal | `~/.cache/wal/colors.json` | `color_sources.d/pywal.sh` |
| 50 | matugen | `~/.config/matugen/config.toml` and the Hyprland output it names | `color_sources.d/matugen.sh` |
| 60 | Manual scan | `~/.config/hypr/*.lua` (two levels deep), falling back to the border gradient in `configs/appearance.lua` | `color_sources.d/manual_hypr.sh` |

Lower number wins. Steps are sorted by priority and ties break by id, so the
order above never changes by accident.

### 🎨 The palette the overlay gets

`colors.sh` declares the roles **once**, in `HVE_PALETTE_TOKENS`:

```
primary secondary tertiary error surface surface_lowest accent
```

Each role is exported as `HVE_PRIMARY`, `HVE_SECONDARY`, `HVE_TERTIARY`,
`HVE_ERROR`, `HVE_SURFACE`, `HVE_SURFACE_LOWEST` and `HVE_ACCENT`, and
`assemble.sh` prints one variable line per role — a role the presets
reference can no longer be missing from the overlay.

When no source supplies a role, a documented fallback does (Catppuccin
Mocha): `primary` `#cba6f7`, `secondary` `#89b4fa`, `tertiary` `#94e2d5`,
`error` `#f38ba8`, `surface` `#1e1e2e`. `surface_lowest` derives from the
surface and `accent` from the primary.

**A missing tertiary never borrows the secondary.** Some rendered Noctalia
templates carry no `tertiary`. When that happens, the source that **won**
the chain is asked to complete the third role from *its own* files — today
only `noctalia_lua.sh` implements that hook, and it accepts the v4 rendering
only when its `primary` matches the one already detected, so a leftover file
from an older scheme cannot leak in. Only when nothing can supply one does
the `#94e2d5` fallback apply.

### 🖥️ The colours HVE itself uses

`assets/scripts/get_colors.sh` wraps `colors.sh` and prints the six roles as
JSON. `Engine::get_colors()` (`src/engine.rs`) parses that into a
`ColorScheme`, and `src/theme.rs` turns it into the interface colours: the
background, surface, card and hover tones, the text pair, six accents
(cyan, amber, green, purple, pink, red) and the border — thirteen colours in
total, plus six palette-token swatches for the tune panes.

Card, hover and border tones are derived from the surface by lightening or
darkening it, and text is chosen from the surface's **relative luminance**, so
the interface keeps its contrast whether the source palette is light or dark.
The `theme` setting (`dark`, `light`, `system`) resolves the base scheme;
`system` asks the desktop preference through the `read-desktop-preference`
capability (`src/theme/desktop_preference/`).

### 📦 Saving and applying a theme

Themes live in `~/.config/hve/themes/<name>/`. Each theme directory holds a
`meta.json` (when it was saved and which providers took part) and one
subdirectory per provider under `providers/<id>/`.

Two kinds of provider register at startup (`src/providers/mod.rs`):

- **one shell backend** — `noctalia-v5` when Noctalia v5 is detected, else
  `noctalia` (v4), which is the fallback;
- **`hve-presets`** — HVE's own animation, border, shader and geometry
  presets; it registers unless its id is listed in `disabled_providers`.

A provider id listed in `disabled_providers` is simply not registered; the
other shell version is never substituted for it.

- **Save** — `ThemeManager::save` creates the directory, runs each selected
  provider's `save`, and writes `meta.json`.
- **Apply** — two passes: every recorded provider first writes its files to
  disk, then runs its `post_apply` hooks (template processors and the
  background hand-off), and only then one reload happens. The name is stored
  in `last_applied_theme` (see [Configuration](Configuration)).
- **List** — a theme is shown only when **every** provider it recorded is
  registered today, so a theme saved under a shell that is not active is
  hidden rather than half-applied.
- **Rename / delete** — a delete also removes the artefacts the theme derived
  outside its own directory, and clears the colour-authority descriptor when
  it names the deleted theme.

#### 🔑 Colour authority

When an applied theme owns the colours, `src/color_authority.rs` writes
`~/.cache/hve/color-authority.json` naming the backend, the theme, its
`palette_file` and the `palette_name`. That descriptor is what priority 10
reads: `colors.sh` accepts it only while `last_applied_theme` still matches
the descriptor's theme, and only for an existing, non-symlinked palette file
**inside** `~/.config/hve/themes/`. Anything else falls through to the live
chain above, exactly as if no theme had claimed the colours.

### 🔄 Keeping colours in sync

`assets/scripts/color_watcher.sh` watches the paths the modules declare,
hashes them to detect real changes, runs the owning module's refresh when a
module declared one, re-runs `assemble.sh`, writes the colour signal to
`~/.cache/hve/colors.json` and notifies HVE over `hve-ipc refresh-theme`.
See [Tray and automation](Tray-and-Automation).

### ➕ Adding a colour source

A colour source is **one new file** under `assets/scripts/color_sources.d/`.
`colors.sh` discovers that directory itself; nothing in the loader is edited
to add a source.

A module defines these four functions and nothing else at top level:

```bash
hve_colour_source_id()        # prints a stable id, e.g. "pywal"
hve_colour_source_priority()  # an integer; lower is tried first
hve_colour_source_try()       # 0 and exports the HVE_* variables on success, 1 otherwise
hve_colour_source_watch()     # the paths it wants watched, one per line
```

and may optionally add two more:

- `hve_colour_source_refresh()` — re-renders the backend's own output after
  one of its watched paths changed (its own tool, its own files);
- `hve_colour_source_tertiary()` — completes the third role from the
  module's own sources when it won the chain but left `HVE_TERTIARY` empty.

Rules the loader enforces: the id must match `[A-Za-z0-9._-]` and the
priority must be an integer, or the module is skipped silently; modules must
stay safe under `set -u`, quote every expansion, and never `eval` file
content or external data. A module may only read its **own** backend's files
— reaching into another backend's files to fill a gap is forbidden.

Because the watcher asks each module which paths it wants, adding a source
also adds its files to the watch list automatically; restart HVE after
adding one.

## 📚 See also

- [Architecture](Architecture) — where the colour system sits among the
  fragment, preset and background systems.
- [Configuration](Configuration) — `theme`, `last_applied_theme` and
  `disabled_providers`.
- [Backgrounds](Backgrounds) — the wallpaper side of a saved theme.
- [Project structure](Project-Structure) — where the scripts and theme files
  live.
