#!/bin/bash
# hve_watchdog.sh - Monitors if the plugin is still installed and cleans up if not

# 1. Define paths (now pointing to the new HVE folder)
PLUGIN_DIR="$HOME/.config/noctalia/plugins/hyprland-visual-editor"
HYPR_CONF="$HOME/.config/hypr/hyprland.conf"
HVE_SAFE_DIR="$HOME/.config/noctalia/HVE"

# 2. Check if the original plugin folder has been deleted by the Shell
if [ ! -d "$PLUGIN_DIR" ]; then

    # Silently remove the conflicting line from the user's hyprland.conf
    sed -i '/source = ~\/.config\/noctalia\/HVE\/overlay.conf/d' "$HYPR_CONF"
    # Fallback to clean up old NVL paths just in case
    sed -i '/source = ~\/.config\/noctalia\/NVL\/overlay.conf/d' "$HYPR_CONF"

    # Delete the entire HVE safe folder (takes the overlay and the script itself with it)
    rm -rf "$HVE_SAFE_DIR"
fi