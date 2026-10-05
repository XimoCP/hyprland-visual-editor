# IPC

HVE runs a Unix-socket server in the background, so scripts, keybinds and a second launch can talk to the running instance without touching the GUI.

- Socket path: `$XDG_RUNTIME_DIR/hve.sock`, falling back to `/tmp/hve.sock`.
- One line per connection: the server reads a single command line, dispatches it onto the UI thread, and writes back a single reply line.
- The socket is created with owner-only permissions (`0700`), a stale socket left by a previous run is removed at startup, and the file is deleted when HVE exits.

## The client: `hve-ipc`

`hve-ipc` is the small Python client shipped as `assets/scripts/hve-ipc`. The installer offers to place it at `~/.local/bin/hve-ipc` (and optionally symlink it into `/usr/local/bin`) — see [Installation](Installation).

```bash
hve-ipc <command>
```

With no socket it prints `HVE is not running` and exits with status 1. With no argument it prints `Usage: hve-ipc <command>`. An unrecognised command is answered with `error: unknown command '<command>'`.

## Commands

| Command | What it does |
|---------|--------------|
| `show` | Show and focus the window. Idempotent: it never hides. |
| `toggle-tray` | Show or hide the window — the same action as the `SUPER + H` bind. Repeats closer than 400 ms are answered `debounced` and ignored. |
| `pause-restart` | Toggle HVE's management on or off (`system_active`). |
| `next-anim` | Apply the next animation preset; the list wraps around. |
| `next-border` | Apply the next border preset; wraps. |
| `next-shader` | Apply the next shader; wraps. |
| `status` | Reply with a JSON object: `system_active`, `active_anim_index`, `active_border_index`, `active_shader_index`, `panel_section`, `is_mutating`, `is_panel_open`. |
| `refresh-theme` | Re-read the colours from the active colour source and re-apply the theme. |
| `assert-color-authority` | Re-assert the applied theme's colours through the providers that own them. Gated: it only runs while a monitor event or a resume from suspend has armed the permission, otherwise it replies `noop`, so a manual wallpaper or theme change is never reverted. |
| `repair-color-authority` | The ungated variant, used at startup to repair a palette that was taken over while HVE was not running. |
| `quit` | Hide the window and exit HVE. |

Typical replies are `ok`, `noop`, `debounced`, the JSON body of `status`, or `error: …`.

`assert-color-authority` and `repair-color-authority` are driven by the [colour watcher](Tray-and-Automation) rather than typed by hand; the other commands are the ones meant for scripts and keybinds.

## Keyboard shortcuts

HVE writes one keybind into the `>>> HVE KEYBINDS <<<` block of `~/.cache/hve/hve-settings.lua` on every launch, so a fresh install always has:

| Combination | Action |
|-------------|--------|
| `SUPER + H` | Show or hide the window (`hve-ipc toggle-tray`) |

```lua
hl.bind("SUPER + H", hl.dsp.exec_cmd("hve-ipc toggle-tray"))
```

That bind runs `hve-ipc`, so the client must be installed for it to work. Four extra default binds (`SUPER + ALT + Q`, `N`, `B`, `S` → `pause-restart`, `next-anim`, `next-border`, `next-shader`) were removed on 2026-09-07; the commands above still exist, so you can bind any of them yourself in the same block.

## Second launch

HVE allows a single instance. A second `hve` never opens a window: it asks the running instance over the same socket to raise its window with `show` and exits. The wait is bounded to three seconds.

- If the instance answers, the message is `Another instance of HVE is already running — its window has been brought forward.` and the exit code is 0.
- If the socket cannot be reached, the message names both ways out — `pkill -x hve` to close it, or `SUPER + H` to show it — and the exit code is 1.

## See also

- [Tray and automation](Tray-and-Automation) — the pieces that call these commands for you.
- [Usage](Usage) — how the commands map onto the interface and the command line.
