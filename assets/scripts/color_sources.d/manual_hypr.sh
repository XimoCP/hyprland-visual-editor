# --- HVE colour source: manual-hypr --------------------------------------------
# Module contract (see the loader header in ../colors.sh): this file defines
# exactly the four hve_colour_source_* functions below and nothing else at
# top level. It must stay safe under "set -u", must quote every expansion,
# and must never eval file content or any external data.
#
# Migrated verbatim from _hve_try_manual in colors.sh (unit 1d3): the
# inline function is gone, the priority (60) is unchanged, and the overall
# chain order does not change.
hve_colour_source_id() {
    printf '%s\n' "manual-hypr"
}

hve_colour_source_priority() {
    printf '%s\n' "60"
}

# Manual/fallback: scan hypr config files
hve_colour_source_try() {
    echo "[HVE] Colors from: manual config scan" >&2
    local found=0

    # Scan .lua files
    while IFS= read -r file; do
        [[ "$file" == *"noctalia"* ]] && continue
        local vars
        vars=$(_hve_extract_lua_vars "$file")
        if [ -n "$vars" ]; then
            eval "$vars"
            found=1
            break
        fi
    done < <(find "$HVE_HYPR_DIR" -maxdepth 2 -name "*.lua" -type f 2>/dev/null)

    # Last resort: border gradient from appearance
    if [ $found -eq 0 ]; then
        local appearance="$HVE_HYPR_DIR/configs/appearance.lua"
        if [ -f "$appearance" ]; then
            local colors
            colors=$(_hve_extract_border_gradient "$appearance")
            if [ -n "$colors" ]; then
                HVE_PRIMARY=$(echo "$colors" | head -1)
                HVE_SECONDARY=$(echo "$colors" | tail -1)
                found=1
            fi
        fi
    fi

    [ $found -eq 1 ] && return 0
    return 1
}

hve_colour_source_watch() {
    local hypr_dir="${HVE_HYPR_DIR:-$HOME/.config/hypr}"
    find "$hypr_dir" -maxdepth 2 -name "*.lua" -type f 2>/dev/null
}
