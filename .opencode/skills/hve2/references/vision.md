# HVE 2 — Visión de Producto y Referencias

## What is HVE 2

HVE 2 is a complete visual rewrite of the HVE user interface (the "facade") on top of the existing HVE engine (the "plumbing"). One window that lives and mutates: click a theme card, it expands into a settings panel inside the same window. No stacked windows.

## Agreed Scope

### In Scope
- Theme gallery IS THE STAR: the first screen and the WOW piece. It concentrates the animations — cards showing existing complete looks (colors + borders + background) applied instantly via the existing engine, with the skwd-wall presentation styles (parallelogram carousel, grid, hexagon) as part of the wow. Themes ALREADY exist (19 animations, borders, shaders, ThemeManager saves/applies complete looks); the gallery presents them.
- Complete settings section (the other new work): create and modify your own borders and animations with full per-property controls, exposed as rich internal sections from the gallery.
- Window transformation: card expands into the settings panel with animation (window resizes with the card, Hyprland animates the frame).
- Workshop: bezier curve editor, border animations/colors, per-property controls.
- Real-time apply ("everything instant" rule) using existing engine + watcher.
- Visual-only scope: colors, borders, animations, shaders, themes, backgrounds.
- EVERYTHING IS MODULAR: a module is not closed until it is fully functional (tests green, reviewed, user-approved). No half-closed modules.

### Out of Scope
- Monitors, HDR, display layout — belongs to hyprmod.
- Rewriting the engine or the provider layer.
- Copying hyprmod code (GPL). Ideas only.
- Waiting for skwd-wall Rust rewrite — it is an app, not a library; its v2 rewrite is public (GPL-3.0, Iced+Vulkan, beta) but there is nothing to integrate: ideas only, with credit.

## Module Tree Order (build order)

1. **Trunk**: new window skeleton (structure, base window, navigation shell).
2. **Theme Gallery**: cards of complete looks, instant apply (first screen).
3. **Presentation styles**: skwd-wall 1:1 (parallelogram, grid, hexagon) — decoration added when gallery works.
4. **Workshop**: bezier curves, border animations/colors, live preview.
5. **Real-time polish**: instant apply everywhere, watcher-driven refresh.

Each module closes (tests green, reviewed) before the next starts. User approves each phase.

## External References

| Source | License | How to use |
|--------|---------|------------|
| [skwd-wall](https://github.com/liixini/skwd-wall) | GPL-3.0 (not MIT) | Translate visual ideas only (card gallery, 3 presentation styles, animations) to Slint with credit; NEVER copy its code. Its Rust rewrite (v2, Iced+Vulkan) IS public now but it is still an app, not a library; do not integrate or wait for it. |
| [hyprmod](https://github.com/BlueManCZ/hyprmod) | GPL-3.0 | Ideas only: bezier curve editor, undo, pending changes, search. NEVER copy its code into HVE (GPL is contagious). |
| [HyprShades](https://github.com/sijan-dev/HyprShades) | — | Future reference: shader collection (amoled, blue-light-filter, cyberpunk, matrix, retro). |
| hyprshade | — | Future reference: shader manager with time-based scheduling. |

## Known Risks

- Hyprland bug: changing `decoration:screen_shader` at runtime with Lua config causes ~3s flicker (hyprwm/Hyprland discussion #15067). BlueManCZ is patching Hyprland for runtime decoration refresh. Risk for the "instant" goal.
- HVE and hyprmod both manage animations — if both run, last writer wins. HVE launches hyprmod CLI optionally (level 1), or yields animations when hyprmod is open (level 2). Level 3 (hyprmod responding to HVE) requires modifying GPL code — avoid.

## Technical Decisions (verified)

- Stay on Slint 1.17 (no Tauri migration). Verified: Slint supports custom `cubic-bezier(a,b,c,d)`, animate/transition with easing, Canvas 2D for drawing curves/cards, and window resize via `set_size`.
- Engine is UI-free (no slint): `src/engine.rs`, `src/config.rs`, `src/settings.rs`, `src/theme_manager.rs`, `src/app_state.rs`, `src/watcher.rs`, `src/utils.rs`, `src/providers/*` — reusable as-is.
- UI-coupled code to rewrite/separate: `src/main.rs`, `src/callbacks.rs`, `src/composer/*`, `src/ipc.rs`, `src/tr.rs`, `src/tray.rs`, `src/hypr_ipc.rs`, `src/presets.rs`, `src/countdown.rs`, `src/theme.rs`, `ui/*.slint`.
- Workflow: SDD phases (proposal -> specs -> design -> tasks -> apply -> verify -> archive), interactive approval, strict TDD (`cargo test`), hybrid artifact store (openspec + Engram).

## Delivery Preferences

- Interactive mode: pause after each phase and ask the user.
- Review budget: stop and ask if a step changes more than ~400 lines.
- PRs: auto-forecast; split into chained PRs when the forecast exceeds the review budget.
- Backup convention: full project backup tarball in ~/Documentos before major milestones (e.g. `hve-backup-YYYYMMDD-visual-rewrite.tar.gz`, excluding .git, target, node_modules).
