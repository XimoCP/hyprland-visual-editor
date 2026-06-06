#!/bin/bash

# --- MAIN PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Detect format
source "$HVE_SCRIPTS_DIR/detect_format.sh"
export HVE_FORMAT=$(detect_format 2>/dev/null || echo "conf")

# Safe directory and overlay
WATCHDOG_FILE="$HVE_SAFE_DIR/hve_watchdog.sh"

# Hyprland config files
HYPR_CONF="$HVE_HYPR_DIR/hyprland.conf"
HYPR_LUA="$HVE_HYPR_DIR/hyprland.lua"

# Internal assembler path
ASSEMBLE_SCRIPT="$HVE_SCRIPTS_DIR/assemble.sh"

# --- RECONSTRUIR VARIABLE DE COLORES PERDIDA ---
# Usamos HVE_COLORS_BASE que viene del utils.sh y añadimos la extensión correcta
if [ "$HVE_FORMAT" = "lua" ]; then
    HVE_COLORS_FILE="${HVE_COLORS_BASE}.lua"
else
    HVE_COLORS_FILE="${HVE_COLORS_BASE}.conf"
fi

# --- MARKERS ---
# Conf mode uses # comments, Lua mode uses -- comments
MARKER_START_CONF="# >>> HYPRLAND VISUAL EDITOR START <<<"
MARKER_END_CONF="# >>> HYPRLAND VISUAL EDITOR END <<<"
MARKER_START_LUA="-- >>> HYPRLAND VISUAL EDITOR START <<<"
MARKER_END_LUA="-- >>> HYPRLAND VISUAL EDITOR END <<<"

ACTION=$1

# --- CLEANUP FUNCTIONS ---

clean_hyprland_conf() {
    if [ ! -f "$HYPR_CONF" ]; then return; fi
    sed -i "/$MARKER_START_CONF/,/$MARKER_END_CONF/d" "$HYPR_CONF"
    sed -i "\|source = .*hyprland-visual-editor/overlay.conf|d" "$HYPR_CONF"
    sed -i "\|source = .*HVE/overlay|d" "$HYPR_CONF"
    sed -i '${/^$/d;}' "$HYPR_CONF"
}

clean_hyprland_lua() {
    if [ ! -f "$HYPR_LUA" ]; then return; fi
    sed -i "/$MARKER_START_LUA/,/$MARKER_END_LUA/d" "$HYPR_LUA"
    sed -i "\|require.*HVE|d" "$HYPR_LUA"
    sed -i "\|source.*HVE/overlay|d" "$HYPR_LUA"
    sed -i "\|hl\.exec_cmd.*hve_watchdog|d" "$HYPR_LUA"
    sed -i '/hl\.on.*hyprland\.start.*function.*HVE/,/end)/d' "$HYPR_LUA"
    sed -i '${/^$/d;}' "$HYPR_LUA"
}

# --- SETUP FUNCTION ---
setup_files() {
    echo "Preparing safe environment and watchdog..."

    mkdir -p "$HVE_FRAGMENTS_DIR"
    mkdir -p "$HVE_SAFE_DIR"

    chmod +x "$HVE_SCRIPTS_DIR/"*.sh

    # Deploy the watchdog script
    cp "$HVE_SCRIPTS_DIR/hve_watchdog.sh" "$WATCHDOG_FILE"
    chmod +x "$WATCHDOG_FILE"

    # Execute the internal assembler
    if [ -f "$ASSEMBLE_SCRIPT" ]; then
        bash "$ASSEMBLE_SCRIPT"

        # SECURITY PATCH: In case assemble.sh has the old hardcoded path
        if [ -f "$HVE_PLUGIN_DIR/overlay.conf" ]; then
            mv "$HVE_PLUGIN_DIR/overlay.conf" "$HVE_SAFE_DIR/overlay.conf"
        fi
    else
        if [ "$HVE_FORMAT" = "lua" ]; then
            echo '#!/usr/bin/env hyprland' > "$HVE_SAFE_DIR/overlay.lua"
            echo "# Hyprland Visual Editor Overlay Base" >> "$HVE_SAFE_DIR/overlay.lua"
        else
            echo "# Hyprland Visual Editor Overlay Base" > "$HVE_SAFE_DIR/overlay.conf"
        fi
    fi
}

# --- MAIN LOGIC ---

if [ "$ACTION" == "enable" ]; then
    setup_files

    if [ "$HVE_FORMAT" = "lua" ]; then
        # === LUA MODE: inject into hyprland.lua ===
        clean_hyprland_lua
        clean_hyprland_conf  # Also clean old conf entries if migrating

        cat >> "$HYPR_LUA" <<EOF

$MARKER_START_LUA
-- 1. Active Uninstall Watchdog
-- 2. Effects Application (Visual Editor)
--    Colors already loaded via require('configs/noctalia-colors') above
dofile("$HVE_SAFE_DIR/overlay.lua")
hl.on("hyprland.start", function()
    hl.exec_cmd("$WATCHDOG_FILE")
end)
$MARKER_END_LUA
EOF

    else
        # === CONF MODE: inject into hyprland.conf ===
        clean_hyprland_conf

        cat >> "$HYPR_CONF" <<EOF

$MARKER_START_CONF
# 1. Active Uninstall Watchdog
exec-once = $WATCHDOG_FILE
# 2. Variable Definition (Color Palette)
source = $HVE_COLORS_FILE
# 3. Effects Application (Visual Editor)
source = $HVE_SAFE_DIR/overlay.conf
$MARKER_END_CONF
EOF
    fi

    hyprctl reload

elif [ "$ACTION" == "disable" ]; then
    clean_hyprland_conf
    clean_hyprland_lua

    rm -rf "$HVE_SAFE_DIR"

    hyprctl reload
fi