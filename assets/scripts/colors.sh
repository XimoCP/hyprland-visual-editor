#!/bin/bash
# shellcheck disable=SC2034
# --- HVE Smart Color Extractor ---
# Detects color tool (Noctalia/pywal/matugen/manual) and extracts colors.

HVE_HYPR_DIR="${HVE_HYPR_DIR:-$HOME/.config/hypr}"

# HVE config root, mirroring Rust's `dirs::config_dir()` (src/config.rs):
# `$XDG_CONFIG_HOME` when set, else `$HOME/.config`, plus `hve`. This file is
# sourced standalone (it does not source utils.sh), so the resolution lives
# here too instead of assuming the legacy config location under `$HOME`.
# A relative `XDG_*_HOME` is invalid under the XDG spec and `dirs` ignores it,
# so it must be ignored here as well — the same helper lives in utils.sh and
# in the installer.
_hve_xdg_dir() {
    case "${1:-}" in
        /*) printf '%s' "$1" ;;
        *) printf '%s' "$2" ;;
    esac
}
HVE_CONFIG_DIR="${HVE_CONFIG_DIR:-$(_hve_xdg_dir "${XDG_CONFIG_HOME:-}" "$HOME/.config")/hve}"

# --- The palette roster: ONE declaration, consumed everywhere ----------
# Every colour role the overlay may reference, in emission order. This list
# is the single source of the whole role lifecycle: the reset, the
# failed-step cleanup, the final fallbacks, the export, the variable-name
# derivation, the extractor's accepted names and the palette lines the
# assembler prints. A further role is added HERE (plus its value in
# hve_palette_token_fallback) and nowhere else — a role that is emitted but
# never reset, or reset but never exported, is exactly the live defect this
# roster exists to make impossible.
# The border presets and the tune-pane specs promise all of these:
# primary, secondary, tertiary, error, surface, surface_lowest, accent.
HVE_PALETTE_TOKENS="primary secondary tertiary error surface surface_lowest accent"

# Variable holding one role's value ("surface_lowest" → HVE_SURFACE_LOWEST).
# A derived name can never disagree with the roster it came from, unlike a
# second hand-written list of variable names.
hve_palette_token_var() {
    printf 'HVE_%s\n' "$(printf '%s' "${1:-}" | tr '[:lower:]' '[:upper:]')"
}

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
# The accepted names ARE the roster (see HVE_PALETTE_TOKENS): a role the
# overlay can reference is therefore extractable from a backend rendering,
# and a new role needs no edit here.
_hve_extract_lua_vars() {
    local file="$1" names
    names=$(printf '%s' "${HVE_PALETTE_TOKENS:-}" | tr ' ' '|')
    [ -n "$names" ] || return 0
    grep -E "^[[:space:]]*(local[[:space:]]+)?(${names})\s*=" "$file" 2>/dev/null | while IFS='=' read -r var val; do
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

    # Cache-dir convention (unit 1e, F5): same default as utils.sh, which
    # mirrors Rust's `hve_cache_dir()` — `$XDG_CACHE_HOME` else `$HOME/.cache`.
    local safe_dir="${HVE_SAFE_DIR:-$(_hve_xdg_dir "${XDG_CACHE_HOME:-}" "$HOME/.cache")/hve}"
    local descriptor="$safe_dir/color-authority.json"
    [ -f "$descriptor" ] || return 1

    local cfg="$HVE_CONFIG_DIR/config.json"
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
        "$HVE_CONFIG_DIR/themes/"*) ;;
        *) return 1 ;;
    esac
    [ -f "$palette" ] || return 1

    # Symlink confinement (capability-routing unit 1e, F3): a symlinked
    # path component can point outside the theme directory, so the
    # textual prefix match above is not enough. Reject a palette path
    # that IS a symlink, and resolve the PARENT directory (never the
    # file itself — `cd -P` plus `pwd`) to confirm it still lives under
    # the themes directory. Cheap, `set -u`-safe, no recursion.
    [ -L "$palette" ] && return 1
    local _hve_parent _hve_real_parent _hve_themes_root _hve_real_root
    _hve_parent=$(dirname -- "$palette")
    _hve_real_parent=$(cd -P -- "$_hve_parent" 2>/dev/null && pwd) || return 1
    _hve_themes_root="$HVE_CONFIG_DIR/themes"
    if _hve_real_root=$(cd -P -- "$_hve_themes_root" 2>/dev/null && pwd); then
        case "$_hve_real_parent/" in
            "$_hve_real_root/"*) ;;
            *) return 1 ;;
        esac
    fi
    # (When the themes root itself cannot be resolved it does not exist,
    # so no symlink could have been traversed — the -f check above
    # governs and the textual prefix already passed.)

    printf '%s\n' "$palette"
}

# The palette name the applied theme owns (from the same descriptor), or empty.
_hve_theme_palette_name() {
    local palette_file
    palette_file=$(_hve_theme_palette_file) || return 1
    [ -n "$palette_file" ] || return 1

    local safe_dir="${HVE_SAFE_DIR:-$(_hve_xdg_dir "${XDG_CACHE_HOME:-}" "$HOME/.cache")/hve}"
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
    # The log line reports what is true: it prints only AFTER the
    # snapshot parsed (unit 1e, F4) — a failed parse falls through to
    # the live chain silently instead of claiming the snapshot won.
    _hve_palette_from_file "$palette_file" || return 1
    echo "[HVE] Colors from: applied theme snapshot" >&2
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
# The active scheme may expose only primary/secondary — some rendered
# templates carry no tertiary. Aliasing the secondary painted the strip
# with two identical chips, which reads as a render bug. The step that
# WON the chain may complete the third role from its OWN backend sources
# through the optional hve_colour_source_tertiary hook (see the loader
# header); no module may read another backend's files to fill the gap.
# Only when the winner declares no hook (or its hook reports 1) does the
# documented, distinct colour below apply — never the secondary's.
_hve_complete_tertiary() {
    [ -n "${HVE_TERTIARY:-}" ] && return 0

    # Route to the winner: only a module winner can be asked (the
    # still-inline snapshot step either set a tertiary itself or has no
    # backend to ask). Re-source the winner so its own hook is the one
    # that runs, then call it when it declares one.
    local winner
    winner=$(hve_colour_winner_module) || return 1
    [ -f "$winner" ] || return 1
    # shellcheck disable=SC1090
    source "$winner" >/dev/null 2>&1 || return 1
    declare -F hve_colour_source_tertiary >/dev/null 2>&1 || return 1
    hve_colour_source_tertiary
}

# Documented fallbacks used only when NO source supplied a role (a tier of the
# palette's own, since this file is the palette authority for the overlay).
# Catppuccin Mocha, the set HVE's colours are drawn from. Each is deliberately
# distinct from its siblings — a missing role is never satisfied by silently
# aliasing another one — except where the role is defined as a derivative of
# another (surface_lowest of the surface, accent of the primary).
HVE_PRIMARY_FALLBACK="#cba6f7"
HVE_SECONDARY_FALLBACK="#89b4fa"
# Tertiary: Mocha teal, deliberately distinct from the primary and secondary
# fallbacks above, and never a silent alias of either.
HVE_TERTIARY_FALLBACK="#94e2d5"
# Error: Mocha red. Distinct from every role above, so `error` in a preset
# narrows to a real alert colour instead of repeating the accent.
HVE_ERROR_FALLBACK="#f38ba8"
HVE_SURFACE_FALLBACK="#1e1e2e"

# The final fallback of one declared role. A role the roster declares but this
# function does not answer is a wiring bug: the caller refuses it loudly
# instead of writing an empty value into the overlay.
hve_palette_token_fallback() {
    case "${1:-}" in
        primary) printf '%s\n' "$HVE_PRIMARY_FALLBACK" ;;
        secondary) printf '%s\n' "$HVE_SECONDARY_FALLBACK" ;;
        tertiary) printf '%s\n' "$HVE_TERTIARY_FALLBACK" ;;
        error) printf '%s\n' "$HVE_ERROR_FALLBACK" ;;
        surface) printf '%s\n' "$HVE_SURFACE_FALLBACK" ;;
        # Derivate roles: the surface and the primary own their values, so
        # their fallbacks are those values — resolved, not re-listed.
        surface_lowest) printf '%s\n' "${HVE_SURFACE:-$HVE_SURFACE_FALLBACK}" ;;
        accent) printf '%s\n' "${HVE_PRIMARY:-$HVE_PRIMARY_FALLBACK}" ;;
        *) return 1 ;;
    esac
}

# Clears every declared role, so no step ever leaves partial state behind for
# the next one. Driven by the roster: a role can never be half-reset.
_hve_reset_palette() {
    local tok var
    # shellcheck disable=SC2086  # the roster is one space-separated list
    for tok in ${HVE_PALETTE_TOKENS:-}; do
        var=$(hve_palette_token_var "$tok")
        printf -v "$var" '%s' ""
    done
}

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
# Module contract: each color_sources.d/*.sh defines these four functions
# and nothing else at top level, except for the optional refresh:
#   hve_colour_source_id()       - prints its stable id (for example "pywal")
#   hve_colour_source_priority() - prints an integer; lower is tried first
#   hve_colour_source_try()      - returns 0 and exports the HVE_*
#                                  variables on success, 1 otherwise
#   hve_colour_source_watch()    - prints the paths it wants watched, one
#                                  per line (globs allowed); nothing when
#                                  there is nothing to watch
# A module MAY also define:
#   hve_colour_source_refresh()  - re-asserts the backend's own rendered
#                                  output after one of its declared paths
#                                  changed (its own tool, its own files).
#                                  Returns 0 when the refresh ran, 1 when it
#                                  had nothing to do; a failed backend tool is
#                                  reported non-zero. The refresh belongs to
#                                  the backend — the core only routes it: it
#                                  asks the loader which module declared a
#                                  changed path and runs that module's refresh
#                                  once, never naming the tool or its layout.
#   hve_colour_source_tertiary() - completes the third role from the
#                                  module's OWN sources when it won the chain
#                                  but left HVE_TERTIARY empty. Sets
#                                  HVE_TERTIARY and returns 0 when its own
#                                  source can supply one, 1 otherwise. It must
#                                  only read that backend's own files:
#                                  reaching into another backend's files to
#                                  fill the gap is forbidden — the documented
#                                  HVE_TERTIARY_FALLBACK covers that case.
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

# The step that won the last hve_load_colors run: "legacy:<priority>" for a
# still-inline step, the module file path for a module winner, empty when
# no step won. _hve_complete_tertiary routes through it; other callers read
# it with hve_colour_winner_module below.
_HVE_COLOUR_WINNER=""

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

# Prints the module file that declares the given path in its watch list
# (exact line match), or nothing when no module declares it. The central
# watcher reads this to route a changed path to its owning module; the
# first match in filename order wins when two modules list one path.
_hve_colour_module_for_path() {
    local needle="${1:-}"
    [ -n "$needle" ] || return 1
    local moddir="${_HVE_COLOR_SOURCES_DIR:-}"
    [ -n "$moddir" ] && [ -d "$moddir" ] || return 1
    local mod line
    for mod in "$moddir"/*.sh; do
        [ -f "$mod" ] || continue
        while IFS= read -r line; do
            [ -n "$line" ] || continue
            if [ "$line" = "$needle" ]; then
                printf '%s\n' "$mod"
                return 0
            fi
        done < <(bash -c 'source "$1" >/dev/null 2>&1; hve_colour_source_watch' _ "$mod" 9>&- 2>/dev/null)
    done
    return 1
}

# Prints the winning module file of the last hve_load_colors run, or
# nothing (status 1) when the winner was a still-inline step or no step
# won. The completion step routes through this so the winner's own hook
# is the one that runs.
hve_colour_winner_module() {
    case "${_HVE_COLOUR_WINNER:-}" in
        ""|legacy:*) return 1 ;;
        *) printf '%s\n' "$_HVE_COLOUR_WINNER" ;;
    esac
}

# Runs one module file's declared refresh (see the module contract above).
# A module without hve_colour_source_refresh has nothing to do (status 1).
# What to log is the caller's business; this helper only routes.
_hve_colour_module_refresh() {
    local mod="${1:-}"
    [ -n "$mod" ] && [ -f "$mod" ] || return 1
    bash -c 'source "$1" >/dev/null 2>&1; declare -F hve_colour_source_refresh >/dev/null 2>&1 || exit 1; hve_colour_source_refresh' _ "$mod" 9>&-
}

# --- Main ---

hve_load_colors() {
    # Every declared role starts empty: the roster drives the reset, so a role
    # added to it can never be forgotten here.
    _hve_reset_palette
    _HVE_COLOUR_WINNER=""

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
            _hve_run_legacy_step "$step_payload" && { _HVE_COLOUR_WINNER="legacy:$step_payload"; break; }
        else
            # shellcheck disable=SC1090
            source "$step_payload" >/dev/null 2>&1 && hve_colour_source_try && { _HVE_COLOUR_WINNER="$step_payload"; break; }
        fi
        # A failed step never leaves partial state for the next one.
        _hve_reset_palette
    done <<< "$steps"

    # A scheme with no tertiary gets one from a real same-scheme source when
    # one exists (see _hve_complete_tertiary). Only when nothing exists do the
    # final fallbacks run — and the tertiary fallback is its OWN documented
    # colour, never the secondary's.
    _hve_complete_tertiary || true

    # Final fallbacks and export, both driven by the roster: a role can never
    # be exported without a value, nor have a value the overlay cannot print.
    local tok var fallback
    # shellcheck disable=SC2086  # the roster is one space-separated list
    for tok in ${HVE_PALETTE_TOKENS:-}; do
        var=$(hve_palette_token_var "$tok")
        if [ -z "${!var:-}" ]; then
            fallback=$(hve_palette_token_fallback "$tok") || return 1
            if [ -z "$fallback" ]; then
                echo "[HVE] palette token '$tok' has no fallback value" >&2
                return 1
            fi
            printf -v "$var" '%s' "$fallback" || return 1
        fi
        export "$var"
    done
}

# Auto-load on source
hve_load_colors
