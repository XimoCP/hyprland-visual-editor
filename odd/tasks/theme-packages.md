# Theme packages — portable themes and shipped reference themes

- **Branch**: `hve2-visual-rewrite`
- **Opened**: 2026-10-05
- **Keeper's mandate**: HVE is a theme applier. A saved theme must be a
  **self-contained package** that travels: copying its folder moves the whole
  look (background, palette, settings, presets). The keeper's own reference
  themes must ship in the repository so a fresh install shows what is possible
  and how it works; users may then duplicate, modify or delete them.

## Objective

Make a theme a **folder that carries everything it needs**, and ship the
keeper's reference themes with HVE.

Today a theme is already one folder (`~/.config/hve/themes/<Name>/`), but its
background is a **reference**, not a file:

- `providers/noctalia-v5/wallpaper.txt` — the static wallpaper **path**.
- `providers/noctalia-v5/video.txt` — `path=/…/x.mp4` + painter metadata.
- `providers/mpvpaper/…` — a manifest of `local_path` / `filename` / `url`
  ("references only, no video bytes", by its own module doc).

So a theme copied to another machine shows up but **without its background**.
The fix is to put the media inside the theme folder and resolve it from there.

## What a theme package will look like

```
<theme>/
  meta.json                 name, author, description, version
  palette.json              (already saved)
  settings.toml             (already saved)
  state.json                active presets + geometry (already saved)
  media/
    poster.jpg              the static frame (JPEG, the video's native size)
    background.mp4          optional: the video, only when the keeper opts in
  presets/                  only the preset files the theme uses that do NOT
    animations/ borders/    ship with HVE (custom ones)
    shaders/
```

Resolution/format decisions already taken with the keeper:

- **Poster**: **native resolution** of the video (his are 4K), **JPEG** quality
  high — measured ~900 KB at 4K, ~520 KB at 1440p. Native JPEG it is.
- **Video**: stays a real file **only when it fits**; the keeper's plan is
  3 video themes. GitHub rejects files >100 MB, so each shipped video must be
  under that (recommend re-encoding 4K→1080p/1440p to ~20–40 MB). No Git LFS.
- The video system (skwd-wall / the mpvpaper plugin) is **not installed by
  HVE**; see T6.

## Tasks

Each task is one work unit: a delegated writer, a cross-model verification,
one commit, suite green.

- [ ] **T1 — save copies the media into the theme.** On save, write the poster
  (`media/poster.jpg`, native size, extracted from the video when the source is
  a video, or the static wallpaper copied as-is when it is an image) and record
  a **theme-relative** path. Keep the existing absolute records for backwards
  compatibility (old themes still resolve).
- [ ] **T2 — apply resolves the media from the theme first.** When a theme
  carries `media/`, apply uses it; otherwise fall back to the current chain
  (absolute path → `video_directory` → `url`). No behaviour change for existing
  themes.
- [ ] **T3 — the video is optional and size-aware.** A theme can carry
  `media/background.mp4` (opt-in at save). If the file exceeds a size bound,
  record a `url` instead of copying. Restore resolves theme file → url.
- [ ] **T4 — custom presets travel.** Bundle the preset files a theme uses
  that are NOT part of HVE's shipped set (`assets/{animations,borders,shaders}`)
  under `presets/`; apply reads them from the theme when present.
- [ ] **T5 — reference themes ship in the repo.** The keeper's reference themes
  live under `assets/themes/<Name>/` (with their `media/`); `install.sh` copies
  them into `~/.config/hve/themes/` **without clobbering** a same-named theme
  the user already has, and the gallery shows them like any other theme.
- [ ] **T6 — the video system is guided, not assumed.** HVE detects the video
  backend (skwd-wall socket / mpvpaper plugin, already probed) and, when a
  theme wants a video and none is present, shows a clear, actionable notice
  (what to install, where). Add a wiki page. Do NOT install third-party
  software from HVE.

## Acceptance criteria

- Copying a theme folder to another machine transports its look (poster,
  palette, settings, active presets) with no external files needed.
- Existing themes on disk keep working exactly as before.
- `cargo test` green at every commit; `cargo build --release` warning-free.
- A fresh install shows the reference themes, each with its background.
- No file >100 MB is ever added to the repository.
- Every step has a MiMo writer and a GLM cross-model verification recorded.

## Out of scope

- Bundling or installing a video backend (skwd-wall / mpvpaper plugin).
- Git LFS.
- Changing which engine paints (skwd vs mpvpaper routing stays).
- HVE 1 compatibility.

## Progress and evidence

- 2026-10-05: contract written from the keeper's requirements and the measured
  theme layout, media sizes and poster sizes. Pending: T1 onward.
