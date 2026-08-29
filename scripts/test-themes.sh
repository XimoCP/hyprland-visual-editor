#!/usr/bin/env bash
# HVE — test-theme cloning for pagination testing.
#
# Clones every real theme N times with a " (test k)" suffix so the gallery
# has enough entries to exercise pagination, then removes them all in one
# command. Real themes are never touched.
#
# Usage:
#   test-themes.sh add [N]   # clone each theme N times (default 6)
#   test-themes.sh remove    # delete every " (test k)" clone
#   test-themes.sh status    # counts

set -euo pipefail

THEMES_DIR="${HOME}/.config/hve/themes"
SUFFIX_MARKER=" (test"

mkdir -p "$THEMES_DIR"

real_themes() {
  find "$THEMES_DIR" -maxdepth 1 -mindepth 1 -type d ! -name "*$SUFFIX_MARKER*" -print0 | sort -z
}

test_themes() {
  find "$THEMES_DIR" -maxdepth 1 -mindepth 1 -type d -name "*$SUFFIX_MARKER*" -print0 | sort -z
}

case "${1:-status}" in
  add)
    n="${2:-6}"
    count=0
    while IFS= read -r -d '' src; do
      base="$(basename "$src")"
      for ((k = 1; k <= n; k++)); do
        dst="$THEMES_DIR/$base (test $k)"
        if [ -d "$dst" ]; then
          rm -rf "$dst"
        fi
        cp -r "$src" "$dst"
        count=$((count + 1))
      done
    done < <(real_themes)
    echo "Created $count test clones ($n per real theme)."
    ;;
  remove)
    removed=0
    while IFS= read -r -d '' dst; do
      rm -rf "$dst"
      removed=$((removed + 1))
    done < <(test_themes)
    echo "Removed $removed test clones. Real themes untouched."
    ;;
  status)
    reals=0
    tests=0
    while IFS= read -r -d '' _; do reals=$((reals + 1)); done < <(real_themes)
    while IFS= read -r -d '' _; do tests=$((tests + 1)); done < <(test_themes)
    echo "Real themes: $reals — test clones: $tests"
    ;;
  *)
    echo "Usage: $0 {add [N]|remove|status}" >&2
    exit 1
    ;;
esac
