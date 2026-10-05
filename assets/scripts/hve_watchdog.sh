#!/bin/bash
# hve_watchdog.sh - Monitors if HVE is still installed and cleans up if not

# Resolve HVE directory from THIS script's location
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
# scripts/ is inside assets/, assets/ is inside the project root
HVE_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"

HYPR_CONF="$HOME/.config/hypr/hyprland.conf"
HYPR_LUA="$HOME/.config/hypr/hyprland.lua"
# Cache root, mirroring Rust's `dirs::cache_dir()` (src/config.rs):
# `$XDG_CACHE_HOME` when set, else `$HOME/.cache`, plus `hve`. This script
# does not source utils.sh, so the resolution lives here too.
HVE_SAFE_DIR="${HVE_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/hve}"

# Check if the HVE directory still exists
if [ ! -d "$HVE_DIR" ]; then

    # Remove markers from hyprland.conf (# style)
    if [ -f "$HYPR_CONF" ]; then
        sed -i '/# >>> HYPRLAND VISUAL EDITOR START <<</,/# >>> HYPRLAND VISUAL EDITOR END <<</d' "$HYPR_CONF"
    fi

    # Remove markers from hyprland.lua (-- style)
    if [ -f "$HYPR_LUA" ]; then
        sed -i '/-- >>> HYPRLAND VISUAL EDITOR START <<</,/-- >>> HYPRLAND VISUAL EDITOR END <<</d' "$HYPR_LUA"
    fi

    # Remove the safe fallback directory and this script itself
    rm -rf "$HVE_SAFE_DIR"
fi
