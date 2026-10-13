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

## Progress and evidence (2026-10-12/13)

Test counts are the full-suite totals observed at each commit; the suite started this contract at 1616.

| Unit | Commit | What | Tests |
|---|---|---|---|
| — | `69f32ce` | this contract opens | 1616 |
| W1 | `d58db4d` | the detector: `inspect_theme_package` (pure, total, read-only) | 1626 |
| W2 | `a7073e1` | the gate + the warning overlay, both apply paths | 1642 |
| W1b | `140f3f3` | detector widened: the executable `.frag` presets + two TOML blind spots | 1645 |
| W6 | `eccdd2b` | refuses a theme value that could read as an option | 1653 |
| W10 | `53eea8c` | bounds the read of an untrusted package (+ W6 residuals `c83f18b`) | 1665 |
| W10b | `8b591e7` | bounds EVERY remaining theme-file reader; rejects non-regular files | 1685 |
| W4 | `3cad59a` | confines `video_directory` to `$HOME/{Pictures,Videos,Downloads}` | 1632 |
| W5 | `67a7ee6` | caps the video download at 1 GiB (plus an on-disk re-check) | 1632 |
| deps | `a0bf10e` | 4 RustSec advisories cleared by lockfile-only bumps | 1616 |
| W2b | `317d55e` | the trust warning is operable from the keyboard (focus scope, Cancel first, Return/Space confirms, Escape declines) | 1688 |
| W7 | `9a98f6d` | the IPC socket: a private 0700 dir created BEFORE the bind, no `/tmp` fallback, refuse without a usable HOME | 1694 |
| W8 | `5670191` | `hve.log` and the decision log created 0600 and tightened when inherited | 1701 |
| W9 | `dad4ca6` | the theme name validated in `apply`/`delete`/`save`/`rename`/`theme_info` | 1705 |
| W7b | `7bbeebd` | the shipped `hve-ipc` client resolves the socket like the server; the docs corrected | 1705 |
| corr | `d5cc429` | the five findings of the W7–W9 cross-model verification | 1711 |
| W1c | `cfda2a0` | the root-level hooks TOML spellings (dotted key, inline table) + the `{` false positive | 1720 |
| W10c | `a3a183e` | the non-regular-file guard moved into the lowest-level reader; both `fs::copy` restore sites; the scan | 1727 |
| W1c-b | `881ae47` | escaped keys, a UTF-8 BOM and multi-line inline tables: FAIL CLOSED instead of silent | 1733 |
| W10c-b | `32b9fcd` | the scan refuses a symlinked ROOT too, not only symlinked children | 1734 |
| W1d | `299af0a` | the hooks detector reads `settings.toml` with the REAL `toml` parser (2290 → 1834 lines, −456) | 1736 |
| W10c-c | `9f05e5e` | packaging refuses a symlinked theme dir BEFORE any read or write | 1737 |
| W1d-b | `0c842d3` | a REFUSED `settings.toml` reports "uninspected", never an empty report | 1738 |
| W10c-d | `246bf1c` | a theme tree carrying a symlinked directory is refused up front | 1741 |
| W2c | `9d2bcf8` | the remembered acceptance: `theme-trust.toml`, keyed by the exact content shown | 1763 |
| W2c-b | `d9e12af` | the acceptance keys a preset's BODY, not only its name (the dual-review blocker) | 1770 |
| W2c-c | `a1091de` | the bound per THEME and the bound per STORE (both re-judges' finding) | 1776 |
| W2c-d | (this commit) | the budget counted in the currency the store writes (both judges, last round) | 1777 |
| docs | `6b2791d`, `788772a`, `e76882f` | this record: the rounds, the residuals, the open units | — |

## Log (W2c) — the record this unit answers for

- **L1 (2026-10-12, the keeper, verbatim)**: `vamos a hacer y resolver todo` — the authorization this whole contract runs
  under, and the source of the W2c unit the keeper approved the same day as `remember-my-acceptance`.
- **L2 (2026-10-13, the keeper, verbatim)**: `sigue con theme-package-trust` — resume the contract and take the next
  unit, which the record above names as W2c, the only substantive unit left in Lanes A/B.
- **L3 (2026-10-13, the parent, a spec change found WHILE implementing, not after)**: S1 originally read "the
  conservative `hooks table` entry is part of that content, so an uninspected package is remembered as
  'uninspected', never as 'clean'". Implementing that showed the hole: a report whose only entry is the conservative
  one is IDENTICAL before and after somebody edits the file it could not read — so a remembered acceptance of it would
  apply rewritten content in silence, which is the exact failure this whole contract exists to prevent. S1 was
  rewritten: an uninspected report is NOT rememberable, and the keeper is asked every time (S8 makes the warning say
  so, because the copy must not promise a silence HVE will not keep).
- **L4 (2026-10-13, the parent, the W4/W5 reconciliation the second-round verification asked for)**: the contract's
  line 51 said the download must not precede the plugin-enabled check, and the code downloaded first. The CONTRACT
  line is the correct intent and the code was wrong: nothing about a disabled plugin needs the media resolved, the
  call returns `Err` either way, and the download is driven by UNTRUSTED theme input — so it was a
  resource-consumption vector (up to the 1 GiB cap) with no user-visible result beyond a message. The check moves
  BEFORE the resolve/download loop, and the by-construction test in `mpvpaper.rs` gains the ordering assertion.
- **L5 (2026-10-13, the keeper, verbatim)**: `Si quieres verificación doble lanza a glm 5.3 flash con el mismo
  trabajo a ver que saca el` — the dual adversarial review this record's fourth round describes, run blind, and the
  reason the blocker below was found at all.
- **L6 (2026-10-13, the keeper, verbatim, answering the mechanism question)**: `El cuerpo tal cual (Recomendado)` —
  the preset body is remembered VERBATIM, not as a digest, with the 16 KiB bound above which the preset is reported
  unreadable and the theme is never remembered.
- **L9 (2026-10-13, the parent, after the last re-judgment)**: the budget's currency. `remembered_bytes` counted the
  CODE while the store's write bound counted SERIALIZED BYTES, so the copy could promise a memory the store refused —
  found independently by BOTH judges, and it is the third time this contract has been bitten by "a list is not the
  rule, a count is not what the store holds". The fix unifies the currency in ONE pair of functions
  (`hook_bytes`/`preset_entry_bytes`) that both the scan and the report call, and writes the sufficiency arithmetic
  next to the constant.
- **L8 (2026-10-13, the keeper, verbatim, answering the W2c-c question)**: `Sí, ciérralo ahora (Recomendado)` — close
  the store-growth defect the two re-judges confirmed, with the bounds described in the W2c-c section above.
- **L7 (2026-10-13, the parent, a spec change forced by the blocker)**: S1's original wording enumerated the fields
  the acceptance keyed on and did NOT include a preset's body, so the fraction of S1 that was written down was
  implemented faithfully — and it was wrong. S1 now says the key covers what RUNS, body included, and S3 names the
  body case explicitly. The lesson is the one this contract keeps re-learning: a list of fields is not a rule; the
  rule is "what executes must be in the key".

### Cross-model verification (Tier 3 — a DIFFERENT model from the writer)

- **W1** — verified by GLM-5.3-Flash. Verdict: sound, but it REFUTED completeness: `.frag` presets in
  `animations/`/`borders/` are `cat`ed into the executed Lua and the `.lua`-only filter missed them. Corrected by
  W1b. It also found two TOML blind spots (multi-line string values, quoted table headers). `pub mod` claim true.
- **W2** — verified by GLM-5.3-Flash. Verdict: **no apply path bypasses the gate** (gallery, panel, mosaic, keyboard,
  startup, tray, IPC all audited); the copy does not claim to block anything. Two MEDIUM findings: a keyboard-only
  user cannot CONFIRM (only Escape/decline) and the confirmed branch does not re-inspect the package.
- **W4+W5** — verified by GLM-5.3-Flash. Verdict: the confinement HOLDS against every escape tried (`..`, symlinks,
  hard links, sibling prefixes, NFD/NFC, 4000-char paths, relative, `/`), the refusal creates nothing, and the cap
  is real. One MEDIUM: `64`-MiB read of the contract says the download must not precede the plugin-enabled check and
  it still does — the contract line and the parent's instruction disagreed; reconcile it.
- **NOT yet verified cross-model**: none. The old backlog (W1b, W6, W10, W10b, W2b) was audited on 2026-10-13 by
  GLM-5.3-Flash — see below — and W7–W9/W7b, W1c and W10c each got their own cross-model round.

### Cross-model verification, second round (2026-10-13) — MiMo wrote, GLM-5.3-Flash verified

- **The old backlog, finally audited (read-only, no cargo)**: **W6 sound** (the option-shaped value is refused at
  every argv slot; the full argv census was walked: `skwd-helm`, every `noctalia msg` verb, `curl`, `ffmpeg`,
  `hyprctl`, `engine`). **W10 sound** with one MEDIUM (the scan followed a symlinked directory out of the
  package). **W1b REFUTED** — a real BLOCKER: `hooks.started = "…"` and `hooks = { started = "…" }` at the
  document root are the SAME TOML table Noctalia executes, and the detector reported EMPTY, so the hook ran with
  no warning. **W10b REFUTED** — two real BLOCKERs: the regular-file guard lived only in the `read_theme_record`
  wrapper while ~20 call sites reached `theme_package::read_theme_text`/`read_theme_bytes` directly (a FIFO named
  like a theme record hangs `File::open` for ever), and both `fs::copy` restore sites were pre-flighted only by
  the size-only `theme_file_is_bounded`, which answers "bounded" for a FIFO (`len = 0`). **W2b sound** with three
  MEDIUMs: the confirmed branch re-reads the package but `TrustGate::request` short-circuits on the armed
  confirmation BEFORE consulting the fresh report (the real close is the queued `remember-my-acceptance`); the
  carriers list is not keyboard-scrollable; and the commit added new source-text assertion tests — the very
  pattern W14 exists to remove.
- **W7–W9 + W7b**: `sound` for all four, no blocker, five findings. Corrected by `d5cc429` (see below).
- **W1c**: verified `sound` first, then **REFUTED** on the recheck with two BLOCKERs the whole feature depends on:
  a TOML escaped key (`"\u0068ooks".started`) and a header (`["\u0068ooks"]`) are the hooks table to any
  spec-1.0 parser and were missed, and a **multi-line inline table is VALID TOML under spec 1.1** — the parser
  family already in this repo's `Cargo.lock` (`toml 1.1.2+spec-1.1.0`) — so the W1c comment that called it
  invalid was itself a false claim in a security comment. Corrected by `881ae47` (fail closed).
- **W10c**: `sound with findings`. The stat-then-open TOCTOU the writer disclosed is REAL and survives (see the
  residuals below). One MEDIUM: the scan checked symlinked CHILDREN but never its own ROOT. Corrected by
  `32b9fcd`.
- **`d5cc429`, the W7–W9 correction batch** (one RED test per finding, all observed failing first):
  `is_private_dir_of` masked 9 bits while the docstring claimed "exactly 0700", and the Python client reads the
  full 12-bit `S_IMODE` — the two resolved DIFFERENT sockets for a runtime dir carrying the sticky bit;
  `resolve_assert_color_authority` joined the configured theme name with no rule and reached a theme OUTSIDE the
  store; an OLD `hve.log.1`/`color-authority.log.1` stayed world-readable because the tighten ran only on
  rotation; `rename` joined `old_name` with NO rule (`rename("../outside/Evil", "Stolen")` pulled a directory
  from outside the store INTO it); `theme_info` kept its own weaker copy of the name rule.
- **The fail-closed rule this produced (W1c-b)**: a `hooks` table the line-based detector cannot confidently
  read now produces a CONSERVATIVE entry instead of an empty report. The keeper cannot consent to what nobody
  inspected, so a detector that lost the table must say so. The user-visible line is: `hooks table — the hooks
  table in <file> (line N) could not be fully read by this scan; it may carry shell commands that are not
  listed here`.

### Cross-model verification, third round (2026-10-13) — and the failure that ended the line-based reader

- **W1c-b REFUTED again**, with the BLOCKER that finally justified a structural rewrite: a header-shaped line
  INSIDE an untracked multi-line string (`note = """\n[other]\n"""` then a root `hooks.started`) corrupts the
  document-root state, so a parser-valid root hook reached an EMPTY report — CRITICAL-1 recurring. The inverse
  also held: `note = """\n[hooks]\nstarted = "not real"\n"""` made a CLEAN theme warn. Three rounds of line-based
  patches had produced three refutations. **The class, not the spelling, was the defect.**
- **W1d (the structural fix)** — the detector now parses with the real `toml` crate (`1.1.2+spec-1.1.0`, already
  in this lockfile transitively; promoting it to a direct dependency changed exactly ONE line of `Cargo.lock`).
  Every spelling the three rounds fought is now handled by the parser, and the hand parser is GONE: −456 lines.
  Verified `sound with findings`, no blocker. The one finding that mattered was a real FAIL-OPEN the structural
  fix exposed: an OVER-BOUND but valid `settings.toml` carrying `[hooks]` reached an empty report, because the
  bounded reader refuses before parsing — and the writer's own test had PINNED that hole as "no hooks from this
  file".
- **W1d-b** closes it: every `Err` the reader can return (over bound, not a regular file, unreadable, not valid
  UTF-8) now reports the SAME conservative "could not inspect" entry a parse failure produces, while a MISSING
  settings file still reports EMPTY (a theme with no settings file carries no hooks, and the apply cannot copy a
  file that does not exist). Verified `sound with findings`.
- **The finding of the third round, and it is about honesty, not code**: the rationale in the comments claimed
  "the apply copies `settings.toml` verbatim, so a hook inside it runs on apply" as the reason the refusal entry
  must exist. Measured: for the over-bound and non-regular classes the apply's own W10c preflight ALREADY refuses
  the copy, so no hook could have run from them. The entry is still required — the gate layer must not depend on
  another module's guard — but the causal story was overstated. The comments were corrected to say "defence in
  depth", not "the only wall". A false claim in a security comment is a defect; this contract now records that
  rule four times, which is what it cost.
- **W10c-c/W10c-d** — refusing a symlinked scan ROOT was not refusing the OPERATION: `package_theme_dir` kept
  going and wrote through the link. It now refuses a symlinked theme dir and (W10c-d) a theme tree carrying any
  symlinked DIRECTORY up front, before anything is read or written, with the canary fingerprint proving nothing
  escaped. Verified `sound` — the verifier's own probe run found NO escaping write across a child link at the
  write site, at depth 3, through a chain, onto `/proc`, hidden, and inside `presets/<category>`, and pinned the
  budget boundaries empirically (10,000 entries and 31 nested dirs package; one more fails closed).
- **A `settings.toml` that is exactly AT the 4 MiB bound is claimed to report its hooks, not the conservative
  entry** — verified by reading (`take(max+1)` + `> max`), NOT by a test, because `inspect_theme_package` is not
  reachable from the CLI. Named as the one unverified boundary of W1d-b.

### Cross-model verification, fourth round (2026-10-13) — the dual adversarial review

Two models, BLIND to each other, on the same frozen target (`9d2bcf8`), with the same brief and told to REFUTE rather
than confirm: **MiMo v2.6-flash** and **GLM-5.3-Flash**. Both answered `refuted`, and each found the SAME BLOCKER by
its own separate route. The writer then confirmed it in the source, and reproduced it as a failing test before
touching anything.

- **THE BLOCKER, and it was a hole in the DESIGN, not a spelling**: the acceptance keyed a preset by its `category` +
  `file_name` and nothing else. But the apply copies the preset file AS IT IS ON DISK (`install_theme_presets`) and
  the assembled `overlay.lua` `dofile`s it, so the BODY is the executable payload. A theme that kept the name and
  rewrote the body therefore applied in SILENCE — exactly the rewire an accepted theme is supposed to re-warn about,
  and exactly the class this store exists to close. Both judges reproduced it; the writer's
  `rewriting_a_preset_body_makes_it_a_different_acceptance` observed `accepts() == true` against the old code (a real
  RED) before the fix.
- **Where the mistake was, stated so it is not repeated**: the detector was built in W1 for the WARNING's wording,
  which names preset FILES; W2c then used the report as the inventory of what EXECUTES. Those are not the same thing,
  and the report was the weaker one — the same class of error as the three refuted line-based rounds, one level up.
  The fix goes at the ROOT (the report carries the body), not downstream.
- **The fix (W2c-b)**: `LuaPreset` carries a `PresetBody` — `Text` verbatim, or `Unreadable` WITH the cause when the
  body cannot be read IN FULL. The body is read through the SAME guarded reader every untrusted theme file goes
  through, under this module's own tighter `MAX_PRESET_BODY_BYTES` (16 KiB, about five times the largest preset this
  project ships). Over that bound it is `Unreadable`, NEVER truncated: a truncated comparison would make two
  DIFFERENT long bodies equal, the collision this store exists to avoid. An `Unreadable` body makes the report
  uninspected, so nothing is remembered and the keeper is asked every time. `StoredPreset` gained the body
  (`Option<String>`; `None` for the unreadable case, which no stored entry can equal), and the warning now prints the
  cause on the preset's own line so the keeper SEES which part could not be read.
- **The keeper's decision on the mechanism** (2026-10-13, verbatim: `El cuerpo tal cual (Recomendado)`): the body is
  stored VERBATIM, not as a digest — no new crypto dependency, no collision argument to defend, and the store stays a
  readable record of what was accepted. The SHA-256 alternative was declined on the same grounds this store's own
  design already used.
- **The second finding, also from both judges** (GLM called it MEDIUM, MiMo an ADVISORY — the same defect with
  different labels, not a contradiction): the remembered branch promised "HVE will not ask again while the content
  above stays exactly as it is", and a RENAMED theme with identical content re-warns, because the entry is also keyed
  by the theme NAME. The copy now says the truth: `with this name and this content, HVE will not ask again, and it
  asks again as soon as either changes`.
- **Advisories both judges raised and this unit did NOT act on** (recorded, not hidden): the cache dir created by
  `save` carries no explicit mode (0777 under umask 000 — pre-existing for the whole cache layer; W7's explicit 0700
  was for the IPC dir); `load` reads THROUGH a symlink at the store path (the store is not authenticated, the
  residual this contract already discloses); and the armed token is spent by the first attempt after arming — a
  microseconds-wide window that fails towards the warning.
- **What both judges tried and could NOT break** (kept because a review that finds nothing is evidence of nothing):
  hostile theme NAMES (MiMo 21 spellings, GLM 20 — brackets, quotes, backslashes, newlines, tabs,
  `[accepted.Other]` payloads, `=`, `[[`, NFC vs NFD, 4000 characters): every one round-trips as its own entry and
  cannot forge or merge another's. Every store failure mode (not UTF-8, `version = "1"`, missing `version`, unknown
  `version`, duplicate tables, a malformed entry, a DIRECTORY at the path, a FIFO, unreadable, over bound, a planted
  symlink at the temp name). 0600 held under umask 000 AND 077. `save` replaces a symlink and a FIFO instead of
  writing through them. The armed token's one-shot and cross-theme behaviour. A clean package costing ZERO store
  reads (MiMo measured it with a counter instead of reading the code). Both read the render PNGs and reported the
  geometry sound.
- **Release mode**: MiMo verified `cargo test --release --bin hve -- theme_trust` → 68 passed, and grepped for
  `cfg(debug_assertions)` (none). GLM did not check it. Recorded as verified by one judge.

### Cross-model verification, fifth round (2026-10-13) — the scoped re-judgment of the fix

Both judges returned to the SAME frozen target, blind to each other, seeing only the round-1 ledger and the immutable fix
delta `9d2bcf8..d9e12af` (the judgment-day re-judgment rule). Both answered **`sound with findings`**.

- **L1 (the blocker) — FIXED, and both judges say COMPLETE, not merely tested.** The body reaches the comparison at
  every level that matters: the store serializes `StoredPreset.body` and compares it, the ARMED confirmation compares
  whole reports so a rewrite while the modal is up is judged again, and an `Unreadable` body makes the report
  uninspected everywhere (never consulted, never stored, warned every time). GLM verified the four paths; MiMo attacked
  with a matrix (one byte, trailing newline, exactly 16 KiB vs one over, non-UTF-8, symlink, empty, deleted, added,
  names swapped across categories) and could not break it. Both confirmed the cap is never TRUNCATED, so two long
  bodies can never collide.
- **MiMo found that the fix closed a hole the ledger never listed**: keying on bodies also made the ARMED confirmation
  body-aware, which at `9d2bcf8` was silently bypassable for a rewrite during the modal. A fix that closes an unlisted
  hole is worth recording as such.
- **L2 (the copy) — FIXED**, and now TRUE: the entry is keyed by the theme name too, so the sentence "with this name and
  this content … as soon as either changes" matches the mechanism.
- **L3-L7 — not fixed, as agreed** (each is a disclosed residual or a pre-existing behaviour of the cache layer; both
  judges re-confirmed that nothing about them was changed or caused by this delta).
- **THE NEW FINDING, confirmed by BOTH judges independently, with numbers**: the store's WRITE path has no bound while
  its READ path refuses anything over 4 MiB, so the store can brick itself and take every acceptance with it (the
  residual above carries the measurements). Both filed it as MEDIUM/ADVISORY, not as a blocker, because the direction
  is fail-safe. MiMo also called out, correctly, that this contract's first wording of that residual UNDERSTATED its
  reachability; the wording above is the corrected one.
- **The terminal verdict of the dual review, on the fix delta: APPROVED**, with the store-growth defect recorded as an
  outstanding non-severe finding. APPROVED here means "the blocker is closed and no severe finding survives", NOT "this
  code is perfect": the outstanding finding is named above with its reproduction, and fixing it is a separate decision
  the keeper owns.
- **Both judges read the render PNGs by eye** in their own per-run directories: the new copy sentence in both branches,
  the unreadable preset's cause wrapped to four lines inside the card, the buttons pixel-identical (the test pins that
  band to a zero diff), the 18-hook list scrolling without moving them. Nothing clipped.
- **A process limit both judges hit and reported honestly**: MiMo could not compile NEW tests in a scratch copy — `/tmp`
  had ~2 GiB free while `target/debug/deps` alone is 9.1 GiB — so it proved the store-growth case with a standalone
  probe crate pinned to this `Cargo.lock`'s exact `toml`/`serde` versions plus the repo's own pinned tests, and said so
  instead of claiming an end-to-end run it did not do. GLM ran 13 compiled probes and a full suite.

### The dual review's ledger and terminal verdict, at a glance

```yaml
target_identity: 9d2bcf8 (unit) -> d9e12af (fix delta 9d2bcf8..d9e12af)
judges: MiMo v2.6-flash and GLM-5.3-Flash, blind to each other, identical brief
rounds_used: one correction round, one scoped re-judgment (the budget allows two of each)
confirmed: [L1 preset BODY not in the acceptance key, L2 copy promised silence a rename breaks,
            F1 the store's write path has no bound while its read path does]
suspect: []
contradictions: []          # MEDIUM vs ADVISORY on the same defect is a label difference, not a disagreement
info: [L3 load reads through a symlink at the store path, L4 the cache dir carries no explicit mode,
       L5 the armed token is spent on a mismatch, L6 store table order, L7 relative path without HOME]
fix_work_units: [d9e12af]
scoped_rejudgment: approved  # both judges: sound with findings, no severe finding survives
terminal_state: approved
outstanding: F1 (non-severe, fail-safe direction) — the keeper decides whether W2c-c closes it
skill_resolution: paths-injected — hve2 -> .opencode/skills/hve2/SKILL.md
```

### The keeper's own themes — measured, not assumed

FOUR of his installed themes trigger the warning: **Cars, Night, Space, Spiderman**. They carry his OWN three
one-line hooks pointing at `/home/ximo/.local/share/sddm/themes/SilentDinamic/sddm-sync.sh` (his script; HVE never
put it there). They were saved BEFORE the sanitiser landed (`91a5f60`, 2026-10-11): Night 10-04, Cars/Space 10-10.
The PACKAGED copies under `assets/themes/` are clean — an empty `[hooks]` section and zero `/home/` paths.

Consequence, and a real gap: **a theme saved before the sanitiser stays dirty forever**, because nothing re-saves
it. Re-saving those four themes would drop the stale hooks and stop the nag for the keeper. The keeper's own
`remember-my-acceptance` idea (approved 2026-10-12) covers the other direction: themes that arrive from someone
else.

### Process lessons (they cost real work here)

- **Run `cargo` in the FOREGROUND in a delegated unit.** A writer that backgrounds a build ends its turn without
  the output and the build dies with it. This happened twice.
- **On a provider error or a cancellation, ASSESS the working tree before reverting.** Twice the dead session's
  leftovers compiled and were finished by the next writer instead of being thrown away (W2's 1037 lines, W10b's
  331).
- **A late reporter is not a zombie.** W10b's completion notice arrived after its commit; the parent briefly told
  the keeper it was dead. Stopping an already-committed writer is harmless either way.
- **Ask for the evidence IN the commit message** on Tier 3 units: W10b's commit body is empty, so the audit trail
  lives only in the chat.
- **`.atl/skill-registry.md` and `.atl/.skill-registry.cache.json` show up modified** in this tree; they are a
  plugin's artifacts, not this contract's — never stage them with a unit. (They were refreshed and committed on
  their own in `417203d`.)
- **A backgrounded build can RESUME the session that launched it.** This is how W2b's dead session came back to
  life: it had ended its turn waiting for a build, the build finished, the session resumed and started writing the
  `.slint` implementation — WHILE the replacement session was verifying. The replacement noticed the files moving
  under it, stopped duplicating and turned into an independent verifier, which is exactly the right reaction. Two
  sessions on one unit is survivable, but it was luck: the lesson is to treat a session that backgrounded work as
  possibly ALIVE, not dead.
- **The flaky module is real and corroborated.** Three DIFFERENT tests in `reload_coalescer` have now failed under
  full-suite parallelism across independent runs — `sandboxed_missing_flock_fails_open` (one run),
  `sandboxed_no_loss_under_idle_exit_stress` ("bash must spawn: NotFound", another) and
  `sandboxed_idle_drainer_exits_and_removes_pidfile` (a third) — and all pass in isolation and as a module. A FOURTH
  test, in a DIFFERENT module (`watcher::tests::watcher_child_dies_when_hve_dies_end_to_end`), failed once the same
  way and passed alone and as a module; the watcher was untouched by that commit. That is the load-dependent flake
  the health audit predicted (19 sleeps, zero `#[serial]`), now with evidence in two modules. It must be fixed in
  Lane C, not waived.
- **A "the detector is complete now" claim needs an adversarial SPELLING hunt, not a review.** Both times the
  cross-model verifier was asked to attack the TOML/spelling hull it REFUTED it, with blockers: first the
  root-level dotted key and inline table, then an escaped key (`"\u0068ooks"`), a leading BOM, and a multi-line
  inline table that is valid TOML under spec 1.1 (the parser family already in this repo's `Cargo.lock`). A
  line-based reader is never complete; the durable fix was the RULE, not the spelling list — fail closed whenever
  the reader knows it lost the table.
- **Re-read the task file before editing it: a live session may have written it while you worked.** This contract's
  `## Still pending` was read at 05:34 and edited at 06:50; the first edit applied, the second failed because a
  late session had committed `5dcccd0` into the middle of the chain implementing W2b's own record. History stayed
  linear and nothing was lost, but the parent's copy of the file was stale for 20 minutes. The tree says the truth;
  the read does not.

## W2c — the remembered acceptance (the keeper's `remember-my-acceptance`)

**Opened** 2026-10-13 on the keeper's instruction `sigue con theme-package-trust`. **Route**: the parent writes
inline (no parallel unit, no context backstop), then **Tier 3 cross-model verification** — it writes a new file,
handles its path and its mode, and closes a security MEDIUM, which is Tier 3 by AGENTS.md.

### Specs

- **S1** — One acceptance PER THEME, keyed by the EXACT content the keeper was shown — everything that RUNS:
  `provider` + hook `key` + hook `command` for every hook, and `category` + `file_name` + the preset's BODY for every
  preset (the apply copies that file as it is ON DISK and the assembled overlay executes it, so the same name with a
  different body is a different package). Two reports are the same acceptance only when every one of those fields is
  identical, order included (the report order is deterministic by construction). An UNINSPECTED report — a hook table
  nobody could read, or a preset body that could not be read in full — is NEVER remembered: a file HVE could not read
  can change inside without the report changing, so a record of it would apply rewritten content in silence. See L3
  and L7 in the log, where this clause replaced two weaker ones.
- **S2** — A remembered acceptance applies the theme with NO warning, from BOTH apply paths. Nothing else changes:
  no strip, no filter, no rewrite of the package.
- **S3** — A theme whose content CHANGED warns again, exactly like one never accepted: rewriting a preset's BODY
  under the same file name counts, because that body is what the overlay runs. Rewiring an accepted package must
  never apply it in silence.
- **S4** — An armed confirmation authorises ONLY the content the warning SHOWED. The re-entry re-reads the package;
  a package whose content no longer matches the one shown warns AGAIN instead of applying on the old consent.
  This is W2's first MEDIUM, closed properly.
- **S5** — A package carrying neither hooks nor presets is NEVER remembered and never warned about, and it costs
  NO store read: pre-W2 behaviour stays untouched.
- **S6** — The store is a file of its own, `theme-trust.toml` in HVE's cache dir, mode 0600, written atomically
  (unique temp created with `O_EXCL` + rename, so a pre-existing symlink at that path is REPLACED, never written
  through). It is never written inside a theme package.
- **S7** — Every way the store can fail — missing, unreadable, over bound, not UTF-8, not TOML, unknown version, a
  FIFO hanging the open — is non-fatal and NEVER a silent consent: the store degrades to "nothing accepted" and the
  warning comes back. Only a MISSING file is silent (a first run is not a problem).
- **S8** — The warning copy states what Continue does — the acceptance is remembered while the content stays as
  shown, and the warning returns when it changes — while keeping every existing honest-limit line and claiming
  nothing about prevention. For a package that could not be inspected it says the OPPOSITE (nothing is remembered,
  it asks again every time), because the copy is told the same rule the store follows: promising a silence HVE will
  not keep would be a false claim in the one place the keeper reads.

### Acceptance

- `cargo test` green and `cargo check --all-targets` clean.
- Both apply paths behave identically; the store is read only for a package that carries something.
- The original W2 criteria still hold: no silent apply, declining changes nothing on disk.
- The headless render check, read by eye, after the copy change (AGENTS.md).

### Residuals of W2c, disclosed rather than hidden

- **The warning lists preset NAMES and their bodies are not printed.** Hooks show their command text verbatim; a
  preset shows `category/file.lua`. The acceptance now covers the body, so a change re-warns and nothing applies in
  silence — but the keeper accepts a body he has not READ. Named as an open decision (it is W1/W2's original UI
  shape, not something this unit introduced; showing bodies is a UI change with its own render work).
- **CLOSED by W2c-c — the store's own bound, kept here as the reason the bound exists.** The store had its 4 MiB read
  bound (the shared guarded reader's) and NO write bound, so it could outgrow its own reader and then be refused AS A
  WHOLE: every acceptance forgotten, every risky theme warned again, and the next confirmation REPLACING the file with
  only that entry ("replace, never merge"). Measured by BOTH re-judges, with numbers rather than adjectives: a single
  LEGAL theme (the package bounds are 4 MiB per file and 64 MiB in total) reached it with **~104 presets** — the TOML
  escaping inflates control-character-heavy bodies up to **2.7×** their raw bytes — and **256 presets** at the 16 KiB
  cap produced a 4,215,481-byte file, over the 4,194,304 bound; the first version of this store needed ~50,000 entries
  to get there, so the BODY KEYING is what made it reachable. One acceptance could also write an arbitrarily large file
  (a 10,000-preset theme → ~160 MB on one click). The direction was fail-safe — the warning came back, nothing applied
  in silence — which is why both judges filed it as MEDIUM/ADVISORY and not as a blocker. W2c-c bounds BOTH ends: the
  write refuses a store it could not read back, and the report spends at most a fixed budget per theme. W2c-d then put
  that budget in the CURRENCY the store writes (fields plus the format's per-entry overhead), because a budget counted
  in code bytes let a theme with thousands of long names pass every rule the copy reads and still be refused by the
  store — the copy promising a silence that never came, which is the one thing this feature may not do.
- **There is no way to FORGET an acceptance from the UI.** The store can be deleted by hand, and re-saving the theme
  drops the stale hooks (Night, Cars, Space, Spiderman — see above), but the panel offers no "stop remembering this".
  Named here so it is a decision, not an oversight.
- **The threat model is unchanged and narrow**: the acceptance only decides whether the WARNING appears again. An
  accepted theme still runs as the keeper — this is a consent gate, not a sandbox, and the store does not change
  that.
- **The store inherits the stat-then-open TOCTOU** of the shared bounded reader, exactly as every other guarded read
  in this contract does. Same disclosure, same reason: the attacker it presumes is already running as the keeper.
- **The store is not authenticated.** Anything that can rewrite `theme-trust.toml` can silence a warning — but
  anything that can write there already runs as the keeper, which is the same limit the whole gate has: consent is
  per-user, and a local process with the keeper's own permissions is outside this feature's threat model (an
  untrusted THEME, which cannot write outside its own directory). Recorded, not pretended. `load` also READS THROUGH
  a symlink planted at the store path (verified by the first judge); `save` never writes through one.

### Decisions recorded here, so they are decisions and not oversights

- **Exact content, not a hash.** A 64-bit fingerprint is trivially collidable, and the attacker in this scenario
  has write access to an already-accepted theme: they would add a malicious hook and search for a colliding
  spelling. A collision-resistant hash needs a new crypto dependency; storing the accepted CONTENT and comparing it
  exactly needs none, cannot collide, and leaves an auditable record of what the keeper actually accepted.
- **The cache dir, not a new state dir.** Losing the file (a cleared cache) costs a re-warning and never a silent
  consent — the failure direction is safe.
- **An unreadable store is REPLACED, never merged**, when the keeper accepts again: the loss is bounded to
  re-warnings, while refusing to write would loop him through the same warning for ever.
- **Consent is recorded only when the apply actually goes through** (the armed re-entry reached the gate), not on
  the Continue click: a swallowed re-entry remembers nothing and warns again.
- **The acceptance is keyed by the theme NAME**, which is the store key the apply already resolved; it is never
  joined to a path, so a name needs no path rule. A rename loses the acceptance — a re-warning, not a risk.

### Tasks

| ID | Task | Route | Evidence |
|---|---|---|---|
| T1 | `src/theme_trust_store.rs`: the store, its format, its bounds and its tests | inline | RED observed (17 tests, 4 failing: symlink written through, no parent dir, mode 0644, FIFO hang), then GREEN. Commit `9d2bcf8` |
| T2 | the gate remembers the SHOWN content and takes the stored acceptance as input; `main.rs` records the acceptance on the apply that goes through | inline | RED observed (2 tests: `Proceed` where the MEDIUM requires `Hold`; `Hold` where the stored acceptance requires `Proceed`), then GREEN. Commit `9d2bcf8` |
| T3 | the warning copy states what Continue does, in both states | inline | copy test + the render PNGs read by eye. Commit `9d2bcf8` |
| T4 | the dual review's blocker: a preset's BODY in the report and in the key | inline | RED observed (`rewriting_a_preset_body_makes_it_a_different_acceptance` failing against `9d2bcf8`), then GREEN. Commit `d9e12af` |

T1 and T2 land as ONE commit: a module with no caller leaves eleven dead-code warnings, and
`cargo check --all-targets` clean is an acceptance criterion per unit. T4 is the correction round the dual
verification demanded (see the fourth and fifth rounds above).

## W2c-c — the bound per THEME and the bound per STORE

**Opened** 2026-10-13 by the keeper's answer `Sí, ciérralo ahora (Recomendado)`, to the defect BOTH re-judges of
`d9e12af` confirmed independently. **Route**: the parent writes inline, then the second (and last) scoped re-judgment.

### Specs

- **S9** — A bound per ITEM with no bound per THEME is half a bound. ONE theme spends at most
  `MAX_REMEMBERED_THEME_BYTES` (128 KiB) of code in a report — every hook's command text plus every preset body.
  Hooks are always reported IN FULL (the warning shows their command text, and hiding one is worse than a large
  report); the budget stops the preset BODIES: a body that does not fit is reported as NOT READ with the cause that
  stopped it, never truncated, and the preset is still NAMED so the warning stays truthful about what the package
  carries.
- **S10** — A report over the budget is never remembered, and the warning says so in its own words ("HVE cannot
  remember a decision for this package — part of it could not be read in full, or it carries more code than HVE
  records"). One rule feeds both the copy and the store, so the text cannot promise a silence the store will not
  keep.
- **S11** — The store's WRITE has a bound of its own (`MAX_STORE_BYTES`, half the 4 MiB its reader enforces) and
  REFUSES a store it could not read back. The refusal leaves the file on disk untouched, so every acceptance already
  in it survives and only the new one is lost — the exact opposite of the old behaviour, where one acceptance could
  brick the store for all of them.

### Acceptance

- `cargo test` green, `cargo check --all-targets` clean.
- A legal theme of 260 presets at the cap (the judges' own reproduction) writes nothing at all and is never answered
  as accepted.
- A store that would exceed its write bound is refused, the old file is byte-identical afterwards, and it is still
  readable.
- The render check read by eye for the new copy sentence.

### Decisions

- **128 KiB per theme**: about three times a realistic theme (the shipped presets are 1-3.5 KB each, so ~40 presets
  fit), and it keeps the report and the store small without refusing anything a real theme does.
- **2 MiB for the store**: half the reader's 4 MiB, so anything this build writes is readable by this build with room
  to spare, and roughly sixteen themes at the per-theme maximum — or fifty realistic ones — fit.
- **The write REFUSES rather than evicting**: silently dropping an acceptance to make room would be a silent consent
  decision, the one thing this whole feature must never do. A full store nags and says so in the log; deleting the
  file is the keeper's call.

## W2c-d — the theme budget in the currency the store writes

**Opened** 2026-10-13 after the LAST scoped re-judgment (the dual review's budget allows two rounds of each, and this
one used the second). Both judges found the same thing independently, with their own reproductions.

### The defect, and why it mattered

`remembered_bytes` counted the CODE (hook commands, preset bodies) while the store's write bound counts SERIALIZED
BYTES — two currencies for one budget. A theme carrying thousands of long names or keys was therefore "inside" the
per-theme budget by the number the copy reads, and still produced a store entry over the write bound, which `save`
refused: the warning showed the REMEMBERED branch ("HVE will not ask again") and the next apply asked again. That is
S10's own absolute claim ("the text cannot promise a silence the store will not keep") broken by the second rule.
Both judges measured it: 10,000 presets with 1-byte bodies and ~200-character names counted 10,000 and serialized to
2,670,037 bytes; with 0-byte bodies and 255-character names the count was **0** and the entry still 2,452,537. One
judge also measured the escaping factor correctly at **6×** (`\u00XX`), not the 2.7× this record had.

### Specs

- **S12** — ONE currency. Every field a store writes is counted — a hook's provider, key and command text; a preset's
  category, file name and body — plus a per-entry overhead for the format itself
  (`STORE_ENTRY_OVERHEAD_BYTES`, measured at ~67 and charged at 80). `hook_bytes` and `preset_entry_bytes` are the
  ONLY places that decide what an entry costs, and BOTH the scan that spends the budget and `remembered_bytes` call
  them, so the two can never drift apart again.
- **S13** — The budget is sufficient by construction, and the arithmetic is written where the constant lives: a report
  inside 128 KiB serializes to at most `6 × 128 KiB` plus the per-entry overhead, and an entry is at least 81 bytes,
  so ~1,600 entries fit → ≈900 KiB, comfortably inside the store's 2 MiB write bound. A theme inside the budget
  therefore cannot be the reason a store refuses to be written.
- **S14** — A cause message must be TRUE of the file it names: a preset the scan READ and then could not keep says
  "could not be KEPT by this scan", and only a file after the budget was already spent says "was not read".

### Acceptance

- `cargo test` green, `cargo check --all-targets` clean.
- The judges' reproduction (2,000 presets with 200-character names and 1-byte bodies) is refused by the rule the copy
  reads, not only by the store.
- No realistic regression: the four themes that trigger the warning and the 42 shipped presets stay far inside the
  budget.
- The render read by eye.

### Decisions

- **The count stays a COUNT (fields + overhead), not a serialization.** Measuring the true serialized size inside
  `remembered_bytes` would make the report depend on the store's format and on the TOML crate; the per-entry overhead
  makes the count a safe UPPER BOUND of what the store writes, and `save` keeps measuring true bytes as the backstop.
  Two mechanisms, one direction: the copy can only ever be too pessimistic, never too generous.
- **The store's write bound is unchanged** (2 MiB): it is the backstop for the accumulated case across themes, which
  no per-theme budget can see. Measured: 15 themes at the maximum fit, the 16th is refused; ~350 keeper-shaped ones
  fit, so the earlier "fifty" was conservative and the "sixteen" was off by one.
- **A legacy store over 2 MiB (written by the `d9e12af` build) cannot be rewritten** and must be deleted; the refusal
  message says so. Measured to be hypothetical on this machine (no `theme-trust.toml` exists in the keeper's cache
  dir), and recorded rather than hidden.

### Evidence

- RED observed first: 2,000 presets with 200-character names counted **2,013** bytes where the store writes ~590 KB.
- 6 new/updated tests. The unified currency also caught a divergence the fix itself had introduced between the scan's
  own accumulator and `remembered_bytes` (the first run of the new code failed three tests on exactly that).
- Verified by ONE independent cross-model verifier, not the dual pair: the judgment-day round budget was spent.

## Still pending

- **W2b — DONE** (`317d55e`), but read how it landed: the implementation was written by the RESUMED old session
  while the replacement verified it. Its own verifier confirmed 6/6 focused, the full suite green apart from the
  `reload_coalescer` flake, `cargo check --all-targets` clean, and the render read by eye (the blue focus ring
  moving from Cancel to Continue with identical geometry). It also corrected two of the pre-written assertions that
  demanded `export function` — syntax Slint 1.17 REJECTS (only `public`/`protected` parse); the fix was legitimate,
  not a weakening.
  **Its two residuals, deliberately not fixed** (the parent can queue them):
  1. The carriers list does not scroll from the keyboard while the gate holds focus — the arrows move the BUTTON
     cursor, so a long list can only be scrolled with the wheel.
  2. If a shell reseed fires while the gate is open (an earlier transition finishing, or the panel closing),
     `shell-kbd.focus()` steals focus from the modal and the keys go back to the gallery behind it. NOT a security
     hole — the risky theme stays held — but the modal goes keyboard-deaf. Stated one-line fix: add
     `&& !root.trust-warning-open` to those observers.
- **W2c — DONE** (`9d2bcf8`, corrected by `d9e12af`), and it went through the harshest review this contract has had: a
  dual adversarial review by two blind judges (MiMo v2.6-flash and GLM-5.3-Flash) which BOTH refuted the first commit
  for the same blocker — a preset's BODY was not in the acceptance key, so a theme could keep the name, rewrite the
  payload and apply in SILENCE — and then both confirmed the fix. It also closed W2's first MEDIUM: the armed
  confirmation now authorises the content the warning SHOWED, never the name. The dual review's terminal verdict on
  the fix is APPROVED, with one outstanding non-severe finding named next.
- **W2c-c — DONE** (`a1091de`), and the last scoped re-judgment confirmed the defect CLOSED at its root: the judges'
  own reproduction (a legal theme of 260 presets) now writes nothing at all, and the accumulated case refuses with the
  old file byte-identical and readable. Both judges also confirmed no realistic theme is affected.
- **W2c-d — DONE**, the finding BOTH judges of that last round raised: the per-theme budget was counted in the wrong
  currency (code bytes instead of what the store writes), so the copy could promise a memory the store refused. The
  currency is now ONE pair of functions used by both the scan and the report. Verified by one independent
  cross-model verifier, since the dual review's round budget was spent.
- **W4b — the W4/W5 reconciliation (L4), decided but not yet applied**: the plugin-enabled check moves BEFORE the
  resolve/download loop in `src/providers/mpvpaper.rs`, so a disabled plugin never downloads up to 1 GiB for a theme
  the apply is going to refuse. Two lines plus the ordering assertion in the existing by-construction test.
- **OWED verification: none.** Every commit in this contract has been verified by a model that did not write it
  (`d5cc429`, `881ae47`, `32b9fcd`, `299af0a`, `9f05e5e`, `0c842d3`, `246bf1c` all closed their rounds, and
  `9d2bcf8`+`d9e12af` closed theirs with a DUAL review by two different models). The ONE boundary that remains
  unverified is stated above: a `settings.toml` exactly AT the 4 MiB bound.
- **The W4/W5 MEDIUM — RESOLVED, see L4 and W4b above**: the contract's line was the correct intent and the CODE was
  wrong (nothing about a disabled plugin needs the media resolved, and the download is driven by untrusted theme
  input). The check moves before the resolve/download loop; the unit is queued as W4b.
- **Lane C (health, serial and alone — start it only when nothing else is in flight)**: `cargo fmt` (23,362 diff
  lines — a dedicated commit, never mixed), the ~70 autofixable clippy lints (NEVER the float-comparison class —
  it changes NaN semantics the layout relies on), the `tray.rs` pixel-chunk guard, the dead-code suppressions that
  silence whole modules, the source-text tests and the sleep-based tests, and the `ksni` 0.2 → 0.3 bump.
  - W14 has MORE work than when this contract opened: `317d55e` added a source-text assertion test
    (`trust_warning_keyboard_wiring_is_pinned_by_construction`) and the W7/W10c units added more of the same
    pattern; they are named here so the cleanup is not read as a regression.
  - The `reload_coalescer` flake is corroborated (two different tests, two independent runs, both pass in
    isolation) and must be fixed in Lane C, not waived.

### Residuals (real, disclosed, NOT to be "fixed" blindly)

- **The stat-then-open TOCTOU survives** in every guarded read, and it is wider at the two restore sites (a stat,
  a second stat inside `theme_file_is_bounded`, then the copy's own open). A local process that swaps a regular
  file for a FIFO inside that window still hangs the read, permanently, with no retry. Exploiting it presumes code
  already running as the user, which is not the at-rest attacker this gate defends against; closing it needs
  `openat`/`O_NONBLOCK` plumbing. Reported by the W10c verification, NOT hidden.
- **The save-side `fs::copy` from a LIVE path** (`noctalia.rs:2109`, the user's own `settings.toml`) has no
  regular-file predicate: a FIFO planted there hangs `save`. **Out of this contract's scope by its own words**
  (live-config reads are not theme files) and pre-existing. Named so it is a decision, not an oversight.
- **CLOSED by W1d — kept here so nobody re-reports them**: the "`true` as a command" false positive and the silent
  loss of a key sharing a multi-line string's closing line are BOTH gone. A real parser reads the document, a
  non-string value is data and not a command, and there is no closing-line residue left to lose. `W1d` deleted
  −456 lines of hand parser and with them both defects.
- **A write whose FINAL path component is a symlinked FILE still writes through it** (`fs::write` follows the last
  link). Measured by the verifier and DISCLOSED in `package_theme_dir`'s own docstring. The packaging guards refuse
  a symlinked DIRECTORY; a file symlink inside a theme is read on purpose, and this is its write-side twin.
- **`check_theme_tree` and `scan_theme_dir` swallow a mid-iteration `read_dir` error** (`entries.flatten()`), so a
  tree whose entries cannot all be listed is walked as if it were complete. No realistic trigger under the threat
  model (the same unreadable path blocks the apply too); ADVISORY, recorded rather than hidden.
- **The maintenance pass initialises no tracing subscriber**, so a packaging refusal reaches the operator as the
  stdout `skipped (…)` line only, never as the `warn!`. The warning TEXT itself is test-pinned; the visibility of
  it in that mode is not.
- **`ThemeManager::save` uses `create_dir_all`, which follows a pre-planted symlink** at `themes_dir/<name>`. The
  interactive save path, user-initiated and name-owned by the keeper — outside Lane B, named so it is a decision.
- **A process lesson that cost a verifier real time**: `cargo test --bin hve` does NOT rebuild `target/debug/hve`,
  so an end-to-end probe of the CLI binary silently exercises the PREVIOUS build. Run `cargo build --bin hve`
  first. The stale binary reproduced the very write-through the fix closed, which is how it was caught.
- **Deliberately NOT bounded** (named by W10b, do not "fix" blindly): binary MEDIA reads (`theme_manager.rs:841/
  858/876`, `shell/gallery/thumbs.rs`) because a wallpaper or video legitimately exceeds the 4 MiB TEXT bound, and
  live-config reads (`noctalia.rs`, `preset_store.rs`) which are not theme files.
