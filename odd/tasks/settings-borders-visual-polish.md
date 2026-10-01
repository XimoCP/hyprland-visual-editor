# ODD — Settings/Borders visual polish (first batch)

Feature name: `settings-borders-visual-polish`
Branch: `hve2-visual-rewrite` (current; not the default branch)
Status: open — items 1, 2 and 3 closed by the writer batch (T1..T3), T4 is the keeper's live look
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

### Sibling defect found, NOT touched in item 1 (keeper approved it afterwards)

The same rule hits the glow switch pill: it is a 22px fixed-size child of the row's
`HorizontalLayout`, so it is also parked at the top (measured on `borders_glow_switch_on.png`: pill
top 839 = row top + 13, i.e. 8px above the centre of the 64px lit row; 4px in the 56px rest state).
It is not a colour module, so it was outside item 1's scope and was left alone. Centring it is one
`cross-axis-alignment: center` on `glow-wrap`'s layout, and it would also make
`borders_glow_switch_renders_off_and_on`'s "the pill is vertically centred" assumption literally
true (today that assertion passes on a loose pixel count). **The keeper approved it in the same
session ("vale centra el interruptor de brillo también") and it is item 2 below.**

### Commit — item 1

`fix(borders): centre the colour swatches and label the glow switch row` — one work-unit commit
holding the fix, its render test and the i18n wiring: **`904c3cf`**. This document lands in the
`docs(odd)` commit that records it, the same pattern as the palette-defence and resume-defence
ledgers.

---

## Item 2 — centre the glow switch pill (keeper-approved follow-up)

Keeper's words: "vale centra el interruptor de brillo también". Same root cause as item 2's colour
swatches, same tool, and it is the only change: the pill is a fixed-size child of a layout whose
default `cross-axis-alignment` is `stretch`, so Slint parks it at the top of the row's content box.

### T1 — the failing assertion (extended the item-1 test)

`borders_colour_swatches_are_vertically_centred_in_their_containers` now also measures the pill and
the row's label against the row's OWN icy ring, in BOTH switch states and at BOTH widths. The colour
swatch assertions are untouched. New helpers beside the test: `icy_row_x_span` (the row's horizontal
span, scanned across the whole frame — the shared `icy_span_at` stops at x=360, which is past the
pane's left edge at the stacked width), `glow_pill_span` (the pill's 22 rows, found by their own
face colour: grey while the glow is off, accent-green while it is on), `ink_span_in_band` (the
label's ink inside the row's box) and `glow_row_centring_problems` (the checks, collected as
sentences so one run names every defect).

RED before the fix (run `/tmp/opencode/render-pill-red3`), pill centre against its row's:

```
the glow-off glow switch pill: centre 813 against the row's 822 (row 791..854, pill 803..824)
the glow-on  glow switch pill: centre 849 against the row's 858 (row 827..890, pill 839..860)
the stacked  glow switch pill: centre 811 against the row's 820 (row 789..852, pill 801..822)
```

### T2 — the fix

`cross-axis-alignment: center` on `glow-wrap`'s `HorizontalLayout` (`ui/panel/sections/
BordersSection.slint`). One property, no pixel value, no container, no height and no keyboard stop.

GREEN, same three frames (`[glow-row]` lines from the run):

```
glow-off: row 791..854 centre=822; pill 812..833 centre 822; label ink 818..826 centre 822
glow-on:  row 827..890 centre=858; pill 848..869 centre 858; label ink 853..861 centre 857
stacked:  row 789..852 centre=820; pill 810..831 centre 820; label ink 815..823 centre 819
```

Offset 0 in all three frames; the label stays within 1px of the row's centre (its glyph box is
asymmetric, which is why the assertion allows 3). No regression on the two things the item had to
protect: the row is still 64px (the icy ring spans 63px between its edges in every frame) and the
pill paints its whole 22px face strictly inside the row (asserted), the label likewise.

### T3 — render and read the PNGs

```
HVE_RENDER_DIR=/tmp/opencode/render-verify-glowpill cargo test --bin hve borders_ -- --nocapture
→ render artifacts for this run: /tmp/opencode/render-verify-glowpill
→ test result: ok. 54 passed; 0 failed; 0 ignored; 0 measured; 1168 filtered out
```

Frames read from THAT directory with my own inspection: `borders_colour_swatches.png` (glow off),
`borders_colour_swatches_glow_on.png` and `borders_colour_swatches_stacked.png` (820px, below the
852px two-column breakpoint). In all three the label and the pill share the row's centre line, the
pill is fully inside the row with equal air above and below, and nothing is clipped — before the fix
the same row showed the pill riding high with the gap below it.

### Full suite and build

```
HVE_RENDER_DIR=/tmp/opencode/render-verify-full2 cargo test --bin hve -- --nocapture
→ test result: ok. 1222 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 74.84s

cargo build → Finished `dev` profile … (0 warnings)
```

### Commit — item 2

`fix(borders): centre the glow switch pill in its row` — **`b00caeb`**, one work-unit commit with
the extended render test. This document lands in the `docs(odd)` commit that records it.

### Not verified for item 2

The keeper's live desktop look (T4): headless frames only. Nothing else in the section was touched.

---

## Item 3 — the preset card's apply switch (keeper's screenshot, "COCO")

Keeper's report: the apply switch of their USER preset card **COCO** in "Mis Preajustes de Borde"
(active, single-line, ★ badge + ✎/✕ buttons, sitting under the built-in cards) is off-centre upwards;
the built-in cards' switches "also look high, aligned with the name line". Reproduced at their own
geometry and language: **1280x800, Spanish UI**, six built-in cards (the ones they named) above the
active user card.

### T1 — measured before touching anything

Two new render tests, both reading the numbers off the frame:

* `preset_card_apply_switch_is_vertically_centred_in_the_card` — the keeper's frame. The card's box is
  read from its own border (3px accent when active, 1px `HveColors.border` when not); the controls from
  the ink only they paint in that card (the pill's face, the ✕ button's red, and the name/badge ink,
  the last two split by their column gap because the active USER card paints both in the same accent).
* `sibling_card_lists_keep_their_apply_control_centred` — Motion (the same `SavedPresetCard`, user card
  active) and the Save section's `SavedThemeCard`.

Measured, card centre against the control's centre (RED):

| surface | card box (centre) | apply switch | name line | ★ badge | ✎/✕ buttons |
|---|---|---|---|---|---|
| COCO (user, active) | 656..755 (705) | 690..711 → **700 (5px high)** | 700..709 → 704 | 703..707 → 705 | 690..721 → 705 |
| Looper (built-in) | 553..608 (580) | 570..591 → **580 (centred)** | 566..578 → 572¹ | — | — |
| Motion user card | 656..755 (705) | 690..711 → **700 (5px high)** | 700..709 → 704 | 703..707 → 705 | 690..721 → 705 |
| Save theme card | 219..318 (268) | 253..284 → **268 (centred)** | not measured² | — | ✕ 253..284 → 268 |

¹ A card WITH a description centres its whole text block, so the name — its first line — legitimately
sits ~8px above the card's centre. That is not a defect and is asserted as such (the name is only
required on the centre line of a single-line card).

² The theme card is a different component and its name is not the subject of this item; its apply
control is what the keeper's report is about.

**Verdict, plainly:** the switch was **high by 5px on the USER card** (COCO and Motion) — the keeper's
report is right. On the **built-in card it was already centred (0px)**: a built-in card has the pill
ALONE in its action row, so the row is 22px tall and the pill fills it, while a user card's two 32px
IconButtons set the row to 32px and the 22px pill is parked at its top. The keeper's "the built-in
switches also look high" is a perception the numbers do not support: their switch is on the card's
centre line, and the name they compared it against is the *upper* of the block's two lines (8px above
the centre), not a line at the centre.

### T2 — the fix

`cross-axis-alignment: center` on the action row's `HorizontalLayout` in
`ui/panel/sections/SavedPresetCard.slint` — one property, no pixel value, no restructuring, and it is
the same rule already justified for the glow row.

GREEN, the same frames re-measured:

| surface | card box (centre) | apply switch | name line | ★ badge | ✎/✕ buttons |
|---|---|---|---|---|---|
| COCO (user, active) | 656..755 (705) | 695..716 → **705 (0)** | 700..709 → 704 | 703..707 → 705 | 690..721 → 705 |
| Looper (built-in) | 553..608 (580) | 570..591 → 580 (unchanged) | 566..578 → 572¹ | — | — |
| Motion user card | 656..755 (705) | 695..716 → **705 (0)** | 700..709 → 704 | 703..707 → 705 | 690..721 → 705 |
| Save theme card | 219..318 (268) | 253..284 → 268 (unchanged) | — | — | 268 |

### Sibling surfaces — what was fixed and what was left alone

* **Motion list — fixed** (same `SavedPresetCard`, same 5px offset, measured). The one-property change
  in the shared component covers it; the test asserts it on Motion's own frame.
* **Save section `SavedThemeCard` — left alone, measured centred.** Its apply control is a **32x32
  square**, the same height as the three IconButtons beside it, so the row has no fixed-size child to
  park at the top. Measured 253..284 against a card centred at 268 → 0px. Nothing to fix, and the test
  keeps it that way.

### T3 — render and read the PNGs

```
HVE_RENDER_DIR=/tmp/opencode/render-verify-card cargo test --bin hve -- --nocapture
→ render artifacts for this run: /tmp/opencode/render-verify-card
→ test result: ok. 1224 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 76.99s

cargo build → Finished `dev` profile … (0 warnings)
```

Frames read from THAT directory with my own inspection: `preset_card_centring_1280x800_es.png` (the
keeper's state — COCO's ✓ check, amber name, ★ badge, green switch, ✎ and ✕ all on one centre line,
with the six built-in cards above), `sibling_motion_card_centring.png` and
`sibling_save_card_centring.png` (the green apply square, the purple refresh and the amber rename all
the same height and centred). Before the fix the pill on COCO sat visibly high, with its top edge
level with the buttons' tops.

RED runs kept for the record: `/tmp/opencode/render-card-red3` (COCO + Looper) and
`/tmp/opencode/render-sib-red` (Motion + Save).

### Size note

The production change is ONE property. The 690-odd authored lines of this item are the two render
tests and the measurement helpers they need (card box, pill rows, ink spans, column clusters): reading
a card's centring off pixels is what the item asked for, and the helpers are the smallest honest way
to do it. Splitting them from the fix would separate the verification from what it verifies, so it
stays one work unit and the count is reported instead of hidden.

### Commit — item 3

`fix(borders): centre the preset card's apply switch on the card` — **`2b9a17e`**, one work-unit
commit holding the fix and both render tests. This document lands in the `docs(odd)` commit that
records it.

### Not verified for item 3

The keeper's live look at 1280x800 (T4): headless frames only. The preset apply/resolution scripts
(`assets/scripts/border.sh`, `assets/scripts/utils.sh`) were NOT touched — separate, unauthorised bug.

