# Panel: mouse → keyboard focus convergence

## Objective

Clicking a card with the mouse must move the panel's keyboard cursor to that
card, so the keyboard resumes from where the mouse left off (keyboard == mouse).

## Problem

A card click only re-seeded Slint focus (`refocus`, a no-op in the sections) and
left the keyboard cursor at its old position: after clicking a card, the next
arrow key continued from the old index, so the keeper navigated blindly until
reaching the clicked card.

The Save section already declared `focus-requested(int)` and the whole chain to
Rust existed (`on_panel_save_focus_requested`, "keyboard == mouse, D8"), but the
card never emitted it — the convergence was dead. Borders/Motion/Filters had no
convergence path at all.

## Fix

- `SavedThemeCard`, `SavedPresetCard` and `components.PresetCard` now emit
  `focus-requested(index)` on the card BODY click (apply behaviour unchanged).
- The list panes forward it; each section declares a `focus-requested(int)`
  callback; `PanelRoot` writes the section's own index (`borders-`,
  `motion-`, `filters-focused-index`); Save reuses the existing Rust path.
- The APPLY SWITCH click also converges, so both the body and the switch move the
  cursor (consistent).

## Commits

- `e376bf4` — `feat(panel): a card click moves the keyboard cursor to it`.
- `a8d93a7` — `fix(panel): the apply switch also converges the keyboard cursor`.

Suite: 1276 passed, 0 failed. Cross-model verified (glm-5.3-flash): PASS with
warnings; the switch-convergence warning was then closed in `a8d93a7`.

## Notes

- The scroll-follow math is untouched.
- The `*_mouse_never_touches_focus_by_construction` tests were rewritten to the
  new contract (mouse DOES converge the keyboard index now).
- Out of scope: the System section rows (no card list).

## Follow-up: the scroll must survive the wheel (2026-10-03)

The keeper reported that after navigating with the mouse/wheel, resuming the
keyboard left the cursor invisible until it reached the mouse's zone. Root cause:
a `viewport-y: <expr>` BINDING is destroyed when the ScrollView assigns
`viewport-y` (the wheel does), so the list stopped following the keyboard focus.
Borders had already been fixed with an imperative `follow-focus()`
(`BordersSection.slint:1927-1934`); Motion, Filters, Save and System still used
the binding.

- `0885305` — ported the Borders imperative pattern to Motion (list + tune),
  Filters and Save.
- `0b06c64` — ported it to System's two panes (the About pane needs only the
  minimal equivalent; its follow target is always the top).

Suite: 1282 passed, 0 failed. Cross-model verified (glm-5.3-flash): PASS.
