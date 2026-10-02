# Noctalia wallpaper authority: pin the static to the winning background

## Objective

When applying a theme, decide WHO owns the background right now and set
Noctalia's option accordingly, so its own static wallpaper never shows a stale
image over/under the real background. Specifically: for a skwd VIDEO background,
pin Noctalia to the video's POSTER (a frame) and neutralize its rotation.

## Verified cause (obs 1087 + live evidence 2026-10-02)

- A saved theme carries the WHOLE Noctalia `settings.toml`
  (`src/providers/noctalia.rs:1293-1303` save, `:1393` restore). The theme
  `Animation` carries `video.txt` AND a `[wallpaper] directory =
  ~/Pictures/Wallpapers/Cars` with `automation` every 600 s. Applying it restores
  that config, so Noctalia paints its Cars static (live: `car9.jpg`).
- Live layers: `noctalia-wallpaper` at level 0 (full screen) and `skwd-wall-vk`
  at level 1 (full screen). The running video is `green-fields-and-peaks`, NOT
  the theme's `ellen-joe-...` — so the video record did not restore the theme's
  video either.
- With a STATIC background the system is coherent (same file to both). With
  VIDEO, HVE deliberately skips the static (`noctalia.rs:1608`) so Noctalia keeps
  its stale static. Apply never queries the authority (`wallpaper_authority.rs`
  is consumed by SAVE only, `noctalia.rs:1183-1222`).

## Design (agreed with the keeper, obs 1087)

At apply time, classify the background and set Noctalia accordingly:

| Case | Noctalia |
| --- | --- |
| Noctalia static (from the theme) | service ON, its image (works today) |
| skwd static | same file to both (works today) |
| **skwd video** | **service ON, pinned to the video POSTER, rotation neutralized (MISSING)** |
| other painter not reflectable | service OFF (current) |

## Implementation plan

### Phase 1 — Poster hand-off for the video case
- In `noctalia.rs` apply, when the background is a video (the theme's
  `video.txt` parsed to an existing path, and/or `animated_applied`):
  - Obtain a poster: reuse `crate::shell::gallery::thumbs::extract_video_frame`
    (`src/shell/gallery/thumbs.rs:410`, `ffmpeg -ss 1`), or `cached_frame_path`.
  - `noctalia msg wallpaper-set "" <poster>` (`noctalia.rs:1613` pattern) so
    Noctalia's static layer shows the poster instead of its stale image.
  - Keep the plugin service ON for this case (the D4 re-assert currently only
    preserves the pre-apply state, `:1401-1429`).

### Phase 2 — Neutralize the rotation
- Point Noctalia's `[wallpaper] directory` at a folder that holds ONLY the poster
  (so the 600 s rotation is a no-op), by editing the restored `settings.toml`
  with a small line-based `[wallpaper]` writer (there is no `toml` crate; the
  existing `mpvpaper_enabled_in_settings` parser is the precedent).
- Then `noctalia msg config-reload` if needed.

### Phase 3 — Apply-time ownership decision
- Query the authority (`wallpaper_authority::snapshot_background`) at apply to
  pick the row of the table, instead of only trusting the theme records. Fall
  back to the theme records when the authority is unavailable.

### Phase 4 — Verification
- By-construction tests for each branch (video → poster hand-off; static
  unchanged; other painter → service OFF), plus the existing apply tests.
- Live verification by the keeper (below).

## Open live questions (need the keeper's eyes; the Noctalia source is not on this machine)

1. Does `wallpaper-set` with the poster override the 600 s rotation, or is the
   directory trick required?
2. Does `enabled = false` turn off only the background layer or also the zones?
3. Is `config-reload` needed after editing `[wallpaper]`?
4. Do the skwd layer and Noctalia's layer fight when both paint the same frame?
5. Should the video record also be restored (the live video was not the theme's)?

## Constraints

- Providers are reused, not rewritten: additive change to the apply path, the
  same shape as the D4 re-assert.
- Strict TDD, runner `cargo test`. English artifacts. No `cargo fmt`.
- This is a large change: chained work units.

## Progress

- [ ] Phase 1
- [ ] Phase 2
- [ ] Phase 3
- [ ] Phase 4

## Next step

Implement Phase 1 + 2, then the keeper live-verifies before Phase 3.
