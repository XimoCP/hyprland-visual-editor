# 🗂️ Project structure

**A map of the repository and of the files HVE creates at runtime.**

This page is for developers working on HVE. If you are here to use the app, you do not need it — see [Usage](Usage).

The repository layout, what each area is responsible for, and the files HVE creates at runtime.

```
hve/
├── Cargo.toml              ← Rust dependencies
├── build.rs                ← Slint UI compilation
├── install.sh              ← Installer (559 lines, bilingual)
├── uninstall.sh            ← Uninstaller (254 lines, bilingual)
├── hve.desktop             ← Desktop entry (base)
├── i18n/
│   ├── en.json             ← English translations
│   └── es.json             ← Spanish translations
├── ui/
│   ├── main.slint          ← Main window (Slint)
│   ├── shell.slint         ← HVE shell: navigation and slot mounting
│   ├── components.slint    ← Reusable components (buttons, panels)
│   ├── tokens.slint        ← Design tokens
│   ├── theme.slint         ← Global colour tokens (HveColors), NavModule
│   ├── modules.slint       ← Legacy stub; the hidden tab modules are gone
│   ├── gallery/            ← Gallery views (slice, mosaic, hexagon), thumbnails, cards
│   └── panel/              ← Panel root, menu, colour picker, curve preview + sections/
├── docs/wiki/              ← This manual (English, one file per page)
├── src/                    ← Source code (see below)
└── assets/
    ├── hve_logo.svg        ← Vector icon
    ├── owl_neon.png        ← Decorative image
    ├── branding/           ← Social preview / promo art (the GitHub OG image)
    ├── themes/             ← Reference themes, shipped as self-contained packages
    ├── scripts/            ← Scripts + color_sources.d/ (see below)
    ├── animations/         ← 19 animation presets (.lua)
    ├── borders/            ← 14 border presets (.lua)
    ├── shaders/            ← 9 GLSL shaders (.frag)
    └── fragments/          ← Dynamic fragments (runtime, git-ignored)
```

## 🦀 Source code (src/)

### ⚙️ Core

| File | Lines | Responsibility |
|------|-------|----------------|
| `main.rs` | 7221 | Entry point, CLI (`--tray`, `-v`), startup of every system, single-instance lock, listeners, navigation |
| `config.rs` | 741 | Persistent JSON config, migrations v0→v8, `deny_unknown_fields`, XDG paths |
| `engine.rs` | 709 | Typed interface over the shell scripts, `EngineError`, `ColorScheme`, `PresetInfo`, `ScanEntry` |
| `settings.rs` | 1061 | Writes `hve-settings.lua`: window rules, the `SUPER + H` keybind and the autostart block |
| `callbacks.rs` | 1894 | UI ↔ logic: preset toggles, system toggle, geometry |
| `app_state.rs` | 135 | Central view-model for the application |
| `presets.rs` | 323 | Preset scanning and population, i18n of metadata, active selection |
| `preset_store.rs` | 970 | User preset store: save/list/delete under `~/.config/hve/presets/{borders,animations}` |
| `animation_preset.rs` | 766 | Pure Lua animation preset model and reader |
| `border_preset.rs` | 1003 | Pure Lua border preset reader |
| `tray.rs` | 277 | Tray icon (ksni), context menu, active/inactive colours |
| `ipc.rs` | 1065 | Unix socket server at `$XDG_RUNTIME_DIR/hve.sock`, commands + `status` JSON |
| `hypr_ipc.rs` | 1097 | Hyprland event listener (`socket2.sock`), throttled UI refresh |
| `theme.rs` | 490 | Theme resolution (dark/light/system), palettes, derived Slint colours |
| `theme_manager.rs` | 2457 | Complete themes: list, save, apply, rename, delete (with providers) |
| `tr.rs` | 332 | Embedded i18n JSON, dot-notation lookup, language detection |
| `countdown.rs` | 595 | Countdown UI and auto-minimise on focus loss |
| `watcher.rs` | 1902 | Spawning and supervising `color_watcher.sh`, logging |
| `show_state.rs` | 262 | Window show/hide state |
| `panel_i18n.rs` | 367 | Fills the Slint text globals from the translation map |
| `utils.rs` | 7 | Shared file-editing helpers |

### 🛡️ Compositor, guards and contracts

| File | Lines | Responsibility |
|------|-------|----------------|
| `composer/mod.rs` | 1429 | Compositor driver abstraction (`Composer` trait) |
| `composer/hyprland.rs` | 1265 | Hyprland implementation: overlay, `hyprctl` queries, workspaces |
| `config_guard.rs` | 243 | Migration guard: HVE enables only on a valid `hyprland.lua` |
| `config_markers.rs` | 93 | Single source of truth for every marker string |
| `color_authority.rs` | 187 | Declarative colour-authority descriptor in the cache dir |
| `reload_coalescer.rs` | 902 | Sandboxed behavioural tests for the reload coalescer |
| `scripts_contract.rs` | 2005 | Deterministic shell-contract assertions for the Lua-only script layer |
| `architecture_contract.rs` | 920 | Capability-routing architecture contract |

### 🔌 Providers (src/providers/)

| File | Lines | Responsibility |
|------|-------|----------------|
| `mod.rs` | 499 | Provider registration router: one entry per shipped backend |
| `noctalia.rs` | 6550 | Noctalia v4/v5: colours, wallpapers, template processor, mpvpaper manifest |
| `noctalia_paths.rs` | 474 | Noctalia's own path and launcher knowledge (v4 Quickshell, v5) |
| `noctalia_runtime.rs` | 655 | Noctalia runtime access: `noctalia msg` IPC and on-disk paths |
| `shell_capabilities.rs` | 60 | The shell-capability seam: what the core asks of the running shell |
| `background.rs` | 1024 | `apply-background` capability router |
| `wallpaper.rs` | 351 | Wallpaper provider state after parsing `wallpapers.json` |
| `wallpaper_authority.rs` | 2817 | Single owner of "who controls the wallpaper right now" |
| `bg_info.rs` | 933 | Shared background (wallpaper) information |
| `mpvpaper.rs` | 763 | Animated (video) wallpapers for Noctalia v5 themes |
| `skwd_engine.rs` | 399 | skwd-wall adapter: the one place HVE hands a wallpaper path over |
| `skwd_policy.rs` | 1284 | Policy toggle primitives around apply |
| `hve_presets.rs` | 451 | HVE preset provider (animation, border, shader, geometry) |

### 🐚 Shell, panels and desktop preference

| File | Lines | Responsibility |
|------|-------|----------------|
| `shell/mod.rs` | 962 | HVE shell: state-driven navigation and slot mounting |
| `shell/nav.rs` | 668 | Pure navigation state machine |
| `shell/slots.rs` | 248 | Slot mounting contract |
| `shell/size.rs` | 233 | Window sizing policy |
| `shell/gallery/` | 9948 | Gallery model, slots, thumbnails and the slice/mosaic/hexagon views |
| `theme/desktop_preference/` | 393 | `read-desktop-preference` capability (darkman, GNOME settings) |

### 🧪 Tests

| File | Lines | Responsibility |
|------|-------|----------------|
| `shell/ui_tests.rs` | 18255 | Headless tests for the Slint UI |
| `shell/integration_tests.rs` | 379 | Headless integration tests for the shell |
| `shell_agnosticism_tests.rs` | 187 | The core driven end to end by a fake shell adapter |
| `test_utils.rs` | 78 | Shared test utilities |

## 📜 Scripts (assets/scripts/)

| Script | Lines | Function |
|--------|-------|----------|
| `utils.sh` | 177 | Shared paths: `HVE_ASSETS_DIR`, `HVE_SCRIPTS_DIR`, `HVE_FRAGMENTS_DIR`, `HVE_SAFE_DIR`, `HVE_HYPR_DIR`; XDG resolution and preset-name safety helpers |
| `assemble.sh` | 117 | **Heart of the system**: colours + fragments → atomic overlay, syntax check, queued reload |
| `init.sh` | 163 | Enable/disable HVE: injects or removes the markers block in `hyprland.lua`, creates `hve-settings.lua` with defaults |
| `colors.sh` | 650 | Colour loader: discovers the modules in `color_sources.d/`, runs them by priority and exports the palette |
| `color_watcher.sh` | 281 | `inotifywait` watcher with hash-based change detection and a 30 s timeout, regenerates the overlay |
| `scan.sh` | 91 | Preset scanner: metadata parsing (@Title, @Desc, @Tag, @Icon, @Color), JSON output |
| `shader.sh` | 63 | Applies a `.frag` shader, writes the wrapper with `decoration.screen_shader`, "none" clears it |
| `geometry.sh` | 73 | Writes the geometry fragment (`border_size`, `rounding`, `gaps_in`, `gaps_out`) |
| `apply_animation.sh` | 33 | Applies an animation preset, copies it to `fragments/animation.lua` |
| `border.sh` | 32 | Applies a border preset, copies it to `fragments/border.lua` |
| `get_colors.sh` | 11 | Wraps `colors.sh`, JSON output |
| `hve_watchdog.sh` | 50 | Safety: if HVE is gone, removes the markers and clears the cache |
| `reload_coalescer.sh` | 194 | One `hyprctl reload` per change-burst, never lost |
| `hve-ipc` | 26 | Python client for the Unix socket |
| `hve-sentinel.py` | 282 | Listens on Hyprland's event socket to catch a theme transition where the shell bar stays above HVE |
| `color_sources.d/` | — | One module per colour source, each declaring its own priority: Noctalia palette (20), Noctalia Lua (30), pywal (40), matugen (50), manual Hyprland scan (60). Lower runs first. |

## 🎬 Animation presets (assets/animations/)

19 animations, each a `.lua` file:

`01_relampago`, `02_inercia_elastica`, `03_seda_minimalista`, `04_minimalismo_snappy`,
`05_material_moderno`, `06_impacto_clasico`, `07_lineal`, `08_cristal`, `09_seda_silk`,
`10_retro_arcade`, `11_futurista`, `12_rebote`, `13_organico`, `14_elastico`,
`15_desvanecido`, `16_dinamico`, `17_sutil`, `18_energico`, `19_stylized2.5D`

## 🔲 Border presets (assets/borders/)

14 borders, each a `.lua` file: `01_cascade` through `14_looper`.

## ✨ Shaders (assets/shaders/)

9 GLSL shaders (`.frag`): `01_night`, `02_mono`, `03_vibrant`, `04_sharp`, `05_ink`, `06_invert`, `07_oled`, `08_vision`, `09_hybrid`.

They have no second format because Hyprland uses the same file regardless of how your configuration is written.

## 🧩 Dynamic fragments (assets/fragments/)

This folder lives in `~/.local/bin/assets/fragments/` (next to the installed assets) and holds the fragments currently in use. It is created at runtime and git-ignored, because every preset change rewrites it:

- `animation.lua` — the active animation
- `border.lua` — the active border
- `geometry.lua` — border width, corner radius and gaps
- `shader.lua` — the active shader (wrapper)

They are **overwritten** every time you change a preset.

## 💾 Runtime files

| Path | Purpose |
|------|---------|
| `~/.config/hve/config.json` | Persistent user configuration |
| `~/.config/hve/themes/` | Saved complete themes |
| `~/.config/hve/presets/{borders,animations}/` | Your saved presets |
| `~/.cache/hve/hve.lock` | Exclusive lock (prevents multiple instances) |
| `~/.cache/hve/hve-settings.lua` | Managed fragment: window rules, keybinds and autostart |
| `~/.cache/hve/overlay.lua` | Active overlay, assembled by `assemble.sh` |
| `~/.cache/hve/overlay.current` | Symlink to the active overlay |
| `~/.cache/hve/overlay.tmp` | Temporary overlay, moved into place with `mv` |
| `~/.cache/hve/colors.json` | Colour signal, written by `color_watcher.sh` |
| `~/.cache/hve/color_watcher.log` | Colour-watcher log |
| `~/.cache/hve/color-authority.json` | Colour-authority descriptor |
| `~/.cache/hve/hve_watchdog.sh` | Watchdog copy deployed by `init.sh` |

See [Configuration](Configuration) for the config file and [Architecture](Architecture) for how the pieces fit together.
