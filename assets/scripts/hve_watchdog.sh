#!/bin/bash
# hve_watchdog.sh - Monitors if HVE is still installed and cleans up if not

# Resolve HVE directory from THIS script's location
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
# scripts/ is inside assets/, assets/ is inside the project root
HVE_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"

HYPR_CONF="$HOME/.config/hypr/hyprland.conf"
HYPR_LUA="$HOME/.config/hypr/hyprland.lua"
HVE_SAFE_DIR="${HVE_CACHE_DIR:-$HOME/.cache/hve}"

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
