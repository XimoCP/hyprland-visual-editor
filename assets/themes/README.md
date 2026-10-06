# Reference themes

These themes ship with HVE so a fresh install shows what the editor can do.
`install.sh` copies each one into `~/.config/hve/themes/<Name>/` and **never
overwrites** a theme you already have under the same name: if the folder
exists, it is left exactly as it is.

Each folder is a self-contained theme package:

```
<Name>/
  meta.json                      name, author, description, version
  media/poster.*                 the static background, carried inside the theme
  media/background.mp4           the video, only when it fits (see below)
  providers/noctalia-v5/         palette, settings and the background records
  providers/hve-presets/         active presets and geometry
  presets/                       only the presets the theme uses that do NOT
                                 ship with HVE
```

Because the background travels inside the folder, copying a theme to another
machine moves the whole look with it — no external file needed. The older
absolute records (`wallpaper.txt`, `video.txt`) are kept for compatibility and
are simply ignored when the theme carries its own `media/`.

The video is only carried when the file is at or under 95 MB, so no package
ever hits GitHub's 100 MB file limit. A heavier video records its source URL
instead, and a theme with no video backend still shows its poster.

## Shipped themes

| Theme | Shows | Background |
| --- | --- | --- |
| `Animation` | a video theme, with its own poster fallback | `ellen-joe-neon-alley-zenless-zone-zero` (moewalls.com) |
| `Cars` | a photo background with a calm border | local `Wallpapers/Cars/car4.jpg` |
| `BordesRedondosCentrados` | rounded, centred borders | local `Wallpapers/Joker/joker4.png` |
| `ClasicoV4` | the classic layout | local `Wallpapers/Joker/joker3.png` |
| `joke2` | a heavier animated border | local `Wallpapers/Joker/joker1.png` |

## Artwork

The background images and the video are third-party artwork, kept here as demo
material for the theme format. They belong to their original authors; the
`MIT` licence of this repository covers the code, not the artwork.
