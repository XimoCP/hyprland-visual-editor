#!/bin/bash

# --- MAIN PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Detect format
source "$HVE_SCRIPTS_DIR/detect_format.sh"
export HVE_FORMAT=$(detect_format 2>/dev/null || echo "conf")

# Safe directory and overlay
WATCHDOG_FILE="$HVE_SAFE_DIR/hve_watchdog.sh"

# Settings extension matches HVE format (lua or conf)
if [ "$HVE_FORMAT" = "lua" ]; then
    SETTINGS_EXT="lua"
else
    SETTINGS_EXT="conf"
fi
SETTINGS_FILE="$HVE_SAFE_DIR/hve-settings.${SETTINGS_EXT}"

# Hyprland config files
HYPR_CONF="$HVE_HYPR_DIR/hyprland.conf"
HYPR_LUA="$HVE_HYPR_DIR/hyprland.lua"

# Internal assembler path
ASSEMBLE_SCRIPT="$HVE_SCRIPTS_DIR/assemble.sh"

# --- RECONSTRUIR VARIABLE DE COLORES PERDIDA ---
# Usamos HVE_COLORS_BASE que viene del utils.sh y añadimos la extensión correcta
if [ -n "$HVE_COLORS_BASE" ]; then
    if [ "$HVE_FORMAT" = "lua" ]; then
        HVE_COLORS_FILE="${HVE_COLORS_BASE}.lua"
    else
        HVE_COLORS_FILE="${HVE_COLORS_BASE}.conf"
    fi
else
    HVE_COLORS_FILE=""
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

    # ── Create hve-settings with default window rules and keybinds section (if not exists) ──
    if [ ! -f "$SETTINGS_FILE" ]; then
        echo "Creating default settings at $SETTINGS_FILE..."
        if [ "$HVE_FORMAT" = "lua" ]; then
            cat > "$SETTINGS_FILE" << 'SETEOF'
-- >>> HVE WINDOW RULES <<<
hl.window_rule({
  name  = "hve-floating",
  match = { title = "^Hyprland Visual Editor$" },
  float = true,
  size  = { "95%", "95%" },
  move  = { "center", "center" },
})
-- >>> HVE WINDOW RULES END <<<
-- >>> HVE KEYBINDS <<<
-- >>> HVE KEYBINDS END <<<
SETEOF
        else
            cat > "$SETTINGS_FILE" << 'SETEOF'
# >>> HVE WINDOW RULES <<<
windowrulev2 = float, title:^(Hyprland Visual Editor)$
windowrulev2 = center, title:^(Hyprland Visual Editor)$
windowrulev2 = size 95% 95%, title:^(Hyprland Visual Editor)$
# >>> HVE WINDOW RULES END <<<
# >>> HVE KEYBINDS <<<
# >>> HVE KEYBINDS END <<<
SETEOF
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
-- 3. HVE Settings (window rules + keybinds)
dofile("$SETTINGS_FILE")
hl.on("hyprland.start", function()
    hl.exec_cmd("$WATCHDOG_FILE")
end)
$MARKER_END_LUA
EOF

    else
        # === CONF MODE: inject into hyprland.conf ===
        clean_hyprland_conf

        {
            echo ""
            echo "$MARKER_START_CONF"
            echo "# 1. Active Uninstall Watchdog"
            echo "exec-once = $WATCHDOG_FILE"
            if [ -n "$HVE_COLORS_FILE" ] && [ -f "$HVE_COLORS_FILE" ]; then
                echo "# 2. Variable Definition (Color Palette)"
                echo "source = $HVE_COLORS_FILE"
            fi
            echo "# Effects Application (Visual Editor)"
            echo "source = $HVE_SAFE_DIR/overlay.conf"
            echo "# HVE Settings (window rules + keybinds)"
            echo "source = $SETTINGS_FILE"
            echo "$MARKER_END_CONF"
        } >> "$HYPR_CONF"
    fi

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