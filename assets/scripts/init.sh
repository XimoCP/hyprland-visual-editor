#!/bin/bash

# --- MAIN PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Safe directory and overlay
WATCHDOG_FILE="$HVE_SAFE_DIR/hve_watchdog.sh"

# Lua-only settings file (config format is never detected at runtime)
SETTINGS_FILE="$HVE_SAFE_DIR/hve-settings.lua"

# Hyprland config files
HYPR_CONF="$HVE_HYPR_DIR/hyprland.conf"
HYPR_LUA="$HVE_HYPR_DIR/hyprland.lua"

# Internal assembler path
ASSEMBLE_SCRIPT="$HVE_SCRIPTS_DIR/assemble.sh"

# --- COLOR PALETTE (HVE_COLORS_BASE) ---
# HVE emits Lua only; `HVE_COLORS_BASE` is not set by utils.sh today, so this
# block is inert. Its removal is deferred to a follow-up change.
if [ -n "$HVE_COLORS_BASE" ]; then
    HVE_COLORS_FILE="${HVE_COLORS_BASE}.lua"
else
    HVE_COLORS_FILE=""
fi

# --- MARKERS ---
# Lua injection uses `--` comments. The conf markers are retained only so
# `clean_hyprland_conf` can strip HVE's legacy block (one-way hygiene).
MARKER_START_CONF="# >>> HYPRLAND VISUAL EDITOR START <<<"
MARKER_END_CONF="# >>> HYPRLAND VISUAL EDITOR END <<<"
MARKER_START_LUA="-- >>> HYPRLAND VISUAL EDITOR START <<<"
MARKER_END_LUA="-- >>> HYPRLAND VISUAL EDITOR END <<<"

ACTION=$1

# --- MIGRATION GUARD ---
# Mirror of src/config_guard.rs: HVE only manages hyprland.lua. A valid Lua
# config (using `hl.` or `require(`) is ready for HVE; a legacy hyprland.conf
# without valid Lua is conf-only and must be migrated before enabling HVE.
hve_lua_is_valid() {
    [ -f "$HYPR_LUA" ] && grep -qE 'hl\.|require\(' "$HYPR_LUA" 2>/dev/null
}

hve_guard_allows_enable() {
    if hve_lua_is_valid; then
        return 0
    fi
    if [ -f "$HYPR_CONF" ]; then
        return 1
    fi
    return 0
}

# --- CLEANUP FUNCTIONS ---

clean_hyprland_conf() {
    if [ ! -f "$HYPR_CONF" ]; then return; fi
    sed -i "/$MARKER_START_CONF/,/$MARKER_END_CONF/d" "$HYPR_CONF"
    sed -i "\|source = .*hve/overlay|d" "$HYPR_CONF"
    sed -i "\|source = .*HVE/overlay|d" "$HYPR_CONF"
    sed -i '${/^$/d;}' "$HYPR_CONF"
}

clean_hyprland_lua() {
    if [ ! -f "$HYPR_LUA" ]; then return; fi
    sed -i "/$MARKER_START_LUA/,/$MARKER_END_LUA/d" "$HYPR_LUA"
    sed -i "\|require.*hve|d" "$HYPR_LUA"
    sed -i "\|require.*HVE|d" "$HYPR_LUA"
    sed -i "\|source.*hve/overlay|d" "$HYPR_LUA"
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
    else
        echo '#!/usr/bin/env hyprland' > "$HVE_SAFE_DIR/overlay.lua"
        echo "-- Hyprland Visual Editor Overlay Base" >> "$HVE_SAFE_DIR/overlay.lua"
    fi

    # ── Create hve-settings with default window rules and keybinds section (if not exists) ──
    if [ ! -f "$SETTINGS_FILE" ]; then
        echo "Creating default settings at $SETTINGS_FILE..."
        cat > "$SETTINGS_FILE" << 'SETEOF'
-- >>> HVE WINDOW RULES <<<
-- HVE 2 manages its own window state via the Composer trait
-- (fullscreen for Gallery, floating for settings). No rules needed here.
-- >>> HVE WINDOW RULES END <<<
-- >>> HVE KEYBINDS <<<
-- >>> HVE KEYBINDS END <<<
SETEOF
    fi
}

# --- MAIN LOGIC ---

if [ "$ACTION" == "enable" ]; then
    if ! hve_guard_allows_enable; then
        echo "HVE 2 only manages hyprland.lua — migrate from hyprland.conf."
        echo "HVE 2 solo gestiona hyprland.lua — migre desde hyprland.conf."
        echo "Migrate your config, then run 'init.sh enable' again."
        echo "Migre su configuración y ejecute 'init.sh enable' de nuevo."
        exit 1
    fi

    setup_files

    # === LUA MODE: inject into hyprland.lua ===
    clean_hyprland_lua
    clean_hyprland_conf  # One-way hygiene: strip a legacy conf block if present

    cat >> "$HYPR_LUA" <<EOF

$MARKER_START_LUA
-- 1. Active Uninstall Watchdog
-- 2. Effects Application (Visual Editor)
--    Colors already loaded via require('configs/noctalia-colors') above
dofile("$HVE_SAFE_DIR/overlay.lua")
-- 3. HVE Settings (window rules + keybinds)
dofile("$SETTINGS_FILE")
hl.on("hyprland.start", function()
    hl.exec_cmd("$WATCHDOG_FILE")
end)
$MARKER_END_LUA
EOF

    hyprctl reload

elif [ "$ACTION" == "disable" ]; then
    clean_hyprland_conf
    clean_hyprland_lua

    # Remove everything except hve-settings (which persists across disable/enable)
    if [ -d "$HVE_SAFE_DIR" ]; then
        for item in "$HVE_SAFE_DIR"/* "$HVE_SAFE_DIR"/.*; do
            [ -e "$item" ] || continue
            basename "$item" | grep -q '^hve-settings\.' && continue
            [ "$(basename "$item")" = "." ] || [ "$(basename "$item")" = ".." ] && continue
            rm -rf "$item"
        done
    fi

    hyprctl reload
fi
