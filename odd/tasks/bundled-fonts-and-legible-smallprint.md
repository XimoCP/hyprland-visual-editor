# Bundled fonts and a legible small-print floor

**Opened**: 2026-10-07 (keeper's report: "me cuesta leer la aplicación desde donde estoy sentado")
**Branch**: hve2-visual-rewrite
**Route**: delegated writer — MiMo (agent `general`, `opencode-go/mimo-v2.6-flash`) for both steps; visual verification by the parent (`opencode-go/deepseek-v4.1-flash`, a different model from the writer); cross-model diff review by GLM (`opencode-go/glm-5.3-flash`)
**Status**: done — four commits: `4c523b8` (fonts), `fa10a71` (sizes), plus `f94d627` and `8c071f6` from the review round; verified by the parent, reviewed by a third model

## The report

The keeper cannot comfortably read HVE from his seat: the smallest text is hard
to read. He also asked whether the fonts should live inside the repo so that
everybody sees the app the way he sees it, and settled that question on the
evidence.

## Findings (verified, with evidence)

- **The app was rendering in two typefaces at once.** Only 97 of the 191 `Text`
  elements under `ui/` declare a `font-family`; the rest inherit the window
  default. `ui/main.slint:45` said `default-font-family: "Inter, sans-serif"` and
  `Inter` is not installed here, so `fc-match "Inter, sans-serif"` resolves to
  `NotoSans-Regular.ttf` — while the shell's own texts ask for
  `SkwdTokens.font-body` ("Roboto Condensed"). Same window, two families, and the
  mix changes with each machine's font set.
- **The declared families existed on the machine, not in the repo.**
  `ui/tokens.slint` declares Roboto (heading), Roboto Condensed (body) and Roboto
  Mono (mono); `assets/fonts/` did not exist. All three were installed as system
  packages *here*, which is exactly why it looked right here and could look wrong
  anywhere else.
- **Slint 1.17 embeds fonts it is asked to import.** `import "…/X.ttf";` registers
  a custom font (compiler parser test, `parser/document.rs:330`), and with
  `slint-build`'s default Rust configuration the bytes go into the binary —
  `EmbedResourcesKind::EmbedAllResources` → `collect_custom_fonts.rs:25` picks
  `RegisterCustomFontByMemory`. Nothing is read from disk at runtime.
- **The small end of the scale was genuinely small**: 23 hardcoded declarations at
  11 px or below (8, 9, 10, 11) plus 28 uses of `SkwdTokens.size-11`.
- **No SemiBold face exists** in the vendored families (400/500/700 only), so the
  47 `font-weight: 600` declarations resolve to the nearest available weight.
  Pre-existing; recorded under "Open questions".
- **Two of the 23 literals live in tight containers**: a colour-token name inside
  a swatch (`ui/panel/sections/BordersSection.slint:1083`, 8 px) and the
  `skwd-wall (MIT, © liixini)` credit pill inside a fixed
  `height: 16px` box (`ui/gallery/GalleryChrome.slint:61`, 9 px) — the two frames
  most likely to clip once the text grows.

## Decisions

- **Vendor the three families, nine faces** (Regular/Medium/Bold × Roboto, Roboto
  Condensed, Roboto Mono), copied byte-for-byte from the system packages, so the
  keeper's current rendering becomes everybody's rendering. 3.1 MB in
  `assets/fonts/`, embedded in the binary at build time.
- **Both licence texts travel with the fonts** (`OFL-Roboto.txt` for Roboto +
  Roboto Condensed, `OFL-RobotoMono.txt` for Roboto Mono). SIL Open Font License
  1.1, neither declaring a Reserved Font Name.
- **`ui/main.slint` inherits the body token** rather than naming a family the repo
  does not ship: `default-font-family: SkwdTokens.font-body`. The Slint compiler
  accepted the binding, so no literal fallback was needed.
- **Step 2 changes values only**: no token renaming, no migration of the hardcoded
  `font-size` values to tokens. The keeper chose "+2 on the small text" and that is
  exactly what it stayed.

## Scope

- In: `assets/fonts/**`, `ui/tokens.slint` (imports only), `ui/main.slint`
  (`default-font-family` only), the 23 hardcoded `font-size` values at 11 px or
  below, the 28 `SkwdTokens.size-11` `font-size` uses, the one stale comment at
  `ui/components.slint:1046`, and the three tests that pin all of it.
- Out: the engine files, every other property in `tokens.slint`, other `.slint`
  structure, and the size-scale redesign (a derived `base-size` token was offered
  and the keeper did not choose it).

## Acceptance criteria

1. The nine vendored faces are byte-identical to the files the keeper's machine
   renders with, and every one is imported by `ui/tokens.slint`.
2. Both licence texts are present and are the unmodified OFL 1.1 texts.
3. `ui/main.slint` no longer names a family the repo does not ship.
4. The ≤11 px band moved by exactly +2: no hardcoded literal below 10 px remains
   anywhere under `ui/`, no `font-size` is bound to a token worth less than 12 px,
   and no `font-size` of 12 px or more was changed. (Written first as "no 8/9/10/11
   px remains", which contradicts the chosen mapping — see Deviations.)
5. `cargo test` green; the render suite re-rendered and the PNGs read by the
   parent, not merely "tests green".

## Tasks

- **T1 — step 1, RED then GREEN**: the two headless contract tests written first,
  then the fonts and the two `.slint` edits. Commit `4c523b8`.
- **T2 — step 2**: the 51 small-print values raised by 2 px, plus the floor test
  written first. Commit `fa10a71`.
- **T3 — visual verification**: render the suite before and after each step and
  read the PNGs. Done by the parent.
- **T4 — cross-model review**, then the doc, the commit and the push.

## Evidence log

### T1 — fonts

- RED (writer's output): `cargo test vendored_fonts` → `assets/fonts/Roboto-Regular.ttf must exist`;
  `cargo test main_window_default_font` → `ui/main.slint must not name the Inter fallback`.
- GREEN: both pass; `cargo test` → 1456 passed, 0 failed (re-run independently by
  the parent, same result).
- Parent's byte check: `cmp` of all nine `.ttf` and both `OFL.txt` against the
  system originals → identical.
- Parent's visual check: baseline `/tmp/opencode/baseline-fonts-1791343334`
  (89 PNGs) vs after `/tmp/opencode/render-after-fonts` (89 PNGs) → **73 frames
  changed, 16 identical** (the identical ones are image-only frames).
  `panel_system.png` and `panel_save_es.png` read directly: body text now
  condensed, no clipping, no overflow.

### T2 — sizes

- RED (writer's output): the floor test named the 8 px literal, the 9 px literal
  and the 28 `size-11` uses. GREEN after the value pass.
- Exactness (parent's check on the commit): removed side = 10×`10px`, 11×`11px`,
  1×`8px`, 1×`9px`, 28×`size-11`; added side = 1×`10px`, 1×`11px`, 10×`12px`,
  11×`13px`, 28×`size-13`. 51 lines each side, nothing else moved.
- `cargo test` → 1457 passed, 0 failed (parent re-ran it).
- Parent's visual check: step-1 renders vs
  `/tmp/opencode/render-after-sizes` (89 PNGs) → **80 frames changed, 9 identical**.
- **The two tight containers, checked with pixel evidence rather than by eye
  alone.** In `mosaic_hero.png` the only pixels that differ between step 1 and
  step 2 are `x1789..1895, y1022..1034` — the credit pill's glyphs: the pill's own
  background is pixel-identical, so the 16 px box is unchanged while the 11 px text
  inside it occupies a 12 px band, i.e. ~2 px of clearance top and bottom, no
  clipping. `borders_strip_flow_picker_open.png` read directly: the token pills
  (`primary`, `accent-amber`, `surface`, `lowest`) grew with their labels and no
  text is cut; `panel_borders_pick.png` reflows with taller rows and no overflow.

### T3 — the floor test is not decoration (parent's mutation runs)

- A `font-size: 7px` literal written into a brand-new nested directory
  (`ui/gallery/__probe/probe.slint`) → test FAILS with
  `ui/gallery/__probe/probe.slint:2 declares font-size: 7px (floor is 10px)`.
- A `font-size: SkwdTokens.size-11;` re-introduced into `GalleryChrome.slint` →
  test FAILS. Both mutations reverted; tree clean.

## Review round (third model) and its follow-ups

GLM (`opencode-go/glm-5.3-flash`) reviewed `dddcf66..fa10a71` read-only, with byte
identity, the value pass and the render reading excluded because the parent had
already done them. Verdict: **approved-with-findings**. Four findings were real
and were fixed in `f94d627` and `8c071f6`:

- **The change did not fully deliver its own goal: three sites still used a system
  font.** `ui/components.slint:725` (the hidden measurement text that sets
  `tree-path-col-width`) and `:901`, `:909` (the tree's path rows and dot leader)
  asked for `font-family: "monospace"`, a generic name the platform resolves
  against whatever the machine calls monospace — and it was **not** monospaced
  here: `"monospace"`, `"sans-serif"` and `"serif"` all measured 81 px for the same
  string, against 101 px for the vendored `"Roboto Mono"`. The About card's tree was
  therefore already machine-dependent and had never actually been monospaced. All
  three now use `SkwdTokens.font-mono`; `ui/components.slint` needed
  `import { SkwdTokens } from "./tokens.slint";` to resolve the token.
- **`assets/fonts/README.md` overstated the packages.** "Only the three weights
  Regular, Medium and Bold are shipped upstream" is false — the same packages also
  ship Light, Thin, Black and italics. Reworded to say those are the only weights
  *vendored* (and the only ones the UI asks for), with the `600` tie stated
  honestly.
- **A second stale comment survived the size pass**: `ui/gallery/GalleryChrome.slint:46`
  still said "9px text" for a pill that is now 11 px. Fixed.
- **Both new tests were weaker than their doc comments claimed**, and both were
  hardened:
  - the font contract test never tied the *declared family names* to the vendored
    files, so a typo in `tokens.slint` would have passed while the app silently fell
    back — it now maps each declared family (spaces stripped) to its three
    Regular/Medium/Bold faces and names the offending family;
  - the floor test's scanner was line-based and digit-first: a ternary
    (`font-size: expanded ? 9px : 12px;` — a form already present at
    `ui/components.slint:786`), a wrapped binding, a non-`px` unit, a plain-property
    binding and a second `font-size` on one line all slipped through. It now scans
    statement-wise to the `;`, checks every `px` literal in the value span (minimum
    against the floor), validates token references as before, and **fails loudly on
    any form it cannot read** instead of skipping it.

The reviewer's judgment call — the contradiction between the parent's "no 8/9/10/11
px remains" checklist and its own `8→10, 9→11` mapping — was upheld: the mapping is
what the keeper approved, and the checklist was wrong.

### The tree column, verified by reading the frames

`f94d627` is a real visual change: the measured column grew (the tallest path,
`hve_presets.rs` at 12 px, measures 139 px under the old generic name and 188 px
under Roboto Mono), so the tree's paths and descriptions shift right. Of the 89
render frames, exactly **one** changed — `panel_system_about_es.png`, the only one
that shows the tree expanded. Its diff is confined to `x1059..1902, y504..803`.
Read at 130 %:

- **before**: dot leaders of visibly different lengths and descriptions that do not
  start at the same x — the proportional face rendered as a "monospace"
  replacement;
- **after**: the path column is monospaced and every description starts at the same
  x, with no elide, no truncation and no overflow.

The other 88 frames are byte-identical, so nothing else moved.

## Checks (continued)

| Check | Command | Result |
|---|---|---|
| Review | GLM read-only review of `dddcf66..fa10a71` | approved-with-findings; four fixed |
| Mono pass | `cmp` of the render dirs before/after `f94d627` | 1 frame changed (`panel_system_about_es.png`), 88 identical |
| Ternary guard | probe `font-size: expanded ? 9px : 12px;` (parent's own run) | FAILS naming `…/ternary.slint:3` — hole closed |
| Unknown form | probe `font-size: root.my-size;` (writer's run) | FAILS with "a font-size this guard does not recognise" |
| Family typo | probe `font-mono: "Aptos"` in `tokens.slint` (writer's run) | FAILS naming the family; file restored |
| Full suite | `cargo test` | 1457 passed, 0 failed |

| Contract tests | `cargo test vendored_fonts main_window_default_font` | RED before, green after |
| Floor test | `cargo test no_ui_font_size` | RED before, green after; still fails under both mutations |
| Full suite | `cargo test` | 1456 → 1457 passed, 0 failed |
| Render suite | `HVE_RENDER_DIR=… cargo test renders -- --nocapture` | 32 passed; 73/89 then 80/89 frames changed |
| Byte identity | `cmp assets/fonts/*.ttf /usr/share/fonts/TTF/*` | identical |
| Value exactness | `git diff -U0 -- ui/ \| grep font-size` | 51/51, exact mapping |
| Visual read | parent reads the PNGs | done for both steps |

## Deviations / tensions observed

- **The parent's own checklist contradicted its own mapping.** It demanded "no
  `font-size` at 11 px or below remains" while the chosen rule (8→10, 9→11,
  10→12, 11→13) necessarily leaves 10 px and 11 px behind. The writer flagged the
  contradiction instead of silently clamping, and the parent kept the mapping as
  authoritative (it is what the keeper approved) and fixed the criterion above.
  The consequence is under "Open questions".
- **The third-model review stalled on its first attempt** (it began waiting for a
  background `cargo test` instead of producing the report) and was relaunched with
  an explicit "do not run cargo" instruction.
- **Step 2 is a visual change with no visual regression gate of its own.** The
  render suite writes PNGs but asserts nothing about the text metrics; the only
  thing standing between a clipped label and the screen is the parent reading the
  frames, which is what the two pixel checks above were for.

## Open questions for the keeper

1. **Two labels are still small.** After the +2 the smallest text in the app is
   10 px (the colour-token name in a swatch) and 11 px (the `skwd-wall` credit
   pill). If those two are still unreadable from your seat, the lever is a hard
   12 px floor for those two lines — a two-line change, and it breaks the "+2
   everywhere" symmetry you approved.
2. **`font-weight: 600` has no exact face.** 47 declarations ask for a weight the
   three families do not ship; which of 500/700 wins is not observable from the
   tests. Fixing it means vendoring a SemiBold face the classic package does not
   provide.
3. **`size-9` and `size-11` are now unused tokens** (they were declared for the
   old band). Left in place on purpose: removing tokens is its own change.
4. **The tree's branch glyphs are still the platform's.** The vendored
   `RobotoMono-Regular.ttf` contains no glyph for `U+2500`, `U+2502` or `U+251C`
   (the `├──` characters), so those three still come from per-glyph system
   fallback; they render as real box-drawing glyphs here, but a machine with no
   covering font would degrade them. The ASCII of every row is now pinned; the
   three branch characters are not. Options: accept, switch the tree to ASCII
   (`|--`), or vendor a face that covers them.
5. **Distribution nuance**: the licence sits in the repo next to the fonts. If an
   installer ever ships only the binary, the OFL notice is still owed there.
