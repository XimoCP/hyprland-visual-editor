# skwd-wall mute is a state, not a time window

Branch: `hve2-visual-rewrite` — delivery: work-unit commits on the feature branch.
Test runner: `cargo test`. Engine config seam: `SKWD_WALL_V2_CONFIG`.

The engine's colour authority must be a STATE owned by the theme claim, not a
race decided by clock windows. Today a stale `keeper-palette` marker releases
the mute at startup, the engine republishes its wallpaper-derived palette on the
next suspend/resume and the palette flips under the keeper.

## Specs

S1 (keeper, verbatim): "Skwd-wall debe de estar silenciado siempre al menos que
abramos el programa." With a valid colour-authority descriptor (a theme claims
the palette) `theme.policy` is `"off"` at every moment of the process lifetime,
including each startup, and no other state may leave it at `"wallpaper"` while
the claim exists.

S2: The `keeper-palette` marker stops having any effect on the engine mute. It
still decides which palette HVE's own overlay paints (the live one instead of
the theme snapshot) and still skips the startup re-assert.

S3 (keeper, verbatim): "eso dejalo como esta... esta bien" — the picker voice
stays exactly as it is: open releases the mute, close re-arms it, and the
post-close grace window is untouched.

S4: With no descriptor (no theme claims the palette) HVE never touches
`theme.policy`; the W2 crash repair keeps its current behaviour.

S5: A switch the engine does not own (`fixed`, missing, ambiguous, exotic
encoding, an `"off"` nobody claims) is never guessed: `plan_yield` refuses it.

## Tasks

- T1 (inline) RED: flip the two tests that encode the old contract and add the
  end-to-end settle case measured on 2026-10-24 (descriptor + keeper marker +
  `"wallpaper"` + a stale crash marker must end `"off"`).
- T2 (inline) GREEN: drop the two keeper guards on the mute path.
- T3 (inline) comments/docs updated; full suite green.
- T4 (delegated) independent cross-model verification of the diff.
- T5 (inline) live check: rebuilt binary, HVE restarted, `theme.policy` reads
  `"off"` while a `keeper-palette` marker is present.

## Risk

Item 3 (contract with another program: the `theme.policy` handshake other
consumers rely on) and item 4 (shared on-disk state between two processes).
The mute is written by HVE and read by `skwd-walld`.

## Log

L1 (keeper, 2026-10-24): "Lo que no debe de ser es por tiempo... si no por
estado. Skwd-wall debe de estar silenciado siempre al menos que abramos el
programa."

L2 (keeper, 2026-10-24): "eso dejalo como esta... esta bien... lo unico que hace
que se cambie la paleta es cuando vamnos a suspension y volvemos... y ninguna
solucion a funcionado... todas fracasan... por favor solucionalo."

L3 (keeper, 2026-10-24): "por si lo ves acabo de re aplicar el tema y su paleta
correcta... he sido yo para que no busqaues culpables."

L4 evidence (measured, 2026-10-24): at 07:39:47Z the startup settle took the
crash-repair branch because `keeper-palette.json` (written 05:07:41Z by a
genuine keeper palette change) was present, and put `theme.policy` back to
`"wallpaper"`. At 15:43:13Z the machine resumed from suspend, skwd-walld
re-applied the theme with `backend=noctalia` and published
`~/.config/noctalia/palettes/skwd-wall.json` (15:43:17Z); HVE's overlay then
adopted the engine palette (`colors.json` `#e4ac67` instead of the theme's
`#879a39`). With the marker gone from the mute decision the engine never
reaches `backend=noctalia`, so the resume cannot repaint the palette.

L5 (measured, 2026-10-24 16:04:43Z): a genuine theme re-apply wrote the palette
and the assert two seconds later attributed it to the keeper, re-creating the
marker. Under S2 that marker no longer touches the mute; the misattribution is
recorded here as a follow-up, not fixed in this unit.

L6 (measured, 2026-10-24): three tests are already red at the parent commit and
fail identically with the change stashed — `scripts_contract::
applied_theme_snapshot_wins_over_live_palette`, `scripts_contract::
hostile_palette_names_never_own_colours` and `watcher::tests::
watcher_reasserts_the_applied_theme_palette_on_an_external_change`. They are a
separate defect (bash contract / watcher ask), not caused by this unit; the
first two fail deterministically even with HOME and XDG redirected.
`cargo clippy --all-targets -- -D warnings` is clean.
