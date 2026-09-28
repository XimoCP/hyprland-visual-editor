# --- HVE colour source: pywal ------------------------------------------------
# Module contract (see the loader header in ../colors.sh): this file defines
# exactly the four hve_colour_source_* functions below and nothing else at
# top level. It must stay safe under "set -u", must quote every expansion,
# and must never eval file content or any external data.
#
# Migrated verbatim from _hve_try_pywal in colors.sh (unit 1d): the inline
# function is gone, the priority (40) is unchanged, and the overall chain
# order does not change.
hve_colour_source_id() {
    printf '%s\n' "pywal"
}

hve_colour_source_priority() {
    printf '%s\n' "40"
}

# Pywal: ~/.cache/wal/colors.json
hve_colour_source_try() {
    local wal_json="$HOME/.cache/wal/colors.json"
    if [ -f "$wal_json" ] && command -v python3 &>/dev/null; then
        echo "[HVE] Colors from: pywal" >&2
        local c0 c3 c5 c10 c13 c7
        c0=$(python3 -c "import json; print(json.load(open('$wal_json'))['colors']['color0'])" 2>/dev/null)
        c3=$(python3 -c "import json; print(json.load(open('$wal_json'))['colors']['color3'])" 2>/dev/null)
        c5=$(python3 -c "import json; print(json.load(open('$wal_json'))['colors']['color5'])" 2>/dev/null)
        c10=$(python3 -c "import json; print(json.load(open('$wal_json'))['colors']['color10'])" 2>/dev/null)
        c13=$(python3 -c "import json; print(json.load(open('$wal_json'))['colors']['color13'])" 2>/dev/null)
        c7=$(python3 -c "import json; print(json.load(open('$wal_json'))['colors']['color7'])" 2>/dev/null)

        [ -n "$c3" ] && HVE_PRIMARY=$(_hve_normalize_color "$c3")
        [ -n "$c13" ] && HVE_SECONDARY=$(_hve_normalize_color "$c13")
        [ -n "$c5" ] && HVE_TERTIARY=$(_hve_normalize_color "$c5")
        [ -n "$c0" ] && HVE_SURFACE=$(_hve_normalize_color "$c0")
        [ -n "$c7" ] && HVE_SURFACE_LOWEST=$(_hve_normalize_color "$c7")
        [ -n "$c10" ] && HVE_ACCENT=$(_hve_normalize_color "$c10")
        return 0
    fi
    return 1
}

hve_colour_source_watch() {
    local wal_json="$HOME/.cache/wal/colors.json"
    if [ -f "$wal_json" ]; then
        printf '%s\n' "$wal_json"
    fi
}
