# Creation audit, dead-artifact cleanup, and a proven uninstaller

Branch: `hve2-visual-rewrite` — delivery: work-unit commits on the feature branch.
Test runner: `cargo test`; shell contracts live in `src/scripts_contract.rs`.

Keeper asked for four things in one pass: an internal audit of everything HVE
creates (what is still functional, what is a leftover, and cleaning the
leftovers with real verification), proof that `uninstall.sh` removes neither
too much nor too little, the list of the keeper's personal files the installer
touches, and confirmation that the wiki documents every file HVE creates.

## Specs

S1 (keeper, verbatim): "quiero que hagas una auditoria interna. Revisaremos lo
que se crea de lo que esta creado y es funcional y limpiaremos todo lo que no
sea necesario, siempre con una verificación real." Every artifact HVE leaves on
disk has a named producer and a named consumer in current code (or a documented
reason to exist without one). An artifact that no current code produces is a
LEFTOVER and is deleted from the keeper's machine; a producer whose artifact
nothing consumes is DEAD CODE and is removed from the repository. Each removal
is proven by an observed check: the focused test, the full suite, or a
before/after listing of the exact path.

S2 (keeper, verbatim): "tenemos que probar o verificar que nuestro
desistalador funcione correctamente. Que no limpie de mas ni de menos." An
automated test installs a representative set of artifacts into a sandboxed HOME
and asserts the exact removal set: nothing outside HVE's own paths is touched
(no user themes, no downloaded videos, no unrelated files), and nothing HVE
installed survives — including HVE's block in `~/.config/hypr/hyprland.lua`,
which today only the watchdog removes, and the watchdog is deleted when the
keeper accepts cache removal.

S3 (keeper, verbatim): "quiero que me digas que archivos toca nuestro
instalador de mis archivos personales." The installer's footprint is reported
path by path, marking which entries are optional (a prompt answered yes) and
which are unconditional, and naming the files it edits rather than only the
files it copies.

S4 (keeper, verbatim): "ver si estan registrados todos los archivos que me has
comentado que crea y para que sirven la wiki." `WIKI.md` documents every path
listed in S1's map with its purpose, and its statements match current behaviour
(no dual `.conf`/`.lua` format, no `hve_format` cache, current config version).

## Tasks

- T1 (inline) this document. Done.
- T2 (delegated, read-only) artifact map: producer, consumer, regenerable vs
  user state, and leftover verdict per path. Done: see "Audit map" below.
- T3 (inline) repository cleanup: drop dead producers/tests; keep the suite green.
  Done: `assemble.sh` no longer creates the `overlay.current` symlink;
  `color_watcher.sh` no longer writes `colors.json`; guard test
  `scripts_do_not_resurrect_retired_cache_artifacts`.
- T4 (inline) uninstaller: sandbox HOME test + the missing cleanup it exposes.
  Done: `uninstaller_removes_everything_it_installed_and_nothing_else` and
  `uninstaller_deletes_cache_and_config_only_when_asked` (RED observed with the
  old script: both failed on the surviving Hyprland block). Two real gaps fixed:
  the HVE block in `hyprland.lua` is now removed by the uninstaller itself
  (before it depended on the watchdog, which the same run deletes when the cache
  is removed), and a pending skwd-wall mute is released before the marker goes
  away. Also: the system symlink is removed with plain `rm` when it is
  user-writable and only falls back to `sudo`, and its path is overridable
  (`HVE_IPC_SYMLINK_PATH`) so the test never touches `/usr/local/bin`.
- T5 (inline) live-machine cleanup of confirmed leftovers only (app closed).
- T6 (inline) installer footprint report + wiki correction.
- T7 (delegated) cross-model verification of T3/T4 (deletes and file handling:
  Tier 3 per AGENTS.md).

## Log

L1 (keeper, verbatim): "bueno pues quiero que hagas una auditoria interna.
Revisaremos lo que se crea de lo que esta creado y es funcional y limpiaremos
todo lo que no sea necesario, siempre con una verificación real. Luego también
tenemos que probar o verificar que nuestro desistalador funcione correctamente.
Que no limpie de mas ni de menos. Luego quiero que me digas que archivos toca
nuestro instalador de mis archivos personales. Y para finalizar ver si estan
registrados todos los archivos que me has comentado que crea y para que sirven
la wiki."

L2 (context): the session started from `~/.cache/hve/overlay.current` — a symlink
`assemble.sh` still creates for a Noctalia plugin generation that no longer
exists. The keeper asked why it exists, then widened the question to the whole
set of artifacts.

## Audit map

Evidence from a read-only sweep of `src/**` and `assets/scripts/**` at
`086f9ea`, cross-checked against the live tree. "Leftover" means a producer with
no consumer, or an artifact with no producer at HEAD.

### Verdict: functional, keep

| Artifact | Producer | Consumer |
|---|---|---|
| `~/.config/hve/config.json` | `Config::save` (`src/config.rs:246`) | `Config::load` |
| `~/.config/hve/themes/` | `ThemeManager` (`src/theme_manager.rs:311`) | gallery/apply |
| `~/.config/hve/presets/` | `PresetStore` (`src/preset_store.rs:42`) | `theme_presets`, panel |
| `~/.cache/hve/hve-settings.lua` | `src/settings.rs:78`, `init.sh` | `hyprland.lua:38` |
| `~/.cache/hve/overlay.lua` | `assemble.sh` | `hyprland.lua:36` |
| `~/.cache/hve/hve_watchdog.sh` | `init.sh:89` | `hyprland.lua:40` |
| `~/.cache/hve/thumbs/` | `gallery/thumbs.rs:263` | gallery views |
| `~/.cache/hve/hve.lock` | `src/main.rs:3500` | single instance |
| `~/.cache/hve/hve.log` | `src/main.rs:3380` | diagnostics |
| `~/.cache/hve/color_watcher.{log,lock}` | `color_watcher.sh`, `src/watcher.rs` | watcher |
| `~/.cache/hve/sentinel.log` | `hve-sentinel.py:53` | diagnostics |
| `~/.cache/hve/color-authority.{json,log}` | `color_authority.rs:45`, `decision_log.rs:35` | `skwd_policy`, `background` |
| `~/.cache/hve/keeper-palette.json` | `color_authority.rs:50` | palette guard |
| `~/.cache/hve/color-write.last` | `color_authority.rs:174` | `ipc`, `noctalia` |
| `~/.cache/hve/assert-color-authority.last` | `color_watcher.sh:169` | cooldown |
| `~/.cache/hve/skwd-policy-yield.json` | `skwd_policy.rs:134` | crash recovery |
| `~/.cache/hve/picker-voice.json` | `skwd_policy.rs:1042` | picker grace |
| `~/.cache/hve/theme-trust.toml` | `theme_trust_store.rs:83` | trust prompts |
| `~/.cache/hve/reload.{request,drain}.lock` | `reload_coalescer.sh` | reload handshake |

### Verdict: leftovers, removed

| Artifact | Why | Action |
|---|---|---|
| `~/.cache/hve/overlay.current` | `assemble.sh:105` created it; zero consumers in `src/`, in `assets/` or in the live `hyprland.lua` (which `dofile`s `overlay.lua`). Born in `c6eb1fe` when the format was detected at runtime. | producer removed from `assemble.sh`; guard test added; stale copy deleted from the keeper's cache |
| `~/.cache/hve/colors.json` | written by `color_watcher.sh:125`; no reader left after the v4 backend removal (`a9697ec`) | writer and its two call sites removed; guard test added; stale copy deleted |
| `~/.cache/hve/hve-settings.conf` | no producer at HEAD (removed in `120eb91`); deliberately orphaned by `settings.rs` (never copied, never read) | deleted from the keeper's cache; documented as removable |
| `~/.cache/hve/hve_format` | removed in `120eb91`/`b89fb73`; already absent from the live cache | nothing to do |
| `~/.config/hve/backups/` | no producer anywhere in the repo; a hand-made directory from task work | left alone (not HVE's) |
| `~/.cache/noctalia/HVE/`, `~/.config/noctalia/{plugins/,}hyprland-visual-editor/` | out-of-repo copy of an OLDER HVE shipped as a Noctalia plugin; the plugin is not enabled | reported to the keeper, not deleted by HVE |

### Verdict: real user state, never treated as cache

`theme-trust.toml` (losing it re-asks every import), `hve-settings.lua` (the
keeper edits keybinds by hand) and everything under `~/.config/hve/`.
