#!/bin/bash
# shellcheck disable=SC2034
# --- HVE Smart Color Extractor ---
# Detects color tool (Noctalia/pywal/matugen/manual) and extracts colors.

HVE_HYPR_DIR="${HVE_HYPR_DIR:-$HOME/.config/hypr}"

# --- Color format converters ---

_hve_rgba_to_hex() {
    local rgba="$1"
    local hex
    hex=$(echo "$rgba" | grep -oE '[0-9a-fA-F]{8}' | head -1)
    [ -n "$hex" ] && echo "#${hex:0:6}"
}

_hve_rgb_to_hex() {
    local rgb="$1"
    local hex
    hex=$(echo "$rgb" | grep -oE '[0-9a-fA-F]{6}' | head -1)
    [ -n "$hex" ] && echo "#${hex}"
}

_hve_normalize_color() {
    local color="$1"
    color=$(echo "$color" | tr -d ' "')

    [[ "$color" =~ ^#[0-9a-fA-F]{6}$ ]] && echo "$color" && return
    [[ "$color" =~ ^rgba ]] && _hve_rgba_to_hex "$color" && return
    [[ "$color" =~ ^rgb ]] && _hve_rgb_to_hex "$color" && return
    [[ "$color" =~ ^[0-9a-fA-F]{6}$ ]] && echo "#$color" && return
}

# --- Extractors per format ---

# Lua variable: primary = "rgb(2ec436)"
_hve_extract_lua_vars() {
    local file="$1"
    grep -E "^[[:space:]]*(primary|secondary|tertiary|surface|surface_lowest|accent|error)\s*=" "$file" 2>/dev/null | while IFS='=' read -r var val; do
        var=$(echo "$var" | tr -d ' ')
        val=$(echo "$val" | tr -d ' "')
        local hex
        hex=$(_hve_normalize_color "$val")
        # Prefix with HVE_ and uppercase
        [ -n "$hex" ] && echo "HVE_$(echo "$var" | tr '[:lower:]' '[:upper:]')=${hex}"
    done
}

# Conf variable: $primary = rgb(2ec436)
_hve_extract_conf_vars() {
    local file="$1"
    grep -E '^\$?(primary|secondary|tertiary|surface|surface_lowest|accent|error)\s*=' "$file" 2>/dev/null | while IFS='=' read -r var val; do
        var=$(echo "$var" | tr -d ' $')
        val=$(echo "$val" | tr -d ' ')
        local hex
        hex=$(_hve_normalize_color "$val")
        # Prefix with HVE_ and uppercase
        [ -n "$hex" ] && echo "HVE_$(echo "$var" | tr '[:lower:]' '[:upper:]')=${hex}"
    done
}

# Lua border gradient: active_border = { colors = { "rgba(...)", "rgba(...)" } }
_hve_extract_border_gradient() {
    local file="$1"
    grep -A5 "active_border" "$file" 2>/dev/null | grep -oE '"rgba?\([^)]+\)"' | head -2 | while read -r color; do
        color=$(echo "$color" | tr -d '"')
        local hex
        hex=$(_hve_normalize_color "$color")
        [ -n "$hex" ] && echo "$hex"
    done
}

# --- Detection & extraction per tool ---

# Noctalia: ~/.config/hypr/noctalia/noctalia-colors.{lua,conf}
_hve_try_noctalia() {
    local noctalia_lua="$HVE_HYPR_DIR/noctalia/noctalia-colors.lua"
    local noctalia_conf="$HVE_HYPR_DIR/noctalia/noctalia-colors.conf"

    # .conf updates live, .lua only on reboot — prefer .conf
    if [ -f "$noctalia_conf" ]; then
        echo "[HVE] Colors from: Noctalia (conf)" >&2
        eval "$(_hve_extract_conf_vars "$noctalia_conf")"
        return 0
    elif [ -f "$noctalia_lua" ]; then
        echo "[HVE] Colors from: Noctalia (lua)" >&2
        eval "$(_hve_extract_lua_vars "$noctalia_lua")"
        return 0
    fi
    return 1
}

# Pywal: ~/.cache/wal/colors.json
_hve_try_pywal() {
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

# Matugen: ~/.config/matugen/config.toml → find generated output
_hve_try_matugen() {
    local matugen_config="$HOME/.config/matugen/config.toml"
    if [ -f "$matugen_config" ]; then
        # Find the output template that targets hyprland
        local output_file
        output_file=$(grep -A5 'hyprland' "$matugen_config" 2>/dev/null | grep 'output' | head -1 | sed 's/.*= *//' | tr -d '"' | sed "s|~|$HOME|")

        if [ -n "$output_file" ] && [ -f "$output_file" ]; then
            echo "[HVE] Colors from: matugen ($output_file)" >&2
            # Try lua first, then conf
            local found
            found=$(_hve_extract_lua_vars "$output_file")
            [ -z "$found" ] && found=$(_hve_extract_conf_vars "$output_file")
            [ -n "$found" ] && eval "$found"
            return 0
        fi
    fi
    return 1
}

# Manual/fallback: scan hypr config files
_hve_try_manual() {
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

    # Scan .conf files
    if [ $found -eq 0 ]; then
        while IFS= read -r file; do
            local vars
            vars=$(_hve_extract_conf_vars "$file")
            if [ -n "$vars" ]; then
                eval "$vars"
                found=1
                break
            fi
        done < <(find "$HVE_HYPR_DIR" -maxdepth 2 -name "*.conf" -type f 2>/dev/null)
    fi

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

# --- Main ---

hve_load_colors() {
    HVE_PRIMARY=""
    HVE_SECONDARY=""
    HVE_TERTIARY=""
    HVE_SURFACE=""
    HVE_SURFACE_LOWEST=""
    HVE_ACCENT=""

    # Detection priority: Noctalia → pywal → matugen → manual
    _hve_try_noctalia ||
    _hve_try_pywal ||
    _hve_try_matugen ||
    _hve_try_manual

    # Final fallbacks
    : "${HVE_PRIMARY:=#cba6f7}"
    : "${HVE_SECONDARY:=#89b4fa}"
    : "${HVE_TERTIARY:=${HVE_SECONDARY}}"
    : "${HVE_SURFACE:=#1e1e2e}"
    : "${HVE_SURFACE_LOWEST:=${HVE_SURFACE}}"
    : "${HVE_ACCENT:=${HVE_PRIMARY}}"

    export HVE_PRIMARY HVE_SECONDARY HVE_TERTIARY HVE_SURFACE HVE_SURFACE_LOWEST HVE_ACCENT
}

# Auto-load on source
hve_load_colors
