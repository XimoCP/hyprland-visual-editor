# Fix: mouse press in the settings panel reaches the theme gallery behind it

**Locator**: `odd/tasks/panel-click-through-to-gallery.md`
**Engram mirror**: topic `odd/panel-click-through-to-gallery/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: IMPLEMENTED (commit `74a6d3e`) — pending parent gate + rebuild/install
**TDD**: strict — runner `cargo test`

## Symptom (user, live on HEAD `beedf42`)

"While interacting with the settings panel — specifically with the border parameters — I suddenly pressed with the mouse and a theme got applied, as if the click had passed through the settings layer and activated a theme in the carousel."

Effect: an unintended desktop change (a theme is applied and persisted).

## Root cause (confirmed by read-only diagnosis + Slint 1.17.0 runtime source)

Two halves that combine:

1. **The gallery is faded, not unmounted.** `ui/shell.slint:434-439` hides `gallery-layer` only by `opacity: root.is-panel-open ? 0.0 : 1.0` (animated). In Slint, the `Opacity` item forwards pointer input unconditionally regardless of its value (`i-slint-core-1.17.0/items.rs:875-893`, `clips_children()` false at `:940-942`), so the carousel stays mounted with live `TouchArea`s. Only `visible: false` cuts hit-testing (compiler pass `passes/visible.rs`, precedent in this repo: commit `6a5d052` fixed the mirror-image defect by gating with `if`).
2. **The panel does not absorb presses over its whole surface.** `PanelRoot` is a plain `Rectangle` (`ui/panel/PanelRoot.slint:20,256,263`) with no backdrop `TouchArea`; its keyboard `FocusScope` has `focus-on-click: false` (`:350`) so it ignores mouse; and the panels' `ScrollView`s compile to a `Flickable` with `interactive: false`, which ignores non-wheel events. Only real controls consume. Dead zones that leak a press: the "Gradient Angle" card title/padding (`BordersSection.slint:425-484`, `TouchArea` only on the 30/45/90 buttons at `470-480`), `BorderCompactColorRow` chip/labels (only "Custom…" at `104-109`), description texts, 1px separators, the `spacing: 8px` gaps (`382-388`) and the panel header (`PanelRoot.slint:269-294`).

The press therefore reaches the centered card's `TouchArea` (`ui/gallery/SliceDelegate.slint:524-545`) → `card-clicked` → `GalleryRoot.slint:364` → `shell.slint:462` → `main.slint:514` → `src/main.rs:1692 on_gallery_card_clicked` → `slot.apply_theme` (`src/main.rs:1729`, persisted at `1736-1738`). The only guard there is `is_mutating` (`main.rs:1701`), which is true only during the 350 ms transition (`src/shell/nav.rs:359-360`), never while the panel is simply open.

**Same class, also reachable**: the mouse wheel (gallery `wheel-zone`, `SliceCarousel.slint:85-97`) and right-click (`SliceDelegate.slint:547-554`, which flips the card).

**Extra risk window**: for ~620-640 ms after opening, the gallery is still fully opaque while the panel fades in (`shell.slint:438-439` vs `488-489`), and `is_mutating` is already false.

**Origin**: pre-existing, introduced 2026-09-01 by `7d1d7d4` ("choreographed settings dock"). Today's commits do not touch layer composition or mouse handling. The existing tests do not cover this: `borders_mouse_never_touches_focus_by_construction` (`src/shell/ui_tests.rs:4716-4751`) and `panel_scopes_ignore_mouse_focus_by_construction` (`:4868-4899`) assert source text, not click behaviour.

## Required fix

Introduce an **input absorber** in the seam between the layers, reusing the existing gating idiom (`ui/shell.slint:660-672`): a full-size transparent `Rectangle` with a `TouchArea` that consumes the press without navigating, declared **after** `gallery-layer` and **before** the panel's `if`, with the panel's own condition (`is-panel-open || is-mutating`). Because a later declaration is in front in hit-testing, the panel's real controls keep receiving input; everything else is absorbed and never reaches the gallery. This also closes the 620-640 ms opening window and the wheel/right-click variants.

**Do NOT** convert the gallery's `opacity` hide into `visible: false`/`if`: the dock's tuck/expand choreography depends on the opacity animation.

## Acceptance criteria

1. With the panel open over a loaded gallery, a mouse press on a panel dead zone (title/padding/gap/header) does **not** apply or persist a theme — covered by an automated test that fails before the fix.
2. The same guard blocks the channel's wheel and right-click paths.
3. Panel controls keep working: existing mouse tests stay green, including `panel_rail_single_click_switches_section` and `borders_preset_card_click_applies_the_file_not_the_title`.
4. The dock's visual choreography is unchanged (opacity fade preserved); verified with the headless render and PNG inspection.
5. Full suite green and `cargo check` clean.

## Out of scope

- Replacing the opacity hide with `visible`/`if` (breaks the choreography).
- The already-recorded debt: duplicated index map, the picker `y` slide-in, the scanner `@Color`/`icon` gap, `sync_preset_indices`, the flaky `providers::noctalia` test.
- Any other panel/gallery behaviour change.

## Delivery

Work-unit commit(s) with Conventional Commit messages, test and docs alongside the behaviour, no AI attribution. Parent rebuilds and installs `~/.local/bin/hve` (backup first) so the user can verify live.

## Progress

- 2026-09-19 — Created from the read-only diagnosis and the parent's own verification that the defect predates today's commits (`git log -S` on the opacity hide → `7d1d7d4`, 2026-09-01). No source changed yet.
- 2026-09-19 — Fix landed in `74a6d3e` (`fix(shell): absorb pointer input in the settings
  panel's dead zones`).

## Evidence

- **Behavioural tests** (strict TDD, real events in the headless harness), in
  `src/shell/ui_tests.rs`:
  - `panel_dead_zone_press_never_reaches_gallery_card` — press at (1049, 500), the
    tune pane's 16px dead padding, inside the centered card (x 519..1387). RED
    before the fix: `gallery-card-clicked fired with [3]`. GREEN after. The same
    test closes the panel and presses the same point, which must apply card 3, so
    a green first half cannot pass by missing the gallery.
  - `panel_dead_zone_wheel_never_scrolls_gallery` — wheel at (80, 500), the rail's
    dead area inside the carousel wheel band. RED: `gallery-wheel-step fired with
    [-1]`. GREEN after.
  - `panel_dead_zone_right_click_never_flips_gallery_card` — right press at
    (1049, 500). RED: `gallery-card-right-clicked fired with [3]`. GREEN after.
- **Fix**: a transparent full-size `Rectangle` + `TouchArea` between
  `gallery-layer` and the panel's `if` in `ui/shell.slint`, gated on
  `is-panel-open || is-mutating`. The `TouchArea` accepts `scroll-event`
  explicitly; a `TouchArea` ignores a wheel it has no handler for. The gallery
  `opacity` hide is untouched (choreography preserved).
- **Suite**: `cargo test` → 753 passed / 0 failed / 0 ignored (750 baseline + 3
  new). One full run flagged the already-recorded flaky
  `providers::noctalia::tests::delegate_noop_when_socket_absent` (env-var race
  between noctalia tests); it passes in isolation and passed on the re-run.
  `cargo check --all-targets` → clean.
- **Visual**: `slice_focus_flow_renders_settled_and_midflight`,
  `panel_morph_midflight_renders` and `panel_borders_tune_renders` green. The
  eight PNGs (`panel_morph_gallery_settled|midflight|panel_settled`,
  `panel_morph_reduced_mid|settled`, `slice_settled|midflight`,
  `panel_borders_tune`) are byte-identical with and without the absorber
  (`cmp`), and were inspected by eye: the dock still tucks the side slats behind
  the centered card at midflight and the panel still settles full-bleed.

## Next step

Parent gate → rebuild + install `~/.local/bin/hve` (backup first) → live check,
then archive this ODD task.
