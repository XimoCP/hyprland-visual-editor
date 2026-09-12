# SDD EXPLORE — `drop-conf-support` (Lua-only refactor)

**Status:** exploration complete
**Change:** drop-conf-support
**Store:** hybrid (openspec + engram)

**Branch context:** skill `hve2` loaded. This change touches the sealed engine
(`src/config.rs`, `src/settings.rs`, `src/providers/`, `assets/scripts/`), which
the skill normally forbids rewriting — this SDD is the negotiation artifact for
that exception. It is a pure REFACTOR: no new user-facing features.

**Method:** every claim below was read directly from the files cited. Anything
that could not be confirmed is marked UNVERIFIED.

---

## 1. Inventory of `.conf` code paths

### 1.1 `src/config.rs`

| Location | What it does | Recommended action |
|---|---|---|
| `src/config.rs:222-232` — `hve_format()` | Reads `~/.cache/hve/hve_format` once per process (via `static OnceLock`). Returns `"lua"` only if the file content trims to exactly `"lua"`; **any other state (missing, empty, `"conf"`) → `"conf"`**. This default-conf fallback is the single root of all branching. | **Delete.** Replace all call sites with hardcoded Lua. (See §4 for the one dissenting consideration: reusing the cache file as a *migration signal* — but that should be a new, separate helper, not this function.) |
| `src/config.rs:248-268` — `hve_settings_path()` | Picks extension via `if hve_format() == "lua" { "lua" } else { "conf" }` (`:249`), then migrates legacy name `hve-windowrules.{ext}` → `hve-settings.{ext}` (`:251-265`). Note the migration is **same-extension only**: it never migrates `hve-windowrules.conf` → `hve-settings.lua`. | **Simplify:** always return `hve_cache_dir().join("hve-settings.lua")`. **Keep a one-way migration**, but retarget it: `hve-settings.conf` → `hve-settings.lua` (and optionally `hve-windowrules.*` → `hve-settings.lua`). See §4 for what to do with the old file's content. |
| `src/config.rs:244-246` — doc comment | Says "hve-settings.lua or .conf" and "Replaces the old hve-windowrules.{lua,conf} naming." | **Update** to Lua-only wording. |

**Bonus discovery (test hazard that this change fixes):** `hve_format()` caches
in a process-wide `OnceLock` (`:223-224`), but tests redirect `HOME` per-test
via `TempEnv` (`src/test_utils.rs:32-50`). The first test to call `hve_format()`
freezes the value for the entire `cargo test` process; since a fresh `TempEnv`
has no `hve_format` cache file, the whole suite effectively runs in `"conf"`
mode. That is *why* the `.conf`-style fixtures in `settings.rs` and
`hyprland_settings.rs` tests pass today. Hardcoding Lua eliminates this
cross-test contamination vector.

### 1.2 `src/settings.rs`

This file is the densest branch site: **5 functions × duplicated marker pairs + duplicated content templates.**

| Location | What it does | Recommended action |
|---|---|---|
| `src/settings.rs:1` — import | Imports `hve_format`. | **Delete** from import. |
| `src/settings.rs:8-77` — `ensure_settings_file()` | Branches 3 marker pairs (window-rules `:15-24`, keybinds `:25-34`, autostart `:35-44`) on `format == "lua"` (`-- …` vs `# …`), then branches the whole file template (`:47-71`). | **Simplify** to one Lua template. The generated content is identical modulo comment prefix. |
| `src/settings.rs:92-191` — `set_tiling_window_rules()` | Reads `hve_format()` (`:99`), picks `marker_start/end` (`:108-117`). Block construction delegated to `window_rules_block`. | **Simplify:** drop the `format` read; use Lua markers (shared constants — see recommendation below). |
| `src/settings.rs:196-214` — `window_rules_block(_tiling, format, marker_start, marker_end)` | Takes `format: &str`, branches `:199` to emit `--` vs `#` comment body. Note `_tiling` is already dead (HVE 2 Composer manages window state; both arms emit the same "No rules needed" text). | **Simplify signature** to `window_rules_block(marker_start, marker_end)` or zero-arg returning a constant block. Both the `format` param and (separately) `_tiling` go away. |
| `src/settings.rs:270-387` — `set_keybinds()` | Reads format (`:277`), picks markers (`:286-295`), and — the only **semantically different** branch in this file — emits different bind syntax: Lua `hl.bind("SUPER + H", hl.dsp.exec_cmd("hve-ipc toggle-tray"))` (`:299-304`) vs conf `bind = SUPER, H, exec, hve-ipc toggle-tray` (`:305-311`). | **Simplify** to the Lua block only. |
| `src/settings.rs:398-520` — `set_autostart()` | Reads format (`:405`), picks markers (`:414-423`), emits Lua `hl.on("hyprland.start", …)` (`:431-439`) vs conf `exec-once = … --tray` (`:440-447`). Log line `:507-509` already hardcodes `"in hve-settings.lua"` even in conf mode (stale string — evidence the Lua path is the assumed one). | **Simplify** to the Lua block only; fix log line (it becomes accurate for free). |
| `src/settings.rs:264` — doc comment | "Supports both Lua and conf formats." | **Update.** |
| `src/settings.rs:81` — doc comment | "Operates on hve-settings.lua/.conf". | **Update.** |
| `src/settings.rs:389-397` — doc comment | Already Lua-only in wording ("hve-settings.lua", "dofile'd from hyprland.lua"). | **Keep** as the template for the other comments. |

**Tests in `src/settings.rs:522-763` that lock in `.conf`:**

| Location | What it does | Recommended action |
|---|---|---|
| `:533` comment | "(created in the current hve_format, lua or conf)" | **Update** comment (cosmetic). Test body itself (`test_set_keybinds_writes_and_removes`, `:530-577`) asserts only format-agnostic substrings (`>>> HVE KEYBINDS <<<`, `hve-ipc`, `toggle-tray`) — **keep, passes unchanged** under Lua. |
| `:580-598` — `test_set_keybinds_noop_when_disabled_and_markers_absent` | Writes a fixture file with **conf-style markers**: `"# >>> HVE WINDOW RULES <<<\nwindowrulev2 = float…\n# >>> HVE WINDOW RULES END <<<\n"` (`:585`). Under Lua-only, `set_keybinds(false)` looks for `--` markers, so the fixture's `#` markers become inert user text and the test would assert a vacuous no-op against content the production code can no longer produce. | **Must update:** rewrite fixture with `-- >>> HVE WINDOW RULES <<<` markers. |
| `:700-719` — `marker_block_needs_update_detects_changes` | Pure-function test using `#` markers as arbitrary marker strings. Technically format-agnostic (any strings work). | **Keep passing, but recommend switching fixtures to `--` markers** for consistency so no `#`-marker example remains for future readers to copy. |
| Remaining tests (`tiling_rules_*`, `keybinds_skip_rewrite_*`, `autostart_skip_rewrite_*`, `:605-762`) | Drive production functions and assert via `assert_floating_content`/`assert_tiling_content` (check `"Composer trait"` substring — present in both templates). | **Keep** — they pass unchanged once production emits Lua, since they never assert the comment prefix. |

### 1.3 `src/providers/hyprland_settings.rs`

| Location | What it does | Recommended action |
|---|---|---|
| `:14-16` — `format: String` field | Stores `hve_format()` snapshot per provider instance. | **Delete field.** Provider becomes stateless w.r.t. format. |
| `:19-23` — `new()` | Calls `crate::config::hve_format().to_string()`. | **Simplify** to empty struct / `Default`. |
| `:25-36` — `markers()` | Returns the 6 marker strings, branching `:26` on `self.format == "lua"`. The only consumer is `save()`/`apply()` block extraction. | **Replace** with constants (or a single function returning the Lua 6-tuple). |
| `:63-65`, `:135` | Delegate to `hve_settings_path()` / `ensure_settings_file()` — no branch of their own. | **Keep** (behavior follows `settings.rs` automatically). |

**Tests that lock in `.conf` — all three in `mod tests` (`:200-344`) must be rewritten:**

- `:229-262` — `test_apply_removes_block_when_state_none`: fixture uses `# >>> HVE …` markers and conf content (`bind = SUPER, H, exec, hve-ipc toggle-tray`, `windowrulev2 = float…`). Under Lua markers the provider would find no blocks → no removal → assertions fail. **Must update to `--` markers + Lua block content.**
- `:268-317` — `test_save_with_absent_markers_then_apply_removes_active_block`: same conf fixtures at `:271-275` and `:290-300` (including `exec-once = hve --tray` autostart content). **Must update both fixtures to Lua.**
- `:321-343` — `test_apply_none_when_no_markers_is_noop`: fixture has no markers at all — **passes unchanged**, keep.

### 1.4 Shell scripts (`assets/scripts/`)

| File:lines | `.conf`/`.lua` branch | Recommended action |
|---|---|---|
| `detect_format.sh` (entire file, 62 lines) | `detect_format()`: lua-validated-first (`:30-36`, requires `hl\.`/`require(` in `hyprland.lua`), else conf if `hyprland.conf` exists (`:39-45`), else **default conf** (`:43-44`); caches to `~/.cache/hve/hve_format` (`:48-54`). Comment `:23` even cites a stale path (`~/.cache/noctalia/HVE/hve_format`). | **Delete** as a runtime detector. Optional: salvage the *signal logic* (`hyprland.lua` validity check + conf-exists check) into a new small `check_lua_migrated.sh` guard (see §3). Do not keep the cache write. |
| `utils.sh:37-73` — `hve_resolve_preset()` | `HVE_FORMAT` env (default **`conf`** at `:40`); explicit-extension passthrough (`:43-50`, format-agnostic, keep); otherwise prefer matching extension with fallback (`:52-70`). | **Simplify** to Lua-only: resolve `$dir/$name.lua`, drop the `conf` fallback (or keep a one-release fallback? see §6). Default `conf` at `:40` must go regardless. |
| `init.sh:9` | `export HVE_FORMAT=$(detect_format … \|\| echo "conf")` | **Delete** (with `detect_format.sh`). |
| `init.sh:15-20` | `SETTINGS_EXT` lua/conf branch. | **Hardcode** `hve-settings.lua`. |
| `init.sh:23-24` | `HYPR_CONF` / `HYPR_LUA` paths. | Keep `HYPR_LUA`; `HYPR_CONF` survives only if the guard/migration-cleanup needs it (one-way cleanup of old conf block is already handled by `clean_hyprland_conf` — see below). |
| `init.sh:31-39` | `HVE_COLORS_FILE` gets `.lua`/`.conf` extension from `HVE_COLORS_BASE`. Note: `HVE_COLORS_BASE` is **never set in `utils.sh`** (full read confirms) — this whole block looks dead. | **Delete** (dead code in both branches). Marked UNVERIFIED whether anything exports `HVE_COLORS_BASE` at runtime — grep found no setter. |
| `init.sh:52-58` — `clean_hyprland_conf()` | Removes HVE marker block + overlay `source` lines from `hyprland.conf`. | **Keep for one migration release** as a one-way cleaner (called from the lua-enable path at `:135`, which already cleans conf when enabling lua), then delete. Do NOT keep the conf *injection* path. |
| `init.sh:60-70` — `clean_hyprland_lua()` | Lua-side cleanup. | **Keep.** |
| `init.sh:90-100` | `overlay.conf` path migration + `overlay.lua` vs `overlay.conf` creation fallback. | **Delete conf arm**; always write `overlay.lua`. |
| `init.sh:103-124` | `hve-settings` template in both syntaxes. | **Keep Lua template only** (`:105-113`). |
| `init.sh:132-171` — `enable` | Lua inject (`:132-150`, `dofile` overlay + settings + watchdog `hl.on`) vs conf inject (`:152-171`, `exec-once` + `source`). | **Delete conf arm** (`:152-171`); keep lua arm. Keep the `clean_hyprland_conf` call at `:135` during migration. |
| `init.sh:175-189` — `disable` | Calls both cleaners; preserves `hve-settings.*` via glob `^hve-settings\.` (`:183`). | **Keep**, minus the conf cleaner after the migration window. The `hve-settings.*` glob is already extension-agnostic — fine. |
| `assemble.sh:9-10, 27-44` | Dual `FINAL_FILE_LUA/CONF`; format-cache read defaulting to conf (`:28`); `COMMENT`/`EXT` branch (`:35-44`, including a stray extra `fi` at `:44` — pre-existing syntax wart, harmless today). | **Simplify** to Lua-only: one target `overlay.lua`, `COMMENT="--"`. |
| `assemble.sh:47-54, 63-88` | Lua vs conf header/colors/curve emission (`primary = "…"` vs `$primary = …`; `hl.curve(…)` vs `bezier = …`). | **Keep Lua arms only.** |
| `assemble.sh:93-104` | Assembles `${MOD}.${EXT}` fragments. | Becomes `${MOD}.lua` — keep structure. |
| `assemble.sh:106-128` | Cross-format validation (lua target must not contain `general{`-style blocks and vice versa). | **Delete the conf half**; keep (or simplify) the lua-side sanity check. |
| `assemble.sh:131-137` | Moves temp to target, deletes opposite-format overlay. | **Delete** (nothing to clean once single-target). Keep the `overlay.current` symlink block (`:142-145`) and `hyprctl reload` (`:148-150`). |
| `scan.sh:17-29` | Format detection defaulting to conf (`:24,26,29`). | **Delete.** |
| `scan.sh:34-64` | Per-basename dedup preferring `HVE_FORMAT` match (`:50-63`); `find` includes `*.conf` (`:64`). | **Simplify:** drop dedup entirely (one file per basename after asset deletion) and remove `-name "*.conf"` from the find. |
| `border.sh:7-30, 37-55` / `apply_animation.sh:7-30, 37-57` | Cache-read format detection; `EXT/ALT_EXT`; opposite-fragment cleanup (`rm -f OLD_FRAGMENT`); `hve_resolve_preset`; per-format security fallback (`hl.config…` vs `general {…}` / `animations {…}`). | **Simplify** each to hardcoded `lua` fragment + Lua fallback; delete opposite-cleanup. |
| `shader.sh:7-30, 45-49, 81-89` | Same detection/fragment pattern; plus `hyprctl keyword decoration:screen_shader` vs `decoration.screen_shader` branch (`:45-49`) and Lua-vs-conf wrapper generation (`:81-89`). | **Simplify** to Lua; keep the v0.55+ `decoration.screen_shader` keyword spelling (the lua arm already tries both at `:46`). |
| `geometry.sh:9-31, 61-117` | Same detection/fragment pattern; Lua `hl.config({general…, decoration…})` (`:68-93`) vs conf `general{…} decoration{…}` (`:94-117`). | **Simplify** to Lua. |
| `colors.sh:51-61` — `_hve_extract_conf_vars()` | Parses `$var = …` conf syntax. Called from `_hve_try_noctalia` (`:254`), `_hve_try_matugen` fallback (`:308`), `_hve_try_manual` conf scan (`:334-344`). | **Delete function + all conf call sites.** Keep `_hve_extract_lua_vars`, pywal/matugen-lua, Noctalia palette (all format-agnostic). Lua arms already preferred everywhere (`:248-256`, `:306-309`, `:321-331`). |
| `colors.sh:240-269` — `_hve_try_noctalia()` | Checks `noctalia.lua` then `noctalia.conf` (v5 `:241-256`), then v4 conf **before** v4 lua (`:259-267`). | **Delete conf arms.** |
| `color_watcher.sh:31-47, 89-93, 192-198` | Watches `noctalia-colors.conf`, v5 `noctalia.conf`, manual `find … -name "*.conf"`, rendered-file list including conf. | **Delete conf watch entries.** (File-watching only; harmless if left, but it is format handling and belongs in scope.) |
| `format_test.sh` (entire file) | References `transpile.sh` and `fragments/border.conf` / `animation.conf` — **neither exists** (no `transpile.sh` in `assets/scripts/` glob; `assets/fragments/` holds only `border.lua`, `animation.lua`, `geometry.lua`). Dead test for a removed transpiler. | **Delete file.** |
| `get_colors.sh` | Delegates to `colors.sh`, no branch of its own. | **Keep.** |
| `hve_watchdog.sh:9,16-19` | `HYPR_CONF` var + conf-marker cleanup (`:16-19`); lua cleanup at `:21-24`. | **Keep conf cleanup** (one-way hygiene for migrating users), keep lua cleanup. Revisit deletion later. |

### 1.5 `install.sh` / `uninstall.sh`

- **`install.sh`: no runtime format handling.** Only two comment references: `:398` (`hve-settings.{lua,conf}`) and `:397-401` (explains `set_autostart` writes `exec-once … --tray` — stale: the Lua autostart block is `hl.on("hyprland.start")`, not `exec-once`). Color-tool detection at `:335-337` already checks `noctalia-colors.lua` only. **Action: update the two comments to Lua-only; no logic change.** This is also the natural home for an install-time guard (see §3).
- **`uninstall.sh`: no format handling at all** — removes whole directories (`HVE_ASSETS`, `HVE_CACHE`, `HVE_CONFIG`). **No change needed.** Note: it does not clean the HVE block from `hyprland.lua` either (that is `init.sh disable` + watchdog territory) — UNVERIFIED whether that gap is intentional; out of scope for this change.

### 1.6 Other `src/` `.conf` mentions (verified NOT format support — keep)

- `src/composer/hyprland.rs` + `src/composer/mod.rs:63-67` — `HyprMode::V4/V5` is **Hyprland IPC dispatch versioning** (`hl.dsp.*` vs classic `dispatch` args), orthogonal to the config-file format. Comments saying "V4/conf fallback" refer to dispatch syntax. **Explicitly OUT of scope — do not touch.**
- `src/callbacks.rs:356` — stale comment "Toggle window rules in hyprland.conf". **Update comment only** (code calls `set_tiling_window_rules`, already covered).
- `src/callbacks.rs:854` (`"01_cascade.conf"` in dead-code `preset_geometry_for`), `:1372`/`:1429-1431` (`animXX.conf` test fixtures), `src/shell/ui_tests.rs:2956` (`animXX.conf` fixtures), `src/providers/shell.rs:40` (`NOCTALIA_RENDERED_FILES` lists both extensions) + `:385` (test asserts `noctalia-colors.conf` present) — cosmetic or Noctalia-external filenames. **Recommend updating the HVE-owned fixtures to `.lua` for consistency; keep `NOCTALIA_RENDERED_FILES` as-is** (those are Noctalia's files, not HVE's; `colors.sh` just reads whichever exists — though after this change it reads only the lua one).
- `src/providers/mpvpaper.rs:634` (`"../../.config/hypr/hyprland.conf"`), `src/utils.rs:70,88` (`test.conf`) — path-traversal test fixture and generic temp filenames. **Keep.**

---

## 2. `.conf` asset inventory

| Directory | `.conf` | `.lua` / other | Twin status |
|---|---|---|---|
| `assets/borders/` | **14** (`01_cascade` … `14_looper`) | 14 `.lua` (same basenames) | All twins. |
| `assets/animations/` | **18** (`01_relampago` … `18_energico`) | 19 `.lua` (`01`…`18` + `19_stylized2.5D.lua`) | All twins, **plus one lua-only file with no `.conf` sibling: `19_stylized2.5D.lua`**. |
| `assets/shaders/` | **0** | 9 × `.frag` (`01_night` … `09_hybrid`) | Format-agnostic by design (referenced by path from the generated wrapper). |
| `assets/fragments/` | **0** | 3 × `.lua` (`border.lua`, `animation.lua`, `geometry.lua`) | Already Lua-only (runtime-generated; `.conf` fragments were already deleted in a prior change — the opposite-cleanup `rm -f` lines in `border.sh` etc. are vestigial). |

**Total `.conf` assets: 32 (14 borders + 18 animations). Zero `.conf`-only files**
(no `.conf` without a `.lua` twin). One `.lua`-only file (`19_stylized2.5D.lua`)
— evidence the asset pipeline already treats Lua as canonical.

**Once runtime is Lua-only, nothing reads the `.conf` twins:** `scan.sh` (after
§1.4 cleanup) no longer lists `*.conf`; `hve_resolve_preset` no longer resolves
`.conf`; `assemble.sh` only cats `*.lua` fragments. They become dead weight
shipped to `~/.local/bin/assets/` by `install.sh:307-315` (which copies whole
directories).

---

## 3. Migration guard design space

### Detection signals (realistic, grounded in current code)

1. **`hyprland.conf` exists AND `hyprland.lua` does not** (`~/.config/hypr/`).
   Strongest signal of a conf-only user. Mirrors the two paths
   `detect_format.sh:15-18` already checks.
2. **`hyprland.lua` exists but is not valid Lua** (missing `hl.`/`require(`
   patterns — the exact validity check at `detect_format.sh:33`). Catches
   stub/empty lua files where Hyprland would fall back to conf. UNVERIFIED
   against Hyprland internals beyond the provided context, but it is the repo's
   own established heuristic — safe to reuse.
3. **Stale `~/.cache/hve/hve_format` containing `conf`.** Weak alone (defaults to
   conf when absent, per `config.rs:229`), but useful as a "previously conf" hint
   combined with signal 1.
4. **Stale `~/.cache/hve/hve-settings.conf` on disk.** Indicates the user ran the
   conf path before; useful for triggering the one-way settings migration (§4),
   not for blocking.

### The BOTH-files rule (critical)

Hyprland loads `.lua` and ignores `.conf` when `.lua` exists.
`detect_format.sh:30-36` already implements lua-priority. **The guard must
reproduce exactly this priority: `hyprland.lua` present-and-valid ⇒ Lua user,
never fire — regardless of any `hyprland.conf` also present.** Only signal 1
(conf without lua) fires the guard. A both-files user is already a Lua user;
their `hyprland.conf` is inert and HVE works.

### Recommended placement + behavior

- **Startup guard in Rust (`src/main.rs`, before `ensure_settings_file()` at
  `:895`):** check `~/.config/hypr/hyprland.lua` validity (port the
  `hl\.`/`require(` heuristic) and `hyprland.conf` existence. On conf-only
  detection: log a clear error, show it in the UI (Slint dialog / status text —
  HVE already has i18n status plumbing), and **refuse the mutating paths**
  (`init enable`, preset apply) while still letting the window open to display
  the message. Rationale: a tray-resident app that exits silently on login is
  worse than one that opens and explains.
- **`install.sh` preflight:** same check before build/install; print the
  migration message with a link to Hyprland's Lua migration docs and
  abort-or-continue prompt. Cheap, catches the problem before the user ever
  launches.
- **Keep `init.sh`'s `clean_hyprland_conf` one-way cleanup** so `enable` on a
  migrated (both-files) system removes the stale conf block rather than leaving
  a zombie.
- **Message behavior:** bilingual (ES/EN, matching `install.sh` convention),
  naming both files, stating that HVE is now Lua-only, and giving the concrete
  next step (create/migrate `hyprland.lua`, then re-run `init.sh enable` /
  relaunch). Never silently write Lua files a conf-only Hyprland will ignore —
  that is the exact silent-breakage mode this guard exists to prevent.

---

## 4. Open product decisions

**D1 — Delete the 32 `.conf` asset twins, or keep them dormant?**
Options: (a) delete in this change; (b) keep dormant (unstallable, unscanned).
**Recommend (a) delete.** They are byte-for-byte legacy twins of Lua files (plus
git history retains them), they double the shipped asset surface via
`install.sh:307-315`, and dormant-but-present files invite future code to
re-acquire conf fallbacks. No `.conf`-only content exists (§2), so nothing is
lost.

**D2 — Hard stop vs warning-and-continue for a detected `.conf` user?**
Options: (a) hard stop (refuse mutating operations, show message); (b) warning
banner but keep applying (Lua writes a conf-only Hyprland ignores).
**Recommend (a) hard stop on mutations + always-visible message, but still open
the window** (per §3). Option (b) reproduces the silent breakage the brief
explicitly forbids: every apply would "succeed" while changing nothing on screen.

**D3 — Keep `hve_format()` at all, or remove it and hardcode Lua?**
Options: (a) remove; (b) keep as `== "lua"`-only reader. **Recommend (a)
remove.** Every call site simplifies (markers become constants), the
`OnceLock`+`TempEnv` test hazard (§1.1) disappears, and the `hve_format` cache
file stops being written. If a migration signal is needed, write a small
purpose-built helper (e.g. `conf_migration_needed()`) rather than keeping a
function whose name promises dual-format support.

**D4 — What to do with an existing `~/.cache/hve/hve-settings.conf` on disk?**
Options: (a) one-way migrate content conf→lua; (b) ignore and generate fresh
`hve-settings.lua`; (c) leave both. **Recommend (b) generate fresh + delete (or
orphan) the `.conf`.** Rationale: the file's content is fully HVE-managed and
regenerable — `set_keybinds(true)` / `set_autostart(auto_start)` re-emit the
blocks on next startup (`main.rs:2663-2664`), and window-rules blocks are
intentionally empty under HVE 2. Content migration (a) would require transpiling
conf bind syntax to Lua API calls for zero benefit. **Corollary (important): do
NOT bulk-migrate by copying bytes** — old `state.json` theme snapshots saved
under conf contain conf-syntax blocks (`bind = …`, `exec-once = …`); pasting
those into a `.lua` file poisons it. Old themes' `hyprland-settings` blocks
should be re-saved under Lua (document as a known migration note), or `apply()`
should skip blocks that fail a Lua-shape check — deferred detail for the plan
phase, flagged here so it is not missed.

---

## 5. Risk / blast radius

1. **Conf-only users break visibly (intended) — must break loudly, not
   silently.** After this change every write targets `overlay.lua` /
   `hve-settings.lua` + `dofile` lines in `hyprland.lua`. On a conf-only system
   Hyprland never loads those files: presets "apply" with no on-screen effect.
   The §3 guard is the mitigation; without it this refactor is a
   silent-breakage machine.
2. **Tests locking in `.conf` (must be updated):** `src/settings.rs:580-598`
   (conf-marker fixture), `src/providers/hyprland_settings.rs:229-317` (two tests
   with conf fixtures + conf block content). All other
   settings/hyprland-settings tests are format-agnostic and survive.
   Cosmetic-only updates recommended for `callbacks.rs:854,1372,1429`,
   `shell/ui_tests.rs:2956`, `providers/shell.rs:385` fixtures.
3. **Ordering constraint: `hyprland.lua` must contain the HVE block.** Rust
   startup (`main.rs:895,2663-2664,2699`) only writes `hve-settings.lua` — the
   `dofile("…/overlay.lua")` + `dofile("…/hve-settings.lua")` injection into
   `hyprland.lua` lives solely in `init.sh:137-150` (`enable`). Dropping conf
   does not change this, but the guard must verify the dofile block exists (not
   just that `hyprland.lua` exists), otherwise a Lua user who never ran `enable`
   gets the same silent no-op. Recommend the startup guard check for the HVE
   marker block (`-- >>> HYPRLAND VISUAL EDITOR START <<<`, cf. `init.sh:45`)
   and, if absent, prompt to enable rather than applying presets into an
   unloaded overlay.
4. **Stale theme snapshots (D4 corollary):** pre-migration saved themes embed
   conf-syntax `hyprland-settings` blocks; applying them post-migration injects
   conf lines into a Lua file (assemble validation in `assemble.sh:111-121` is
   being deleted in the same change, so nothing would catch it). Mitigate by
   documenting re-save + considering a Lua-shape skip in `apply()`.
5. **Noctalia conf color files:** `colors.sh`/`color_watcher.sh` stop reading
   `noctalia*.conf`. A user whose Noctalia renders only conf output falls back
   to pywal/matugen/manual scan or the hardcoded `#cba6f7`-family defaults
   (`colors.sh:385-390`). Acceptable and consistent with Lua-only, but worth one
   line in the changelog.
6. **What does NOT break:** `HyprMode::V4` dispatch fallbacks
   (`composer/hyprland.rs`) are IPC-version handling, not config format —
   untouched. `uninstall.sh` is format-agnostic. `get_colors.sh`, `engine.rs`,
   `theme_manager.rs`, shaders (`.frag`) are format-agnostic.

---

## 6. Suggested scope boundary

**IN (this change):**
- `src/config.rs`: delete `hve_format()`; `hve_settings_path()` → always
  `hve-settings.lua` + one-way `.conf`→`.lua` file migration.
- `src/settings.rs`: all five functions to Lua-only (markers as shared
  constants); update the two conf-locked tests + stale comments.
- `src/providers/hyprland_settings.rs`: delete `format` field/`markers()` branch
  → Lua constants; rewrite the two conf-locked tests.
- Scripts: delete `detect_format.sh` + `format_test.sh`; Lua-only `utils.sh`
  resolver, `init.sh` (keep conf *cleanup*, delete conf *injection*),
  `assemble.sh`, `scan.sh`, `border.sh`, `apply_animation.sh`, `shader.sh`,
  `geometry.sh`, `colors.sh`, `color_watcher.sh`.
- Assets: delete the 32 `.conf` twins (14 borders + 18 animations).
- Guard: startup check (lua-valid-wins; conf-without-lua → blocking message +
  refuse mutations) + `install.sh` preflight + message copy (ES/EN).
- Stale-comment sweep: `settings.rs:81,264`, `config.rs:244-246`,
  `callbacks.rs:356`, `install.sh:397-401`.

**DEFERRED (later cleanup, explicitly not this change):**
- Removing the one-way conf *cleanup* helpers (`clean_hyprland_conf`, watchdog
  conf arm) after one release.
- Theme `state.json` migration tooling / Lua-shape validation in
  `hyprland-settings apply()` (flagged in D4; needs its own design).
- `HVE_COLORS_BASE` dead code in `init.sh:31-39` (UNVERIFIED whether anything
  sets it — confirm separately, then delete).
- `HyprMode::V4` dispatch fallbacks — unrelated axis, never touch in a format
  change.
- `uninstall.sh` cleaning the HVE block from `hyprland.lua` (pre-existing gap,
  UNVERIFIED intent).

**TDD note:** per project rules, each step lands failing-test-first via
`cargo test`: (1) update the three conf-locked tests to Lua fixtures (they fail
against current code — proving the lock), (2) simplify production until green,
(3) add a new guard test (conf-without-lua detected; both-files not flagged),
(4) implement the guard.
