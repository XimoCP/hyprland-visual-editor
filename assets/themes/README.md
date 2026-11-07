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
machine moves the whole look with it — no external file needed.

## This set is FROZEN (keeper's instruction, 2026-11-08)

**Do not repackage these themes, and do not regenerate them from the keeper's
own `~/.config/hve/themes/`.** Code changes ship; the theme packages already
here do not change.

The reason is concrete: since the OpenRGB provider landed, a theme saved on a
machine that drives OpenRGB records the `openrgb` provider in its `meta.json`
and carries `providers/openrgb/profile.json`. The gallery only lists a theme
whose recorded providers are ALL registered, so a sample generated from the
keeper's folder would disappear for every user who does not run OpenRGB.

If a NEW sample is ever added on purpose, it must be stripped first: remove
`providers/openrgb/` and the `openrgb` entry from `meta.json`. The guard test
`providers::openrgb::tests::the_shipped_themes_carry_no_lighting` fails when
this folder holds a lighting record, so an accidental repackage is caught
before it is pushed.

**A package carries no path from the machine that saved it.** Every value that
names a file or a folder outside the theme is dropped when the theme is saved
and when it is packaged (`hve --package-themes`), so the machine that applies it
keeps its own screenshot folder, its own wallpaper directory and its own shell
hooks. What defines the LOOK — background, palette, presets, geometry, bar and
widget layout — travels inside the folder. The absolute background records
(`wallpaper.txt`, `video.txt`) are gone for the same reason: the packaged
`media/` is what paints the background, and a foreign absolute path is not a
fallback, it is a broken promise.

The video is only carried when the file is at or under 95 MB, so no package
ever hits GitHub's 100 MB file limit. A heavier video records its source URL
instead, and a theme with no video backend still shows its poster.

## Shipped themes

| Theme | Carries | Background | Palette |
| --- | --- | --- | --- |
| `Cars` | poster | `media/poster.jpg` | `community Garnet` |
| `Fantasy` | poster + video | `media/background.mp4` | `community Catppuccin Mocha Sapphire` |
| `JoKer` | poster | `media/poster.png` | `custom JokerTheme` |
| `Night` | poster + video | `media/background.mp4` | `custom Night-palette` |
| `Space` | poster | `media/poster.png` | `custom skwd-wall` |
| `Spiderman` | poster | `media/poster.jpg` | `community Spiderman` |
| `Work` | poster | `media/poster.jpg` | `custom skwd-wall` |

## Artwork

The background images and the videos are third-party artwork, kept here as demo
material for the theme format. They belong to their original authors; the
`MIT` licence of this repository covers the code, not the artwork.
