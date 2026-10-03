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

- [ ] T1 Slint panel
- [ ] T2 GalleryRoot integration
- [ ] T3 main.slint / shell.slint plumbing
- [ ] T4 render test
- [ ] T5 Rust reader (TDD)
- [ ] T6 Rust wiring
- [ ] Cross-model verification
