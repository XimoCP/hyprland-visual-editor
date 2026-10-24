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

Status: T1-T5 done.

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

L7 (T4, cross-model): independent verification by mimo-v2.6-flash over
847cd2d..e761733 returned PASS on S1-S5 with its own probe module, including a
probe at the baseline that reproduces the exact measured failure and fails
there. It also confirmed S3 (`background.rs` untouched) and that no production
path other than the overlay/repair-skip reads the marker.

L8 (T5, live): rebuilt the release binary (md5 1774f3b2...), backed the running
one up to `~/.local/bin/hve.bak-2026-10-24-mute`, installed and restarted HVE
with the `keeper-palette` marker present and `theme.policy` set to `"wallpaper"`
(the exact pre-fix startup state). Result: `MUTE RE-ARMED a theme owns the
palette after a restart; theme.policy "wallpaper" -> "off"` at 16:45:00Z, where
the old binary wrote `crash repair: theme.policy put back to "wallpaper"`. The
marker that HVE had written over its own apply at 16:04:43 (stamp
`color-write.last` unchanged since 06:48, so the apply path never stamped) was
removed as false; the overlay keeps the theme's palette (`#879a39`).

L9 (follow-up of L5, closed): `NoctaliaV5Provider::apply` now stamps
`color-write.last` before its first write, so the palette an apply puts in the
backend is recognised as HVE's own instead of the keeper's hand. Test:
`v5_apply_stamps_its_own_palette_write`, RED before the stamp.

L10 (test isolation, closed): the three tests that were red at the parent commit
were reading the keeper's LIVE `$XDG_CONFIG_HOME/hve` from the spawned scripts,
because the sandboxes overrode `HOME` but not the XDG roots a desktop session
exports (`scripts_contract` now derives them in `sandbox_xdg_roots`, the watcher
spawns pin `XDG_CONFIG_HOME`). Full suite green: 1850 passed, 0 failed.
`cargo fmt --check` is NOT a gate and already reports drift on lines this unit
never touched; clippy `-D warnings` is clean.
