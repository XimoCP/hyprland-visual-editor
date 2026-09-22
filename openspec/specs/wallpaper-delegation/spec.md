# Wallpaper Delegation Specification

## Purpose

Defines how HVE applies a theme's wallpaper and provider files while staying daemon-agnostic: it owns files and reload, delegates best-effort to the running wallpaper daemon, and never assumes a single backend.

## Requirements

HVE MUST remain wallpaper-daemon agnostic. Applying a theme MUST copy saved provider files from `themes/{name}/providers/{id}/` back to their live config locations and trigger a reload, without assuming which daemon paints the wallpaper. HVE MUST NOT hardcode a single wallpaper backend.

#### Scenario: Apply restores files and reloads

- GIVEN a saved theme "forest" with wallpaper `forest.jpg` and config files
- WHEN user applies "forest"
- THEN HVE copies provider files to live locations and triggers reload_after_post_apply
- AND visible wallpaper reflects `forest.jpg` regardless of which daemon paints it

#### Scenario: Save captures current files

- GIVEN user changed settings and wallpaper is `current.jpg`
- WHEN user saves theme "my-theme"
- THEN HVE copies current live config files (including wallpaper reference) into `themes/my-theme/providers/`

The force-wallpaper-off policy introduced 2026-09-06 in `src/providers/noctalia.rs` (force_noctalia_wallpaper_off / post_apply skip gate) MUST be removed. Wallpaper IPC MUST be unconditional (pre-Sep-6 behavior). No gate may suppress wallpaper application.

#### Scenario: Wallpaper IPC not suppressed

- GIVEN theme apply includes wallpaper `wall.jpg`
- WHEN Noctalia provider post_apply runs
- THEN wallpaper IPC is attempted unconditionally (no force-off check)

After successful Noctalia wallpaper IPC, HVE MUST best-effort forward the same wallpaper path to `skwd-walld` ONLY when its socket exists at `$XDG_RUNTIME_DIR/skwd-wall-v2/wall.sock` (typically `/run/user/1000/skwd-wall-v2/wall.sock`). Delegation MUST be silent no-op when socket absent, and MUST never fail the theme apply.

#### Scenario: Delegation when daemon present

- GIVEN theme with `wall.jpg` and skwd-walld socket exists
- WHEN theme apply succeeds via Noctalia
- THEN HVE sends `wall.jpg` path to skwd-walld socket (best-effort)
- AND theme apply still succeeds even if socket write fails

#### Scenario: No-op when daemon absent

- GIVEN theme with `wall.jpg` and no skwd-walld socket
- WHEN theme apply runs
- THEN no delegation is attempted and apply succeeds unchanged

#### Scenario: Delegation failure does not fail apply

- GIVEN skwd-walld socket exists but is unresponsive
- WHEN delegation is attempted
- THEN error is logged as warning and apply returns success

Delegation MUST be invisible — no UI indicator for delegated wallpaper. External manual wallpaper changes made outside HVE (direct skwd-walld calls) MUST be ignored by HVE (no sync, no conflict warning).

#### Scenario: No UI indicator

- GIVEN wallpaper was delegated to skwd-walld
- WHEN gallery renders
- THEN no badge, text, or icon indicates delegation

#### Scenario: External change ignored

- GIVEN user changed wallpaper externally via skwd-walld to `other.jpg`
- WHEN HVE lists or shows themes
- THEN HVE continues to show its own theme state, not `other.jpg`

ThemeManager apply MUST keep two passes: (1) all providers `apply` files, (2) all providers `post_apply` hooks, then single `reload_after_post_apply`. Errors in `post_apply` MUST be logged as warnings and MUST NOT fail apply (existing behavior preserved).

#### Scenario: Post-apply warning does not block reload

- GIVEN a provider post_apply returns error
- WHEN theme apply runs
- THEN warning is logged and reload still executes

## Out of Scope

- Deep bidirectional sync, watching external wallpaper changes
- Noctalia 600s automation handling
- Visual styles, monitor/HDR, other providers
