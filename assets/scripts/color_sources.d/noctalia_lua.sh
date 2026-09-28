# --- HVE colour source: noctalia-lua -------------------------------------------
# Module contract (see the loader header in ../colors.sh): this file defines
# exactly the four hve_colour_source_* functions below and nothing else at
# top level. It must stay safe under "set -u", must quote every expansion,
# and must never eval file content or any external data.
#
# Backend home: Noctalia. This module IS the Noctalia backend's home for the
# rendered template step; the central scripts (colors.sh, color_watcher.sh)
# must not name these lua paths themselves.
#
# Migrated verbatim from _hve_try_noctalia in colors.sh (unit 1d2): the
# inline function is gone, the priority (30) is unchanged, and the overall
# chain order does not change.
hve_colour_source_id() {
    printf '%s\n' "noctalia-lua"
}

hve_colour_source_priority() {
    printf '%s\n' "30"
}

# Noctalia template output fallback (Lua-only):
#   v5: ~/.config/hypr/noctalia.lua
#   v4: ~/.config/hypr/noctalia/noctalia-colors.lua
hve_colour_source_try() {
    local noctalia_v5_lua="$HVE_HYPR_DIR/noctalia.lua"
    local noctalia_v4_lua="$HVE_HYPR_DIR/noctalia/noctalia-colors.lua"

    # Prefer v5 output (templates-apply writes here).
    if [ -f "$noctalia_v5_lua" ]; then
        echo "[HVE] Colors from: Noctalia v5 (lua)" >&2
        eval "$(_hve_extract_lua_vars "$noctalia_v5_lua")"
        return 0
    fi

    # Fallback to v4 path.
    if [ -f "$noctalia_v4_lua" ]; then
        echo "[HVE] Colors from: Noctalia v4 (lua)" >&2
        eval "$(_hve_extract_lua_vars "$noctalia_v4_lua")"
        return 0
    fi
    return 1
}

hve_colour_source_watch() {
    local hypr_dir="${HVE_HYPR_DIR:-$HOME/.config/hypr}"
    local noctalia_v5_lua="$hypr_dir/noctalia.lua"
    if [ -f "$noctalia_v5_lua" ]; then
        printf '%s\n' "$noctalia_v5_lua"
    fi
    local noctalia_v4_lua="$hypr_dir/noctalia/noctalia-colors.lua"
    if [ -f "$noctalia_v4_lua" ]; then
        printf '%s\n' "$noctalia_v4_lua"
    fi
}
