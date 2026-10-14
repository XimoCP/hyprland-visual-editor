# Trust warning i18n (HVE)

Branch `hve2-visual-rewrite`. Test runner `cargo test`. The gate's copy is a VISUAL surface:
its render test is `theme_trust_warning_renders_over_the_gallery_and_over_the_panel` and the frames must be
read by eye (`HVE_RENDER_DIR=<dir> cargo test --bin hve <test> -- --nocapture`).
Delivery: one work-unit commit (i18n + the detector's causes + the wiring + tests + the record).

## Specs

- **S1 — every string the gate shows that HVE writes is translated.** The twelve copy strings of
  `ui/gallery/TrustWarning.slint` (title, intro with the theme name, the two carrier headings with their counts, the
  two carrier descriptions, the remembered paragraph, the not-remembered paragraph, the "not a sandbox" limit, the
  keyboard hint, Cancel, Continue) plus the four `PresetBody::Unreadable` causes that ride on the preset lines.
  DATA stays data and is never translated: the theme name, the hook `key — command` lines and the
  `category/file.lua` names.
- **S2 — the honesty contract survives translation.** The Spanish copy must keep, claim by claim: that this is a
  consent gate and NOT a sandbox (nothing is filtered or blocked), that HVE installs the theme unchanged, that
  Continue is for themes whose origin the keeper trusts, that the code runs with his own permissions, and the
  remembered/not-remembered distinction (including the reason a package cannot be remembered). The test that pinned
  those claims by reading the ENGLISH text inside the `.slint` moves to the i18n files, which is where the copy now
  lives.
- **S3 — the language follows the app's setting, not the system locale.** The gate is translated with the language
  the app was started with (`cfg.language` → the window's `language` property), so the same strings the rest of the
  UI already uses.
- **S4 — no English left in the gate.** A test proves the `.slint` holds no literal copy, STRUCTURALLY — every `text:`
  binding must read a property (or the loop variable), so any sentence smuggled back into a `Text` fails, not just the
  ones a phrase list happens to name — and that every key the gate needs exists in BOTH `i18n/en.json` and
  `i18n/es.json`. Known limit: the check strips `//` lines, so a literal hidden in a comment is invisible to it (it is
  never painted). The `{error}` an unreadable preset carries is the reader's own diagnostic and stays in the system's
  language, by design — the copy around it is translated.
- **S5 — nothing else changes.** The gate's geometry, its keyboard (Cancel first, Escape declines), both gated apply
  paths and the acceptance store behave exactly as before.
- **S6 — the causes become structured data.** `PresetBody::Unreadable(String)` carries pre-formatted English
  sentences today; it becomes a small cause enum that the UI formats in the keeper's language. The distinction the
  module insists on — "was not read" vs "was read but could not be KEPT" — must survive, and the tests that asserted
  English substrings assert the variant instead.
- **S7 — checks.** Full suite green, `cargo check --all-targets` clean, clippy without new warnings, `cargo fmt`
  clean, and a headless render of the gate IN SPANISH read by eye.
- **S8 — the record.** The `.slint` header and this document state where the copy lives now and why the honesty
  claims are pinned in the i18n files.

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1-S8 | inline (high risk: the consent gate's copy; independent verifier after) | DONE, all checks green; independent verification under way | `i18n(trust): the consent gate speaks the app's language` |

### T1 evidence

- **RED (runtime, on the real contract)**: the rewritten pin test
  (`trust_gate_copy_lives_in_the_i18n_files_and_keeps_the_honest_limit`) failed against the untouched component with
  `the gate's copy must live in i18n/{en,es}.json and reach the UI as a property — found "This theme carries code" in
  ui/gallery/TrustWarning.slint`.
- **GREEN**: `cargo test --bin hve trust` → **87 passed / 0 failed** (measured serially), including the new
  `theme_trust_warning_speaks_the_app_language`, which drives the REAL path (`trust_gate_request`) twice on the same
  package and checks the title, both buttons, the filled `{theme}`/`{count}` placeholders and that all twelve copy
  properties are non-empty in `en` and in `es`.
- **Four existing pins had to move, and each one moved to where the copy now lives**: the honesty pin now reads the
  i18n files in both languages (and forbids prevention claims in both); the keyboard pin checks `trust.kbd_hint` in
  both; the render test fills the copy through the SAME function the app uses (`crate::apply_trust_warning_copy`), in
  Spanish; and the two cause assertions in `theme_trust` now assert the VARIANT, each in the fixture that can prove
  it (`BudgetSpent` where the budget is spent before the file is opened, `TooLarge`/`OverBudget` where the file was
  read and could not be kept).

### Final checks (on the exact bytes committed)

- Full suite: **1788 passed / 0 failed / 0 ignored** in 164.71 s — it was 1786 before this unit, and the +2 are this
  unit's own tests (`theme_trust_warning_speaks_the_app_language` and `tr::tests::tr_or_en_falls_back_to_the_english_text_of_the_same_key`);
  the honesty pin was rewritten, not added.
- `cargo check --all-targets` clean; `cargo clippy --all-targets` **67 warnings, the same count as before this unit**;
  `cargo fmt --check` clean.
- Frames: `HVE_RENDER_DIR=/tmp/opencode/render-verify-trust-i18n-2 …`, read by eye (see below).

### The architecture pin caught a real duplication (and what was done)

The first full-suite run after the change failed **one** test:
`architecture_contract::architecture_pins_hold_or_improve` →
`NEW COUPLING: src/main.rs mentions hyprland 13x (pin 12)`. The extra mention was the ENGLISH fallback I had
written into `main.rs` for the presets line ("…read by Hyprland when it assembles the overlay") — a Rust literal
DUPLICATING a string that already lives in `i18n/en.json`.

The pin was not touched: raising it would have weakened a routing guard to accommodate a copy. Instead the
duplication was removed at its root, which is the better fix anyway: `Tr` gained `tr_or_en(key)`, the MIDDLE step of
its own documented chain (selected → **en** → raw key), backed by a `OnceLock`-cached parse of the English map, and
every literal in the gate's copy block was deleted. So `src/main.rs` now carries no copy at all — the i18n files are
the single source — and the pin stays at 12. New test `tr::tests::tr_or_en_falls_back_to_the_english_text_of_the_same_key`
covers all three steps: the active language wins, a key missing there resolves to its English text, and a key missing
from BOTH resolves to the key itself (visible on screen, never blank).


### Independent verification (c896f7d, `opencode-go/mimo-v2.6-flash`)

- **Verifier**: a different model from the writer (`deepseek-v4.1-flash`), fresh subagent, explicitly asked to refute.
- **Verdicts**: S1, S2, S3, S5, S6 and S7 SOUND — it re-measured the suite (1788/0), clippy (67, no warning in the
  changed hunks), fmt, reproduced the render run and read five frames with its own eyes, and checked each cause
  variant against the constant that actually stops it (`TooLarge` → the 16 KiB per-preset bound; `BudgetSpent` /
  `OverBudget` → the 128 KiB theme budget; the "was read / was not read" distinction intact).
- **ONE BLOCKER, and it was right**: S4's claim was stronger than the test. The pin listed eight fixed phrases, so
  ANY other English sentence could return to the component unnoticed — the verifier proved it by injecting the old
  intro sentence into a `Text` and watching the whole trust suite stay green. **Fixed** by making the check
  STRUCTURAL (every `text:` binding must read a property; a literal fails), and the fix was proved by re-running the
  verifier's exact injection: it now fails with `a text: binding may not hold a string literal … Offending binding:
  text: "HVE installs it unchanged when the theme is applied"`, and the component was restored to green afterwards.
- **Advisories folded in**: the `tr_or_en` panic text was inaccurate (a missing key falls back to the English TEXT,
  a raw key only if English lost it too); the render fixture left stale carrier COUNTS in the headings of two frames
  (a frame saying "Hooks de shell (2)" over one line misleads the eye that verifies it — the copy is now refilled per
  frame); the S6 number was corrected to the serially measured 87. Left as documented, deliberate limits: the `{error}`
  inside an unreadable cause is the reader's own diagnostic (system language), and the structural check strips `//`
  lines (a commented literal is never painted).
- **Could not verify**: live GUI behaviour on the keeper's desktop (his own check), and a baseline clippy rebuild.


### The visual verification, and the defect it caught (S7)

Frames: `HVE_RENDER_DIR=/tmp/opencode/render-verify-trust-i18n-2 cargo test --bin hve
theme_trust_warning_renders_over_the_gallery_and_over_the_panel -- --nocapture`, then read by eye. Four frames were
inspected (gate open, not-remembered, unreadable preset, many carriers): the whole gate paints in Spanish, long
sentences wrap inside the card, the buttons stay at their fixed geometry, and nothing is clipped.

**The frame caught a real defect that no assertion had**: the "unreadable preset" frame painted its cause sentence in
ENGLISH, because the render test HAND-WROTE that line as a fixture instead of building it with the code under test —
so the test was decorating a copy of the old behaviour and could never have seen the translation at all. Fixed by
building the line through the real `crate::trust_warning_lines` with a synthetic unreadable report, which also adds
assertions the fixture could not make: the line says `no se pudo RETENER` and never `KEPT`, and it still names the
file. Lesson: a render test that fabricates the very string it is supposed to verify verifies nothing about it.

## Log

- **L1 (2026-10-14, the keeper, verbatim)**: `vale he visto una cosa... y es que me ha saltado el aviso pro primera
  vez cuando he puesto el tema Cars... cosa que se supone que estaba previsto... y que para que no pase solo tenía
  que cargar y volver a guardar el tema otra vez... me equivoco. Bueno pero te escribo por que ese aviso no se ha
  traducido... y es algo que se necesita.`
- **L2 (2026-10-14, the parent, answer to the Cars question — verified, not guessed)**: the warning on Cars was
  expected: the theme carries THREE shell hooks (`colors_changed`, `shutdown`, `started`, all running his SDDM sync
  script), and a package that carries code is never applied in silence. His belief that "loading and re-saving the
  theme" silences the warning is the OPPOSITE of the rule: the acceptance is remembered only when Continue is
  pressed, keyed to the exact content that runs (hook commands + preset bodies verbatim, S1/S3 of
  `odd/tasks/theme-package-trust.md`), so re-saving a theme — which rewrites that content — is precisely what brings
  the warning BACK. Measured evidence: `~/.cache/hve/theme-trust.toml` was written at **18:18:23 today** and already
  holds `[accepted.Cars]` with its three hooks, i.e. it records the Continue he pressed minutes before writing this
  message (`ls`/`stat` + the file's contents read, read-only).
- **L3 (2026-10-14, the parent, scope decision)**: the gate's copy is hardcoded English in the `.slint` (eleven
  strings) plus four English cause sentences produced by the detector. Translating only the copy would leave English
  sentences inside a Spanish consent gate, which is the defect he reported, so the causes are included (S6). The
  language source is the window's `language` property (the app's setting), NOT `detect_language()`, which would
  ignore his choice. Spanish register: the project's existing `i18n/es.json` uses Rioplatense voseo consistently, so
  the new keys follow that corpus instead of importing a neutral register into the same file.
