# Feature: a theme from outside cannot run code behind your back

**Locator**: `odd/tasks/theme-package-trust.md`
**Engram mirror**: topic `odd/theme-package-trust/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: OPEN — authorized by the keeper 2026-10-12 ("vamos a hacer y resolver todo").
**Restore point**: `~/Proyectos/hve-backups/hve-20261012-103732.{bundle,tar.gz}` (bundle verified complete).
**Findings**: the 2026-10-12 security audit (2 CRITICAL, 1 HIGH, 2 MEDIUM, 6 LOW) + the health audit's items.
**TDD**: strict — runner `cargo test`.
**Route**: delegated writer, then cross-model verification (paths and permissions are Tier 3 per AGENTS.md).

## The keeper's decision (2026-10-12) — the policy this contract implements

A theme that carries **shell hooks** or **Lua presets** must not be applied in silence. The keeper chose a
**warning with confirmation**: HVE tells him what could happen if the theme is not trusted, and HE decides.
**Nothing is stripped**; the feature stays fully available.

**Honest limit, to be stated in the code and in the warning itself**: a confirmation is a **consent gate, not a
sandbox**. If he accepts, the theme's code runs as him. The warning must not be worded as if it prevented that.

## Why the features are dangerous (the two CRITICAL findings, verified)

- Noctalia's `settings.toml` has a `[hooks]` section whose values are **shell commands** Noctalia runs on its
  events (started / wallpaper_changed / colors_changed). The apply copies the theme's `settings.toml`
  **verbatim** over the live one (`src/providers/noctalia.rs:2334`, `fs::copy`) and then fires reload / wallpaper
  / colour changes, so a theme's hook executes on apply.
- A theme's Lua presets are copied into the preset store, `cat`ed unvalidated into the assembled overlay
  (`assets/scripts/apply_animation.sh` → `assemble.sh` → `~/.cache/hve/overlay.lua`), and Hyprland `dofile`s
  that. Lua there can run commands (`hl.exec_cmd` is used by this very repo in that context).
- `sanitize_settings_toml` (`noctalia.rs:543`) does NOT help here: it drops only ABSOLUTE-PATH values, and it
  runs only on the save/package side (`noctalia.rs:2073`, `theme_package.rs:281`) — never on apply.

## Units

### Lane A — the theme-trust gate (the two CRITICALs)

- **W1 — detect, with no UI.** A pure function over a theme directory returning what the package carries:
  which shell hooks (key + the command text) and whether it carries Lua presets. New module
  `src/theme_trust.rs` so it does not fight `noctalia.rs`. Tested directly against fixtures.
  No behaviour change: nothing calls it yet.
- **W2 — the gate.** Every APPLY path consults the detector. When the report is non-empty, HVE shows the
  warning (what the theme carries + what it could do + "only continue if you trust where this theme came
  from") and applies ONLY on confirmation. Declining leaves the live state untouched.
  Must cover the gallery path AND the panel's apply-saved-theme path.
  UI is new: the headless render check is MANDATORY (AGENTS.md) — render, read the PNGs, confirm geometry.

### Lane B — the other security findings (no product decision needed)

- **W4 — confine `video_directory`** (HIGH, `src/providers/mpvpaper.rs`): the value comes from the (untrusted)
  theme. Refuse to create or write outside a known-safe root; the download must not happen before the
  plugin-enabled check (`:371`).
- **W5 — cap the download**: add a size limit to the curl call (`--max-filesize`), keeping the existing
  `--max-time 300` and `--proto =https`.
- **W6 — `--` before positional arguments** passed to `noctalia` / `skwd-helm` so a theme value starting with
  `-` can never be read as an option (`skwd_engine.rs`, `noctalia.rs`).
- **W7 — the IPC socket**: set the mode BEFORE `bind` (no umask window) and refuse the `/tmp/hve.sock` fallback
  or make it safe (`src/ipc.rs`).
- **W8 — log permissions**: create `hve.log` and the decision log with mode 0600 (`src/main.rs`,
  `src/decision_log.rs`).
- **W9 — validate the theme name** in `ThemeManager::delete` / `apply` (defence in depth; `PresetStore`
  already does it) — `src/theme_manager.rs`.
- **W10 — bound the package read**: a size and depth limit when reading a theme package
  (`src/theme_package.rs`, `src/providers/noctalia.rs`).

### Lane C — health (serial, last, and alone)

- **W11 — clippy**: the autofixable, behaviour-neutral categories. NEVER the float-comparison class
  (`!(a < b)`) — it changes NaN semantics that the layout code relies on.
- **W12 — the one real panic candidate**: guard the pixel-chunk slice in `src/tray.rs`.
- **W13 — the dead-code suppressions** that silence whole modules (`src/shell/gallery/views/mod.rs`).
- **W14 — the fragile test patterns**: the source-text assertions and the worst `thread::sleep` tests.
- **W15 — `cargo fmt`**: LAST or FIRST, never in between — 23,362 diff lines across 12+ files, one dedicated
  commit.
- **W16 — the `ksni` 0.2 → 0.3 bump** (dedupes and drops clap v2): needs care, the tray API changes.

## Acceptance criteria

1. `cargo test` green and `cargo check --all-targets` clean after EVERY unit.
2. No behaviour change for a theme that carries neither hooks nor Lua.
3. After W2: a theme carrying hooks or Lua cannot be applied without an explicit confirmation, on BOTH apply
   paths, and declining changes nothing on disk.
4. After W2: the warning text does not claim to prevent execution.
5. Every unit has a commit, and its verification is recorded with the model named.
6. The keeper's own themes and his live Noctalia config, theme packages and `sddm-watch.sh` are never touched.

## Out of scope (stated)

- Removing the features. The keeper keeps hooks and Lua; the gate is a warning.
- A real Lua sandbox: Lua runs via `dofile` with full access, so text inspection is not a wall. Recorded, not
  pretended.
- The `sha256` in a theme's video record cannot authenticate anything (the same untrusted file supplies both
  URL and hash). Recorded as a limit, not "fixed".
- Personal paths in test fixtures and `odd/` docs (privacy, no secrets) — the keeper's call, not this contract.
