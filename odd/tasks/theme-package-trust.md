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
| docs | `6b2791d`, `788772a` | this record: the second round, the residuals, the open units | — |

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
- **The flaky module is real and corroborated.** Two DIFFERENT tests in `reload_coalescer` failed under full-suite
  parallelism across independent runs — `sandboxed_missing_flock_fails_open` (one run) and
  `sandboxed_no_loss_under_idle_exit_stress` ("bash must spawn: NotFound", another) — and both pass in isolation
  and as a module. That is the load-dependent flake the health audit predicted (19 sleeps, zero `#[serial]`), now
  with evidence. It must be fixed in Lane C, not waived.
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
- **W2c — the keeper's `remember-my-acceptance`** (approved 2026-10-12, now the ONLY substantive unit left in
  Lanes A/B): remember the acceptance PER THEME keyed by a fingerprint of what was accepted, so a theme whose
  contents changed warns again. It also closes W2's first MEDIUM properly — the confirmed branch DOES re-read the
  package today (`main.rs:1453-1459`), but `TrustGate::request` short-circuits on the armed confirmation before
  consulting the fresh report, so a package rewired while the modal is open still applies on the old consent.
  Needs a small state file of its own, never a change to the theme packages.
- **OWED verification: none.** Every commit in this contract has been verified by a model that did not write it
  (`d5cc429`, `881ae47`, `32b9fcd`, `299af0a`, `9f05e5e`, `0c842d3`, `246bf1c` all closed their rounds). The ONE
  boundary that remains unverified is stated above: a `settings.toml` exactly AT the 4 MiB bound.
- **The W4/W5 MEDIUM (unresolved, needs one decision)**: the verified report says the download still happens
  BEFORE the plugin-enabled check, while this contract's line 51 says it must not. The contract line was written
  by the parent; the code follows the older instruction. One of the two is wrong — reconcile them and record which.
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
