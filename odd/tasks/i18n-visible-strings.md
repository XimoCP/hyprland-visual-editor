# Translate the strings that are still English in the live UI

**Opened**: 2026-10-04 (keeper's call: "he visto partes que están en inglés y deberían traducirse")
**Branch**: hve2-visual-rewrite
**Route**: delegated writer per slice — MiMo (agent `general-free`); review by `jd-judge-a` (glm-5.3-flash)
**Status**: in progress — S1 done (`d487dc3`), S2/S3/S4 pending

## Objective

Finish the Spanish coverage of the live interface. `i18n/es.json` is already
complete and correct; the gap is that many live strings never consult it. After
this change, a keeper running in Spanish sees Spanish in Save, Filters, System
and Motion with no English left except brand names, glyphs and credits.

## What is already true (verified, do not re-derive)

- `i18n/en.json` and `i18n/es.json` hold the same 308 keys and the ES side is
  correctly translated. Presets are translated too (`scan.sh` emits i18n key
  paths; `src/presets.rs` resolves them).
- Borders is fully wired: `src/panel_i18n.rs::apply_borders` fills the
  `BordersText` global, read directly wherever the pane mounts.
- Panel chrome is wired: `apply_panel_chrome` fills `PanelText` (nav rail, esc
  hint, back to gallery).
- Motion is PARTIALLY wired: `apply_motion` fills only `MotionText`'s four
  controls (speed label, style label, three style buttons). The rest of the
  Motion chrome is hardcoded English — `src/panel_i18n.rs` says so in its own
  doc comment ("The existing hardcoded Motion chrome is out of scope").
- The Save dialogs ARE wired (`themes.*` keys, `src/main.rs:4680-4694`).

## The mechanism to follow (do not invent a new one)

`ui/panel/sections/borders_text.slint` + `MotionText` + `PanelText` are this
codebase's channel for section text: an exported Slint **global** holds the
strings with their English defaults, `src/panel_i18n.rs` fills it from `Tr`, and
the section reads it directly. That is why Borders could carry ~50 strings
without threading them through `MainWindow → ShellRoot → PanelRoot → section`.

Every slice below must use that same pattern for its section, with English in
the `.slint` global as the fallback and the Spanish in `i18n/es.json`. No new
mechanism, no per-string property threading.

## Work order (the exact inventory)

### Slice 1 — Save + Filters (this launch]

Hardcoded literals (no property backs them):

| File:line | String |
| --- | --- |
| `ui/panel/sections/SaveSection.slint:43` | `Save Current Look` |
| `ui/panel/sections/SaveSection.slint:50` | `Save your current setup as a theme. It appears instantly in the Gallery — no restart.` |
| `ui/panel/sections/SaveSection.slint:132` | `Tip: ↑↓ pick theme • Enter applies • Esc closes • Tab next section` |
| `ui/panel/sections/FiltersSection.slint:92` | `Filters` |
| `ui/panel/sections/FiltersSection.slint:99` | `Pick a shader — instantly applied. Repick active to turn off.` |
| `ui/panel/sections/FiltersSection.slint:167` | `No shader presets found.` |
| `ui/panel/sections/FiltersSection.slint:175` | `↑↓ navigate • Enter/Space apply • Tab pick→next • Esc back` |

Rust-side English literals that bypass `Tr`:

| File:line | String |
| --- | --- |
| `src/main.rs:4676` | `set_panel_save_placeholder("Theme name…")` |
| `src/main.rs:4677` | `set_panel_save_button_text("Save")` |

Both must go through `tr.tr_shared(...)` like the block right below them.

### Slice 2 — System / About

| File:line | String |
| --- | --- |
| `ui/components.slint:589-591` | `System Activation` / `Visual effects are safely managed by HVE.` / `System halted. Enable to start.` |
| `ui/panel/sections/SystemSection.slint:369` | `Restart` |
| `ui/panel/sections/SystemSection.slint:586` | `System` |
| `ui/panel/sections/SystemSection.slint:593` | `Activation, settings and About. Tiling stays as-is.` |
| `ui/shell.slint:287-296` | `System`, `Auto-minimize`, `Timer:`, `Language`, `Tiling mode`, `Autostart`, `Theme`, `Reset presets` |
| `ui/shell.slint:288-289` | `⚠ Restart required`, `Restart` |
| `ui/shell.slint:297-305` | `About HVE`, about short/full, `Project structure`, `View documentation` |

### Slice 3 — Motion chrome

| File:line | String |
| --- | --- |
| `ui/panel/sections/MotionSection.slint:349` | `Save as Preset` |
| `ui/panel/sections/MotionSection.slint:397` | `Save` |
| `ui/panel/sections/MotionSection.slint:429` | the Motion keyboard hint line |
| `ui/panel/sections/MotionSection.slint:528` | `Built-in` |
| `ui/panel/sections/MotionSection.slint:551` | `My Animation Presets` |
| `ui/panel/sections/MotionSection.slint:574` | `No animation presets found.` |
| `ui/panel/sections/MotionSection.slint:709` | `Motion` |
| `ui/panel/sections/MotionSection.slint:716` | `Shape the curve, then save as a preset. Or pick an existing animation.` |
| `ui/shell.slint:253-257` | animation rename/delete titles, placeholder, `Cancel` |

### Slice 4 — Gallery leftovers

| File:line | String |
| --- | --- |
| `ui/gallery/ThemeInfoPanel.slint:163` | `Saved ` prefix |
| `ui/main.slint:292` / `ui/shell.slint:114` | `No themes yet. Save your current setup as a theme.` (Rust feeds it from `gallery_slot.empty_message()`) |
| `ui/gallery/SliceDelegate.slint:453` | `VIEW  RETAG  DELETE  STEAM` — VERIFY FIRST whether this is user-visible; if it is not, leave it and say so. |

## Explicitly OUT of scope

- `ui/gallery/GalleryChrome.slint:59` `skwd-wall (MIT, © liixini)` — a required
  MIT credit, stays in English.
- `ui/shell.slint:425-458` the `H`/`yprland `/`V`/`isual `/`E`/`ditor` brand
  fragments — they compose the wordmark, not prose.
- Glyphs, numerals, `EN`/`ES`, `dark`/`light`/`system`, `p:...` color tokens,
  `default`/`slide` machine values, URLs.
- The ~109 dead keys of the removed old UI. This task does not clean them.
- Any geometry, layout, color or animation change. Text only.

## Acceptance criteria

- In Spanish, Save, Filters, System and Motion show no English prose. Brand
  names (`HVE`, `Hyprland`, `Noctalia`), glyphs, numerals and the MIT credit are
  the only English left.
- In English, every string reads exactly as it reads today (the English default
  in the `.slint` global must be byte-identical to the string it replaces).
- No `.slint` geometry changes: no new `x`/`y`/`width`/`height`, no layout
  edits, no property added beyond the text ones.
- Missing ES keys degrade to English, never to an empty label.

## Checks

- Strict TDD is ON. Runner: `cargo test`. RED FIRST: a table-driven test
  asserting, per wired string, that the ES value differs from the English one
  and that the English one still matches the `.slint` default. The Borders
  precedent is the one to copy (`src/shell/ui_tests.rs`, the
  "One row per user-visible `BordersText` property" table around line 16600).
- VISUAL: the change touches `.slint`. Render into a per-run directory
  (`HVE_RENDER_DIR=/tmp/opencode/render-i18n-<slice>`) and a human MUST read the
  PNGs. A UI change proved only by "tests green" is not verified here.
- `git commit` is NOT the writer's job for these slices: the repository's bash
  permission sets `git commit *` to `ask`, and the parent commits every slice.
- Do not reformat unrelated code; no blanket `cargo fmt`.

## Tasks

- [x] **S1 (RED+GREEN)** — Save + Filters wired through `save_text` /
  `filters_text` globals; the two `src/main.rs` literals routed through `Tr`.
- [x] **S1 (VISUAL)** — Render Save and Filters in Spanish, PNG read.
- [x] **S1 (COMMIT)** — one work-unit commit.
- [x] **S2** — System / About (commit `b8113df`).
- [ ] **S3** — Motion chrome.
- [ ] **S4** — Gallery leftovers.

## Evidence log

### S1 — Save + Filters (done)

Writer: `general-free` (MiMo V2.6 Flash Free). Commit
`d487dc3` (`feat(i18n): the Save and Filters panels speak Spanish`).

RED before GREEN, four assertions plus one compile error, all reproduced by
the writer and re-checked by the parent:

```
save_text.slint must exist for the Save/Filters i18n slice: No such file
main must call panel_i18n::apply_save(&window, &tr); exactly once, got 0
these labels are never read from their text global: ["save-title", "save-desc",
    "kbd-hint", "title", "desc", "empty", "kbd-hint"]
the English i18n map does not carry today's copy: ["panel.save.title: want
    \"Save Current Look\", got None", ... 7 more]
set_panel_save_placeholder must go through tr.tr_shared instead of a bare literal
error[E0425]: cannot find function `apply_filters` in module `crate::panel_i18n`
```

GREEN: `cargo test` — **1324 passed, 0 failed** (89.68s), run by the PARENT
after the writer's own run, not taken from the writer's report. Baseline before
this slice: 1311.

Keys: reused `themes.save_button` (`Save`) and `panel.nav.filters` (`Filters`)
because their English matched byte for byte; added `panel.save.title/desc/
kbd_hint/placeholder` and `panel.filters.desc/empty/kbd_hint`. The
near-duplicate `themes.save_placeholder` (`Theme name...`, three ASCII dots) was
deliberately NOT reused: it is not byte-identical to the `Theme name…` (U+2026)
it would replace.

VISUAL: `/tmp/opencode/render-i18n-slice1/`. Read by the parent with vision:
`panel_save_es.png` shows "Guardá tu configuración actual", the description and
"Consejo: ↑↓ elegir tema • …"; `panel_filters_es.png` shows "Filtros", its
description and its tip. Glyph integrity checked programmatically as well: the
seven new ES values keep every separator codepoint of their EN twin (U+2191,
U+2193, U+2022, U+2192, U+2014, U+2026) and the EN defaults are byte-identical
to the literals they replaced.

### S1 — second-model review

`jd-judge-a` (glm-5.3-flash — a different family from the writer's MiMo):
**VERDICT PASS**, claims 1-8 PASS, no BLOCKER/CRITICAL. It reported three
SUGGESTIONs and honestly marked claims 9 and 10 UNVERIFIABLE because that
agent has no shell.

- Suggestion 1 (fixed): the Save keyboard hint mixed Rioplatense voseo
  (`elegí`) with third-person verbs (`aplica`, `cierra`), while the sibling
  Filters hint added in the same change and the pre-existing `borders.kbd_hint`
  both use the infinitive. Aligned to the infinitive.
- Suggestion 2 (accepted, not fixed): `the_save_and_filters_text_defaults_are_
  todays_english` asserts a raw substring, so it would still pass if a
  declaration were commented out. Cheap to strengthen; recorded as a follow-up
  rather than widening this slice.
- Suggestion 3 (accepted, not fixed): the label assertions read the globals
  rather than the sections, so section-side binding rests on source-text checks
  — but the RENDER test does prove the sections paint Spanish, which is the
  check that matters.
- Claims 9 and 10 were verified by the PARENT instead: `git status --short`
  shows exactly the ten declared files (8 modified, 2 new) plus this ficha, and
  the full suite run above is the count of record.

### Deviations / tensions observed

- The register fix above invalidated the expectation the writer had pinned at
  `src/shell/ui_tests.rs:17267`, so that one line was updated. Reported here
  because a test changed under me is exactly the kind of edit that must not
  pass silently: it encoded the wording I deliberately replaced, nothing else.
- The two `src/main.rs` literals are covered by a source-text test and by the
  key table, but NOT visually: the render harness does not run `main()`'s
  startup wiring, so the Save button and the placeholder still paint English in
  these PNGs. The Spanish arrives at runtime through `tr.tr_shared`, and the
  judge confirmed both lines run at startup (`src/main.rs:4753-4754`, inside
  `main()` which begins at 2664).

### S2 — System / About (done)

Writer: `general-free` (MiMo). Commit `b8113df`
(`feat(i18n): the System and About panels speak Spanish`).

New global `ui/panel/sections/system_text.slint` (`SystemText`, 21 string
properties), filled by `panel_i18n::apply_system`, read by `SystemSection.slint`
and by the activation card in `ui/components.slint`.

Keys: 16 reused because their English matched byte for byte (`panel.nav.system`,
`settings.restart_banner`, `settings.restart_button`, `settings.auto_minimize`,
`settings.timer`, `settings.language`, `settings.tiling_mode`, `settings.autostart`,
`settings.theme`, `settings.reset_presets`, `home.about_title`,
`home.about_tree_label`, `home.about_docs`, `welcome.activation_title`,
`welcome.toast.enabled`, `welcome.toast.disabled`); 5 added
(`panel.system.desc`, `panel.system.about_short`, `panel.system.about_full`,
`panel.system.activation_desc_active`, `panel.system.activation_desc_inactive`).
`settings.title` deliberately NOT reused: it says "Settings", not "System".

The writer corrected this document twice, and both corrections stand:

- The 15 `shell.slint` `panel-*` System properties were ALREADY fed from `tr` by
  existing `main.rs` setters. This document's "Work order" claimed none of them
  was; that claim came from a flawed parent grep and is wrong.
- The activation card INSTANCE inside `SystemSection.slint` overrides the
  `components.slint` defaults, so wiring only the listed defaults would have
  left the painted text English. The inventory had missed it.

GREEN: `cargo test` — **1332 passed, 0 failed** (90.08s), run by the PARENT
after the parent's strengthening of `test_system_rows_render`.

VISUAL: `/tmp/opencode/render-i18n-slice2/`. Read by the parent with vision:
`panel_system_es.png` shows "Sistema", "Activación, ajustes y Acerca de. El modo
tiling se mantiene como está.", "Activación del Sistema", "Editor Visual
Habilitado", "Retardo al ocultar", "Idioma", "Inicio automático", "Tema",
"Acerca de HVE"; `panel_system_about_es.png` shows "Estructura del proyecto".

Second-model review (`jd-judge-a`, glm-5.3-flash): **VERDICT PASS**, all ten
claims PASS, no BLOCKER, scope confirmed as the nine declared files, D1-D5 all
judged correct. It raised one WARNING and three SUGGESTIONs; the WARNING was
fixed by the parent before the commit:

- WARNING (fixed): the writer's D3 had WEAKENED `test_system_rows_render` — it
  accepted a needle found in EITHER the section or its text global, and the
  global contains all five needles, so deleting a whole row would have stayed
  green. The guard now asserts the row's binding to the global
  (`autostart-label:`, `theme-label:`, `reset-label:`, `restart-button-text:`,
  `about-title:`), which disappears when a row is deleted.
- SUGGESTION (accepted, not fixed): four of the new keys never reach a pixel —
  `panel.system.about_short` / `about_full` are overridden by `home.about_*`
  (via `PanelRoot.slint`), and `activation_desc_*` are read only by the
  component defaults the instance overrides. No user impact; recorded below.
- SUGGESTION (accepted, same as S1): the English-default test asserts a raw
  substring, so it would still pass if a declaration were commented out.
- SUGGESTION (pre-existing, real): `ui/components.slint:632` paints a literal
  `ON` / `OFF`, so a Spanish user reads two English words in the activation card.

## Follow-ups recorded by this task (none blocking)

1. `ui/panel/sections/SystemSection.slint` settings-pane keyboard hint
   (`↑↓ navigate • ←→ switch panel • Enter/Space toggle • Esc back`) is still
   English. Not in any slice's inventory; needs two keys + infinitive Spanish.
2. `ui/components.slint:632` literal `ON` / `OFF` — user-visible English.
3. Four dead new keys from S2 (see above): either delete them or make them the
   real source for About and the card descriptions.
4. `SystemSection`'s `settings-title` property is dead (nothing reads it) while
   the chain still delivers into it; a text-only slice cannot remove it.
5. `ui/shell.slint:290-291` still defaults to `Auto-minimize` / `Timer:`, now at
   odds with `SystemText`'s `Hide delay` / `Delay:`. Both are dead (always
   overridden), so no painted text disagrees — but the inconsistency is real.
6. The substring-based English-default tests in S1 and S2 would be better
   asserting the property NAME, not a substring.
