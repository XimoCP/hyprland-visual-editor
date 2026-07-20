## Exploration: hve-panel-phase0

### Current State

**IPC Protocol** (`src/ipc.rs`):
- Unix socket at `$XDG_RUNTIME_DIR/hve.sock` (`0o700` — owner only)
- Plain text protocol: one line per command, response with `\n` suffix
- Sequential per-connection (blocking I/O, 5s read timeout), single-threaded listener
- **8 commands**: `pause-restart`, `next-anim`, `next-border`, `next-shader`, `toggle-tray` (400ms debounce), `status` (returns JSON), `quit`, `refresh-theme`
- No authentication beyond Unix socket permissions
- No keepalive — one command per connection

**Tray** (`src/tray.rs`):
- ksni (StatusNotifierItem) — full system tray integration
- **6 menu actions**: Toggle System, Next Animation, Next Border, Next Shader, Toggle Window (hide/show), Quit
- Communicates with HVE via `slint::Weak<MainWindow>` + `invoke_from_event_loop()` + `WINDOW_HIDDEN` atomic
- Tray icon updated via `GLOBAL_TRAY: OnceLock<Mutex<TrayHandle>>`
- System state via `system_active: Arc<AtomicBool>`

**Config** (`src/config.rs`):
- Version **3** (v3 adds `keybinds_enabled`)
- Path: `~/.config/hve/config.json`
- Deny unknown fields on deserialize
- Migration chain: v0→v1 (mark version) → v2 (add auto_minimize, language, tiling, theme) → v3 (add keybinds)
- Save/load with `config_path()` → `dirs::config_dir()/hve/config.json`

**UI Slint** (`ui/`):
- `main.slint`: MainWindow with `HorizontalLayout` — sidebar (240px) + content area
- Tab switching via `selected-module` (0=Home, 1=Animations, 2=Borders, 3=Shaders) using `width: 0%/100%` visibility toggle
- Navigation: FocusScope with keyboard arrows + mouse clicks
- Components: PresetCard, NavButton, SectionHeader, SettingsPanel, ActivationCard, AccordionCard, NavCard, CloseButton, GeometrySlider, WelcomeCard
- Modules: HomeModule, AnimationsModule, BordersModule, ShadersModule (all ScrollView-based)
- Theme: `HveColors` global — 11 color tokens (bg-dark, bg-surface, bg-card, bg-hover, text, text-muted, accent-cyan/amber/green/purple/red, border)
- Animations: Slint `animate` blocks (background, border-color, width, height, drop-shadow, x position)

**wayle-hyprland v0.2.2**:
- `HyprlandService::new()` — async, returns `Arc<Self>`, connects to Hyprland IPC
- Reactive `Property<T>` fields: `.get()` snapshot, `.watch()` stream
- Fields: `workspaces`, `clients`, `monitors`, `layers`
- `monitors`: each has `x`, `y`, `width`, `height`, `reserved`, `focused`, `scale` — all reactive
- `cursor_pos()` → `CursorPosition { x: i32, y: i32 }`
- `dispatch()` for hyprctl commands
- `events()` → Stream of `HyprlandEvent`
- Requires tokio runtime

### Affected Areas

- `src/ipc.rs` — Need to add new IPC commands for panel (e.g., `get-config`, `set-ui-mode`, `action-click`) and support for structured JSON responses for panel consumption
- `src/tray.rs` — Migration target: tray actions become panel actions. Tray retained as optional via `ui_mode` config
- `src/config.rs` — v3→v4 migration: add `ui_mode`, `panel_edge`, `panel_width`. Bump `CONFIG_VERSION` to 4
- `src/main.rs` — Panel lifecycle start/stop. New `--panel` CLI flag. Panel process spawning
- `Cargo.toml` — New `hve-panel` workspace member or crate dependency. Add `wayle-hyprland` and `tokio` to panel binary
- `ui/main.slint` — The sidebar styling is a reference for the panel's look. Panel has its own Slint file
- `openspec/changes/hve-panel/` — New change directory for SDD artifacts
- `src/hypr_ipc.rs` — Focus listener currently detects focus loss for countdown. Panel needs its own focus-awareness (e.g., show on screen edge, hide on focus loss)

### Approaches

1. **Panel as separate process (recommended)** — `hve-panel` binary in workspace
   - Pros: Independent lifecycle (panel crash ≠ HVE crash); uses tokio for wayle-hyprland (no conflict with HVE's thread-per-task model); proper layer-shell via different windowing strategy; scales independently; clean architectural boundary
   - Cons: IPC complexity between HVE and panel; need to share config; duplicate some HVE logic (theme, i18n)
   - Effort: High (new binary, IPC protocol extension, config sharing)

2. **Panel as thread within HVE (à la tray)** — Same process, new thread
   - Pros: Shared state directly (no IPC); same event loop for Slint UI; reuse existing theme/translations/tray callbacks trivially; fast time-to-implement Phase 0
   - Cons: wayle-hyprland needs tokio (conflicts with current thread model); thread crash kills HVE; single window management is awkward for a layer-surface panel; couples both lifecycles
   - Effort: Medium

3. **Panel inside HVE window (extend sidebar)** — Make the existing 240px sidebar the panel, add toggle to show/hide from Hyprland edge
   - Pros: Minimal code changes; reuse all existing UI; no IPC needed; trivial to implement
   - Cons: Not a real layer-shell panel; can't reserve screen space; HVE must be showing for the panel to work; defeats the "system panel always visible" goal
   - Effort: Low

### Recommendation

**Approach 2 (thread within HVE) for Phase 0**, with an eye toward extracting to a separate process in a later phase.

Rationale:
- Phase 0 is about replacing the tray as primary interaction — not building a full layer-shell panel
- A thread-based approach reuses ALL existing HVE infrastructure (theme, i18n, config, Engine, callbacks) without any IPC
- The panel UI is Slint, which needs the HVE event loop anyway
- wayle-hyprland can run on a separate tokio runtime (multi-runtime tokio is fine), or we use `hyprctl` via `std::process::Command` for cursor/monitor queries (simpler)
- The "separate process" migration is a natural Phase 1 if/when we need proper layer-shell

For edge detection:
- Use `hyprctl cursorpos` (simpler, no new dependency) to get cursor position
- Use `hyprctl monitors` to get monitor geometry
- Or: add `wayle-hyprland` with a dedicated lightweight tokio runtime
- Phase 0: `hyprctl cursorpos` + `hyprctl monitors -j` via `std::process::Command`
- Phase 1+: proper wayle-hyprland with reactive monitor subscriptions

### Risks

- **Tokio + HVE threading**: HVE uses blocking threads. If we add tokio for wayle-hyprland, we need careful runtime isolation (tokio::runtime::Runtime in a dedicated thread). Mitigation: Phase 0 uses `hyprctl` commands instead of wayle-hyprland
- **Slint layer-shell limitations**: Slint 1.17 doesn't support wlr-layer-shell directly. Panel positioning relies on `hyprctl keyword monitor ... addreserved` + window rules. Mitigation: Use window geometry + hyprctl windowrule to pin the panel window
- **Countdown auto-minimize conflicts**: The countdown system hides the window on focus loss. A panel-window shouldn't auto-minimize, or should behave differently. Need separate countdown logic for the panel window
- **Dual-window management**: HVE + panel = 2 windows. Close/quit logic must handle both. Need coordinated lifecycle

### Ready for Proposal

Yes — all findings are documented. The orchestrator should:
1. Proceed to `sdd-propose` for `hve-panel-phase0`
2. Key decisions for the proposal: `ui_mode` config semantics, panel dimensions, Phase 0 scope boundaries (what the panel actually shows — quick controls like current tray functions?)
3. Decide if wayle-hyprland dependency is Phase 0 or deferred to Phase 1
4. Define the `ui_mode` enum and backward compatibility with existing tray
