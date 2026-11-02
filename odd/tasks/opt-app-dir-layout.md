# One basket: the app dir holds the binary and its assets

Branch: `hve2-visual-rewrite` — delivery: work-unit commits on the feature branch.
Test runner: `cargo test`; install/uninstall contracts live in `src/scripts_contract.rs`.

Today the installer drops the binary and a generic `assets/` folder side by side
in `~/.local/bin`. The keeper asked why the assets are not inside an `hve`
folder, and after seeing the cost decided to move them: `~/.local/bin` is for
executables, `assets` is a name that collides with anything else, and moving the
binary alone silently breaks every script.

## Specs

S1 (keeper, verbatim): "osea que sería recomendable hacer lo que te he dicho
yo.. además de que así no tenemos tantos huevos fuera del cesto, no?" One HVE
folder holds everything HVE installs: `~/.local/opt/hve/` with the real binary
`hve` and `assets/` next to it. `~/.local/bin` keeps exactly one entry, the
`hve` symlink that points at that binary.

S2 (keeper, verbatim): "vale pues dale caña" — after the analysis the keeper
authorizes the change: installer, uninstaller, code, tests and docs as one unit.

S3: Upgrading an existing install must not leave two copies. The installer
moves the binary and the assets into the app dir and removes the legacy
`~/.local/bin/assets` (and a legacy regular file at `~/.local/bin/hve`) in the
same run.

S4: The uninstaller removes the app dir, the symlink, and — for installs that
never ran the new installer — the legacy `~/.local/bin/assets` and regular
binary, exactly as before. Nothing else.

S5: The application finds its assets in both layouts. Resolving stays a chain:
the real executable's directory (the app-dir layout) first, the legacy
`~/.local/bin/assets` second, the source tree last, so a dev build and an
upgraded install both work. The sentinel autostart line follows the same chain,
and a missing `assets/` must be observable rather than silently ignored.

## Tasks

- T1 (inline) this document. Done
- T2 (inline) `settings.rs`: sentinel path follows the shared chain, with unit
  tests for the order and for the app-dir layout.
- T3 (inline) `install.sh`: app dir, symlink, assets into the app dir, legacy
  migration.
- T4 (inline) `uninstall.sh`: app dir + symlink + legacy paths.
- T5 (inline) tests: install-layout contract, uninstaller sandbox with the new
  layout, sentinel chain.
- T6 (inline) WIKI/README: install table, runtime table, uninstall table.
- T7 (delegated) migrate the keeper's real install and verify the app resolves
  its assets; cross-model verification of the change (Tier 3: moves files).

## Log

L1 (keeper, verbatim): "pregunta... por que no hemos creado dentro de .local/bin/hve
la carpeta de assets? Por que no esta así: .local/bin/hve/assets?"

L2 (keeper, verbatim): "entonces es la mejor opción la que tenemosh??"

L3 (keeper, verbatim): "osea que sería recomendable hacer lo que te he dicho yo..
además de que así no tenemos tantos huevos fuera del cesto, no?"

L4 (keeper, verbatim): "quiero que analices lo que has dicho el coste de
cambiarlo... si es recomendable y si son buenas practicas y el riesgo."

L5 (keeper, verbatim): "vale pues dale caña..." → authorized after the analysis.

L6 (analysis, measured): production seams are four functions
(`project_dir` main.rs:69, `Engine::new` engine.rs:113, `assets_dir`
engine.rs:120, `sentinel_script_path` settings.rs:417); `project_dir()` needs no
change because `current_exe()` resolves the symlink to `~/.local/opt/hve/hve`
and that directory already contains `assets/scripts`. `utils.sh` derives every
asset path from `BASH_SOURCE`, so the scripts follow for free. The one real
code change is the sentinel path, which today assumes the legacy
`~/.local/bin/assets`.

L7 (progress): T2-T6 delivered in `9b6409d`, marker guard in `0e13bde`. Suite
1872 passed / 0 failed. Advisory fixes from the previous verification are in the
same commits: the wiki no longer claims a dual `.conf`/`.lua` format or an XDG
autostart file nothing creates, the script count matches, and the uninstaller,
`init.sh` and the watchdog require BOTH markers before a `sed` range runs (a
lone START used to delete to end of file).

L8 (T7, live migration): moved the keeper's install by hand, exactly as the
installer now does — `~/.local/opt/hve/{hve,assets}`, `~/.local/bin/hve` as a
symlink, binary md5 identical to `target/release/hve`
(`022a04ef30726c7de5617df7ed2dd5f5`). Verified: `readlink -f` resolves to the
real binary, `hve --version` answers through the symlink, and
`assets/scripts/utils.sh` derives `HVE_ASSETS_DIR=~/.local/opt/hve/assets`.

L9 (cleanup, authorized by the keeper: "todo lo que sea basura que se ha quedado
en el sistema si esta verificado que es borrable .. se borra"). Verified before
each deletion, then deleted:

- 26 stale binaries in `~/.local/bin` (`hve.backup-*`, `hve.bak-*`,
  `hve.pre-*`, `hve-first-toggle`) — all ELF builds of older HVE versions, no
  config or script referenced any of them, and the running process uses the new
  path.
- 3 stale asset trees in `~/.local/bin` (`assets.old-hve`, two dated backups) —
  `~/.local/opt/hve/assets` holds the same 66 files.
- `~/.cache/noctalia/HVE/` — the old plugin's cache (an `hve_format` file).
- `~/.config/noctalia/{plugins/,}hyprland-visual-editor/` — two vendored copies
  of an older HVE as a Noctalia QML plugin; `settings.json` lists no such plugin,
  so nothing loaded them.
- `~/.config/noctalia/user-templates/test-hve.lua` (4-line test template) and
  `~/.config/OpenRGB/__pycache__/hve-palette-render.cpython-314.pyc` (bytecode
  with no source and no reference).

Measured: `~/.local/bin` went from 978 MB of HVE leftovers to none; total
`~/.local/bin` is now 611 MB, all of it other tools.
