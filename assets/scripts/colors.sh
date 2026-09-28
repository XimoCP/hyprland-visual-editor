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

# Lua variable: primary = "rgb(2ec436)" or local primary = "rgb(2ec436)" (v5)
_hve_extract_lua_vars() {
    local file="$1"
    grep -E "^[[:space:]]*(local[[:space:]]+)?(primary|secondary|tertiary|surface|surface_lowest|accent|error)\s*=" "$file" 2>/dev/null | while IFS='=' read -r var val; do
        # Strip 'local' prefix and whitespace from variable name
        var=$(echo "$var" | sed 's/^[[:space:]]*local[[:space:]]*//' | tr -d ' ')
        val=$(echo "$val" | tr -d ' "')
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

# --- The applied theme is the colour authority -------------------------------
# Rule: the applied theme's backend owns its palette and declares it in the
# colour-authority descriptor; this central script only reads that
# declaration and knows no backend's directory layout. When the descriptor
# is absent or invalid, the live chain below applies, exactly as before.
# See odd/tasks/hve-capability-routing.md (unit 1b).

# Resolve the applied theme's declared palette file. Prints the path, or
# nothing. The descriptor and config.json are read with python3 (never eval).
_hve_theme_palette_file() {
    command -v python3 &>/dev/null || return 1

    local safe_dir="${HVE_SAFE_DIR:-$HOME/.cache/hve}"
    local descriptor="$safe_dir/color-authority.json"
    [ -f "$descriptor" ] || return 1

    local cfg="$HOME/.config/hve/config.json"
    [ -f "$cfg" ] || return 1

    local palette
    palette=$(python3 - "$descriptor" "$cfg" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1]) as f:
        desc = json.load(f)
    with open(sys.argv[2]) as f:
        cfg = json.load(f)
except Exception:
    sys.exit(1)
theme = desc.get("theme")
pfile = desc.get("palette_file")
pname = desc.get("palette_name")
for v in (theme, pfile, pname):
    if not isinstance(v, str) or not v.strip():
        sys.exit(1)
applied = (cfg.get("last_applied_theme") or "").strip()
if not applied or applied != theme.strip():
    sys.exit(1)
print(pfile)
PY
) || return 1
    [ -n "$palette" ] || return 1

    # Defence in depth: the only place a provider snapshot may live is under
    # this machine's theme directory. Absolute path, no traversal, an
    # existing regular file — otherwise fall through to the live chain.
    case "$palette" in
        /*) ;;
        *) return 1 ;;
    esac
    case "$palette" in
        *..*) return 1 ;;
    esac
    case "$palette" in
        "$HOME/.config/hve/themes/"*) ;;
        *) return 1 ;;
    esac
    [ -f "$palette" ] || return 1

    printf '%s\n' "$palette"
}

# The palette name the applied theme owns (from the same descriptor), or empty.
_hve_theme_palette_name() {
    local palette_file
    palette_file=$(_hve_theme_palette_file) || return 1
    [ -n "$palette_file" ] || return 1

    local safe_dir="${HVE_SAFE_DIR:-$HOME/.cache/hve}"
    local descriptor="$safe_dir/color-authority.json"
    local name
    name=$(python3 - "$descriptor" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1]) as f:
        desc = json.load(f)
except Exception:
    sys.exit(1)
pname = desc.get("palette_name")
if not isinstance(pname, str) or not pname.strip():
    sys.exit(1)
print(pname)
PY
) || return 1
    [ -n "$name" ] || return 1

    # SECURITY: this name is interpolated into a live palette path and passed
    # to a CLI. Allow only plain names: letters, digits, dot, underscore,
    # space and hyphen; no slash, no traversal, no leading dot or dash, and
    # no control characters (a hostile descriptor must not inject a path or
    # an argument).
    local stripped
    stripped=$(printf '%s' "$name" | tr -d 'A-Za-z0-9._ -')
    [ -z "$stripped" ] || return 1
    case "$name" in
        */*|*..*|.*|-*) return 1 ;;
    esac
    printf '%s\n' "$name"
}

# Priority 0: the applied theme's saved snapshot.
_hve_try_theme_palette() {
    local palette_file
    palette_file=$(_hve_theme_palette_file) || return 1
    [ -n "$palette_file" ] || return 1
    echo "[HVE] Colors from: applied theme snapshot" >&2
    _hve_palette_from_file "$palette_file"
}

# Turn a Noctalia-shaped palette file into validated HVE_* assignments.
_hve_palette_from_file() {
    local palette_file="$1"

    # SECURITY: the palette path is passed to python as argv, never embedded
    # in the python source, so no injection is possible via the file path.
    # Python prints sanitized HVE_* assignments to a private temp file (0600).
    local tmp_palette
    tmp_palette=$(mktemp) || return 1
    chmod 600 "$tmp_palette"

    if ! python3 - "$palette_file" >"$tmp_palette" 2>/dev/null <<'PY'
import json, sys

with open(sys.argv[1]) as f:
    data = json.load(f)

# Handle both {dark: {...}, light: {...}} (full scheme)
# and {mPrimary: ..., ...} (direct palette)
if 'dark' in data:
    palette = data['dark']
else:
    palette = data

mapping = {
    'mPrimary': 'HVE_PRIMARY',
    'mSecondary': 'HVE_SECONDARY',
    'mTertiary': 'HVE_TERTIARY',
    'mSurface': 'HVE_SURFACE',
    'mSurfaceVariant': 'HVE_SURFACE_LOWEST',
    'mError': 'HVE_ERROR',
    'mOutline': 'HVE_OUTLINE',
    'mShadow': 'HVE_SHADOW',
}

# HVE uses 'accent' which maps to primary (the main brand color)
if 'mPrimary' in palette:
    val = palette['mPrimary']
    print('HVE_ACCENT=' + ('#' + val if not val.startswith('#') else val))

for m3_key, hve_key in mapping.items():
    if m3_key in palette:
        val = palette[m3_key]
        print(hve_key + '=' + ('#' + val if not val.startswith('#') else val))
PY
    then
        rm -f "$tmp_palette"
        return 1
    fi

    # SECURITY: validate every emitted assignment BEFORE it can run. Variable
    # names must match ^HVE_[A-Z_]+$ and values must be exactly
    # ^#?[0-9a-fA-F]{6}$. Lines that fail are skipped — raw palette content
    # is never eval'd. Only validated lines are sourced (from a 0600 file).
    local safe_palette
    safe_palette=$(mktemp) || { rm -f "$tmp_palette"; return 1; }
    chmod 600 "$safe_palette"

    local line var val
    while IFS= read -r line; do
        [ -z "$line" ] && continue
        case "$line" in
            *=*) ;;
            *) continue ;;  # not an assignment
        esac
        var="${line%%=*}"
        val="${line#*=}"

        case "$var" in
            HVE_[A-Z_]*) ;;
            *) continue ;;
        esac
        # Strict: var must be exactly HVE_ + one-or-more [A-Z_]. The prefix
        # pattern above alone is insufficient: HVE_X;payload passes it, and a
        # palette value containing a newline lets that whole line be sourced.
        case "$var" in
            *[!A-Z_]*) continue ;;
        esac

        # Value must be exactly [#]rrggbb (strip optional leading '#').
        case "$val" in
            '#'*) val="${val#\#}" ;;
        esac
        case "$val" in
            ??????) ;;
            *) continue ;;
        esac
        case "$val" in
            *[!0-9a-fA-F]*) continue ;;
        esac

        printf '%s\n' "$line" >>"$safe_palette"
    done <"$tmp_palette"
    rm -f "$tmp_palette"

    if [ -s "$safe_palette" ]; then
        # shellcheck disable=SC1090
        . "$safe_palette"
        rm -f "$safe_palette"
        return 0
    fi
    rm -f "$safe_palette"
    return 1
}

# --- Tertiary completion (a missing tertiary must never alias the secondary) ---
# The active scheme may expose only primary/secondary — Noctalia v5's wallpaper
# templates render no tertiary (verified live). Aliasing the secondary painted
# the strip with two identical chips, which reads as a render bug. Resolve the
# third role from a REAL source of the SAME scheme, in preference order, and
# only then fall back to a documented, distinct colour:
#
#   1. (already covered) the Noctalia palette path sets HVE_TERTIARY directly.
#   2. The v4 palette file, ACCEPTED ONLY when its primary agrees with the
#      detected primary: a v4 file left over from an older scheme must not leak
#      its tertiary into the current one (this machine has exactly that case).
#   3. A shell-generated terminal template of the current scheme: its color4
#      is the palette's 4th accent — the third role after primary (color2) and
#      secondary (color3). Verified live as the coherent violet #9e70d6.
#   4. HVE_TERTIARY_FALLBACK (see hve_load_colors) — documented, distinct.
_hve_tertiary_from_v4_palette() {
    local v4="$HVE_HYPR_DIR/noctalia/noctalia-colors.lua"
    [ -f "$v4" ] || return 1

    local v4_vars v4_primary v4_tertiary
    v4_vars=$(_hve_extract_lua_vars "$v4")
    v4_primary=$(printf '%s\n' "$v4_vars" | grep '^HVE_PRIMARY=' | head -1)
    v4_primary="${v4_primary#HVE_PRIMARY=}"

    # Coherence gate: skip a palette whose primary is a different scheme.
    if [ -n "$v4_primary" ] && [ -n "$HVE_PRIMARY" ] && [ "$v4_primary" != "$HVE_PRIMARY" ]; then
        echo "[HVE] Skipping stale v4 palette (primary $v4_primary != $HVE_PRIMARY)" >&2
        return 1
    fi

    v4_tertiary=$(printf '%s\n' "$v4_vars" | grep '^HVE_TERTIARY=' | head -1)
    v4_tertiary="${v4_tertiary#HVE_TERTIARY=}"
    [ -n "$v4_tertiary" ] && echo "$v4_tertiary" && return 0
    return 1
}

_hve_tertiary_from_terminal_template() {
    local candidate hex
    for candidate in \
        "$HOME/.config/kitty/themes/noctalia.conf" \
        "$HOME/.config/kitty/current-theme.conf"
    do
        [ -f "$candidate" ] || continue
        hex=$(grep -E '^[[:space:]]*color4[[:space:]]+' "$candidate" 2>/dev/null \
            | head -1 | grep -oE '[0-9a-fA-F]{6}' | head -1)
        hex=$(_hve_normalize_color "$hex")
        [ -n "$hex" ] && echo "$hex" && return 0
    done
    return 1
}

# Only fills HVE_TERTIARY when the detection chain left it empty.
_hve_complete_tertiary() {
    [ -n "$HVE_TERTIARY" ] && return 0

    local resolved
    resolved=$(_hve_tertiary_from_v4_palette) && HVE_TERTIARY="$resolved" && return 0
    resolved=$(_hve_tertiary_from_terminal_template) && HVE_TERTIARY="$resolved" && return 0
    return 1
}

# Documented fallback when no real tertiary source exists at all. Catppuccin
# Mocha teal, deliberately distinct from the primary (#cba6f7) and secondary
# (#89b4fa) fallbacks below, and never a silent alias of either.
HVE_TERTIARY_FALLBACK="#94e2d5"

# --- Theme colour ownership (read-only predicate) ---------------------------
# Answers "does the applied theme own its colours?": the descriptor is
# present, valid and non-stale. Reuses _hve_theme_palette_file (which checks
# the theme match, the safe path and the existing file) plus the palette
# name check. READ-ONLY: it never writes, copies or calls a backend CLI —
# re-asserting is the app's business through the `assert-color-authority`
# IPC verb. See odd/tasks/hve-capability-routing.md (unit 1c3).
# Usage: hve_theme_owns_colours
hve_theme_owns_colours() {
    local palette_file palette_name
    palette_file=$(_hve_theme_palette_file) || return 1
    palette_name=$(_hve_theme_palette_name) || return 1
    [ -n "$palette_file" ] && [ -n "$palette_name" ] || return 1
}

# --- Colour-source loader (one new source = one new file) -------------------
# A colour source declares its capability; this file only reads those
# declarations and knows no source's layout. Adding a source means adding
# one module under color_sources.d/, never editing this chain.
#
# Module contract: each color_sources.d/*.sh defines exactly these four
# functions and nothing else at top level:
#   hve_colour_source_id()       - prints its stable id (for example "pywal")
#   hve_colour_source_priority() - prints an integer; lower is tried first
#   hve_colour_source_try()      - returns 0 and exports the HVE_*
#                                  variables on success, 1 otherwise
#   hve_colour_source_watch()    - prints the paths it wants watched, one
#                                  per line (globs allowed); nothing when
#                                  there is nothing to watch
# Modules must be safe under "set -u", must quote every expansion, and must
# never eval file content or any external data. Sourcing a module only runs
# its top-level function definitions; shipped modules are trusted exactly
# like this file (both ship in the same scripts directory), while palette
# files and descriptors stay untrusted data and are only ever read.
#
# Canonical order in ONE place: the priorities below preserve the historic
# chain exactly (10 applied-theme snapshot, 20 scheme palette file,
# 30 rendered template, 40 module slot, 50 generated output, 60 config
# scan). Priorities 20, 30, 40, 50 and 60 now live in color_sources.d/
# modules; the legacy table below keeps only the still-inline step
# (priority 10), so the overall order does not change.
# Steps run sorted by priority; ties break by id.
_HVE_LEGACY_PRIORITIES="10"

# Modules live beside this file so installs, checkouts and sandboxes that
# copy both keep working without a configured path.
_HVE_COLOR_SOURCES_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-}")/color_sources.d" 2>/dev/null && pwd)"

# Historic id of the still-inline step, for the tie-break only.
_hve_legacy_id_for() {
    case "$1" in
        10) printf '%s\n' "theme-snapshot" ;;
        *) return 1 ;;
    esac
}

# Runs one still-inline step by priority. The module slots
# (20, 30, 40, 50, 60) are NOT here: a module step sources its file and
# calls hve_colour_source_try instead.
_hve_run_legacy_step() {
    case "$1" in
        10) _hve_try_theme_palette ;;
        *) return 1 ;;
    esac
}

# Prints one "priority<TAB>id<TAB>kind<TAB>payload" line per available step:
# the legacy table plus every valid module. A module is valid only when it
# defines its id and priority functions, its id is non-empty, and its
# priority is an integer; anything else is skipped silently.
_hve_colour_steps() {
    local prio id
    # shellcheck disable=SC2086
    for prio in $_HVE_LEGACY_PRIORITIES; do
        id=$(_hve_legacy_id_for "$prio") || continue
        printf '%s\t%s\t%s\t%s\n' "$prio" "$id" "legacy" "$prio"
    done
    local moddir="${_HVE_COLOR_SOURCES_DIR:-}"
    if [ -n "$moddir" ] && [ -d "$moddir" ]; then
        local mod
        for mod in "$moddir"/*.sh; do
            [ -f "$mod" ] || continue
            id=$(bash -c 'source "$1" >/dev/null 2>&1; hve_colour_source_id' _ "$mod" 2>/dev/null) || continue
            prio=$(bash -c 'source "$1" >/dev/null 2>&1; hve_colour_source_priority' _ "$mod" 2>/dev/null) || continue
            case "$id" in
                ''|*[!A-Za-z0-9._-]*) continue ;;
            esac
            case "$prio" in
                ''|*[!0-9]*) continue ;;
            esac
            printf '%s\t%s\t%s\t%s\n' "$prio" "$id" "module" "$mod"
        done
    fi
}

# Prints every path the modules want watched, one per line. The central
# watcher reads this list instead of naming sources; the dedupe and the
# ordering of the final list stay the watcher's business. Takes one
# optional module id to skip: the watcher skips "manual-hypr" here and
# adds its paths only under its own empty-list rule (see color_watcher.sh).
_hve_colour_module_watch_paths() {
    local skip_id="${1:-}"
    local moddir="${_HVE_COLOR_SOURCES_DIR:-}"
    [ -n "$moddir" ] && [ -d "$moddir" ] || return 0
    local mod id
    for mod in "$moddir"/*.sh; do
        [ -f "$mod" ] || continue
        if [ -n "$skip_id" ]; then
            id=$(bash -c 'source "$1" >/dev/null 2>&1; hve_colour_source_id' _ "$mod" 2>/dev/null) || continue
            [ "$id" = "$skip_id" ] && continue
        fi
        bash -c 'source "$1" >/dev/null 2>&1; hve_colour_source_watch' _ "$mod" 2>/dev/null || continue
    done
}

# --- Main ---

hve_load_colors() {
    HVE_PRIMARY=""
    HVE_SECONDARY=""
    HVE_TERTIARY=""
    HVE_SURFACE=""
    HVE_SURFACE_LOWEST=""
    HVE_ACCENT=""

    # Detection priority:
    #   1. Applied theme snapshot (the theme owns the colours; see above)
    #   2. Noctalia full palette (direct from scheme file — has ALL M3 colors)
    #   3. Noctalia template output (fallback for wallpaper schemes, or v4)
    #   4. Pywal / Matugen / Manual
    local steps step_prio step_id step_kind step_payload tab
    tab=$(printf '\t')
    steps=$(_hve_colour_steps | LC_ALL=C sort -t "$tab" -k1,1n -k2,2)
    while IFS="$tab" read -r step_prio step_id step_kind step_payload; do
        [ -n "$step_kind" ] || continue
        if [ "$step_kind" = "legacy" ]; then
            _hve_run_legacy_step "$step_payload" && break
        else
            # shellcheck disable=SC1090
            source "$step_payload" >/dev/null 2>&1 && hve_colour_source_try && break
        fi
        # A failed step never leaves partial state for the next one.
        HVE_PRIMARY=""
        HVE_SECONDARY=""
        HVE_TERTIARY=""
        HVE_SURFACE=""
        HVE_SURFACE_LOWEST=""
        HVE_ACCENT=""
    done <<< "$steps"

    # A scheme with no tertiary gets one from a real same-scheme source when
    # one exists (see _hve_complete_tertiary). Only when nothing exists do the
    # final fallbacks run — and the tertiary fallback is its OWN documented
    # colour, never the secondary's.
    _hve_complete_tertiary || true
    : "${HVE_PRIMARY:=#cba6f7}"
    : "${HVE_SECONDARY:=#89b4fa}"
    : "${HVE_TERTIARY:=${HVE_TERTIARY_FALLBACK}}"
    : "${HVE_SURFACE:=#1e1e2e}"
    : "${HVE_SURFACE_LOWEST:=${HVE_SURFACE}}"
    : "${HVE_ACCENT:=${HVE_PRIMARY}}"

    export HVE_PRIMARY HVE_SECONDARY HVE_TERTIARY HVE_SURFACE HVE_SURFACE_LOWEST HVE_ACCENT
}

# Auto-load on source
hve_load_colors
