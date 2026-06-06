#!/bin/bash
# hve_watchdog.sh - Monitors if the plugin is still installed and cleans up if not

PLUGIN_DIR="$HOME/.config/noctalia/plugins/hyprland-visual-editor"
HYPR_CONF="$HOME/.config/hypr/hyprland.conf"
HYPR_LUA="$HOME/.config/hypr/hyprland.lua"
HVE_SAFE_DIR="$HOME/.cache/noctalia/HVE"

# Check if the original plugin folder has been deleted
if [ ! -d "$PLUGIN_DIR" ]; then

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
