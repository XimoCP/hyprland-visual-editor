# ODD — Settings/Borders visual polish (first batch)

Feature name: `settings-borders-visual-polish`
Branch: `hve2-visual-rewrite` (current; not the default branch)
Status: open — T1..T3 by the writer batch, T4 is the keeper's live look
Batch: this is the FIRST item of a polish series the keeper is about to send ("vamos a pulir unas
cuantas cosas visuales"). Keep the document open and append the next items as they arrive; do not
close it per item.

## Objective

Two small visual fixes in Settings -> Bordes:

1. **The glow/brightness activation block is an unlabelled, over-wide switch row.** It reads as an
   empty block. It must carry a text label naming what it does (the keeper's wording: "Activar
   brillo" o similar).
2. **The colour modules are not vertically centred in their containers — they all sit too high.**
   They need more top margin (the keeper's estimate: about 5 px more).

## Evidence

- Item 1 was already recorded as an open decision when the tune pane closed, not as a defect
  ("la fila del interruptor quedó ancha sin texto", session 2026-10-01), and the same session noted
  the rows "no contrastan con el relleno del contenedor". The keeper has now decided the label.
- Item 2 is a fresh observation from the keeper looking at the live panel.
- The glow control was shipped as a single animated switch in `ui/panel/sections/BordersSection.slint`
  (see `odd/tasks/border-tune-pane-sections-and-picker.md`); the surrounding blocks already have their
  own containers (commits `34ba472`, `85002c3`, `b9ea81a`).

## Constraints

- **Nothing hard-coded.** The project rule is that the design structure is agnostic and values come
  from the design system, not from magic numbers in the UI (see the spec set leveled with the shipped
  tune pane, commit `fd75057`). So: use the existing spacing/typography tokens from `SkwdTokens` for
  the margin. If the scale cannot express the keeper's ~5 px, do NOT invent a raw pixel value: use the
  nearest token, and report the exact token and the resulting value so the keeper can judge the look.
- **UI copy language and casing must match the section's existing neighbours.** Follow whatever the
  section already does; do not introduce a third style.
- No layout restructuring, no new blocks, no new windows. One window that mutates.
- Strict TDD with `cargo test`.
- Visual verification is MANDATORY for this change: a `.slint` change verified only by "tests green +
  build clean" is NOT verified.

## Tasks

- [ ] **T1 — Failing test first.** Extend the existing render/panel test(s) in `src/shell/ui_tests.rs`
      so the glow row's label and the colour block's top margin are covered by a real assertion (not a
      source-text match). If a guarantee can only be judged by looking at pixels, say so explicitly and
      lean on the render artifacts instead of inventing a fake assertion.
- [ ] **T2 — Implement the two fixes** in `ui/panel/sections/BordersSection.slint`.
- [ ] **T3 — Render and read the PNGs.** Run the headless render (own directory per run):
      `HVE_RENDER_DIR=/tmp/opencode/render-verify-<unique> cargo test slice_focus_flow_renders -- --nocapture`
      (or a sibling render test you extend for the new state). Report the exact per-run directory and
      confirm with your OWN inspection of those PNGs that the label is visible and the colour modules
      look centred. A frame from a previous run is never evidence.
- [ ] **T4 — The keeper's live look.** The orchestrator reads the PNGs as well and reports; the keeper
      confirms on the real desktop.

## Acceptance criteria

- The glow/brightness block shows a clear label naming the action, in the section's existing language
  and casing, and the row no longer reads as an empty block.
- The colour modules are visually centred in their containers, with the added top margin, and no
  element is clipped or pushed out of its container.
- All spacing/typography values come from the design tokens; no raw magic number is introduced.
- Full suite green and the build warning-free.
- Render artifacts exist for this change and were inspected, not assumed.

## Delivery

- Trivial cosmetic scope: one file plus its test, one work-unit commit, Conventional Commit message,
  no AI attribution. Tier 2 (orchestrator review) under the project's review-proportionality rule;
  the mandatory render check covers the visible behaviour. No cross-model round for purely cosmetic
  polish — but the next items of this batch get their own assessment, and anything structural
  (state, I/O, timing) goes back to Tier 3.

## Progress / evidence

### T1 — failing test first

New render test: `borders_colour_swatches_are_vertically_centred_in_their_containers`
(`src/shell/ui_tests.rs`), beside `strip_chip_ring_span` and the compact-row tests. It mounts the
tune pane (2 colour slots, glow off), parks the pane on the glow-header stop — the one seat where
BOTH containers are inside the viewport — and reads the render:

- the compact colour row's 16px chip (magenta, its own resolved colour) against the row's own
  full-width `HveColors.border` hairlines, and
- the strip chip's 16px swatch (red) against the 40×32 chip the active chip's 2px icy ring draws.

It collects BOTH mismatches and fails once, so the failure names the defect and its size instead of
stopping at the first. Red before the fix (run `render-tdd-fail2`):

```
[swatch] compact row 441..488 centre=464, chip 454..467 centre=460;
         strip chip 578..609 centre=593, swatch 579..592 centre=585
these colour swatches are NOT vertically centred in their containers: [
    "the compact colour row's chip: centre 460 against the row's 464 (row 441..488, chip 454..467 at x 1082..1095)",
    "the strip chip's swatch: centre 585 against the chip's 593 (chip 578..609, swatch 579..592 at x 1077..1090)",
]
```

Green after the fix: `chip 458..471 centre=464` and `swatch 587..600 centre=593` — both offsets are
exactly 0.

### T2 — what changed

`ui/panel/sections/BordersSection.slint`, two rules, no layout restructuring:

1. **The glow switch row carries its own label.** A `Text` (`BordersText.glow-enable-label`) is the
   row's first child, and the row's layout went from `alignment: end` to `alignment: space-between`
   — with the label back, `end` packed the label up against the switch (measured: the label landed at
   x≈1743..1803, hugging the pill) instead of parking the two children on the row's two edges. The
   switch's own box did NOT move: the green pill still measures x 1841..1878, y 839..860, identical
   to the pre-change frame.
2. **Both colour swatches are centred in their containers.** `cross-axis-alignment: center` on the
   compact row's `HorizontalLayout` and on the strip chip's inner `HorizontalLayout`.

### The margin token question (the keeper's constraint)

**No spacing/typography token was used, because no margin can express this.** The root cause is not
a missing margin but a layout rule: a layout's default `cross-axis-alignment` is `stretch`
(`i-slint-compiler-1.17.0/builtins.slint`), and a child with an explicit `height` cannot stretch, so
Slint places it at the START (top) of the box the layout gave it. The 16px swatch was therefore glued
to the top of its container and the empty space fell BELOW it — the keeper's "not vertically centred,
they all sit too high".

- Compact colour row: content box 22px (48px row, 1px borders, 12px padding), 16px chip → the chip
  sat 4px above the row's centre. Nearest token `SkwdTokens.spacing-4` = **4px**, and a 4px top pad
  does shift the chip onto the centre — but it also moves the two Texts 2px down (their box shrinks
  from 22px to 18px) and leaves the 22px "Custom…" button 4px taller than its content box, i.e. it
  overflows the padding it was measured against. That is a worse row, not a centred one.
- Strip chip: content box 30px (32px chip, 1px borders), 16px swatch → the swatch sat 8px above the
  chip's centre. The nearest token that would centre it is `SkwdTokens.spacing-8` = **8px**, but the
  same pad also drops the chip's number text 4px below centre.

`cross-axis-alignment: center` centres both exactly (offsets 4px → **0px** and 8px → **0px**),
introduces no number at all, and touches no container, no size and no keyboard stop. The keeper's
"~5px" sits between the two measured defects (4px and 8px), which is what the two containers' heights
predict. If the keeper still wants a literal top margin instead, the change is one property per
layout and the two pixel values above are the numbers to judge.

### T3 — render and read the PNGs

Run (whole Borders suite in one process, so every frame in this directory comes from this run):

```
HVE_RENDER_DIR=/tmp/opencode/render-verify-borders cargo test --bin hve borders_ -- --nocapture
→ render artifacts for this run: /tmp/opencode/render-verify-borders
→ test result: ok. 54 passed; 0 failed; 0 ignored; 0 measured; 1168 filtered out
```

Frames read from THAT directory with my own inspection:

- `borders_glow_switch_off.png` / `borders_glow_switch_on.png` — the row now reads "Enable Glow" at
  its left edge with the switch (grey / green) at its right edge, inside the lit row. Before the
  change the same row was an empty box with a lone pill.
- `borders_t3c2_glow_group_es.png` — the same row in Spanish: "Activar Brillo" (the ES string reaches
  the pixels, not only the text global).
- `borders_colour_swatches.png` — the strip chips: red "1" and green "2" now sit with their swatches
  centred against their numbers; the magenta chip of the inactive compact row is centred against
  "surface_lowest" and "Custom…".
- `borders_tune_glow_colors.png` — the two glow-body compact rows ("Glow Color" / "Inactive Glow"),
  the other two instances of the same component: both chips centred.

Also run as instructed: `HVE_RENDER_DIR=/tmp/opencode/render-verify-slice cargo test --bin hve
slice_focus_flow_renders -- --nocapture` → ok, dir `/tmp/opencode/render-verify-slice`.

### Full suite and build

```
HVE_RENDER_DIR=/tmp/opencode/render-verify-full cargo test --bin hve -- --nocapture
→ test result: ok. 1222 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 75.16s

cargo build
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 36.96s   (0 warnings)
```

### Sibling defect found, NOT touched (for the keeper to decide)

The same rule hits the glow switch pill: it is a 22px fixed-size child of the row's
`HorizontalLayout`, so it is also parked at the top (measured on `borders_glow_switch_on.png`: pill
top 839 = row top + 13, i.e. 8px above the centre of the 64px lit row; 4px in the 56px rest state).
It is not a colour module, so it is outside this item's scope and was left alone. Centring it is one
`cross-axis-alignment: center` on `glow-wrap`'s layout, and it would also make
`borders_glow_switch_renders_off_and_on`'s "the pill is vertically centred" assumption literally
true (today that assertion passes on a loose pixel count).

### Commit

`fix(borders): centre the colour swatches and label the glow switch row` — one work-unit commit
holding the fix, its render test and the i18n wiring: **`904c3cf`**. This document lands in the
`docs(odd)` commit that records it, the same pattern as the palette-defence and resume-defence
ledgers.

