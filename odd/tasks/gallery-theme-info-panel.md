# Gallery: automatic theme info panel in the main Slider

## Objective

Right-clicking a card of the main Slider (style 0 = `SliceCarousel`) — or
pressing `i` on the keyboard — slides in a skewed panel shaped like the slider
cards that shows what the theme actually carries: palette colors, active
animation, active border, sizes, providers. The panel covers one slice of the
window and leaves almost all of the wallpaper visible.

All values come from a module that reads each saved theme automatically; the
user never types them.

## Problem

- `SliceDelegate` has a back-face stub (`is-flipped`) with placeholder text
  ("VIEW RETAG DELETE STEAM") that is never driven: `SliceCarousel` does not bind
  `is-flipped` and the `flipped()` callback is unconnected.
- Right-click on a gallery card only jumps the focus to that card
  (`src/main.rs:on_gallery_card_right_clicked`), the PR2 stub for the flip.
- The gallery card model carries placeholder values: `ThemeCard::from_info`
  fills colors/border/shader with `Default` and `to_gallery_cards` just reads
  them, so every card shows the same cyan and the same border.
- The real data already exists on disk per theme:
  - `providers/hve-presets/state.json` — active animation / border / shader file,
    `border_size`, `border_radius`, `gaps_in`, `gaps_out` (written on every save by
    `HvePresetsProvider::save`).
  - `providers/noctalia-v5/palette.json` — `dark` / `light` maps with `mPrimary`,
    `mSecondary`, `mTertiary`, `mSurface`, ...
  - `meta.json` — `providers`, `saved_at`.
  - Gap: themes whose palette comes from the wallpaper or from a named community
    palette (e.g. `Cars`: `source.txt = "wallpaper vibrant"`, `settings.toml
    [theme] community_palette = "Garnet"`) have no `palette.json`; those must
    report the palette NAME instead of inventing colors.

## Architecture rule (repo doctrine)

`theme_manager.rs`: "The gallery must never learn a backend's layout or record
format." The core resolves a provider directory from `meta.json` and asks the
registered provider. So the reader is a new **trait method**, never a gallery
side file read.

## Interface contract (frozen — both writers follow it)

`ui/gallery/ThemeInfoPanel.slint`:

```slint
export struct ThemeInfoRow { label: string, value: string }
export struct ThemeInfoData {
    name: string,
    saved_at: string,
    providers: [string],
    swatches: [color],
    rows: [ThemeInfoRow],
}

export component ThemeInfoPanel inherits Rectangle {
    in property <bool> open: false;
    in property <ThemeInfoData> info;
    in property <bool> reduced-motion: false;
    in property <length> stage-width: 1920px;
    in property <length> stage-height: 1080px;
    callback closed();
}
```

`ui/main.slint` (MainWindow) additions:

```slint
in property <ThemeInfoData> gallery-info;
in-out property <bool> gallery-info-open: false;
callback gallery-info-requested(int);   // keyboard `i`
```

Rust trait addition (`theme_manager.rs`):

```rust
/// Displayable facts about this provider's own record for a saved theme.
fn theme_facts(&self, provider_dir: &Path) -> Vec<ThemeFact> {
    let _ = provider_dir;
    Vec::new()
}
```

with `ThemeFact { label: String, value: String, swatches: Vec<String> }`
(hex strings; the aggregator parses them with `crate::theme::parse_hex`).

`ThemeManager` gains one public reader:

```rust
/// Automatic, display-only facts for a saved theme directory.
pub fn theme_info(&self, name: &str) -> ThemeFacts;
```

returning `ThemeFacts { name, saved_at, providers: Vec<String>,
swatches: Vec<String>, rows: Vec<(String, String)> }` (pure Rust, no Slint).

## Tasks

- **T1 — Slint panel** (`ui/gallery/ThemeInfoPanel.slint`): skewed parallelogram
  panel, right edge, width ≈ 32% of the stage (clamp 320–520px), full stage
  height, shear angle equal to the slider cards' (skew 35 per 520 height, same
  slant direction), radius from `SkwdTokens`, credits comment "MIT credit:
  visual language translated from skwd-wall". Content: theme name, saved date,
  provider chips, a row of color swatches, then one line per `ThemeInfoRow`.
  Slides in from off-stage right on `open` (`animate x`, 250ms OutCubic, 0ms when
  `reduced-motion`). Emits `closed()` when the user clicks outside the panel.
- **T2 — GalleryRoot integration** (`ui/gallery/GalleryRoot.slint`): host the
  panel above the stage (declared after the stage so it paints on top), pass
  `cards[gallery-info-index]`-independent data straight from the new
  `info`/`info-open` properties, forward `open` from `info-open`, and forward
  `closed` to a new `gallery-info-close` callback chain.
- **T3 — Window/Shell plumbing** (`ui/main.slint`, `ui/shell.slint`): declare the
  properties/callbacks above, bind them into `GalleryRoot`, and route the `i`
  key in `shell-kbd` (`mounted-screen == 1`, panel closed) to
  `gallery-info-requested(root.gallery-focused)`; Escape closes the info panel
  before any other Escape behaviour.
- **T4 — Render test** (`src/shell/ui_tests.rs`): a headless render test that
  feeds a `ThemeInfoData` through `gallery-info` + `gallery-info-open` and saves
  the settled frame as a PNG in the per-run directory.
- **T5 — Rust reader** (TDD, failing test first): `theme_facts` on
  `HvePresetsProvider` (state.json) and `NoctaliaProvider` (palette.json, else
  the palette name from settings.toml), `ThemeFact`, `ThemeFacts`, and
  `ThemeManager::theme_info`.
- **T6 — Rust wiring** (`src/main.rs`): `on_gallery_card_right_clicked` opens the
  info panel for the clicked card (replacing the PR2 focus-jump stub) and fills
  `gallery-info` via `theme_info`; `on_gallery_info_requested` does the same for
  the focused card; `on_gallery_info_close` closes it; refresh the data on theme
  save/delete while the panel is open.

## Acceptance criteria

- Opening the panel never blocks the carousel and never covers the whole window;
  the wallpaper stays visible around it.
- The panel carries real per-theme data: palette swatches + palette name, active
  animation file, active border file, sizes — read from disk, nothing hand-typed.
- A theme without `palette.json` shows its palette NAME, never invented colors.
- Closing works from Escape, from the outside click and from right-click again.
- `reduced-motion` turns the slide into an instant placement.

## Checks

- Strict TDD ON (source: project `AGENTS.md`, runner `cargo test`): failing test
  first for the Rust reader.
- `cargo test` green, no `cargo fmt`.
- Visual: `HVE_RENDER_DIR=<dir> cargo test <render test> -- --nocapture`, then read
  the PNGs from that printed per-run directory.
- Cross-model verification (model from `opencode.json`, different family from the
  writer).

## Route and delegation

- T1–T4: one bounded writer (Slint layer).
- T5–T6: one bounded writer (Rust layer), after the Slint layer lands.
- Verification: separate model, after both.

## Delivery

- `single-pr` for this feature; the branch already runs with `size:exception`
  accepted by the keeper for oversized commits.

## Progress

- [x] T1 Slint face — `ui/gallery/ThemeInfoPanel.slint` is now the theme's own
      card face (not a stage overlay): the data-driven content revealed from
      inside the card, with a right-click-inside close. SliceDelegate places it
      in the card's own slant-safe box, so the geometry IS the card's geometry.
- [x] T2 GalleryRoot integration — the face is hosted by the card; a focus
      change closes it (the face belongs to the theme it was opened for).
- [x] T3 main.slint / shell.slint plumbing — `gallery-info`, `gallery-info-open`,
      `gallery-info-index`, `gallery-info-requested`, `gallery-info-close`; `i`
      opens it for the focused card; Escape closes it first; leaving the gallery
      closes it.
- [x] T4 render test — `theme_info_panel_renders_closed_and_open`, which now
      asserts the face paints INSIDE the card's box and leaves the wallpaper
      around the carousel untouched.
- [x] T5 Rust reader (TDD) — `theme_facts` on hve-presets + noctalia-v5,
      `ThemeManager::theme_info`.
- [x] T6 Rust wiring — right-click brings the card to the centre AND opens its
      own face inside it; `i` opens it; Escape / right-click-inside close it;
      Save / rename / delete / overwrite close it instead of leaving stale data.
- [x] Cross-model verification — DONE (twice, `jd-judge-a` on
      `opencode-go/glm-5.3-flash`): once over the Slider's face and once over
      the mosaic ladder. Caveat that applies to both: the reviewer's runtime
      exposes no shell tool, so its reviews are static and every suite result
      below is the author's.

## Commits

- `0e18a76` — `feat(gallery): read each theme's own records for the info panel`.
- `2256ffc` — `feat(gallery): right-click or i opens the theme info panel`.
- `6b5a4c5` — `fix(gallery): leaving the gallery closes the theme info panel`.
- `15c860a` — `refactor(gallery): the theme info face lives inside the card, not over the window`.
- `56bdcaf` — `feat(gallery): the info face slides out of the card, in the first version's format`.
- `5a922cd` — `fix(gallery): keep the info face open on the card it was opened for`.
- `b983092` — `feat(gallery): the info face is a third of the card, slanted and translucent`.
- `552292c` — `feat(gallery): the info face comes out of the card's right edge, a touch more opaque`.
- `53c7a4c` — `feat(mosaic): a tile grows into the Slider card, and the wall never moves`.

## Mosaic (the keeper's ladder, no hover)

A mosaic tile is far too small for the info, so the tile itself grows.

- **First click on a tile (or Enter on the focused one)** grows that tile into
  the Slider card's size and shape, on top of its neighbours. The wall is never
  recalculated: reflowing it would move tiles under the cursor and change the
  pagination on every movement. The card is centred on its own tile and clamped
  inside the viewport.
- **Two beats**, both sequenced in Rust (a Slint Timer inside a repeater panics
  on 1.17, and the wall's geometry is Rust's anyway): beat 1 the tile grows in
  the Slider's 350ms cadence, beat 2 the info face slides out inside it on the
  right third, over the theme's image — the same face component.
- **Second click on the grown card applies** the theme. Leaving the card with
  the pointer, moving the focus with the arrows, or Escape contracts it
  **without applying**: growing is only a look. Before this, one mosaic click
  applied the theme straight away.
- **Keyboard**: arrows move a cursor (never expanding anything — the wall would
  be covered the whole time); Enter walks the same ladder. Left/right follow
  reading order, up/down use the geometry because a justified wall has rows of
  different tile widths, and at a page edge the move steps the page. The
  neighbour rule is `mosaic_neighbor` in Rust, unit-tested on a two-row wall of
  unequal tiles.
- **Proof of "no recalculating"**: the render test
  `mosaic_grown_tile_covers_only_its_own_card` asserts that with a tile grown,
  everything more than 620px from that tile's centre is pixel-identical.

The Slider is untouched: `ThemeInfoPanel` is back to its committed state.

## PENDING — the mosaic keyboard steals the drawers (found 2026-10-04)

**Symptom**: in the Mosaic, Up/Down no longer bring out the hidden drawers
(Settings upward, Slider/Mosaic downward). The keeper relies on those keys.

**Cause (verified)**: `shell-kbd` gives the Mosaic its own branch
(`mounted-screen == 1 && gallery-style == 2`) that consumes all four arrows and
routes them to `gallery-mosaic-nav`. Before that branch, Up/Down fell through to
`nav-move`, and `src/callbacks.rs:143-161` says exactly what they did: "V6.1:
Up/Down no longer move the carousel — they deploy the drawers (top Settings,
bottom Slider/Mosaic)". The mosaic branch runs first, so the drawers lost their
only keyboard path. The Slider is unaffected (its branch is untouched), and the
mouse path (the ChromeButton arrows) still works.

**Options (not implemented — the keeper parked it)**:

1. **Edge escape (preferred)**: keep Up/Down for the cursor, and when the cursor
   is already on the first row an extra Up opens Settings, and on the last row an
   extra Down opens the Slider/Mosaic drawer. The signal already exists:
   `mosaic_neighbor` returns `None` at a page edge, and the nav handler already
   branches on `None` (today it steps the page for left/right and does nothing
   for up/down). It mirrors how scrolling past an end reveals the next thing, and
   keeps one meaning per key.
2. **Split the keys**: Left/Right move the cursor, Up/Down keep the drawers, and
   the cursor's vertical movement moves to PageUp/PageDown (or Ctrl+arrows). Keeps
   the Slider's convention intact but costs the wall its most natural axis.
3. **Different key for the drawers in the mosaic**: Up/Down keep the cursor and
   the drawers move to PageUp/PageDown. Cheapest, but the same key means
   different things in the two styles, which is exactly what this session kept
   fixing elsewhere.

Whatever is chosen must keep the Slider's Up/Down behaviour byte-identical.

**Minor pendings — CLOSED (keeper's live test, 2026-10-04)**: both keeper-owned
live questions are resolved, so neither is open any more. (a) the mosaic
expansion fade (`805231c` carries the current version) and (b) the legibility of
the translucent face over a bright image (the suggestion struck in the review
section below). No source change followed either one. The keeper did not record
which outcome was chosen, so no decision detail is invented here.


## Final shape of the face (keeper's brief)

- A slanted parallelogram **inside** the theme card, covering **one third** of
  its width and its full height, sheared with the card's own ratio (35px of
  shear per 520px of height) so its edges are parallel to the card's.
- **Translucent** surface (`bg-card` at 65% opacity) so the theme's image shows
  through; a 1px border keeps the shape readable.
- Opens by sliding out of the card's left edge AND unfolding (x and width
  animate together); closes by **contracting back into that same edge**. The
  content keeps its place and the panel's own clip reveals it, so nothing
  reflows mid-animation.
- Layout is the first version's: title, saved date, palette swatches, one line
  per fact (label left, value right) and the provider chips.


## Cross-model verification, second round: the mosaic ladder

Same reviewer and model. Findings and disposition:

- **CRITICAL — the mouse ladder was dead.** The two-beat wiring had been
  attached to the under-layer cells (a frozen visual copy that never receives a
  click) while the live page cells still applied the theme on the first click.
  FIXED: both layers walk the same ladder, and the new test
  `mosaic_first_click_grows_and_the_second_applies` drives the production input
  path — with the old wiring restored it fails, which is the proof it catches
  this class.
- **CRITICAL — a page flip left the grown card mounted**, re-anchored onto the
  new page's tile while the face still showed the previous theme: a click there
  applied a different theme than the one on screen. FIXED: the wheel path and
  the keyboard page-edge path both drop the grown card before the flip.
- **CRITICAL — a card still growing (or already contracting) could apply**: a
  quick Escape-then-Enter matched the "second Enter" branch, and a click on the
  collapsing card applied too. FIXED: applying requires
  `card_open && face_open`.
- **WARNING — the hover contract was half-implemented**: a keyboard-opened card
  could be contracted by a mouse elsewhere, and a card whose pointer left during
  the morph could stay open forever. FIXED: the card latches whether the pointer
  was ever inside it and re-checks when the face opens.
- **WARNING — the four mosaic callbacks did not gate on `is_mutating`** like
  every neighbouring handler. FIXED.
- **SUGGESTION — `mosaic_tile_real` accepted a position past the model.** FIXED.
- **SUGGESTION — the ladder callbacks are not unit-tested end to end**: the
  render test drives the flags directly. NOT fixed; the input-path test above
  covers the click half of it.
- ~~**SUGGESTION — legibility of the face over a bright theme image**~~ (the face
  is translucent by the keeper's own request). **CLOSED by the keeper's live test
  (2026-10-04).** No source change followed: `805231c` is still the last source
  commit on the branch.
- Clean per the reviewer: the no-reflow claim itself (nothing in the grown-card
  path rebuilds the wall), the keyboard branches versus the Slider, and the
  `mosaic_neighbor` unit tests.
- The reviewer also flagged a doc contradiction (a "DONE" verification section
  beside a "BLOCKED" checkbox); corrected above.


Independent review by `jd-judge-a` on `opencode-go/glm-5.3-flash` (different
family from the author) over the five feature commits. It could not run the
suite itself (its runtime exposes no shell tool), so its review is static; the
suite result below is the author's.

Findings and disposition:

- **CRITICAL — the primary gesture never opened the face.** Right-clicking an
  off-centre card moves the focus first and opens the face right after, while
  Slint runs `changed` handlers queued on the next event-loop tick — so the
  focus-change close fired once the face was open. FIXED in `5a922cd`: the
  close is now conditional on the identity (`info-index != focused-index`).
- **WARNING — Return applied the focused theme while the face was open**,
  contradicting the read-only contract the mouse path already had. FIXED.
- **WARNING — Mosaic/Hexagon** set the open flag with no face to render and no
  way back except Escape; a style switch left a stale flag. FIXED (request
  ignored outside the Slider, style switch closes it).
- **SUGGESTION — hover-brighten painted its white veil over the sheet** while
  the mouse rested on the card. FIXED.
- **SUGGESTION — the containment tolerance (<2% of outside samples) could hide
  a small leak.** FIXED: the assertion is now "no outside sample changes".
- **SUGGESTION — the dead `flipped()` call** in the right-click path is a
  pre-existing PR2 stub. NOT fixed: out of this feature's scope.
- Clean per the reviewer: path safety, never-invent/leak data, identity, and
  the lifecycle paths (save/rename/delete/overwrite/leaving the gallery/Esc).
- Not verifiable by the reviewer: mid-slide frames (no event loop in the
  harness) and commit hygiene (no git access there).

## Verification evidence

- `cargo test`: **1295 passed / 0 failed** (baseline 1282; +13 new tests).
- Visual (author's own eyes, per-run directories):
  - `/tmp/opencode/render-info-v9` — the slide proof: `theme_info_sliding.png`
    is byte-identical to the closed frame (the face is NOT painted at t=0) and
    `theme_info_open.png` is the settled face in place.
  - `/tmp/opencode/render-info-v10` — containment with the tightened
    assertion: no pixel outside the card changes.
  - `/tmp/opencode/render-info-v11` — identity: the face does not follow the
    carousel to another card.
- Structural assertions: face paints inside the card and nowhere else; t=0
  frame identical to closed; a card never shows another theme's facts.


## Known gaps to iterate on

- The rows use a two-column sheet layout (label left, value at the column's
  right edge); a tighter label/value pairing is a taste call.
- The face covers the card's own 1px border where they overlap.
- Arrow keys still move the carousel: that closes the face (focus change) —
  deliberate, so it never shows another theme's facts.
- noctalia-v4 records are not reported (only v5).


