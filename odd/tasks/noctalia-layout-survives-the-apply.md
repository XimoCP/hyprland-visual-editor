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

- [x] **U1** — the predicate and its documented scope.
- [x] **U2** — verify + bounded repair wired into the apply.
- [x] **U3** — the tests above.
- [ ] **U4** — the parent's build, install, relaunch and the keeper's live look.

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
