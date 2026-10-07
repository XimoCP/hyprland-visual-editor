# Vendored UI fonts

These files are the three font families HVE's design declares, copied here so
the application renders identically on every machine instead of depending on
whatever fonts a system happens to have installed.

## What is here

The exact files, unmodified:

| Family | Family name in the font | Files |
|---|---|---|
| Roboto | `Roboto` | `Roboto-Regular.ttf`, `Roboto-Medium.ttf`, `Roboto-Bold.ttf` |
| Roboto Condensed | `Roboto Condensed` | `RobotoCondensed-Regular.ttf`, `RobotoCondensed-Medium.ttf`, `RobotoCondensed-Bold.ttf` |
| Roboto Mono | `Roboto Mono` | `RobotoMono-Regular.ttf`, `RobotoMono-Medium.ttf`, `RobotoMono-Bold.ttf` |

The family names above are what `ui/tokens.slint` asks for. Only the three
weights Regular (400), Medium (500) and Bold (700) are shipped upstream, so a
`font-weight: 600` request resolves to the nearest weight the family provides.

## Source

Copied verbatim from the Arch Linux packages on the machine:

- `ttf-roboto` (`/usr/share/fonts/TTF/Roboto-*.ttf`,
  `/usr/share/fonts/TTF/RobotoCondensed-*.ttf`)
- `ttf-roboto-mono` (`/usr/share/fonts/TTF/RobotoMono-*.ttf`)

## How they are used

`ui/tokens.slint` imports each `.ttf` at the top level. `slint-build` (see
`build.rs`) compiles that tree with its default resource embedding, so the font
bytes are embedded in the binary at build time and registered when the window is
created. Nothing is read from disk at runtime, and no system font is required.

## Licence

Both families are licensed under the SIL Open Font License, Version 1.1. The
licence text travels with the fonts:

- `OFL-Roboto.txt` — covers Roboto and Roboto Condensed
  (`Copyright 2011 The Roboto Project Authors`).
- `OFL-RobotoMono.txt` — covers Roboto Mono
  (`Copyright 2015 The Roboto Mono Project Authors`).

Neither licence declares a Reserved Font Name, so the fonts may be redistributed
and embedded as long as the licence text is kept alongside them.

## Updating

Re-copy the same nine files, and the two `OFL.txt` files, from the same
packages. Overwrite the copies here without editing, subsetting or re-encoding
them, and keep `ui/tokens.slint`'s imports and `src/shell/ui_tests.rs`'s
`vendored_fonts_are_complete_and_imported` test in sync.
