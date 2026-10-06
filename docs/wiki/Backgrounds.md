# Backgrounds

A **static** wallpaper is one image sitting on the background layer. An
**animated** wallpaper is a video, played by `mpvpaper` — one process per
output. HVE saves both with a theme and puts them back when you apply it,
but it never owns the desktop's background itself: the piece that is
currently painting wins, and HVE only hands paths over.

## Static wallpapers

On a theme apply, when no video ends up on screen, HVE restores the saved
image with the shell's own `wallpaper-set` call and then passes the **same**
path to the wallpaper engine through the `apply-background` router
(`src/providers/background.rs`). The engine (`skwd-wall-v2` / `skwd-helm`)
is optional: if its socket is absent, HVE does nothing and no error occurs.

If a video *does* end up on screen, the static restore is skipped on
purpose — setting an image afterwards would cover the running video.

A theme stores its static wallpaper as `wallpaper.txt` inside its provider
directory.

## Animated wallpapers (Noctalia mpvpaper)

HVE integrates video backgrounds for **Noctalia v5** themes through the
official `noctalia/mpvpaper` plugin. The plugin supervises one `mpvpaper`
instance per output and persists its assignments in
`~/.local/state/noctalia/mpvpaper/assignments.json`.

Two facts drive the whole design:

- the plugin reads `assignments.json` **only at boot** (`loadAssignments()` +
  `applyAll()`), and
- its IPC surface has no `set` event — only clear, clear-all, pause, resume
  and toggle.

So HVE writes the file and then **bounces the plugin**:

```text
noctalia msg plugins disable noctalia/mpvpaper
noctalia msg plugins enable  noctalia/mpvpaper
```

which is exactly what clicking a video in the plugin's own picker does.
HVE never starts `mpvpaper` itself; before bouncing it kills any current
instance (`pkill -9 -x mpvpaper`) so a config reload cannot leave two
decoders fighting over the same video.

### The theme manifest

Themes store a lightweight manifest — references only, never video bytes —
at `{theme_dir}/providers/noctalia-v5/mpvpaper-assignments.json`:

```json
{ "version": 1,
  "assignments": {
    "*": { "filename": "movie.mp4",
           "local_path": "/abs/path/movie.mp4",
           "url": "https://...",
           "sha256": "..." } } }
```

The key is the output: `"*"` means every output, or name a connector such as
`DP-3`. Saving a theme captures the plugin's live assignments into this file;
a machine with no live state simply saves no manifest, which means "not an
animated theme".

### Resolution at apply time

For each assignment, in order:

1. `local_path` exists on disk → use it, no download;
2. `filename` is already in the plugin's `video_directory` → use it;
3. there is a `url` → download it with `curl` over **https** and verify
   `sha256`;
4. otherwise → warn and skip that assignment.

`sha256` is **mandatory** for the download step: a manifest that carries a
`url` without one is refused rather than fetched unverified. An assignment
that cannot be resolved is skipped with a warning; the theme apply itself
never fails because of it — if *no* video resolves, the leg reports it and
the apply continues without a video.

### Apply and clear

- If the theme carries a manifest but the plugin is missing or disabled, HVE
  warns you with a desktop notification **before** it tries to play anything,
  so a video theme does not silently lose its videos. The plugin is never
  re-enabled by a theme apply: if you turned it off, it stays off.
- A theme **without** a manifest sends `clear-all` first, so a video left
  over from a previous theme cannot hijack the screen. HVE then waits 1.5 s
  for the plugin to extract its static frame before the static restore runs.
- After writing the live assignments and bouncing the plugin, HVE waits 1.2 s
  for the new instances to spawn, so a following static apply cannot
  clear-all into a half-booted plugin.

### Installing a video backend

HVE **does not install third-party software**: no package manager, no
downloads, nothing written outside the theme and provider directories it
manages. An animated background needs a backend that HVE only *drives*:

- the **Noctalia `noctalia/mpvpaper` plugin** (with `mpvpaper` itself), which
  plays the theme's saved manifest; or
- the **skwd-wall engine** (`skwd-wall-v2` / `skwd-helm`), which paints one
  exact video path on request and is available when its socket exists under
  `$XDG_RUNTIME_DIR/skwd-wall-v2/wall.sock`.

Install one of those yourself — through your distribution or the plugin's own
instructions — to get animated backgrounds.

When a theme wants a video and **neither** backend is available, HVE still
applies the theme: it paints the theme's packaged poster (`media/poster.*`)
statically when the theme carries one, and raises a desktop
notification that names what is missing and points here for the install
steps. The decision and the copy live in `src/providers/bg_info.rs`
(`video_backend_notice`), so the notice can be tested without spawning
anything.

### Availability

Animated backgrounds depend on the active provider: they are a **Noctalia v5**
feature (`providers/noctalia-v5/`). Noctalia v4 and HVE's own presets carry
no video.

## Who owns the live background

HVE does not decide this by guessing. `src/providers/wallpaper_authority.rs`
is the single module that answers "who controls the wallpaper right now", and
it uses two signals for what each actually knows:

- the **compositor's** layer stack (`hyprctl -j layers`) says which layer is
  on top — ground truth for what you see;
- the **wallpaper daemon's** own report (`outputs --json`) says what kind of
  layer it is (`static`, `video`).

A painter's name alone is never trusted for the kind: the skwd suite paints
both still images and videos, so a topmost skwd layer is typed through the
daemon, and a dedicated video painter such as `mpvpaper` is video by nature.
Anything else is unknown, and an unknown fullscreen painter vetoes the
decision instead of being ignored.

Theme save consults this module so a theme captures **only** the currently
active background — the video or the image, not both.

## The two backends

`src/providers/background.rs` routes an apply by declared choice, never by
side effect:

| What is handed over | Backend |
|---------------------|---------|
| One exact path (a static image, or a video the painter recorded) | the wallpaper engine (`src/providers/skwd_engine.rs`) |
| The theme's saved animated manifest | the Noctalia mpvpaper plugin (`src/providers/mpvpaper.rs`) |

A backend never calls another one, and every hand-off reports whether it
really painted something — a `false` means the caller keeps its fallback
routes instead of claiming a background that was never set.

## See also

- [Themes and colours](Themes-and-Colours) — what else a saved theme
  captures.
- [Architecture](Architecture) — the seams these backends sit behind.
- [Project structure](Project-Structure) — where the provider files live.
