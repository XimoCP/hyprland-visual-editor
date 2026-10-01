# Border Tune Pane — Interactive Blocks and Anchored Picker

**Locator**: `odd/tasks/border-tune-pane-sections-and-picker.md`
**Repo**: `hve` — branch `hve2-visual-rewrite`
**Opened**: 2026-09-29
**Surface**: right-hand tune pane, `ui/panel/sections/BordersSection.slint`
(`BordersTunePane` from ~125; `BordersSection` owner from ~1420)

## Workflow note (why this is organic)

The keeper's standing choice is Organic Driven Development. SDD is blocked in this
runtime: its preflight gate requires exactly three questions inside one `question`
tool call, and that call fails with `Attempted to assign to readonly property`
(reproduced three times, two payload shapes). Two-question calls are refused with
"exactly three questions"; one question works. The gate forbids collecting those
answers from chat, so no SDD phase can be launched here. Organic routing it is.

Delegation policy for this document: mapping to a free model, writing to a free
model, verification always on a *different* model than the writer. Engine files
stay sealed (`src/engine.rs`, `config.rs`, `settings.rs`, `theme_manager.rs`,
`app_state.rs`, `watcher.rs`, `utils.rs`, `src/providers/*`).

## Objective

Four changes to the right tune pane, requested by the keeper:

- **A — Glow enable becomes a switch.** Today the glow block is enabled/disabled by
  two rectangles that send a command: an "Add" rectangle
  (`BordersSection.slint:1002-1024`) and a ✕ rectangle (`:1063-1087`), both calling
  `glow-changed(bool, …)`. The keeper wants the same switch the left preset cards
  use: inactive, animated, and coloured when active.
- **B — Activated blocks fold open.** Blocks that a switch turns on must stay hidden
  until the switch is on, then unfold with an animation, like the colour picker card
  does today.
- **C — The picker opens next to the edited field.** Today the picker renders in one
  fixed in-flow slot; the keeper wants it adjacent to the row being edited, "if it is
  possible".
- **D — Block boundaries become obvious.** The pane is a long stack of controls and
  the keeper cannot tell where one block ends and the next begins. Wanted: a title
  and an explanation per block, and/or each block inside a container with its own
  surface.

## Exploration findings (evidence, read-only)

**Canonical switch to copy (A).** `SavedPresetCard.apply-switch`,
`ui/panel/sections/SavedPresetCard.slint:138-174`: pill 40×22, radius 11px,
`background: is-active ? accent-green : border`, `animate background` 250ms
`ease-in-out`, knob 16px travelling `x` 3px ↔ right-3px with `animate x` 350ms
`ease-in-out-back`, driven by `switch-ta` click. Colours: off `HveColors.border`
(`#30363d`), on `HveColors.accent-green` (`#10b981`), knob `HveColors.bg-dark`.
Other switch-like components exist (`ui/components.slint:152-174` for the filter
cards, `:667-692` for system cards, `SavedThemeCard.slint:131-173` which is a ✓
square) — the left-preset one is the pattern the keeper named.

**Animated fold to copy (B).** Only the picker card animates today:
`picker-visible: root.editing-slot >= 0` (`BordersSection.slint:147`), the card is
always mounted and animates its own geometry through `states [ picker-shown when
root.picker-visible ]` with `height 0→380px`, `opacity 0→1`, `transform-scale 0.95→1`,
`in` 250ms `ease-in-out-back` / `out` 150ms `ease-in` (`:671-704`). The source
comment at `:649-657` records why: Slint only tweens a property that changes on a
live element, so an element created by `if` comes up at its target values and cannot
animate. The glow block today is exactly that — `if !root.glow-enabled`
(`:980`, `:1031`) and `if root.glow-enabled : VerticalLayout` (`:1039`), instant.

**Picker placement (C) — the hard one.** `HveColorPicker` (`ui/panel/ColorPicker.slint:28`)
has exactly one instantiation: `BordersSection.slint:802-820`, inside `picker-block`
(`:646`, deliberate) → card (`:671-822`) → `tune-col` (`:374`) → `tune-scroll`
(`ScrollView :344`). It is normal in-flow layout, and the source states the card's
`y` is owned by the layout so a y-shift cannot render (`:661-663`). There is no
`PopupWindow`, no `absolute-position`, and no `Container` anywhere in `ui/`. The
pane is instantiated twice behind `if root.two-col` (`:1956` / `:2059`), and Slint
forbids reaching into a conditional child — which is why every cross-cutting signal
in this file is lifted to the section. No control reports its geometry:
`request-edit-slot(int)` (`:155`) carries only the channel index, and the section's
only `out` properties are `idx-strip` (`:1541`) and `tune-count` (`:1547`).
The current spec *requires* the in-flow behaviour: `border-color-strip` R3 says the
picker MUST open above the strip in the normal document flow (`spec.md:60-94`), R5
pins the entry animation and forbids animating `y` (`:121-164`), R10 the
scroll-into-view (`:287-298`). Anchoring per field is a change of written contract,
not a tweak.

**Landmine to respect.** The keyboard stop count lives in two places that must move
together: Slint `tune-count: 9 + slot-stops + (glow ? 4 : 0)` (`:1547`) and the Rust
mirror `borders_tune_stop_count()` (`src/callbacks.rs:1087-1096`). Splitting them
lands the keyboard walk one seat off for every later stop.

**State that must keep working.** `glow-changed(bool,int,int,string,string)` (`:178`),
`tune-glow-enabled` (`:1492`, forwarded at `:2020-2038` and `:2123-2140`), the pane's
glow row indices `idx-glow-*` (`:244-251`), `PresetStore::decode_glow/encode_glow`
(`src/main.rs:249,318`), and the whole keyboard sub-navigation (`:1702-1800`).

**Specs that must stop lying after the change.** `openspec/specs/border-tune-pane/spec.md:61`
(glow field set, "addable empty state") and `openspec/specs/border-color-strip/spec.md`
R3/R5/R10/R11 for C.

**Render verification.** Per-run artifacts come from `render_run_dir()`
(`src/shell/ui_tests.rs:527`) keyed by `HVE_RENDER_DIR`, and
`save_slice_png()` (`:546`). Existing coverage that this work touches:
`borders_glow_group_renders_addable_and_expanded` (`:7806`), `borders_tune_zero_colours_has_no_ghost_stops`
(`:7610`), `borders_tune_full_focus_reaches_last_and_middle` (`:7906`),
`borders_tune_glow_color_cards_render_inside_their_box` (`:8062`),
`borders_strip_picker_opens_above_strip` (`:5705`), `borders_strip_flow_renders`
(`:7267`), `picker_scale_animation_stays_declared_by_construction` (`:4890`, a
source-text test that WILL fail if the picker's scale declarations are restructured).
No test currently pins the glow Add/✕ control itself, nor the glow reveal, nor any
per-field picker anchor.

## Scope

In scope: the right tune pane's glow block, its activated-block reveal behaviour,
the picker's placement, and the pane's block delimitation — plus the specs and tests
that describe them.

Out of scope: the gradient-angle control (keeper decided to leave it alone,
2026-09-29), the left preset list's behaviour, the engine, the gallery, and
monitors/HDR.

## Constraints

- Engine files stay sealed.
- Strict TDD: failing test first, `cargo test` is the runner.
- Visual verification is mandatory for any `.slint` change: run the headless render
  into a per-run directory, then read the PNGs and confirm the look by eye.
- One block at a time; the keeper approves each before the next starts.
- `SavedPresetCard`/engine visuals are reused, not reinvented.
- 400 changed lines is a planning heuristic for delivery slicing, not a hard cap.

## Tasks

- [x] **T1 — Glow enable becomes a switch (A)** — commit `d3be1a3`
      Replaced the "Add" rectangle and the ✕ with ONE header row mounted in both
      states, carrying the title and the same switch the left preset cards use.
      The duplicate enabled-state header was deleted with its ✕. The click still
      calls the unchanged `glow-changed(bool,int,int,string,string)`; no Rust,
      callback, keyboard-stop or spec surface moved.
      Evidence: `cargo test` 1183 passed / 0 failed; `cargo build` clean;
      `cargo test architecture_contract` 7 passed; new render test
      `borders_glow_switch_renders_off_and_on` read frame by frame
      (`/tmp/opencode/render-t1-glow-switch/`) plus the five existing glow renders.
      Independent verification on a different model: PASS, no refutations.
      Deferred, not defects: the `glow-add` label chain is now dead in three
      places (property, i18n setter, generated getter) and the title swaps
      font-family/weight by state to preserve each state's previous look.
- [x] **T2 — Thin-block reveal behaviour (B)** DONE — commits `13e015c` (the fold) and
      `ce2ea8d` (the fixes cross-model verification demanded). The glow body is always
      mounted inside `glow-body-clip`, whose height animates 0 → the inner layout's own
      `preferred-height` (250ms ease-in-out-back in, 150ms ease-in out, `clip: true`), and
      the OFF-state description folds too, in BOTH directions. Height only, no opacity and
      no scale: `clip` is only provable at full opacity and the software renderer the
      headless tests use does not implement transforms. Cross-model verification: PASS,
      with two refutations (a false claim in a test comment, and the instant appearance of
      the description when glow turned OFF) — both fixed and spot-checked by the
      orchestrator on the frames. Suite `1193 passed / 0 failed` at that point.
- [x] **T3 — Block delimitation (D)** DONE across three slices: T3a `9fe052f` (the honest
      scroll-follow, the prerequisite), T3b `34ba472` (the Geometry container, pattern
      proven end to end), T3c-1 `85002c3` (Angle, Inactive, Save) and T3c-2 `b9ea81a`
      (Active colours, Slot add/remove, Glow). All SEVEN blocks now live in a container
      with its own surface and a title + description header; three new strings were
      registered for real in all five points; four `focus-block-y()` sums gained their
      container's live `y`. See the T3 slicing section for the evidence and the residuals.
- [x] **T4 — Picker anchored to the edited field (C)** DONE — STUDY ONLY, and it REFUTES
      the anchored picker. The mechanism exists in the pinned Slint (`PopupWindow`, and it
      DOES render in the headless software backend — the "unverifiable" objection was
      checked and is false), but the geometry kills it: the picker card is ~819×380px and
      the tune pane is ~879px wide, so it cannot sit next to a 48px row without covering
      the pane and the row's neighbours; it would also rewrite R3/R5/R10/R11 and move the
      always-mounted card out of its animated in-flow slot. The strip case already IS
      adjacent (the card opens directly above it). One real gap found, NOT implemented
      because it changes visible behaviour and contradicts the freshly-written R10:
      for channels 8/9/10 `follow-focus()` pins the card to the top of the pane and scrolls
      the edited row out of sight. See Decisions for the two cheaper options.
- [x] **T5 — Specs and tests brought level** DONE — commit `fd75057`. Both specs audited
      requirement by requirement; the false ones rewritten (the glow "addable empty state",
      the animation-leaf and rule-toggle editing surfaces that an earlier change had
      removed, R8's stop-count values, R9's literal index, R10's wording) and four new
      requirements added for genuinely new behaviour (containers, the glow switch, the
      glow fold, the label-sourcing contract). C was NOT written into the specs because it
      is not delivered. R3/R5/R11 still describe the shipped in-flow picker and were left.
- [x] **T6 — Close** DONE 2026-10-01: full suite `1201 passed / 0 failed`, `cargo build`
      with zero warnings, and a final headless render of the pane in its new shape read by
      the orchestrator and shown to the keeper (per-run dir `/tmp/opencode/final-t6/`).
      Every acceptance criterion below is either met or answered in writing.
      **The keeper then read the shipped pane LIVE and approved the look
      ("ha quedado muy chulo… me gusta", 2026-10-01)** — per this project's rules the
      keeper's own live test is the strongest check for visible behaviour, so the visual
      acceptance is closed by him, not by the renders. The two cosmetic items in the
      pending list (the wide switch-only glow row, and the rows sharing the container's
      fill) were NOT raised by him as problems; they stay recorded as optional, not as
      defects.

## Acceptance criteria

- The glow block is turned on and off by a switch that looks and behaves like the
  left preset cards' switch, with the same animation and colour language.
- No activated block appears instantly where its sibling animates.
- Whatever C delivers is either a picker anchored to the edited field or a written,
  evidence-backed statement of why not.
- A reader of the pane can tell, without guessing, where each block starts and ends.
- No block loses its description; no keyboard stop is lost or displaced.
- The two specs match the shipped UI.

## Decisions taken

- 2026-09-29 — Gradient angle control: left as is (see
  `border-tune-pane/gradient-angle-decision` in Engram).
- 2026-09-29 — Workflow: organic from now on; SDD only if the keeper asks and the
  preflight tooling is fixed.
- 2026-09-29 — Block delimitation (D): the keeper chose a CONTAINER PER BLOCK —
  each block inside a frame with its own surface (`bg-card`), a header with title
  plus description, so a reader always sees where a block starts and ends.
- 2026-09-29 — Free-tier models are unreachable in this runtime: the two free
  agents exist only under `agent`, not under the spawn registry `agents`, and
  forcing the free model onto a registered agent answers "OpenCode's free tier can
  only be used from within OpenCode". Work continues on the cheap Go lane
  (deepseek-v4-flash) with verification on a different family (glm-5.3-flash).

- 2026-09-30 — **T2 scope: the glow body fold only.** The keeper asked whether other
  blocks answer to the same "activating it shows extra parameters" system. Measured
  answer: in the whole of `ui/` the glow body is the ONLY switch-driven reveal
  (`BordersSection.slint:1170` is the sole `if root.<something>enabled` reveal in the
  tree). The colour strip and the +/− row are the same FAMILY but not the same gesture:
  they appear because a "+" was clicked and disappear when the list empties, and they
  sit behind the same `slot-count > 0` condition, so animating them touches the "add a
  first colour" path, not just the look. Deferred: decide it later, with a render in
  front of us. The picker card is excluded because it already animates.

## T3 slicing (container per block)

Inventory from a read-only pass: `tune-col` (`BordersSection.slint:374`) holds **29
first-level children** across 2128 lines. Four thin separators already exist
(`:531`, `:626`, `:977`, `:1157`). Some blocks already carry their own frame
(`size/radius/gap-in/gap-out/angle-wrap`, the compact colour rows, the picker card, the
glow header and its sliders, the save row and button); the rest are bare text or
loose controls. Only two headings exist today (`geometry-heading`, `active-colors-label`).

Natural containers, and the insertion points:

| Group | Wrap lines | Notes |
| --- | --- | --- |
| Geometry | before 386 → after 528 | contains the existing heading |
| Gradient angle | before 534 → after 602 | |
| Inactive colour | before 607 → after 623 | |
| Active colours | before 629 → after 895 | sliceable on its own |
| Slot add/remove | before 898 → after 974 | |
| Glow | before 988 → after 1154 | header row added in T1 |
| Save | before 1160 → after 1245 | |

Adding a heading is NOT a one-file edit: a new text property must be registered in
`ui/panel/sections/borders_text.slint`, `i18n/en.json`, `i18n/es.json`,
`src/panel_i18n.rs`, and `src/shell/ui_tests.rs`'s `borders_labels()` table — whose
fixed-size array and the source-text test `the_borders_section_reads_every_label_from_the_text_global`
both break if the table and the pane drift apart.

**Prerequisite found before touching geometry (the reason T3 is sliced).**
`BordersSection.slint` positioned the keyboard's scroll-follow with
`property <length> focus-y: root.local-focus * 96px;` — one hardcoded height per stop,
consumed by `follow-focus()`. It worked only because 96px happened to be the
real average. A header on every group raises that average, and the error compounds per
stop, so the last controls would start scrolling short. A magic constant is also exactly
what the keeper's agnostic norm forbids. T3a removed it (commit `9fe052f`); the slices are:

- [x] **T3a — make the scroll-follow honest.** DONE — commit `9fe052f`. The dead
  `focus-y: local-focus * 96px` property was deleted and `follow-focus()` now reads
  `focus-block-y()`, which maps each keyboard stop to the real laid-out `y` of the
  block that owns it; a stop with no mounted block returns `-1px` and the pane leaves
  the viewport alone instead of scrolling to a wrong place.
  Evidence: the old test (`borders_tune_focus_reaches_late_stops_from_real_geometry`)
  was a FALSE GREEN — it passed with the constant still wired — and was replaced by
  three invariant-based tests. Independently reproduced RED/GREEN on a different model
  (glm-5.3-flash): with the constant restored the two discriminating tests FAIL
  (`324136` panel pixels differ between two stops of the SAME block; the geometry stops
  land on three different screen y's), and pass with the derived geometry; md5s
  identical before/after the temporary patch. Full suite `1191 passed / 0 failed`,
  `cargo build` warning-free, neighbours and `picker_scale_animation_stays_declared_by_construction`
  green, the keyboard stop count and `glow-changed` untouched, engine files untouched,
  fresh per-run PNGs read by both models. Honest limits recorded below.
- [x] **T3b — the container pattern, one group.** DONE — commit `34ba472`. The Geometry
  group lives in one `geometry-group` container (the pane's own card idiom: `bg-card`
  fill, 1px `border` hairline, `radius-8`, `spacing-12` padding) with a header carrying
  the EXISTING `geometry-heading` title plus a new `geometry-desc` description, whose
  label is registered for real in all five points (`borders_text.slint`, `i18n/en.json`,
  `i18n/es.json`, `src/panel_i18n.rs`, the `borders_labels()` table) — EN and neutral ES,
  the Spanish frame proven rendered. `focus-block-y()` now sums the container's LIVE `y`
  for the four geometry stops; no stop was added, moved or reordered; `tune-count` and
  `borders_tune_stop_count()` untouched. Evidence: two new render tests RED first then
  GREEN, full suite `1195 passed / 0 failed`, build warning-free, PNGs read in both
  languages. Independent verification on a different model: PASS, no refutations, and it
  red-proved the pixel assertion itself (patched the description to invisible ink → FAIL).
- [ ] **T3c-1 — the three plain groups** (Gradient angle, Inactive colour, Save): no
  conditional children, so no geometry sum changes. Same pattern as T3b, several new
  labels.
- [ ] **T3c-2 — the three conditional-bearing groups** (Active colours [the strip], Slot
  add/remove [+/− row], Glow [the fold and its sliders]): each wraps a block that reports
  its own `y`, so every `focus-block-y()` sum must gain its container's LIVE `y`. One
  coherent change, one verification.

**Recorded residuals from T3b/T3a (not defects of T3b; tracked here so they stop living
only in a test comment).**

- **The walked-down centring bias (~16-17px), real and pre-existing.** `follow-focus()`
  reads the geometry synchronously on `changed local-focus`, when the row LEAVING focus is
  still lit at +16px. Measured on the T3b pane: 560 (read after walking down from the row
  above) vs 577 (fresh pane) for the same stop. Stepping down therefore parks every
  focused row ~17px above the centre; stepping UP is unbiased. Non-cumulative, cosmetic,
  but it contradicts the centring intent. T3b exposed it by adding a header (T3a's clamp
  had hidden it) and the honest fix (re-trigger `follow-focus` from the rows'
  `changed height`, whose final fire carries settled geometry) is a behaviour change of
  its own. **Untested and untracked until now**: no test covers the walked-down path.
  Candidate for its own slice.
- **The container's fill has no contrast against the rows inside it** (both `bg-card`,
  ratio 1.0). The container edge is carried by its 1px `border` hairline and the
  container-vs-pane contrast (~1.17:1). Verification measured that `bg-surface` would be
  WEAKER, so the real lever, if the keeper wants more separation, is making the rows
  transparent inside the container.
- **Never red-proven pixel assertions** in the new T3b test: `card > 1500` and
  `repaint > 60` (measured far above, directionally sound; the description assertion WAS
  red-proved by the verifier).

**More residuals recorded at T6 close (2026-10-01), none of them blockers.**

- `borders_tune_full_focus_reaches_last_and_middle` does NOT discriminate the Save stops'
  sum: with `save-group.y + save-wrap.y` removed it still passes, because its
  `count_icy_pixels > 200` is satisfied by the left pane's own ink. The new Save container
  test is the real discriminator. That older test is weaker than its comment suggests.
- The fold test's row reader (`glow_row_boxes`) is layout-DEPENDENT: it returns 0/2/4 on the
  shipped layout and 0/1/3 pre-container, because the enable row only falls inside its
  40..60px pair window once its title moved to the header. Corrected in `ff2846d`. Also, the
  settled count includes the ENABLE row's box (it enters via the focus flip), so the delta
  is 3 fold rows plus one focus-dependent box.
- `borders_strip_picker_opens_above_strip` can only run at a window of ~1250: below ~1120
  the chips are clipped, above ~1420 R10's "flush with the pane top" clause degenerates
  because `follow-focus()`'s picker branch bottom-pins content that does not overflow. That
  is PRE-EXISTING (the pre-rollout pane failed harder at 1440: card top 400 vs 263) and the
  card is still fully visible, so R10's intent holds; the window bound is a test artefact.
- The zero-colour pane is ~16px taller than before (two layout gaps from the gated
  containers); the gated containers paint no hollow card at zero colours (verified in frame).
- Two authored `beside` floors in the container tests (`> 150` strip, `> 3000` slots) are
  presence thresholds; the strip's pre-container value was measured 0, the slots row's was
  never measured. Documented in the assertion messages themselves.
- Budget: five commits of this document exceed the 400-line review heuristic (510, 604,
  1036, 1094, 2159 changed lines), most of it mechanical re-indentation of blocks into
  their containers and measured-mechanism prose. None was shrunk by deleting comments or
  tests. If the keeper wants smaller review units in future, slice by container, not by
  commit type.

**T3a — what the fix rests on, and its honest limits (2026-09-30).**

- The conditional blocks (strip, add/remove row, glow body and its sliders) cannot be
  referenced from outside their `if`, so each reports its own `y` through `init` (which
  *does* see the post-layout value) **plus** `changed y` for later reflows. `changed y`
  alone was NOT enough: it fires only on a change, never for the value an element is
  created with, so the strip sat at `-1px` for a whole session while mounted and the
  pane silently refused to scroll to that stop.
- Element `y` is parent-relative, so the strip's (picker-block-relative) and the glow
  sliders' (glow-body-relative) offsets are summed with the **live** parent `y` at call
  time. Pre-folding them would go stale when the picker card grows its 380px.
- `focus-centre-inset` (48px) remains a constant, named and justified: the only
  derivable source is the focused block's rendered height, and that height ANIMATES
  (250ms ease-in-out-back), so a derived inset would compute the target from a
  mid-flight value and make the scroll jump.
- The late glow stops and Save cannot discriminate old vs new: the ScrollView's bottom
  clamp (`height - content`) pins them under both rules, measured at the suite's
  1920×1080 fixture. The discrimination therefore comes from the glow-ENABLE row and
  the geometry stops. Outside that fixture this is unverified.
- Unconfirmed, stated so it is not mistaken for proof: the exact root-cause narrative
  for the `changed y` failure (that a real 16px move fired nothing) could not be
  reproduced independently. The shipped `init` + `changed` mechanism is correct under
  either reading, but the narrative is a hypothesis, not a measured fact. Also reasoned
  rather than exercised: the two-col instance flip re-running `init`, and window-resize
  reflow.
- Process deviation, recorded honestly: the commit is 510 authored changed lines, over
  the 400-line review budget. It was NOT shrunk by deleting the prose that explains the
  mechanism, and splitting it would separate the fix from the tests that prove it.

## Progress

- 2026-09-29 — Document opened. Exploration done (two read-only passes).
- 2026-09-29 — T1 closed and committed (`d3be1a3`). Suite 1183/1183, build clean,
  renders read by the writer and re-read independently by a second model.
- 2026-09-29 — Next: the container-per-block treatment (T3) and the animated fold
  (T2), both still pending the keeper's pick of order. The C feasibility study on
  the anchored picker (T4) has not run yet.
- 2026-09-30 — **T3a closed and committed (`9fe052f`)**: the scroll-follow now derives the
  focused block's real `y` instead of an index times a constant. The test that was
  supposed to prove it was a FALSE GREEN (passed with the constant wired) and was
  replaced; RED/GREEN reproduced independently on a different model. Suite
  `1191 passed / 0 failed`, build warning-free. Honest limits and the 510-line budget
  deviation recorded in the T3a section above. **No commit was made for T3b, T2 or T4.**
- 2026-09-30 — Next blocker to keep in mind: the fix is verified at the suite's 1920×1080
  fixture only. A `focus-centre-inset` constant (48px) remains, justified because the
  focused block's height animates.
- 2026-09-30 — **T2 closed** (`13e015c` fold, `ce2ea8d` fixes after verification): the glow
  body folds open instead of appearing at full size, and the OFF-state description folds
  too, in both directions. Cross-model verification found the fold PASS with two
  corrections (a false claim in a test comment, and an undisclosed instant appearance of
  the description when glow turns OFF); both fixed and spot-checked by the orchestrator on
  the frames. Full suite `1193 passed / 0 failed`. The colour strip stays OUT of T2 by the
  keeper's decision; it is T3c-2's business.
- 2026-09-30 — **T3b closed** (`34ba472`). Pattern proven; residual risk is the recorded
  walked-down 16px bias above.
- 2026-09-30 — Writer lane for this work is `space-bunny-free` (the keeper's choice) with
  verification on `glm-5.3-flash`. Its track record so far: one accidental `cargo fmt`
  over 54 files (fully reverted, nothing stray committed), one false claim in a test
  comment, one undisclosed behaviour change — all caught by cross-model verification, none
  by the writer itself. Watch it: prefer tight scopes and always verify.
- 2026-10-01 — **T3c-1 closed** (`85002c3`): Angle, Inactive and Save containers; the ONE
  new string (`save-desc`) registered in all five points. Three chip tests maintained
  (window only, teeth re-proved) and the fold test narrowed from the whole pane to the glow
  block's own span with an explicit delta assertion. Suite `1198 passed / 0 failed`.
- 2026-10-01 — **T3c-2 closed** (`b9ea81a`): Active colours, Slot add/remove and Glow
  containers; `slots-title` and `glow-desc` registered in all five points (table 63 rows);
  four sums gained their container's live `y`, each proved by removal. Suite
  `1201 passed / 0 failed`. Cross-model verification: PASS on what ships, with ONE
  refutation — the author claimed the replaced row reader returned the same counts on the
  pre-container layout (0/2/4); measured, it returns 0/1/3, because the enable row only
  falls inside the reader's 40..60px window once T3c-2 moved its title out. The claim lived
  in the commit message and in the docstring; the docstring was corrected by the
  orchestrator in `ff2846d` (the commit message stands as written — history is not
  rewritten).
- 2026-10-01 — **T4 closed as a study that REFUTES the anchored picker** (see the T4 entry).
- 2026-10-01 — **T5 closed** (`fd75057`): both specs levelled with the shipped pane.
- 2026-10-01 — **T6 closed**: suite `1201 passed / 0 failed`, build with zero warnings, and
  the pane's new shape rendered (`/tmp/opencode/final-t6/`) and read by the orchestrator.
  Every commit of this whole document is on `hve2-visual-rewrite`; nothing was pushed.

## Keeper decisions pending (nothing below is implemented)

1. **The glow enable row is now a wide, empty-looking row with only the switch on the
   right** (its title moved up into the container header). Cross-model verification called
   it "reads unlabelled". Visible in `/tmp/opencode/final-t6/borders_strip_flow_closed.png`.
   Options: leave it, or give the row a short inline label. Changing its height would touch
   the T1 ring window, so it is not free.
2. **Inside a container the rows share the container's fill** (`bg-card` on `bg-card`,
   contrast ratio 1.0): block boundaries are carried by the hairline only. The real lever
   is making the rows transparent inside the container — `bg-surface` was measured and is
   WORSE, so it is not the answer.
3. **C, the cheaper version.** The anchored picker is refuted, but the study found one real
   gap: for channels 8/9/10 (inactive, glow, glow-inactive) `follow-focus()` pins the
   picker card to the top of the pane and the edited row scrolls out of sight. A small
   change to that branch would keep the edited row visible next to the card. It was NOT
   implemented because it changes visible behaviour and contradicts R10 as T5 has just
   written it. The other option — wrapping the card and the strip in one `HorizontalLayout`
   so the card sits BESIDE the strip in flow — serves only the gradient chips.
4. **The walked-down ~16-17px centring bias** (recorded above): fix it in its own slice, or
   accept it. It is cosmetic, non-cumulative, and only affects stepping down.
5. **The two specs quote shipped constants** (250ms/150ms, 12px padding, `bg-card`), which
   can drift from the `.slint`. The tests, not the prose, are the guard.
