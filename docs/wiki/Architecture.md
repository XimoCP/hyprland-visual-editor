# Architecture

HVE splits the look of your desktop into small snippets called **fragments**
— one for the animation, one for the border, one for the geometry, one for
the shader. `assets/scripts/assemble.sh` collects the active fragments
together with the colours detected on your system and writes a **single
overlay file** that Hyprland loads alongside your own configuration.

HVE never rewrites your files. Enabling it appends one block between markers
in `hyprland.lua`; that block only `dofile`s `~/.cache/hve/overlay.lua` and
`~/.cache/hve/hve-settings.lua` and registers the watchdog. Disabling removes
the whole block again, and every generated file is written to a temporary
path and moved into place — so an uninstall leaves your configuration exactly
as it was.

This page covers how fragments are assembled, which configuration format HVE
emits, and where each subsystem lives. The seams that keep the pieces
independent are at the end; each individual system has its own page.

## Fragments and assembly

The core of the system is `assets/scripts/assemble.sh`, which:

1. **Resolves the active colours** by sourcing `colors.sh`.
2. **Writes a temporary overlay** containing:
   - the header (`HYPRLAND VISUAL EDITOR - MASTER OVERLAY`, `Lua mode`);
   - the colour variables — one line per role in the palette roster
     (`primary`, `secondary`, `tertiary`, `error`, `surface`,
     `surface_lowest`, `accent`);
   - the baseline linear bezier curve;
   - the active fragments, in this order: `animation.lua`, `border.lua`,
     `shader.lua`, `geometry.lua`.
3. **Validates the result**: classic `general {`, `decoration {` or
   `animations {` blocks are classic syntax, not Lua, so the write is
   aborted and the previous overlay is kept.
4. **Replaces the destination atomically** with `mv`.
5. **Refreshes the `overlay.current` symlink** to point at the new overlay.
6. **Queues one `hyprctl reload`** through `reload_coalescer.sh` — bursts of
   changes fold into a single reload instead of one per click.

If `colors.sh` declares no palette roster, or validation fails, `assemble.sh`
deletes the temporary file and leaves the active overlay untouched.

```
assemble.sh
  ├── colors.sh        → colour variables
  ├── fragments/
  │   ├── animation.lua   ← written by apply_animation.sh
  │   ├── border.lua      ← written by border.sh
  │   ├── shader.lua      ← written by shader.sh
  │   └── geometry.lua    ← written by geometry.sh
  └── overlay.current  → symlink to the active overlay
```

The fragments directory lives inside the installed assets (see
[Project structure](Project-Structure)), and is overwritten every time you
pick a preset.

## Lua vs Conf

Hyprland 0.55+ defaults to a Lua configuration — `hl.` calls and `require()`
— and HVE 2 targets exactly that. Earlier versions shipped every animation
and border preset twice (`.conf` and `.lua`) and probed your installation
with `detect_format.sh`, caching the answer in `~/.cache/hve/hve_format`.

HVE 2 **never detects the format at runtime**:

- `assemble.sh` writes `overlay.lua` and nothing else.
- `scan.sh` lists `*.lua` and `*.frag` only — no format detection, no `.conf`
  globbing.
- `init.sh` refuses to enable HVE on a legacy `hyprland.conf`-only setup and
  asks you to migrate first; it manages `hyprland.lua`.
- `detect_format.sh` and `format_test.sh` and the 32 `.conf` preset twins
  were removed; `src/scripts_contract.rs` fails the build if any script
  mentions `detect_format` or `hve_format` again.

The rule itself is written down in `openspec/specs/lua-only-config/spec.md`.
Shaders are `.frag` files with no second format, because Hyprland reads the
same shader file regardless of how your configuration is written.

## The systems

| System | What it does | Where | Page |
|--------|--------------|-------|------|
| Colour and theme | Picks the colour source, feeds the overlay, and derives the app's own colours | `assets/scripts/colors.sh`, `assets/scripts/color_sources.d/`, `src/theme.rs`, `src/theme_manager.rs` | [Themes and colours](Themes-and-Colours) |
| Presets | Scans animation, border and shader presets and their metadata, applies one or none | `assets/scripts/scan.sh`, `src/presets.rs` | [Presets](Presets) |
| Backgrounds | Static wallpapers and animated (video) wallpapers, and who paints them | `src/providers/background.rs`, `src/providers/mpvpaper.rs`, `src/providers/wallpaper_authority.rs` | [Backgrounds](Backgrounds) |
| IPC | A Unix socket plus the `hve-ipc` client and the default shortcuts | `src/ipc.rs`, `assets/scripts/hve-ipc` | [IPC](IPC) |
| Tray | The tray icon and its menu, plus the colour watcher, auto-minimise and the safety watchdog | `src/tray.rs`, `src/countdown.rs`, `assets/scripts/hve_watchdog.sh` | [Tray and automation](Tray-and-Automation) |
| Internationalisation | English and Spanish strings embedded at compile time, resolved by dot-notation key | `src/tr.rs`, `i18n/` | [Configuration](Configuration) |
| Configuration | The persistent `config.json`, its fields and its migrations | `src/config.rs` | [Configuration](Configuration) |

## Hyprland is the base, never a backend

HVE is built for Hyprland and nothing replaces it. Compositor-specific
knowledge — the `hyprctl` calls, the reload, the workspace queries — lives
behind the `Composer` seam (`src/composer/mod.rs`), whose only implementation
is `HyprlandComposer` (`src/composer/hyprland.rs`). The rest of the code talks
to `Composer`, never to Hyprland directly.

## Shell agnosticism

HVE is agnostic of the **desktop shell** — the layer that sits on top of
Hyprland (Noctalia, DMS, Calestia, …). It is **not** agnostic of Hyprland:
Hyprland is the base, hence the name. The shell is a separate axis from the
compositor.

This is exactly what is true today, and the tests that keep it true.

**The core never names a shell in its production code.** A core file's
production code never spells a shell name, a shell path, a shell command line
or a shell plugin id (the check strips comments and test fixtures). The core
files are `src/main.rs`, `src/callbacks.rs`, `src/theme_manager.rs`,
`src/config.rs`, `src/watcher.rs`, `src/ipc.rs`, `src/tray.rs`,
`src/engine.rs` and `src/shell/**`.

**Everything the core needs is a capability.** The operations the core needs
from the running shell are methods of `ShellCapabilities`
(`src/providers/shell_capabilities.rs`): the live wallpaper, a theme's saved
wallpaper, whether an apply arms the late palette re-assert, and
Do-Not-Disturb status and set. Every method has a declined default, so a shell
that cannot provide one keeps the neutral path and nothing breaks. The core
resolves the running shell through `providers::active_shell()` and calls only
these methods.

**Adding another shell is one entry.** The registration router
(`src/providers/mod.rs`) carries a data-driven backend list
(`id / detect / fallback / make / declare`). Adding a shipped shell is one
entry plus its provider module, never an `if` in the core.

**The proof is a test.** `src/shell_agnosticism_tests.rs` drives the core end
to end through a test-only fake shell adapter and a fake theme provider:
backfill, palette re-assert arming, DND engage and restore, and
list/save/apply/preview/facts. If adding a new shell ever forces an edit to
those tests, the claim is false.

**The guard is in the build.** `src/architecture_contract.rs` fails the build
when any shell name (`noctalia`, `quickshell`, `calestia`, `dms`) appears in a
core file, and a tripwire fails when a new `src/shell/` module joins the tree
unpoliced.

### What is pluggable

- The colour sources are genuinely pluggable: each source is one module under
  `assets/scripts/color_sources.d/`, discovered from that directory, so adding
  a source is one new file there — `assets/scripts/colors.sh` is the loader
  and is never edited to add one.
- A theme provider can be implemented against `ThemeProvider`
  (`src/theme_manager.rs`) without touching the core.
- The compositor sits behind `Composer` (`src/composer/mod.rs`).
