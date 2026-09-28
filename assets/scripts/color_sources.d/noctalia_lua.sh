# --- HVE colour source: noctalia-lua -------------------------------------------
# Module contract (see the loader header in ../colors.sh): this file defines
# the four hve_colour_source_* functions plus the optional
# hve_colour_source_refresh and hve_colour_source_tertiary below, and
# nothing else at top level. It must stay safe under "set -u", must quote
# every expansion, and must never eval file content or any external data.
#
# Backend home: Noctalia. This module IS the Noctalia backend's home for the
# rendered template step; the central scripts (colors.sh, color_watcher.sh)
# must not name these lua paths themselves. Both renderings (v5 and v4)
# belong to this backend, so the coherence-gated v4 tertiary fill lives
# here too — never in the central script and never in another backend.
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

# Declared tertiary (see the loader header in ../colors.sh): the v4
# rendering is this backend's own earlier output, so when this module won
# the chain on the v5 rendering but no tertiary was detected, the v4 file
# may complete it — ACCEPTED ONLY when its primary agrees with the
# detected primary, so a v4 file left over from an older scheme never
# leaks its tertiary into the current one. Sets HVE_TERTIARY and returns
# 0 on success, 1 otherwise. Reads only this backend's own files.
hve_colour_source_tertiary() {
    [ -n "${HVE_TERTIARY:-}" ] && return 1
    local hypr_dir="${HVE_HYPR_DIR:-$HOME/.config/hypr}"
    local v4="$hypr_dir/noctalia/noctalia-colors.lua"
    [ -f "$v4" ] || return 1

    local v4_vars v4_primary v4_tertiary
    v4_vars=$(_hve_extract_lua_vars "$v4")
    v4_primary=$(printf '%s\n' "$v4_vars" | grep '^HVE_PRIMARY=' | head -1)
    v4_primary="${v4_primary#HVE_PRIMARY=}"

    # Coherence gate: skip a rendering whose primary is a different scheme.
    if [ -n "$v4_primary" ] && [ -n "${HVE_PRIMARY:-}" ] && [ "$v4_primary" != "$HVE_PRIMARY" ]; then
        echo "[HVE] Skipping stale v4 palette (primary $v4_primary != $HVE_PRIMARY)" >&2
        return 1
    fi

    v4_tertiary=$(printf '%s\n' "$v4_vars" | grep '^HVE_TERTIARY=' | head -1)
    v4_tertiary="${v4_tertiary#HVE_TERTIARY=}"
    [ -n "$v4_tertiary" ] || return 1
    HVE_TERTIARY="$v4_tertiary"
    export HVE_TERTIARY
    return 0
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
