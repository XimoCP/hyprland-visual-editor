# --- HVE colour source: noctalia-palette ---------------------------------------
# Module contract (see the loader header in ../colors.sh): this file defines
# exactly the four hve_colour_source_* functions below and nothing else at
# top level. It must stay safe under "set -u", must quote every expansion,
# and must never eval file content or any external data.
#
# Backend home: Noctalia. This module IS the Noctalia backend's home for the
# scheme palette file step; the central scripts (colors.sh, color_watcher.sh)
# must not name these palette paths themselves.
#
# Migrated verbatim from _hve_try_noctalia_palette in colors.sh (unit 1d2):
# the inline function is gone, the priority (20) is unchanged, and the
# overall chain order does not change.
hve_colour_source_id() {
    printf '%s\n' "noctalia-palette"
}

hve_colour_source_priority() {
    printf '%s\n' "20"
}

# Try to read the full M3 palette directly from the Noctalia scheme/palette file.
# This gives us ALL colors (tertiary, surface_variant, etc.) regardless of what
# the hyprland template happens to render. For wallpaper schemes (no palette
# file), falls back to template output via the noctalia-lua module.
hve_colour_source_try() {
    command -v noctalia &>/dev/null || return 1
    command -v python3 &>/dev/null || return 1

    local scheme_raw
    scheme_raw=$(noctalia msg color-scheme-get 2>/dev/null) || return 1
    scheme_raw="${scheme_raw%[$'\r\n']}"  # strip trailing newline

    local source="${scheme_raw%% *}"
    local name="${scheme_raw#* }"

    # SECURITY: reject anything outside the exact supported source set.
    case "$source" in
        custom|builtin|community|wallpaper) ;;
        *) return 1 ;;
    esac

    # SECURITY: the scheme name is theme-controlled and would be interpolated
    # into file paths. Only [A-Za-z0-9_-] plus spaces are allowed; quotes,
    # slashes, $, backticks, ;, (), control chars etc. make the whole palette
    # attempt fall through safely to the template output. (Spaces are mapped
    # to '_' before matching: a literal space inside a case bracket class is
    # a bash parse error, while a space in the VALUE is harmless.)
    local testname
    testname=$(printf '%s' "$name" | tr ' ' '_')
    case "$testname" in
        ''|*[!A-Za-z0-9_-]*)
            echo "[HVE] Invalid Noctalia scheme name, skipping palette" >&2
            return 1
            ;;
    esac

    local palette_file=""
    case "$source" in
        custom)
            [ -f "$HOME/.config/noctalia/palettes/${name}.json" ] && palette_file="$HOME/.config/noctalia/palettes/${name}.json"
            ;;
        builtin)
            if [ -f "$HOME/.config/noctalia/colorschemes/${name}/${name}.json" ]; then
                palette_file="$HOME/.config/noctalia/colorschemes/${name}/${name}.json"
            elif [ -f "/etc/xdg/quickshell/noctalia-shell/Assets/ColorScheme/${name}/${name}.json" ]; then
                palette_file="/etc/xdg/quickshell/noctalia-shell/Assets/ColorScheme/${name}/${name}.json"
            fi
            ;;
        community)
            [ -f "$HOME/.local/state/noctalia/community-palettes/${name}.json" ] && palette_file="$HOME/.local/state/noctalia/community-palettes/${name}.json"
            ;;
        wallpaper)
            return 1  # no palette file, fall through to template output
            ;;
    esac

    [ -n "$palette_file" ] && [ -f "$palette_file" ] || return 1

    echo "[HVE] Colors from: Noctalia palette (${source})" >&2
    _hve_palette_from_file "$palette_file"
}

hve_colour_source_watch() {
    local noctalia_palettes_dir="$HOME/.config/noctalia/palettes"
    if [ -d "$noctalia_palettes_dir" ]; then
        find "$noctalia_palettes_dir" -name "*.json" -type f 2>/dev/null
    fi
    local noctalia_community_palettes_dir="$HOME/.local/state/noctalia/community-palettes"
    if [ -d "$noctalia_community_palettes_dir" ]; then
        find "$noctalia_community_palettes_dir" -name "*.json" -type f 2>/dev/null
    fi
}
