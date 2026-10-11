# Feature: the applied theme keeps its Noctalia layout

**Locator**: `odd/tasks/noctalia-layout-survives-the-apply.md`
**Engram mirror**: topic `odd/noctalia-layout-survives-the-apply/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: IN PROGRESS — decided by the keeper 2026-10-11 ("el tema de noctalia que lo haga un subagente
gratuito… mimo 2.6 free"), right after the Mosaic topic was closed as finished.
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~120–220 authored changed lines → single PR.
**Route**: delegated writer (`src/providers/noctalia.rs`), parent verification including the keeper's live look.
**Restore point before starting**: tag `restore-point/pre-noctalia-layout-guard-2026-10-11` plus the themes
and presets tarball in `~/Proyectos/hve-backups/`.

## Objective

When HVE applies a theme, the theme's Noctalia LAYOUT must end up in force on the live desktop — the bar
(and its widgets), the desktop widgets, the dock, the shell panels — and it must still be there when the
apply is over. Today the apply can lose it silently, and the keeper sees a theme whose palette and wallpaper
changed while the layout stayed as the previous theme's.

## What the keeper reported (2026-10-11)

He applied a theme; the palette and the wallpaper changed, but the bar's position and the rest of the
Noctalia layout stayed as the theme he came from. He also observed that the layout DID move on every apply
while it was healthy, so this is a loss, not a reload that was ignored.

## Evidence (collected live, read-only, 2026-10-11)

- `noctalia config export` — the RUNNING instance's active config — had **no `[bar]` section at all**, and no
  `[widget.*]`, no `[desktop_widgets]`, no `[dock]`, no `[shell]`, no `[plugins]`. So the instance was
  rendering Noctalia's DEFAULT layout. The live file `~/.local/state/noctalia/settings.toml` matched that
  minimal content (2471 B).
- The THEME PACKAGES do carry layouts: `~/.config/hve/themes/JoKer/providers/noctalia-v5/settings.toml` is
  11741 B with `[bar]`, `[bar.barra_lateral]`, `[bar.default] position = "left"`, `[desktop_widgets]`,
  `[dock]`, `[shell]`, `[plugins]`; `noctalia config validate` reports it valid (3 benign warnings).
- HVE's apply copies the theme's file over the live one and then asks Noctalia to reload it
  (`src/providers/noctalia.rs`, apply step 0, ~:1700-1740).
- Timeline of the failing apply (journal + `~/.cache/hve/hve.log`, local time): 13:26:31.541 HVE copies the
  file; 13:26:31.694 HVE runs `noctalia msg config-reload`; 13:26:32 the keeper's OWN watchdog
  (`noctalia-sddm-sync.path` → `sddm-watch.sh`) logs `[hooks] re-inserted into settings.toml` — i.e. Noctalia
  had ALREADY rewritten the file within ~1 s of the reload and dropped `[hooks]`; 13:26:34.089 the final live
  file is 2471 B with no `[bar]`. The watchdog's own header documents the same behaviour for `[hooks]` /
  `community_ids` ("Noctalia rewrites the whole file … a recurring bug, seen 08-08 and 17-08").
- Mechanism: Noctalia re-persists its IN-MEMORY config on its own IPC-driven changes (theme/palette,
  templates, wallpaper). That makes it a RATCHET — once a section is gone from the file, the persisted memory
  keeps writing it away and no theme apply brings it back.
- Hand test that restored it: copy the theme's `settings.toml` over the live file and run
  `noctalia msg config-reload` → `[bar]` present again in BOTH the file and the active config, for Space and
  for JoKer, with the keeper's watchdog running. A 0.2 s sampler across a healthy apply pair then showed
  `bar=1` in every sample (Space writes 5798 B, JoKer 11741 B, Noctalia re-persists 5726 B / 12119 B 1–3 s
  later). Each individual HVE IPC step was isolated and is innocent: `config-reload`,
  `color-scheme-set custom <name>`, `plugins disable noctalia/mpvpaper`, `templates-apply`,
  `wallpaper-set "" <path>`.
- Why it went unnoticed for so long: **Space carries Noctalia's DEFAULT bar**, so falling back to the default
  LOOKS like Space. The loss only became visible on a theme with a different bar.

## Scope (authorized)

- **U1 — the predicate**: define, as a pure function over two texts (the theme's packaged `settings.toml` and
  the live one), whether the layout the theme carries is still in force. Decide WHICH sections/keys count as
  "layout" (at least `[bar]` and its widgets; judge whether `[desktop_widgets]`, `[dock]`, `[shell]` and the
  per-widget `[widget.*]` keys belong in the same check) and document the choice. It must NOT require a full
  textual equality: Noctalia legitimately rewrites the file (wallpaper paths, hooks, `config_version`).
- **U2 — verify and repair in the apply**: after the layout is restored and the reload has happened, verify
  the live file still carries the theme's layout; when it does not, rewrite it and reload ONCE more, bounded
  (no loop), and log what happened clearly enough to diagnose the next occurrence. This mirrors the existing
  D4 re-assert of `noctalia/mpvpaper` in the same function: same shape, same best-effort contract — a layout
  problem must never fail the whole theme apply.
- **U3 — tests**: (a) a live file whose layout section was wiped triggers exactly one repair; (b) an intact
  live file triggers none (no needless rewrite); (c) a theme that carries no layout sections is a no-op;
  (d) the repair is bounded to one attempt even if the rewrite does not stick; (e) the check tolerates the
  legitimate rewrites (a changed wallpaper path, a re-added `[hooks]`, a bumped `config_version`) and reports
  "still in force".
- **U4 — parent**: `cargo test`, `cargo check --all-targets`, release build, backup + install
  `~/.local/bin/hve`, relaunch with the window shown, then the keeper's live look: applying a theme whose bar
  differs from the current one must move the bar, every time.

## Out of scope

- Fixing Noctalia itself, or its persistence behaviour.
- The keeper's `sddm-watch.sh` watchdog and its `[hooks]` / `community_ids` healing (his own tool).
- The palette, the wallpaper, the colour-authority mute and the `noctalia/mpvpaper` D4 re-assert: unchanged.
- Any migration of the keeper's live Noctalia config.

## Verification of U12–U15 (2026-10-11/12, cross-model): verified_with_findings

The identity token closed the seconds-scale hole the second refutation found: the check now
runs after the settle and again before the repair reload, so the residual window between the
check and the write is microseconds-to-milliseconds, and the same-theme re-apply also
supersedes correctly. The U12 scenario test genuinely fails on `c70c198`, the U14 pin really
dies when the short-circuit is deleted, and nothing else in the apply was weakened. Three
residues it surfaced are registered below as (e), (f) and (g).

## Stated limits (U15 — registered, NOT fixed)

- **(a) The colour re-assert worker can still land AFTER the layout worker verified.**
  Both are detached by design (U9 / the off-thread colour re-assert idiom); neither
  coordinates with the other. A colour re-assert that rewrites live state after the
  layout verify ran is out of this contract's control — same asynchronous-by-design
  caveat U7 already stated, kept registered here.
- **(b) A detached write during process exit can still truncate the file — from the OTHER
  writers.** U13 covers exactly ONE writer: the layout repair worker now publishes through
  a temporary sibling + `rename`, so IT can leave only the complete old file, the complete
  new file, or (exit between temp write and rename) the old file plus a stale
  `.part-settings.toml` sibling — never a torn `settings.toml`. It does NOT cover the
  apply's own `fs::copy` (step 0), the rotation neutralizer's `fs::write`, Noctalia's own
  re-persist, or the keeper's watchdog: all still truncate-and-write in place, and a
  process exit or crash mid-write of any of them can leave `~/.local/state/noctalia/settings.toml`
  truncated. Converting those is out of this contract's authorized scope.
- **(c) `ColorStub` waits only for re-assert workers**, not for layout-verify workers: a
  future test author who arms a layout worker through a `ColorStub` sandbox (a theme
  carrying `providers/noctalia-v5/settings.toml`) must ALSO wait for the layout worker
  (`wait_for_verify`, as `LayoutStub` does) before the sandbox's process-global env is
  restored — otherwise the worker can outlive the test with a stale PATH/HOME, exactly
  the pollution failure `LayoutStub::drop` and `ColorStub::drop` exist to prevent.
- **(d) The timing pins are margins with conditions, not guarantees.** The
  `elapsed < 500 ms` pin (`apply_returns_without_waiting_for_the_layout_verify`) and the
  re-pinned worker pin (`elapsed < 1500 ms` against a 3000 ms configured settle, U14)
  both separate "paid a multi-hundred-ms settle" from test noise with a wide margin; their
  conditions are: tests run `#[serial]`, the stub `noctalia` CLI on the sandboxed PATH must
  resolve and exit in milliseconds, and the machine must not be so loaded that a stub CLI
  invocation itself takes hundreds of ms. Under extreme CI load they could flake by
  slowing the measured side, never by hiding a real regression (a deleted short-circuit
  pays 3000 ms — measured 3.10 s in the mutation check — and cannot pass).

## Acceptance criteria

1. `cargo test` green, `cargo check --all-targets` clean.
2. After an apply, the predicate reports the theme's layout in force.
3. A live file whose layout was wiped is detected and repaired once, and the repair is logged.
4. An apply over an intact live file performs no extra rewrite.
5. The keeper applies a theme whose bar differs from the current one and SEES the bar move.

## Checks

- `cargo test`
- `cargo check --all-targets`
- Release build + backup + install + relaunch (the keeper's live look).

## Work units

- [x] **U1** — the predicate and its documented scope. **REFUTED by the independent
  verification (2026-10-11): the predicate compared KEY PRESENCE, which Noctalia's own
  re-persist migrates — see HIGH-1 below and the correction in U5.**
- [x] **U2** — verify + bounded repair wired into the apply. **REFUTED by the independent
  verification (2026-10-11): the repair copied the theme's settings.toml VERBATIM — undoing
  the D4 plugin protection — and sat mid-apply; see HIGH-2 and MEDIUM below, corrections in
  U6 and U7.**
- [x] **U3** — the tests above. **REFUTED with U1/U2: the tolerance test pinned a synthetic
  layout Noctalia never migrates, so the suite certified a tolerance set reality falsifies;
  corrected in U8.**
- [ ] **U4** — the parent's build, install, relaunch and the keeper's live look.
- [x] **U5** — correction of U1: the predicate at SECTION level (see Progress log for REDs).
- [x] **U6** — correction of U2's repair: re-add ONLY the missing layout sections; never
  restore `[plugins]`, never revert the apply's palette/wallpaper writes (two invariants,
  both proven by tests).
- [x] **U7** — correction of U2's placement: verify at the END of the apply, after the last
  write/reload it performs; bounded (verify once, repair at most once); cost stated.
- [x] **U8** — the tests the refutation demanded (a)–(e), on top of the kept stub harness.
- [x] **U9** — correction of U7's regression: the verify + bounded repair moved OFF the
  apply's critical path onto a detached worker (the apply runs on the UI thread; the
  settle froze the transition — the keeper felt it live). Behaviour intact: same
  predicate, same surgical repair, same bounded-once rule; only WHERE the wait and the
  work happen changed.
- [x] **U10** — the repair restores EVERY entry of a repeated `[[array-of-tables]]`
  block (it restored only the first, leaving `group:g2` dangling while the check
  reported `[]`), and no longer claims success on a half-restored layout.
- [x] **U11** — the small ones: the entry-loss detection limit documented next to the
  existing stated limits (presence semantics kept, no predicate redesign), and the
  format-note baseline corrected in the U8 entry below (measured: the baseline is 64,
  not 67; 67 is the REFUTED delivery's count).
- [x] **U12** — the apply-generation token: every `apply()` increments a module-level
  atomic counter at its START; the detached layout-verify worker captures the generation
  of the apply that spawned it and, after its settle and again immediately before its
  reload, yields WITHOUT writing and WITHOUT reloading when a newer apply has started.
  **Second refutation (independent verification, 2026-10-11): the worker U9 detached
  carried no identity, so with the keeper's real themes — whose layout section SETS
  DIFFER — a superseded worker re-laid its theme's layout over a newer apply's file;
  see the refutation and Progress log below.**
- [x] **U13** — atomic writes on the live settings.toml from the repair worker: the
  write publishes through a temporary sibling (`.part-settings.toml`) + `rename`, the
  `copy_preset` idiom, so the concurrent writers of the same path can never observe a
  torn file.
- [x] **U14** — the layoutless short-circuit re-pinned on the WORKER: the round-2 pin
  measured the APPLY's elapsed time, which U9's off-thread move had left vacuous.
- [x] **U15** — the limits this round does NOT fix, registered in the contract (see
  "Stated limits" below), not silently left implicit.

## The refutation (verified against the keeper's real files, 2026-10-11 — recorded as fact)

- **HIGH-1 — the predicate compared KEY PRESENCE, and Noctalia's own re-persist MIGRATES the
  schema.** Measured on the keeper's real, healthy pair — `~/.local/state/noctalia/settings.toml`
  (the Noctalia re-persist of the JoKer theme) against
  `~/.config/hve/themes/JoKer/providers/noctalia-v5/settings.toml`: `[bar.barra_lateral]`
  renamed `auto_hide` → `smart_auto_hide`; `[bar.default]` lost `position`,
  `radius_bottom_right`, `radius_top_right` for `concave_edge_corners`, `radius`;
  `[widget.clock]` lost `color` for `capsule`, `capsule_fill`. So on an INTACT apply the
  predicate reported five missing items, the repair fired on a healthy state, acceptance
  criterion 4 failed on the keeper's own data, every apply paid ~3 s and two extra reloads,
  and a false "Noctalia dropped the theme layout" warning was logged. The tolerance test only
  pinned a synthetic layout Noctalia never migrates — the suite certified a tolerance set
  reality falsifies. The keeper's binary was ROLLED BACK to
  `restore-point/pre-noctalia-layout-guard-2026-10-11` because of this delivery.
- **HIGH-2 — the repair undid the D4 plugin protection.** It copied the theme's
  `settings.toml` VERBATIM (including its `[plugins] enabled` list) and ran `config-reload`
  AFTER the D4 `plugin_reassert_action` block — exactly the operation D4 exists to forbid. A
  firing repair re-enabled or disabled `noctalia/mpvpaper` behind the keeper's back.
- **MEDIUM — the verify sat mid-apply.** It ran at the end of step 0, but the apply still
  performed later writes and reloads: `neutralize_noctalia_wallpaper_rotation` (writes the
  live file and runs its own `config-reload`), `wallpaper-set`, `color-scheme-set`,
  `templates-apply`, and an async palette reassert; the keeper's `sddm-watch.sh` also fires
  `templates-apply` after any rewrite it heals. Acceptance criterion 2 was only sampled early.
- **LOW — `[[array-of-tables]]` was invisible** to the line parser (a `[[...]]` header parsed
  as a name that never matched layout and terminated the enclosing section), so the
  layoutless short-circuit and the post-repair re-check were not really pinned.

## The second refutation (verified against the keeper's real theme files, 2026-10-11 — recorded as fact)

- **HIGH — the detached worker carries no identity, and it re-kills the bug the contract
  exists to kill.** `7a6f1f5` moved the layout verify onto a DETACHED thread so the apply
  would stop freezing the UI thread (it did — the stutter is gone and that part stands).
  But `spawn_layout_verify_and_repair` (:927-952) captured two `PathBuf`s and nothing
  else, and `verify_and_repair_layout` (:812-845) read, decided, wrote and ran
  `config-reload` with no check that ITS apply was still the current one. Measured on the
  keeper's real themes, the two themes' layout SECTION SETS DIFFER: JoKer declares
  `[desktop_widgets]`, `[desktop_widgets.grid]`, `[[bar.barra_lateral.capsule_group]]` ×2,
  `[widget.battery]`, `[widget.sysmon]`, `[widget.mpvpaper]`, `[widget.network_rx]`,
  `[widget.network_tx]`, `[shell.screenshot]`, `[shell.session]` — none of which Space
  declares; Space declares `[widget.tray]`, `[widget.workspaces]`, `[widget.bluetooth]`,
  `[widget.volume]`, `[widget.session]` — none of which JoKer declares. So
  `missing_theme_layout(A, live_of_B)` is NON-EMPTY in BOTH directions on HEALTHY files.
  Timeline: apply A ends, worker A armed; the keeper clicks B 0.4 s later; at t≈1.5 s
  worker A reads B's live file, decides A's sections are missing, APPENDS A's blocks and
  reloads — the desktop renders A's bar under B. The synchronous version could not do
  this because apply and verify were serialized on one thread; moving it off-thread
  created a new correctness hole.
- **MEDIUM — two HVE threads doing unsynchronized truncate-and-write on the same file**
  (`:832`'s `fs::write` in the worker and the apply's `fs::copy` at `:2141`), which can
  tear the file under a concurrent reader.

Corrected as U12 (identity + yield) and U13 (atomic publish) below by a delegated writer.

## Progress log

- 2026-10-11: reported by the keeper, diagnosed to the mechanism above with evidence, and queued as contract
  the same day. NOT started.
- 2026-10-11 (delegated writer): **U1 done** — `missing_theme_layout(theme, live)` in
  `src/providers/noctalia.rs`, a pure line-based decision (no toml crate, module convention). Layout scope
  chosen and documented: `[bar]`, `[bar.*]`, `[desktop_widgets]`, `[desktop_widgets.*]`, `[dock]`, `[dock.*]`,
  `[shell]`, `[shell.*]` and `[widget.*]`; NOT layout (documented in `is_layout_section`): `[plugins]` (machine
  state, owned by the D4 re-assert), `[hooks]`, `[wallpaper*]`, `[osd*]`, `config_version`. Key PRESENCE is
  compared, never values, so Noctalia's legitimate rewrites pass. RED observed (TDD, `cargo test layout_predicate`):
  with the decision stubbed to "in force", `layout_predicate_reports_wiped_sections_as_whole` and
  `layout_predicate_reports_a_key_missing_from_a_surviving_section` failed with `left: []` — the stub certified a
  wiped layout. One test correction during the unit: an emptied `widgets = []` still carries the key, so the
  key-loss test removes the key line (presence semantics, per the documented choice). Commit `d8de95e`.
- 2026-10-11 (delegated writer): **U2 + U3 done** — `verify_and_repair_layout` wired after step 0's reload and
  the D4 plugin re-assert, same best-effort shape (warn-only, never fails the apply). One design addition beyond
  the literal contract wording, flagged for the parent: a single bounded SETTLE (`LAYOUT_SETTLE`, 1500 ms,
  `HVE_LAYOUT_SETTLE_MS` test override) before the verify, because the journal places Noctalia's re-persist
  ~1 s after the reload — a verify at t+0 would only read HVE's own copy and certify a layout about to vanish.
  A theme declaring no layout short-circuits before the settle (no cost for layoutless themes, keeps the existing
  rotation-neutralize test fast). Repair is bounded by construction: copy + reload ONCE, then one re-check that
  only LOGS whether it stuck. RED observed with the helper stubbed to a no-op: `apply_repairs_a_wiped_layout_exactly_once`
  failed with `reloads: left 1, right 2`, and `apply_repair_is_bounded_when_the_rewrite_does_not_stick` failed the
  same way — the repair never fired. After implementation: `cargo test` 1601 passed / 0 failed;
  `cargo check --all-targets` clean, no warnings. Commit `198a2bc`. Tests use a new `LayoutStub` (stub `noctalia`
  on PATH whose `config-reload` wipes the live file: never / first / always) — the keeper's live config, theme
  packages and `sddm-watch.sh` were never touched.
- 2026-10-11 (independent verification): **U1–U3 REFUTED** — HIGH-1 / HIGH-2 / MEDIUM / LOW as recorded in
  "The refutation" section above, measured against the keeper's real files. The keeper rolled his binary back to
  `restore-point/pre-noctalia-layout-guard-2026-10-11`; a delegated writer was asked to correct the delivery as
  U5–U8 below. Commits `d8de95e` / `198a2bc` stay in history as the refuted baseline.
- 2026-10-11 (delegated writer, correction): **U5 done** — the predicate is now SECTION-presence only
  (`missing_theme_layout`, rule and stated limit documented in one comment): layout is in force iff every layout
  section the theme declares is present in the live text; keys inside a surviving section are invisible because
  Noctalia migrates its schema under sections that stay present. New `section_header_name` parses `[section]` AND
  `[[array-of-tables]]` headers (indentation, comments, quotes), fixing the LOW finding. The repudiated
  `layout_predicate_reports_a_key_missing_from_a_surviving_section` test went with the old half. RED observed
  (`cargo test layout_predicate`, pre-implementation): `layout_predicate_reports_the_real_migrated_pair_in_force`
  FAILED and `layout_predicate_sees_array_of_tables_sections` FAILED with left = `[bar.barra_lateral].auto_hide,
  [bar.default].position, [bar.default].radius_bottom_right, [bar.default].radius_top_right, [widget.clock].color` —
  exactly the refutation's five false positives — and no sight of the `[[...]]` capsule-group sections. The fixture
  pair `THEME_MIGRATED` / `LIVE_MIGRATED` is condensed from the keeper's real files (read-only); his files were
  never touched. Commit `243a753`.
- 2026-10-11 (delegated writer, correction): **U6 done** — `restore_missing_layout_sections(theme, live)` re-adds
  ONLY the missing layout sections (verbatim blocks, theme order, first occurrence, appended at the end); every
  other live byte is left alone. `verify_and_repair_layout` now `fs::write`s that text instead of `fs::copy`ing the
  theme. The two invariants are pinned pure (on texts) and through the real apply path. RED observed:
  `repair_readds_only_missing_sections_and_preserves_every_other_byte` FAILED with `layout was missing` against a
  stubbed repair; `apply_repairs_a_wiped_layout_exactly_once` FAILED with "the repair must re-add the layout
  without touching a byte of the file it found";
  `apply_repair_keeps_plugins_off_and_keeps_the_apply_writes` FAILED with "every byte of the mid-apply live state
  must survive the repair" — all against the refuted verbatim copy. The bounded test was strengthened: after the
  failing rewrite the last wipe stands (no second repair). Commit `deb95d4`.
- 2026-10-11 (delegated writer, correction): **U7 done** — the verify is armed at step 0 (`layout_guard`) and runs
  as the LAST synchronous step of the apply, after the rotation neutralize, `wallpaper-set`, `color-scheme-set`
  and `templates-apply`; the D4 re-assert and every other apply step are untouched. Cost stated on the function and
  the call site: one 1.5 s settle per layout-declaring apply, one extra reload + one extra settle only on a real
  repair, zero for layoutless themes; the off-thread colour re-assert can still land after the verify
  (asynchronous by design). RED observed: `apply_verifies_after_the_later_steps` FAILED with "a wipe landing on a
  LATER step must still be repaired before the apply returns" — the stub wiped on `templates-apply`, the last
  synchronous step, and the mid-apply verify never saw it. Commit `b10bfae`.
- 2026-10-11 (delegated writer, correction): **U8 done** — the refutation's test list, all on the kept `LayoutStub`
  harness: (a) `layout_predicate_reports_the_real_migrated_pair_in_force` (the measured pair, U5); (b)
  `layout_predicate_reports_wiped_sections_as_whole` + `apply_repairs_a_wiped_layout_exactly_once` (whole-section
  wipe detected and repaired ONCE); (c) the pure byte-preservation test +
  `apply_repair_keeps_plugins_off_and_keeps_the_apply_writes` (repair leaves `[plugins]` and the applied
  palette/wallpaper writes untouched); (d) `apply_with_a_layoutless_theme_never_repairs` kept + the genuine pin
  `apply_with_a_layoutless_theme_skips_the_settle` (3000 ms settle configured, layoutless theme must return
  without paying it — deleting the short-circuit fails the test on elapsed time);
  (e) `apply_repair_is_bounded_when_the_rewrite_does_not_stick` (one repair attempt even when every reload
  re-wipes). Final checks on the correction tree: `cargo test` 1606 passed / 0 failed; `cargo check --all-targets`
  clean. ~~`rustfmt --check` chunk count on `noctalia.rs` back to the pre-task baseline (67 — repo-wide fmt
  uncleanliness is pre-existing and out of scope).~~ **CORRECTED by U11 (2026-10-11, measured with the same
  rustfmt): the pre-task baseline `8e52f99` is 64 chunks, not 67; 67 is the count of the REFUTED delivery
  (`198a2bc`, +3 added with the U2/U3 guard code — `d8de95e` still measured 64). What U5–U8 added: nothing to
  the count (64 -> 67 happened in the refuted commit; every correction commit held 67). What they did NOT do:
  bring it back to the true baseline of 64 — the refuted delivery's 3 chunks were never removed. Repo-wide fmt
  uncleanliness remains pre-existing and out of scope; this round (U9–U11) re-measured and added zero new
  chunks (67 before, 67 after).** The keeper's live Noctalia config, theme packages and
  `sddm-watch.sh` were never touched; the migrated-pair fixtures were condensed from READ-ONLY measurements.
  Commits `243a753`, `deb95d4`, `b10bfae`.
- 2026-10-11 (delegated writer, second correction round): **U9 done** — the keeper felt U7's regression live
  ("ahora me parece que algo rasca y antes era todo fluido"): the apply runs SYNCHRONOUSLY ON THE UI THREAD
  (Slint callback -> gallery slot `apply_theme` -> theme manager `apply` -> provider `apply`, no spawn on that
  path), so `verify_and_repair_layout`'s two `LAYOUT_SETTLE` sleeps (1.5 s each, up to ~3 s) froze the
  transition mid-flight — the transition's own cadence was unchanged (0.50 + 0.95 + 0.40 = 1.85 s) while the
  wall-clock from Noctalia's config reload to the visible wallpaper grew by ~1 s. The verify + bounded repair
  now runs on a DETACHED worker (`spawn_layout_verify_and_repair`, spawned as the apply's last step) — the
  established idiom of the off-thread colour re-assert, same spawn discipline (`Builder::spawn`, guard built in
  the parent, worker-active counter the `LayoutStub` harness now waits on exactly like `ColorStub` waits on the
  re-assert counter). The predicate, the surgical repair and the bounded-once rule are UNCHANGED; only WHERE
  the wait and the work happen moved. RED observed (`cargo test apply_returns_without_waiting_...`, pre-
  implementation): FAILED with "the apply must return without waiting for the layout settle ... but it took
  2.127087703s" (settle override 1000 ms, threshold 500 ms) — the sync apply paid both settles. After the
  spawn: apply returns in ~50 ms and the detached worker still detects and repairs the wipe (reloads 2, layout
  in force — `stub.wait_for_verify()` pins the worker's final state; every pre-existing layout test gained the
  same wait). Commit `7a6f1f5`.
- 2026-10-11 (delegated writer, second correction round): **U10 done** — verified against the keeper's real
  files: a whole-layout wipe left `bar.barra_lateral.capsule_group` restored as 1 of 2 entries while
  `center = [ "group:g1", "ram", "group:g2" ]` still referenced the missing `group:g2`, and
  `missing_theme_layout` then reported `[]` — the apply logged success on a HALF-restored layout (JoKer, Night
  and Work carry two groups each; Space one, so it never showed there). `restore_missing_layout_sections`
  captured only the FIRST block per missing name (`!emitted.contains`); it now takes EVERY block whose header
  names a missing section, in theme order — so everything the repair recognised as missing is fully back before
  it writes and the check's success claim is honest. A plain `[section]` occurs once in valid TOML, so nothing
  changes for it. RED observed (`cargo test repair_restores_every_entry_...`, pre-implementation): FAILED with
  "both array-of-tables entries must land, not just the first — left: 1, right: 2". After the fix: both entries
  land, the check reports in force, and a second repair finds nothing to rewrite (bounded, idempotent).
  Commit `9835101`.
- 2026-10-11 (delegated writer, second correction round): **U11 done** — (a) the entry-loss limit documented
  next to the existing stated limit in `missing_theme_layout`'s doc: losing ONE of several entries of a
  repeated `[[array-of-tables]]` block is invisible to the presence predicate (it counts the table NAME, not
  its entries); stated by design, no predicate redesign — the repair restores entries only when the whole
  table is gone (U10). (b) The format-note claim corrected in the U8 entry above with measurements: baseline
  `8e52f99` = 64 chunks, `d8de95e` = 64, refuted `198a2bc` = 67 (+3), corrections held 67, this round 67 -> 67
  (zero new chunks). Final checks on this round's tree: `cargo test` 1608 passed / 0 failed;
  `cargo check --all-targets` clean. The keeper's live Noctalia config, theme packages and `sddm-watch.sh`
  were never touched. Commits `7a6f1f5`, `9835101`, plus this docs commit.
- 2026-10-11 (independent verification): **U9–U11's delivery REFUTED — the second
  refutation of this contract** — HIGH/MEDIUM as recorded in "The second refutation"
  section above, measured against the keeper's real theme files (the asymmetry of the
  layout section sets between JoKer and Space). The refuted delivery is `7a6f1f5`
  (the detached worker — its UI-freeze fix stands) plus the guard as it then stood; a
  delegated writer was asked to correct it as U12–U15 below.
- 2026-10-11 (delegated writer, third round): **U12 done** — `APPLY_GENERATION`, a
  module-level `AtomicU64`, incremented at the START of every `apply()`; the worker
  captures that apply's generation through `layout_guard` and, after its settle and
  BEFORE it writes anything, yields (log + return, no write, no reload) when a newer
  apply has started — plus the same check immediately before the repair's
  `config-reload`. Why stated in code: a superseded worker must never lay its theme's
  layout over a newer apply's file. Fixtures `THEME_LAYOUT_A` / `THEME_LAYOUT_B` mirror
  the measured JoKer/Space asymmetry (only `[bar]` shared; `missing_theme_layout`
  non-empty in both directions — pinned as fixture sanity inside the test). RED observed
  (`cargo test -- superseded_layout_worker_yields current_apply_worker_still_repairs`,
  pre-implementation): BOTH FAILED for the right reason —
  `superseded_layout_worker_yields_to_the_newer_apply` with `reloads: left 3, right 2`
  (the superseded worker fired a repair reload on B's healthy file — the refutation's
  timeline reproduced) and `current_apply_worker_still_repairs_a_wipe_after_its_predecessor_yielded`
  with `left 4, right 3` (the predecessor did not yield). After the token: 2 and 3
  reloads, live file byte-for-byte B's. The healthy single-apply case (the CURRENT
  worker still repairs a wipe) was already pinned by
  `apply_repairs_a_wiped_layout_exactly_once` and `apply_returns_without_waiting_for_the_layout_verify`,
  both still green. Commit `27dac45`.
- 2026-10-12 (delegated writer, third round): **U13 done** — `write_settings_atomically`:
  the repair publishes the live settings.toml through a temporary sibling
  (`.part-settings.toml`) + `rename`, the `copy_preset` idiom, so no concurrent writer
  (the apply's `fs::copy`, the rotation neutralizer's `fs::write`, the keeper's
  watchdog) can observe a torn file from the worker's side. RED observed
  (`cargo test layout_repair_publishes_the_live_file_atomically`, pre-implementation):
  FAILED with `assertion left != right — left: 832777, right: 832777` — the in-place
  write kept the inode; a rename must replace it. After: inode replaced, layout in
  force, no `.part-` sibling left behind. One correction during the unit, kept honest:
  the helper's first draft used `let _ = fs::remove_file(...)`, which trips this file's
  own source-construction guard (`stale_artifact_deletes_log_failures_by_construction`
  — observed FAILING in the full suite); aligned with `copy_preset`'s fully-qualified
  `std::fs::remove_file`, guard green again. Commit `8721a92`.
- 2026-10-12 (delegated writer, third round): **U14 done** — the round-2 pin measured
  the APPLY's elapsed time, but U9 moved the settle into the worker, so deleting the
  layoutless short-circuit failed no assertion (the verification proved this by
  construction). The pin now measures from before the apply until `wait_for_verify()`
  returns — the WORKER's cost: with a 3000 ms settle configured, a layoutless theme must
  return (apply + worker) in under 1500 ms. Mutation-checked both ways: with the
  short-circuit deleted (`&& false`) the test FAILED at `3.102111735s` (the worker paid
  the settle); restored, green. Commit `9445547`.
- 2026-10-12 (delegated writer, third round): **U15 done** — the four limits this round
  does NOT fix registered in the contract's new "Stated limits" section (next to Out of
  scope): (a) the colour re-assert worker can still land after the layout worker
  verified; (b) what U13's atomic publish covers (the repair's own write) and what it
  does not (the apply's copy, the neutralizer's write, Noctalia's re-persist, the
  watchdog — all in-place), including the exit-between-temp-and-rename residue;
  (c) `ColorStub` waits only for re-assert workers, so a future test author who arms a
  layout worker through it must wait for that worker too; (d) the margins and conditions
  of the `elapsed < 500 ms` pin (U9) and the re-pinned worker pin (U14). Final checks on
  this round's tree: `cargo test` 1611 passed / 0 failed (1608 before + 3 new);
  `cargo check --all-targets` clean; `rustfmt --check` chunk count on `noctalia.rs`
  67 -> 67 (zero new chunks). The keeper's live Noctalia config, theme packages and
  `sddm-watch.sh` were never touched. Commits `27dac45`, `8721a92`, `9445547`, plus
  this docs commit.
- **(e) A FAILED apply cancels its predecessor's repair without inheriting the duty.**
  `APPLY_GENERATION` is bumped as `apply()`'s first statement, but the guard is armed only
  after the settings copy and the reload succeed, and the worker spawns at the END of the
  apply. So if apply B bumps the generation and then fails on ANY early-return path
  (missing provider dir, failed settings copy, failed step-0 `config-reload`, or any
  synchronous failure between the arm and the spawn), apply A's worker wakes, sees a
  superseded generation, yields — and NO worker verifies or repairs. A layout wipe that
  happened during apply A then goes unrepaired until the next SUCCESSFUL apply. The token
  protects the file, but the failed apply inherits the supersession authority without
  inheriting the duty. Narrow (needs a wipe during A AND a failing B) and not in the
  round's scope.
- **(f) The repair's temporary sibling name is a constant.** `.part-settings.toml` is
  shared by every worker, so in an extreme schedule in which two workers are inside the
  atomic write at once, one can delete or rename the other's in-flight temp and publish a
  partial file. A unique suffix (the generation or the pid) closes it. Practically
  near-unreachable; registered because this guard's whole subject is concurrency.
- **(g) The post-repair re-check does not re-test the generation.** After its repair reload
  the worker sleeps a second settle and re-reads the live file; if a newer apply started
  during that settle it compares the OLD theme's sections against the NEWER theme's file —
  and with the keeper's real themes those sets differ in both directions, so it can log
  "STILL missing after the single repair" against a healthy file. Log-only (no write, no
  reload after that point), but it is a diagnostic lie in a contract whose whole point is
  honest diagnosis.
