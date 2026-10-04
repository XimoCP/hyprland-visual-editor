# An animation style Hyprland rejects: fade on windowsIn / windowsOut

**Opened**: 2026-10-04 (keeper's report, with Hyprland's own error on screen)
**Branch**: hve2-visual-rewrite
**Route**: delegated writer — MiMo (agent `general-free`); review by `jd-judge-a` (glm-5.3-flash)
**Status**: done (fixed, tests green, second-model reviewed, correction applied; commit recorded below)

## The report

The keeper saved an animation preset ("prueba") after picking the **Fundido**
(Fade) style in the Motion panel. Hyprland then refused the config:

```
Your config has errors:
/home/ximo/.cache/hve/overlay.lua:30: hl.animation("windowsIn"): unknown style
/home/ximo/.cache/hve/overlay.lua:31: hl.animation("windowsOut"): unknown style
```

Those two lines read `style = "fade"` in the assembled fragment.

## Root cause (verified, with evidence)

`src/animation_preset.rs::style_for_leaf` translates the chosen style per
animation leaf, because Hyprland validates the style **per leaf** and one global
family cannot be copied verbatim. Two leaves were already corrected this way,
each after a live rejection:

- `windowsMove` → always `"slide"` (the comment records that all 19 built-in
  presets use `slide` there).
- `workspaces` → `Popin` falls back to `"fade"` (the comment records that
  `workspaces` rejects popin).

`windowsIn` / `windowsOut` fall through to `_ => style.as_lua()`, whose comment
claims they "accept the chosen family". That claim is FALSE for `fade`:
Hyprland rejects it, exactly as the keeper's error shows.

The evidence that `fade` is not a windows style, independent of the error
message itself: none of the 19 shipped presets in `assets/animations/*.lua`
uses `fade` on `windowsIn` or `windowsOut`. They use `popin N%`, `slide` and
`slidevert` there. `fade` appears only on the `fade` leaf, on `layers*`, and on
`workspaces`.

## Why the existing tests could not catch it

`allowed_styles_for` — the table the test asserts against — is written next to
the generator and encodes the same assumption the generator makes. The test
`generate_emits_a_style_valid_for_each_leaf` therefore asks "is what I emitted
in my own table?", which can only ever answer yes. It is circular: a wrong table
entry is invisible to it by construction. This is why `windowsMove` and
`workspaces` are correct (fixed from live rejections) while windows is not.

## Scope

Single file: **`src/animation_preset.rs`**. No UI, no JSON, no scripts.

## Decisions

1. **Fade on windows becomes `popin 100%`** (keeper's choice). It is what the
   shipped presets already do for the faded look — "Faded"
   (`assets/animations/15_desvanecido.lua`) uses `popin 90%` / `popin 95%` on
   the two windows leaves — so the visual result is the one already expected,
   and Hyprland accepts the family.
2. **`fade` must never be emitted on `windowsIn` / `windowsOut` again.** That is
   the regression this task has to prevent, not just the one file it repairs.
3. `windowsMove` stays `slide`; `workspaces` keeps its popin → fade fallback.
   Both are already correct and their behaviour must not move.
4. **The circular test gets an independent source of truth.** The validator must
   stop sharing its assumptions with the generator. The independent evidence
   available in-repo is the shipped preset corpus: `assets/animations/*.lua` are
   presets Hyprland already accepts, so the style *family* HVE emits for a leaf
   must be a family those presets use on that same leaf.
5. **Anything the corpus cannot confirm must be named, not blessed.** If HVE can
   emit a family on a leaf that no shipped preset uses there, the test must
   carry it as an explicit, commented exception marked as unconfirmed against
   live Hyprland — never silently allowed.

## Known limitation, must be stated (not a defect of this fix)

`assets/scripts/apply_animation.sh` copies the preset file verbatim into the
fragment (`cat "$TARGET_FILE" > "$TARGET_FRAGMENT"`). Fixing the generator
therefore does NOT repair presets already saved with an invalid style. The
keeper's `prueba.lua` keeps its `style = "fade"` until it is re-saved through
the panel (which regenerates it from the fixed generator) or the file is
repaired by hand. The desktop stays in its config-error state until then.

## Acceptance criteria

- With the Fade style, `windowsIn` and `windowsOut` emit `popin 100%`, and the
  emitted fragment contains no bare `fade` on a windows leaf.
- The other three styles are unchanged: Slide → `slide`, Popin → `popin 80%`,
  SlideFade → `slidefade 20%` on windows; `windowsMove` always `slide`;
  `workspaces` never `popin`.
- A test fails if anyone re-introduces `fade` on a windows leaf.
- The per-leaf validator is no longer sourced from the generator's own mapping.
- `cargo test` green.

## Checks

- Strict TDD, runner `cargo test`. RED FIRST: paste the failing run. The RED must
  be an assertion failure that names the offending leaf and value, not a compile
  error, if it can be arranged.
- Do NOT run git write commands; the parent commits.
- Do NOT run `cargo fmt`.
- Report anything the shipped corpus cannot settle, as a named uncertainty.

## Tasks

- [x] **T1 (RED)** — Failing test(s): Fade on windowsIn/windowsOut is not `fade`;
  the emitted families per leaf appear in the shipped corpus.
- [x] **T2 (GREEN)** — `style_for_leaf` windows arm; `allowed_styles_for`
  corrected so it no longer claims `fade` for windows.
- [x] **T3 (REVIEW)** — second model over the diff.
- [x] **T4 (COMMIT)** — one work-unit commit.

## Evidence log

### T1 — RED (writer's real output, before the fix)

```
every_emitted_family_is_used_by_the_shipped_corpus_on_the_same_leaf ... FAILED
styles Hyprland may reject:
chosen style Fade emits "fade" (family "fade") on leaf "windowsIn", but no
  preset in assets/animations uses that family on that leaf
  — corpus families for "windowsIn": {"popin", "slide", "slidevert"}
chosen style Fade emits "fade" (family "fade") on leaf "windowsOut", ... same
generate_emits_a_style_valid_for_each_leaf ... FAILED
style "fade" is invalid for leaf "windowsIn" (style Fade)
test result: FAILED. 7 passed; 2 failed
```

### T2 — change

`src/animation_preset.rs` only. `style_for_leaf` gained an explicit
`"windowsIn" | "windowsOut"` arm: `Fade` → `popin 100%`, the other three styles
unchanged. `windowsMove` → `slide` and `workspaces` popin → `fade` untouched.
`allowed_styles_for` no longer claims `fade` for the windows leaves.

### GREEN

`cargo test` — **1326 passed, 0 failed** (89.32s), run by the PARENT. Test count
reconciles: HEAD had 8 `#[test]` in this file, the writer added 1, the parent
added 1, and the file now has 10.

### T3 — second-model review

`jd-judge-a` (glm-5.3-flash): **VERDICT PASS**, all ten claims PASS except two it
honestly marked UNVERIFIABLE (it has no shell) — the parent covered those:
`git status --short` showed only this file plus the ficha, and the suite run
above is the count of record.

It raised one WARNING that mattered, against the writer's own out-of-spec
change: `parse` keyed the Fade witness on the comment `-- @Source: user`. A
keeper who strips that line from a saved preset would get `Popin` back in the
pane, and the next save would write `popin 80%`, silently changing the
animation. The judge also pointed out a structural witness already present in
the output: `generate(Fade)` puts `popin 100%` on BOTH windows leaves while
`generate(Popin)` puts `popin 80%` on both.

**Correction applied** (parent, inline — one file, already understood): the
witness is now the emitted pair, never a comment, and
`the_fade_witness_survives_a_stripped_source_comment` fails if anyone goes back
to a marker-based witness. A comment the keeper can delete must not decide
behaviour.

### T4 — commit

Hash: `98aad5c` (`fix(animations): the Fade style stops emitting a style
Hyprland rejects`). `src/animation_preset.rs` alone; the ficha follows in a
`docs(odd)` commit. No AI attribution.

## Deviations / tensions observed

- **The writer changed `parse`, which was outside the written spec.** It flagged
  this itself rather than burying it. Cause: the fix made the round trip lossy
  (`parse(generate(Fade))` resolved to `Popin`). Reviewed by the second model and
  then corrected to the structural witness described above.
- **The parent's own edit errors, found and fixed before the commit.** While
  repairing a formatting slip introduced by a bad edit, the parent left an
  orphan duplicate `#[test]` above the new test, and a first repair attempt added
  a third. Caught by counting `#[test]` against HEAD instead of trusting the
  green suite: the suite reported 1327 passed with the duplicate, 1326 after the
  cleanup, and HEAD had 8 against the 10 that belong. Recorded because "tests
  green" hid a real defect in the diff for one run.
- **`slidefade` on the windows leaves is UNCONFIRMED and unchanged.** No shipped
  preset uses it on `windowsIn`/`windowsOut`, so the corpus cannot vouch for it;
  the judge confirmed it is correctly listed in the test's
  `CORPUS_UNCONFIRMED`. It is the same class of bug as the reported one and it
  is still unproven either way — a keeper applying "Deslizar + Fundido" and
  saving would settle it in seconds. Left unchanged on purpose: it is
  pre-existing behaviour, not the reported defect.
- **The existing broken preset is NOT repaired by this fix.**
  `assets/scripts/apply_animation.sh` copies the preset file verbatim into the
  fragment (`cat "$TARGET_FILE" > "$TARGET_FRAGMENT"`), so `prueba.lua` keeps
  its `style = "fade"` until it is re-saved through the panel. The desktop stays
  in its config-error state until then. Stated, not hidden: the fix prevents new
  broken presets, it does not heal old ones.
- `popin 100%` has no corpus instance on `windowsIn` (the corpus shows 70–95%
  there; `11_futurista.lua` uses `popin 100%` on `windowsOut`). Hyprland
  validates the style NAME per leaf, and the family `popin` is confirmed on both
  windows leaves, so the judge judged this covered rather than excepted.
