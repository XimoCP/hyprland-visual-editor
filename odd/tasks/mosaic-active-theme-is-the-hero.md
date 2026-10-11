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
  fighting the keeper's own page flips.
- **U3 — R3**: pages without the active theme keep the first-of-page hero (asserted by test).
- **U4 — tests**: (a) the active theme is the hero of its page; (b) a page without it keeps the
  first-of-page hero; (c) no active theme (empty `last_applied`) → first-of-page everywhere;
  (d) opening lands on the active theme's page; (e) the capacity/page count is unchanged by R1.
- **U5 — parent**: headless render + reading the PNGs (the active theme must be the big centred
  tile), then install + relaunch for the keeper's live look.

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
