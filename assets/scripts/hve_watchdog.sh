#!/bin/bash
# hve_watchdog.sh - Monitors if HVE is still installed and cleans up if not

# "Still installed" is defined by the binary the installer places. This script
# is DEPLOYED into the cache (~/.cache/hve/hve_watchdog.sh) and runs from
# there, so its own location says nothing about where HVE lives: the old
# `dirname(dirname(SCRIPT_DIR))` derivation resolved to `$HOME`, which always
# exists, and the cleanup below could never fire. `install.sh` writes
# `$HOME/.local/bin/hve` and `uninstall.sh` removes exactly that path, so its
# absence is the honest "HVE is gone" signal.
HVE_BIN="${HVE_BIN:-$HOME/.local/bin/hve}"

HYPR_CONF="$HOME/.config/hypr/hyprland.conf"
HYPR_LUA="$HOME/.config/hypr/hyprland.lua"
# Cache root, mirroring Rust's `dirs::cache_dir()` (src/config.rs):
# `$XDG_CACHE_HOME` when set, else `$HOME/.cache`, plus `hve`. This script
# does not source utils.sh, so the resolution lives here too. A relative
# `XDG_CACHE_HOME` is invalid under the XDG spec and `dirs` ignores it; this
# path is handed to `rm -rf` below, so the check is load-bearing here.
_hve_xdg_dir() {
    case "${1:-}" in
        /*) printf '%s' "$1" ;;
        *) printf '%s' "$2" ;;
    esac
}
HVE_SAFE_DIR="${HVE_CACHE_DIR:-$(_hve_xdg_dir "${XDG_CACHE_HOME:-}" "$HOME/.cache")/hve}"

# Check if HVE is still installed (its binary is present)
if [ ! -e "$HVE_BIN" ]; then

    # Remove markers from hyprland.conf (# style)
    if [ -f "$HYPR_CONF" ]; then
        sed -i '/# >>> HYPRLAND VISUAL EDITOR START <<</,/# >>> HYPRLAND VISUAL EDITOR END <<</d' "$HYPR_CONF"
    fi

    # Remove markers from hyprland.lua (-- style)
    if [ -f "$HYPR_LUA" ]; then
        sed -i '/-- >>> HYPRLAND VISUAL EDITOR START <<</,/-- >>> HYPRLAND VISUAL EDITOR END <<</d' "$HYPR_LUA"
    fi

    # Remove the safe fallback directory and this script itself.
    # Guard the delete: the only shape HVE ever resolves is an absolute path
    # ending in `/hve`. An exported relative `HVE_CACHE_DIR`, or a relative
    # `$HOME`, would otherwise send `rm -rf` chasing a path relative to
    # whatever directory this script happened to run in.
    case "$HVE_SAFE_DIR" in
        /*/hve) rm -rf "$HVE_SAFE_DIR" ;;
        *) echo "hve_watchdog: refusing to remove '$HVE_SAFE_DIR' (not an absolute .../hve path)" >&2 ;;
    esac
fi
