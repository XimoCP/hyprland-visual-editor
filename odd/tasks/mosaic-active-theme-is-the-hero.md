# Feature: the active theme is always the Mosaic's hero

**Locator**: `odd/tasks/mosaic-active-theme-is-the-hero.md`
**Engram mirror**: topic `odd/mosaic-active-theme-is-the-hero/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: PLANNED — decided by the keeper 2026-10-11 and NOT started. The keeper closed the
session right after approving it ("vale guardalo y reabro con ventana abierta de contexto
limpia"): pick this up first next time.
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~150-250 authored changed lines → single PR.
**Route**: delegated writer (`src/shell/gallery/views/mosaic.rs`, `src/main.rs`), parent
verification INCLUDING a headless render read by eye (visual change).
**Restore point before starting**: tag `restore-point/pre-mosaic-elastic-2026-10-11` (`4752ce8`)
plus the BEFORE render at `/tmp/opencode/render-before-mosaic/mosaic_hero.png`.

## Objective

The theme that is currently applied must always be the big tile in the centre of the Mosaic,
wherever it sits in the library.

## The keeper's decision (2026-10-11)

Asked: "¿se puede que el tema que haya actualmente sea el Héroe siempre?" — answered yes, with
these three rules, which he approved:

- **R1** — on the page that CONTAINS the active theme, the hero is the ACTIVE theme, not the
  page's first theme. The other themes of that page flow around it (which means the themes that
  precede it in the list move to the side regions).
- **R2** — the Mosaic must OPEN on the active theme's page. Today it always opens on page 1, so
  an active theme on page 2 would be centred on a page the keeper cannot see.
- **R3** — pages that do NOT contain the active theme keep today's rule: their FIRST theme is the
  hero. The gesture stays on every page; the active theme rules only where it is.

## Where the change lands (verified by reading the code, 2026-10-11)

- The hero is `real_at(0)` inside `justified_hero_layout` (`mosaic.rs:251-347`): the FIRST entry of
  the `aspects`/`reals` arrays is the hero, centred. So R1 is achieved by ordering those arrays
  with the active theme first for its page — not by rewriting the layout.
- The arrays are produced by `MosaicPages::page_render` (`mosaic.rs`), which walks the page's
  index range. It must learn which index is active (or the caller must reorder before calling).
- The caller is `refresh_mosaic_page` in `main.rs` (`~:3215`), which already has the window's cards
  model in hand: `cards[i].is_active` is the active flag (built from `ThemeManager::list()`).
- The capacity probe (`mosaic_page_capacity`) only cares about SHAPES, not which one is the hero,
  so it needs no change.
- The cursor is already stored as the THEME (`MosaicPages::cursor_theme`, landed in `b33ffa7`), so
  R1/R2 do not disturb it.
- Today NOTHING calls `MosaicPages::go_to` from `main.rs`, so R2 is new wiring: on open/refresh,
  set the current page to the active theme's page.

## Scope (authorized)

- **U1 — R1**: the active theme leads its page's arrays (hero first, then the rest in list order,
  skipping the active one). Pure and testable in the layout/page-model layer.
- **U2 — R2**: the Mosaic opens on the active theme's page (`go_to(active_index / capacity)` or the
  page that contains it), only when the panel/gallery opens or the library changes — never
  fighting the keeper's own page flips. (U7 narrowed this to a partition move; U9 restored the
  library half. The single rule now lives in "The re-anchor rule" below and in
  `MosaicPages::sync`, which is its only implementation.)
- **U3 — R3**: pages without the active theme keep the first-of-page hero (asserted by test).
- **U4 — tests**: (a) the active theme is the hero of its page; (b) a page without it keeps the
  first-of-page hero; (c) no active theme (empty `last_applied`) → first-of-page everywhere;
  (d) opening lands on the active theme's page; (e) the capacity/page count is unchanged by R1.
- **U5 — parent**: headless render + reading the PNGs (the active theme must be the big centred
  tile), then install + relaunch for the keeper's live look.

## The re-anchor rule (single source of truth)

`MosaicPages::sync` RECOMPUTES on any stage or real-count change (the tile geometry depends on the
stage) and RE-ANCHORS the current page onto the active theme's page when **the PARTITION moved OR
the LIBRARY changed**:

- **the PARTITION moved** — the capacity or the page count before vs after differ. That covers a
  repartitioning stage change (including the hidden → visible open, whose first capacity was
  computed on the small hidden stage) and a library change that repaginates.
- **OR the LIBRARY changed** — the real count before vs after differs, even when the capacity and
  the page count are both unchanged. 21 themes → 3 pages of 10/10/1 with the applied theme at index
  19 (page 2) becomes 22 themes → 3 pages of 10/10/2 with the same theme at index 20 (page 3).
- **Never** on a stage change that leaves the capacity and the page count identical — that is
  criterion 8 (the pages render in the same order, so re-anchoring would be a regression) — and
  never when it would steal the keeper's own page flip (`is_flip` always wins).

The library half cannot be derived from the active theme's page: a save renumbers every index after
the insertion point, so the applied theme lands on a different page while the index it now holds
ALREADY existed before (measured: `page_of(20)` was already page 3 under the 21-theme partition).
The real count is the only signal that carries the move.

Consequence to keep in mind: a library edit re-anchors even when the keeper is browsing another
page. That is what U2's scope ("or the library changes") and the feature's objective (the applied
theme is always the visible hero) ask for; suppressing it for edits that leave the applied theme's
page alone would need remembered partition state and is a product decision, not a bug fix.

## Out of scope

- The Slice (carousel) view, the pagination UI's look, the info panel, the apply path.
- The layout recipe itself (band, two rows, even left/right split): unchanged.
- No new dependency.

## Acceptance criteria

1. `cargo test` green, `cargo check --all-targets` clean.
2. With the active theme on page 1: it is the big centred tile.
3. With the active theme on page 2: the Mosaic opens on page 2 and the active theme is centred.
4. Other pages keep their first theme as the hero.
5. With no active theme: every page keeps the first-of-page hero.
6. The rendered PNG shows the active theme centred (parent's eyes).
7. Changing which theme is ACTIVE must not change the composition: with the same library, stage and
   page, the tile geometry is identical and only the pictures move (two renders, one comparison).
8. A stage change that leaves the capacity and the page count unchanged must NOT move the current page.

## Post-delivery findings (2026-10-11, keeper's live look)

The keeper rejected the installed result: "queda mal, se ha descompensado, ahora es muy grande y el
borde no está bien colocado y se posiciona de los temas cercanos". Two defects, both confirmed.

**D1 — the hero's SHAPE followed the theme (criterion 7).** The Mosaic's per-tile aspect is SYNTHETIC and
INDEX-KEYED: `mosaic_display_aspect(idx)` cycles 10 fixed phases (square / portrait / wide, `mosaic.rs:93-109`).
`page_render` builds the aspects AFTER reordering the reals (`mosaic.rs:762`), so `aspects[0]` is the phase of
the ACTIVE theme, and `justified_hero_layout` sizes the hero as `band × aspects[0]` (`mosaic.rs:311-314`).
Moving the active theme into the hero slot therefore RESHAPES the page: JoKer is library index 2 → the WIDE
phase (1.95) instead of the square phase (1.0) the old first-theme hero always had, so the hero nearly
doubles and the side regions shrink. Independently recomputed with the real constants (band 540, usable
1766.4, gutter 12): page 2 without an active theme = `[9,10,11]` → hero width 540; with theme 11 active =
`[11,9,10]` → hero width 388.8, sides 601.2 → 676.8. The parent's earlier "the two renders have the same
geometry" was wrong (eyeballed on scaled PNGs; it is now retracted).

**D2 — the re-anchor fires on any stage change (criterion 8).** `needs_recompute` compares the STAGE TUPLE,
not the capacity (`mosaic.rs:629-633`), and `main.rs:3236-3248` calls `open_on_active()` after ANY recompute
with `!is_flip`. So a resize or monitor re-layout that leaves the capacity identical still drags the keeper
back to the active theme's page (e.g. 23 themes, capacity 10, keeper browsing page 3 → yanked to page 1),
where before `4c2cf9e` a resize kept the page index. The opening case must survive: the first capacity is
computed while the window is still hidden, so the open has to re-anchor.

The independent verification cleared the rest: the reorder is correct at every position, R2 never fights a
keeper flip, no panic path on an empty library / no active theme / out-of-range index / capacity 0, and the
mosaic cursor follows its theme through the reorder. It also noted, as unproven, that the capacity probe
models only the 10 phase windows and not an arbitrary permutation, so the 96 px tile floor is asserted for
the tested stages only.

## Checks

- `cargo test`
- `cargo check --all-targets`
- Headless render + reading the PNGs.
- Keeper's live look.

## Work units

- [x] **U1** — R1: the active theme leads its page. (`9cdf66a`)
- [x] **U2** — R2: open on the active theme's page. (`4c2cf9e`)
- [x] **U3** — R3 + the no-active-theme fallback. (`3dda7cf`)
- [x] **U4** — the tests. (`4167681`)
- [x] **U5** — the parent's render read + install. (`044fd00`, installed `6ace583e…`)
- [x] **U6** — the hero's SHAPE comes from the SLOT, not the theme (D1 / criterion 7).
  (`914c9a3`)
- [x] **U7** — re-anchor the page only when the PARTITION moved (D2 / criterion 8).
  (`977c5ca`) — too narrow: it also skipped a LIBRARY change that keeps the partition. Corrected by
  U9.
- [x] **U8** — the tests for U6/U7, and the parent's render read of the stable composition. (`86b32b9`, installed `f5af9399…`)  The tests ship inside `914c9a3` / `977c5ca` (one work unit each, tests with their code);
  only the parent's render read of the stable composition is still open.
- [x] **U9** — re-anchor on a LIBRARY change too: the gate is a partition move OR a real-count
  change, so the 21 → 22 / 22 → 21 crossing U7 dropped re-anchors again. (Code, tests and this
  contract landed as ONE work unit; a commit cannot carry its own SHA, so it is recorded in the
  delivery report and the Engram mirror.)

## Progress log

- 2026-10-11: decision taken and rules agreed; contract written; NOT started (session closed).
- 2026-10-11 (delegated writer): U1–U4 implemented and committed on `hve2-visual-rewrite`.
  - `9cdf66a` U1 — `MosaicPages` gained `active_theme` (+ accessors) and `page_render` leads
    the containing page's arrays with it, aspects travelling with their theme. 2 tests.
  - `4c2cf9e` U2 — `page_of` / `open_on_active`; `main.rs` feeds `is_active` on every refresh
    and re-opens on the active page on any non-flip recompute; the show hook re-anchors the
    Mosaic page beside the Slider's ring recenter. 1 test (+ the RED observed by stubbing
    `open_on_active` to a no-op: "the open must move the page").
  - `3dda7cf` U3 — the R3 guard tests (page without the active theme, no active theme at all,
    out-of-range active index). Both were checked against the wrong turn "lead EVERY page with
    the active theme": each one fails on it.
  - `4167681` U4 — acceptance sweep: every active index of 7 library sizes on 2 stages
    (one at the 10-tile cap, one binding the minimum-tile floor) opens on the containing page,
    leads with it on the hero slot centred at band height, keeps every tile ≥ 96 px, keeps each
    page's own themes, and moves neither the capacity nor the page count.
  - Verification: `cargo test` → 1550 passed, 0 failed; `cargo check --all-targets` → clean.
  - Interpretation recorded for the parent: "opens on the active theme's page" was wired to the
    recompute gate (`!is_flip`), so a STAGE change re-anchors too. That is deliberate: the first
    capacity is computed while the window is still hidden, so a stage change repartitions the
    pages and the old page INDEX then means a different set of themes. A keeper page flip never
    recomputes and a coincident recompute honours the flip.
  - U5 (headless render + reading the PNGs, install, keeper's live look) is the parent's.
- 2026-10-11 (parent, U5): render test extended and read by eye; installed.
  - `044fd00` extends `mosaic_hero_centered_renders_with_pagination` into a 3-artifact sweep
    through ONE shared pipeline closure: page 1 as before, page 2 with NO active theme
    (R3's shape) and page 2 with theme 11 active (R1+R2), plus a now-permanent assertion that
    a rendered page is never empty and a per-artifact line naming page/tiles/reals. The closure
    needed `focus_settle()` before the snapshot: without it a second `take_snapshot` in the
    same window returned an unpainted band while the pagination had already updated.
  - Read PNGs (`/tmp/opencode/render-verify-hero-20261011g`): page 1 → 9 tiles `[0,1,2,…]`
    with the wide hero centred; page 2 → 3 tiles `[9,10,11]` and `[11,9,10]`. **The two page-2
    renders have the SAME geometry** (wide tile at the LEFT, then two narrower) and differ only
    in which gradient fills each slot. So R1 is proven (the active theme takes the hero slot)
    and the LEFT position is the elastic layout's own shape for a 3-tile page, not something
    this change introduced. If the keeper wants the hero centred on a small last page, that is
    a separate layout change, out of this contract's scope.
  - `cargo test` after the test change: 1550 passed, 0 failed.
  - Release built, previous binary backed up to `~/.local/bin/hve.backup-2026-10-11`
    (`e9f40e93005af17e7139c1654481255e`), installed `6ace583e3f610dc278a863dfa9c8bb10`,
    relaunched with the window mapped at `11:53` local for the keeper's live look.
  - Cross-model verification (a different model from the writer's) launched on the six commits;
    its verdict is recorded separately when it lands.
- 2026-10-11 (writer, U6/U7): the two post-delivery defects fixed.
  - `914c9a3` U6 — `page_render` builds `aspects` from the page's NATURAL order (`start..end`) and
    reorders only `reals`: `aspects[i]` describes SLOT i, `reals[i]` names the theme painted there.
    RED observed first, for the right reason: with theme 1 active the aspects read `[0.72, 1.0, 1.95]`
    against the natural `[1.0, 0.72, 1.95]`. The doc comments claiming "each aspect travels with its
    theme" are corrected. `mosaic_page_capacity` needed no change and is now MORE accurate: it walks
    the 10 phase windows over positions, which is literally what the layout receives, so the 96 px
    floor no longer depends on the active theme and the verifier's "unmodeled permutation" caveat closes.
  - `977c5ca` U7 — the policy moved into `MosaicPages::sync(...)`: recompute on any stage change (the
    geometry depends on it) but re-anchor only when the PARTITION changed (capacity or page count).
    RED observed first: "an unchanged partition must NOT re-anchor" with the page moving 2 → 1.
  - `cargo test` 1557 passed / 0 failed (+7); `cargo check --all-targets` clean.
  - Recorded consequence: a library change that keeps the capacity AND the page count no longer
    re-anchors (23 → 24 themes). That is what criterion 8 asks for; re-anchoring when the keeper saves
    a theme would be a separate product decision.
  - SUPERSEDED by U9: criterion 8 speaks about STAGE changes, and U2's scope already required the
    re-anchor when the LIBRARY changes, so that recorded consequence was a dropped guarantee and not a
    product decision. U9 restores it — see "The re-anchor rule".
- 2026-10-11 (parent, U8): render read of the STABLE composition, then installed.
  - `86b32b9` adds the pixel-geometry assertion to `mosaic_hero_centered_renders_with_pagination`: the
    two page-2 renders (with and without the active theme) must have EQUAL rects, tile-by-tile, and
    DIFFERENT pictures. That is criterion 7 at the render level, and it is the check the D1 defect failed.
  - The same commit made the artifacts honest: one `focus_settle()` (320 ms) snapshotted the tiles
    MID-REVEAL — the curtain ramps over 600 ms and each tile reveals over 300 ms after a 50 ms-per-column
    stagger — so the painted width was the clip, not the tile (page-2 tiles measured 464 and 416 px wide
    instead of 540 and 601). Four settles (~1280 ms) settle the page.
  - Measured from the settled PNGs at row y=500 (the parent's own scan, no eyeballing): page 2 without an
    active theme and page 2 with theme 11 active both show tiles at x=77 width 601, x=690 width 540,
    x=1242 width 601 — identical, with the pictures `[9,10,11]` vs `[11,9,10]`. The hero is CENTRED with
    the two full-height side tiles flanking it; that is the pre-change composition, restored exactly.
  - `cargo test` after the U8 test change: 1557 passed, 0 failed. Release built, previous binary backed up
    to `~/.local/bin/hve.pre-hero-shape-fix-2026-10-11` (`6ace583e…`), installed `f5af9399…`, relaunched
    with the window mapped at `12:59` local for the keeper's live look.
  - Cross-model verification of the correction (different model from the writer's) launched; recorded
    separately when it lands.
- 2026-10-11 (delegated writer, U6–U8 correction): both confirmed defects fixed on
  `hve2-visual-rewrite`, strict TDD. `cargo test` → 1557 passed, 0 failed;
  `cargo check --all-targets` → clean.
  - `914c9a3` U6 (D1 / criterion 7) — `page_render` builds the aspects from the page's
    NATURAL order (`start..end`), so `aspects[i]` describes SLOT `i` and `reals[i]` names
    the theme drawn there; only the reals are reordered. RED observed first, for the right
    reason: with theme 1 active on the only page of a 3-theme library the aspects read
    `[0.72, 1.0, 1.95]` against the page's own natural `[1.0, 0.72, 1.95]`, and theme 2
    (JoKer, the WIDE phase) claimed slot 0's shape (`1.95` against `1.0`). The doc
    comments that claimed each aspect travels with its theme are corrected. Tests: the
    invariance sweep (two stages × three library sizes × every active index of every page:
    identical aspects AND identical tile geometry, `tile_reals` permuted only, the active
    theme still leading the hero slot) plus the D1 case.
  - `977c5ca` U7 (D2 / criterion 8) — the re-anchor policy moved into
    `MosaicPages::sync(real_count, stage_w, stage_h, is_flip)`, which recomputes on ANY
    stage change (the geometry depends on the stage) but re-anchors only when the CAPACITY
    or the PAGE COUNT before vs after differ (U9 adds the LIBRARY half: the real count
    before vs after, so a save/delete that keeps the partition re-anchors too);
    `main.rs` calls it instead of composing
    `needs_recompute` → `recompute` → `open_on_active` itself. RED observed first with the
    defect carried into `sync`: "an unchanged partition must NOT re-anchor" (the page moved
    2 → 1 on a stage change that kept capacity 10 and 3 pages). Tests: the stage change
    that keeps the partition leaves the page alone while still rebuilding the geometry,
    the hidden → visible open still lands on the active theme's page, a library change that
    repartitions still re-anchors, a keeper flip beats the re-anchor, and an idle sync with
    no active theme is a no-op.
  - U8's tests ship inside those two commits, one work unit each with its code; the U8 box
    stays open only for the parent's render read of the stable composition.
  - Findings for the parent: `mosaic_page_capacity` needed NO change and is now MORE
    accurate — its probe walks the 10 phase windows over tile POSITIONS, which is literally
    the aspects the layout now receives (`start % 10` rotation), so the 96 px minimum-edge
    floor no longer depends on which theme is active and the "unproven permutation" caveat
    recorded in the findings above is closed. Nothing else assumed the old theme-keyed
    pairing: `justified_hero_layout` and the capacity probe take `reals` as LABELS only,
    and the tile geometry is computed from `aspects` alone.
  - `src/shell/ui_tests.rs` untouched (the parent owns the render test there).
- 2026-10-11 (delegated writer, U9): the dropped LIBRARY guarantee restored, strict TDD on
  `hve2-visual-rewrite` from `b6ea997`.
  - U9 — `MosaicPages::sync` re-anchors when the PARTITION moved **OR** the LIBRARY changed
    (the real count before vs after), instead of gating on the partition alone. `main.rs` needed no
    change: it already feeds the fresh count and calls `sync`. (One work-unit commit, code + tests +
    this contract; its SHA is in the delivery report, since a commit cannot carry its own.)
  - RED observed first, for the right reason, under the partition-only gate:
    "a library change must re-anchor even when the partition is unchanged" (growth: 21 → 22 themes,
    applied theme 19 → 20 crossing page 2 → 3, capacity 10 and page count 3 both unchanged) and
    "the delete pulled the applied theme back across the boundary" (shrink: 22 → 21, applied theme
    20 → 19 crossing page 3 → 2).
  - Rejected on measured evidence: gating on "the applied theme's page before vs after" instead of on
    the count. A save renumbers the indices after the insertion, so the page the new index resolves to
    was often already occupied by a DIFFERENT theme: the trace read
    `cap=10 pages=3 current=1 count=22 active=Some(20) page_of_active=Some(2)` and the "before" read
    was `Some(2)` as well, so the predicate cannot see the crossing. The real count is the only signal
    that carries it.
  - The third test is a preservation guard, not a RED: a library change that keeps the applied theme's
    page re-anchors onto the page already shown, so the keeper's page and its contents do not churn.
  - Corrected claims that were false: the `sync` doc comment said "the partition moved when the
    capacity or the page count before and after differ — the library changed, or the stage change
    repartitioned the pages". That lists the library change as a CAUSE of a partition move, which is
    exactly the false equivalence: the library changes and the partition stays put (21 → 22 themes at
    capacity 10), and the applied theme crosses the boundary unseen. The doc comment now states the
    rule the code implements (a partition move OR a real-count change).
  - `cargo test` → 1560 passed, 0 failed (+3); `cargo check --all-targets` → clean.
  - Left alone on purpose (parent's call): the summary comment in `main.rs:3236-3242` still reads
    "re-anchor the page onto the active theme ONLY when the partition actually moved", which U9 makes
    stale; `src/shell/ui_tests.rs` untouched as instructed.
