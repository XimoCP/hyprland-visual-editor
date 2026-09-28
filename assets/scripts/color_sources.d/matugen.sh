# --- HVE colour source: matugen -------------------------------------------------
# Module contract (see the loader header in ../colors.sh): this file defines
# exactly the four hve_colour_source_* functions below and nothing else at
# top level. It must stay safe under "set -u", must quote every expansion,
# and must never eval file content or any external data.
#
# Migrated verbatim from _hve_try_matugen in colors.sh (unit 1d3): the
# inline function is gone, the priority (50) is unchanged, and the overall
# chain order does not change.
hve_colour_source_id() {
    printf '%s\n' "matugen"
}

hve_colour_source_priority() {
    printf '%s\n' "50"
}

# Matugen: ~/.config/matugen/config.toml → find generated output
hve_colour_source_try() {
    local matugen_config="$HOME/.config/matugen/config.toml"
    if [ -f "$matugen_config" ]; then
        # Find the output template that targets hyprland
        local output_file
        output_file=$(grep -A5 'hyprland' "$matugen_config" 2>/dev/null | grep 'output' | head -1 | sed 's/.*= *//' | tr -d '"' | sed "s|~|$HOME|")

        if [ -n "$output_file" ] && [ -f "$output_file" ]; then
            echo "[HVE] Colors from: matugen ($output_file)" >&2
            # Lua-only output.
            local found
            found=$(_hve_extract_lua_vars "$output_file")
            [ -n "$found" ] && eval "$found"
            return 0
        fi
    fi
    return 1
}

hve_colour_source_watch() {
    local matugen_config="$HOME/.config/matugen/config.toml"
    if [ -f "$matugen_config" ]; then
        printf '%s\n' "$matugen_config"
        local output_file
        output_file=$(grep -A5 'hyprland' "$matugen_config" 2>/dev/null | grep 'output' | head -1 | sed 's/.*= *//' | tr -d '"' | sed "s|~|$HOME|")
        if [ -n "$output_file" ] && [ -f "$output_file" ]; then
            printf '%s\n' "$output_file"
        fi
    fi
}
