# --- HVE colour source: noctalia-lua -------------------------------------------
# Module contract (see the loader header in ../colors.sh): this file defines
# the four hve_colour_source_* functions plus the optional
# hve_colour_source_refresh below, and nothing else at top level. It must
# stay safe under "set -u", must quote every expansion, and must never eval
# file content or any external data.
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
    # The backend's own settings file: watched only when the tool is
    # available — the same condition the central watcher applied before
    # this module owned the trigger.
    local backend_settings="$HOME/.local/state/noctalia/settings.toml"
    if [ -f "$backend_settings" ] && command -v noctalia &>/dev/null; then
        printf '%s\n' "$backend_settings"
    fi
}

# Declared refresh (see the loader header in ../colors.sh): re-render the
# backend's own template output after its settings changed, BEFORE the
# central overlay reads the lua files. The core only routes this — the tool
# and the files below are this backend's own. Returns 0 when the refresh
# ran; the guards below report 1 (nothing to do) and a failed tool call
# reports its own non-zero status. Either way the caller logs the outcome
# and continues — this never aborts the watcher's loop.
hve_colour_source_refresh() {
    local backend_settings="$HOME/.local/state/noctalia/settings.toml"
    [ -f "$backend_settings" ] || return 1
    command -v noctalia &>/dev/null || return 1
    noctalia msg templates-apply 9>&-
}
