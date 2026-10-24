#!/usr/bin/env bash
# ============================================================================
# Regenerate the launcher icons: assets/icons/hicolor/<size>/apps/hve.png
#
# The launcher icon is a STATIC brand snapshot. A launcher reads a file from
# disk and never runs HVE, so it cannot follow the user's live theme the way
# the in-app logo and the tray icon do (src/theme.rs renders those at runtime
# from the same vector). The four fills below are therefore frozen to the
# DEFAULT DARK palette declared in src/theme.rs:
#
#   H  -> secondary          #fbbf24  (amber)
#   HV -> tertiary darkened  #865cb0  (interlaced part, tertiary x 0.7)
#   V  -> tertiary           #c084fc  (violet)
#   E  -> accent             #34d399  (emerald)
#
# Several sizes are shipped because an icon theme resolves a request for N
# pixels to the CLOSEST image it has: with only 256 available, a 48px request
# gets a downscale, and any older hve.png already sitting in a size directory
# would win instead. The tree mirrors the installed layout so `install.sh` can
# copy it straight into ~/.local/share/icons/hicolor/.
#
# Requires: rsvg-convert (librsvg) and ImageMagick (magick or convert).
# Usage:    scripts/gen-icon.sh
# ============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/assets/hve_logo.svg"
OUT_ROOT="$ROOT/assets/icons/hicolor"

# Sizes published into the icon theme. 256 is the original artwork resolution.
SIZES=(48 64 128 256)

command -v rsvg-convert >/dev/null || { echo "missing tool: rsvg-convert" >&2; exit 1; }
if command -v magick >/dev/null; then
    IM=magick
elif command -v convert >/dev/null; then
    IM=convert
else
    echo "missing tool: ImageMagick (magick or convert)" >&2
    exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

sed \
    -e 's|%HV%|#865cb0|g' \
    -e 's|%H%|#fbbf24|g' \
    -e 's|%V%|#c084fc|g' \
    -e 's|%E%|#34d399|g' \
    "$SRC" > "$work/logo.svg"

# Start from a clean tree so a removed size never lingers in the repo.
rm -rf "$OUT_ROOT"

for size in "${SIZES[@]}"; do
    # The artwork is a wide logotype (1371x730), so a square canvas is mostly
    # transparent side margin. Give it 29/32 of the width and centre it.
    art_width=$(( size * 29 / 32 ))
    rsvg-convert --width "$art_width" --background-color none \
        --output "$work/logo-$size.png" "$work/logo.svg"

    dir="$OUT_ROOT/${size}x${size}/apps"
    mkdir -p "$dir"
    "$IM" "$work/logo-$size.png" -background none -gravity center \
        -extent "${size}x${size}" "$dir/hve.png"
    echo "wrote $dir/hve.png"
done
