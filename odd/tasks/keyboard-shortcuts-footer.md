# Keyboard shortcuts in the bottom bar (first look)

**Opened**: 2026-10-04 (keeper: "poner en la parte de abajo de HVE todos los atajos de teclado a ver como queda")
**Branch**: hve2-visual-rewrite
**Route**: delegated writer, sequential AFTER the Ctrl-only drawer change
**Status**: delivered (probe) — commit `476c80a`; awaiting keeper review of wording/scope

## Objective

Show the keyboard shortcuts in HVE's bottom bar, so the keys are discoverable
instead of only hinted per section. The keeper wants to SEE how it looks before
polishing wording or scope.

## Where it goes

The bottom bar already exists and is always visible: a 32px `Rectangle` at the
end of the shell layout, `ui/shell.slint:805-821`, which today paints only
`root.back-hint` ("Esc hides", set from `src/main.rs:2743`). That bar is the
target — extend it, do not invent a new strip.

## Content: the REAL keymap, context-sensitive

A single global list would show panel keys while you are on Home. The first
version shows the keys for the CURRENT context, assembled from what the code
actually does (evidence, not invention):

- **Home** — `ui/shell.slint:842-920` (`shell-kbd`, the non-gallery arms).
- **Gallery / Slider** — the same handler: Left/Right move the carousel, Enter
  applies, `i` opens the theme info face, Ctrl+Up/Down deploy the drawers,
  Escape hides the window to the tray (same as Home).
- **Gallery / Mosaic** — Left/Right/Up/Down move the cursor, Enter grows the
  tile, a second Enter applies it, Ctrl+Up/Down deploy the drawers, Escape
  hides the window to the tray (same as Home).
- **Panel (Settings)** — `ui/panel/PanelRoot.slint` `panel-kbd` (from line 366):
  Up/Down move, Left hands focus to the rail, Right/Enter re-enters a section,
  Enter engages a slider, Escape back.

Layout: ONE line, size-12, `·` separators, `clip: true`, in the existing bar.
Keep `Esc hides` as the first item or merge it in — the writer's call, stated in
the report.

## i18n (mandatory, do not hardcode English in the .slint)

The project embeds `i18n/en.json` and `i18n/es.json` at compile time
(`src/tr.rs:6-7`) and resolves dot-notation keys with an English fallback
(`Tr::tr_shared`, and `Tr::tr_array` for lists). Add keys under
`shell.shortcuts.*` in BOTH files, in neutral professional Spanish for `es.json`,
and set the strings from Rust next to the existing
`window.set_shell_back_hint(...)` (`src/main.rs:2743`). Do not put English text
literals into `ui/shell.slint`.

## Scope

- `ui/shell.slint` — the bottom bar rendering.
- `ui/main.slint` — only if a property/callback has to be forwarded.
- `i18n/en.json`, `i18n/es.json` — the new keys.
- `src/main.rs` — setting the properties (and any small helper it needs).
- Tests in `src/shell/ui_tests.rs`.
- Do NOT touch the gallery, the panel sections, or the drawer behaviour (the
  previous task owns those).

## Acceptance criteria

- The bottom bar paints the shortcut list for the current context, in the active
  language, with no clipped mid-word text and no overflow past the window.
- Home, Slider, Mosaic and Panel each show their own set; switching context
  changes the list without a stale value left behind.
- Nothing else in the bottom bar changes: the bar's height, background and the
  `Esc hides` affordance stay as they are unless the writer explains why.

## Checks

- Strict TDD is ON. Runner: `cargo test`. Write the failing test FIRST (assert
  the bar's text property/content for a given context, and that the key list is
  non-empty and translated — not a hardcoded English literal).
- VISUAL (mandatory): render into a per-run directory and READ the PNG for at
  least TWO contexts (Slider and Panel), confirming with your own eyes that the
  list fits, is legible, and does not collide with the window edge or the MIT
  footer. Report the paths and what you saw.
- Do not reformat unrelated code; no blanket `cargo fmt`.

## Tasks

- [x] **T1 (RED)** — Failing test for the context-sensitive, translated list.
- [x] **T2 (GREEN)** — i18n keys + `main.rs` wiring + the bottom bar rendering.
- [x] **T3 (VISUAL)** — Render and read the PNGs of at least two contexts.
- [x] **T4 (COMMIT)** — One Conventional Commit; record the hash here.

## Open for the keeper's next pass (deliberately not decided here)

- Final wording and ordering of each hint.
- Whether every key belongs in the bar or only the frequent ones.
- Behaviour at narrow window widths (today: one clipped line).

## Evidence log

**T1 (RED) — on the pre-change head (`7841d16`), two stages**

1. Translation keys missing (runnable RED):

```
$ cargo test test_shell_shortcut_keys
test tr::tests::test_shell_shortcut_keys_resolve_in_english ... FAILED
test tr::tests::test_shell_shortcut_keys_resolve_in_spanish ... FAILED
panicked at src/tr.rs:257: assertion `left == right` failed
  left: None
 right: Some("←→ move · Enter open · Esc hides")
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 1307 filtered out
```

2. The bar has no shortcut property yet (compile RED):

```
$ cargo test shell_bottom_bar_shows_context_shortcuts
error[E0599]: no method named `get_shell_shortcuts` found for struct `MainWindow`
error[E0599]: no method named `set_shell_shortcuts_home` found for struct `MainWindow`
error: could not compile `hve` (bin "hve" test) due to 9 previous errors
```

**T2 (GREEN) — after i18n + wiring + rendering**

```
$ cargo test shell_shortcut
test tr::tests::test_shell_shortcut_keys_resolve_in_spanish ... ok
test tr::tests::test_shell_shortcut_keys_resolve_in_english ... ok
test result: ok. 2 passed; 0 failed

$ cargo test shell_bottom_bar_shows_context_shortcuts
test shell::ui_tests::shell_bottom_bar_shows_context_shortcuts ... ok
test result: ok. 1 passed; 0 failed
```

**T3 (VISUAL) — headless render, per-run directory**

```
$ HVE_RENDER_DIR=/tmp/opencode/render-shortcuts-20261004 cargo test shell_shortcuts_bar_renders -- --nocapture
render artifacts for this run: /tmp/opencode/render-shortcuts-20261004
test shell::ui_tests::shell_shortcuts_bar_renders ... ok
```

Frames: `shortcuts_bar_home.png`, `shortcuts_bar_slider.png`,
`shortcuts_bar_mosaic.png`, `shortcuts_bar_panel.png`.

Read with vision (crops at
`/tmp/opencode/render-shortcuts-20261004/crop_shortcuts_bar_*.png` and
`zoom_*_left.png`):

- **Slider** — `←→ themes · Enter apply · i info · Ctrl+↑↓ drawers · Esc back`.
  One line at the bottom, size-12, fully legible; starts at the 16px left
  padding and ends far from the right edge; no clipped word; no collision with
  the window edge or the MIT footer.
- **Panel (Settings/System)** — `↑↓ move · ← menu · →/Enter section · Enter
  engage · Esc back`. Same: one line, legible, no clipping.
- **Home** — `←→ move · Enter open · Esc hides` (the existing "Esc hides"
  affordance is preserved inside the Home list).
- **Mosaic** — `←↑↓→ move · Enter grow/apply · Ctrl+↑↓ drawers · Esc back`.

**T4 (COMMIT)**

```
476c80a feat(shell): the bottom bar lists the keyboard shortcuts
7 files changed, 273 insertions(+), 2 deletions(-)
```

Full suite after the change: `cargo test` → **1311 passed; 0 failed**
(baseline 1307 + 2 tr tests + 1 UI test + 1 render test).

**Notes / decisions**

- The context is derived in Slint from state the shell already mirrors
  (`is-panel-open` > Mosaic > Slider > Home), not recomputed in Rust, so a
  context switch cannot leave a stale string.
- `Esc hides` was **merged** into the Home list (same text, same place) rather
  than painted twice. In Home, Slider and Mosaic, Escape hides the window to
  the tray: `ui/shell.slint:877-881` routes it to `root.back-activated()` →
  `src/callbacks.rs:103-116`. Only the Settings panel's Escape really goes back
  (`ui/shell.slint:855-857` → `root.panel-back()`), so its `Esc back` hint is
  correct. The `back-hint` property and its `main.rs` setter are untouched.
- Workshop (mounted-screen 2, a stub) falls back to the Home list.
- Hints wording/ordering is the probe's call and is open for the keeper's next
  pass (see "Open for the keeper's next pass").

**Correction (post-review)** — commit `f704f74`

An independent cross-model review found the Slider/Mosaic `Esc back` hint was a
lie: Escape there hides the window to the tray, not "back". Fixed in `f704f74`:

- `i18n/en.json` / `i18n/es.json` — `shell.shortcuts.slider` and
  `shell.shortcuts.mosaic` now end in `Esc hides` / `Esc oculta`.
  `shell.shortcuts.home` and `shell.shortcuts.panel` are unchanged (Home really
  hides; the panel really goes back).
- Tests updated FIRST (RED on the old strings), then the strings: the
  `src/tr.rs` key tests and the bottom-bar test in `src/shell/ui_tests.rs`
  (explicit `ends_with` assertions per context).
- The two stale comments that claimed plain Up/Down toggles the drawers from
  Rust (`ui/gallery/GalleryRoot.slint`, `ui/main.slint`) now state that
  Ctrl+Up / Ctrl+Down — plus the mouse arrow buttons — are the only drawer keys.
- VISUAL: headless render into `/tmp/opencode/render-esc-fix-20261004a`; the
  Slider bar reads `←→ themes · Enter apply · i info · Ctrl+↑↓ drawers · Esc hides`.
- Full suite: `cargo test` → 1311 passed; 0 failed.
