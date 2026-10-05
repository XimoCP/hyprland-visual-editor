#!/bin/bash
# --- HVE Color Watcher ---
# Watches color source files for changes and triggers overlay regeneration.
# Supports: Noctalia, pywal, matugen, manual Hyprland config scan.
# Runs as a background process while HVE is active.
#
# Changes:
#   - Uses inotifywait -e close_write (fires AFTER file is fully written)
#   - Logs to $HVE_SAFE_DIR/color_watcher.log (utils.sh resolves it via XDG)
#   - Errors from assemble.sh are visible in the log

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"
# colors.sh is sourced for the theme-authority helper only. NOTE: its
# auto-load runs hve_load_colors on source and prints one extra
# `[HVE] Colors from:` line into the log at startup — that is expected.
source "$SCRIPT_DIR/colors.sh"

HVE_HYPR_DIR="$HOME/.config/hypr"
ASSEMBLE_SCRIPT="$HVE_SCRIPTS_DIR/assemble.sh"
LOG_FILE="$HVE_SAFE_DIR/color_watcher.log"
COLOR_SIGNAL="$HVE_SAFE_DIR/colors.json"
GET_COLORS_SCRIPT="$HVE_SCRIPTS_DIR/get_colors.sh"

_log() {
    echo "[HVE Watcher] $(date '+%H:%M:%S') $*" >> "$LOG_FILE"
}

# Find which color tool is active and return the files to watch
find_watch_files() {
    local files=()

    # Rendered, palette and settings paths are declared by their own
    # colour-source modules (see color_sources.d/ and the loader header in
    # colors.sh) and picked up through _hve_colour_module_watch_paths
    # below — this function must not name any backend's paths itself.

    # Colour-source modules declare their own watch paths (see the loader
    # header in colors.sh); this list only reads those declarations. The
    # manual-hypr module is skipped here: its paths are watched only when
    # nothing else was found (the fallback below).
    if command -v _hve_colour_module_watch_paths >/dev/null 2>&1; then
        while IFS= read -r module_path; do
            [ -n "$module_path" ] && files+=("$module_path")
        done < <(_hve_colour_module_watch_paths "manual-hypr" 2>/dev/null)
    fi

    # Manual fallback: the manual-hypr module's lua paths. HVE's own
    # policy, not the module's declaration: these paths are watched ONLY
    # when the list is otherwise empty. The module declares the paths;
    # this block only applies the rule.
    if [ ${#files[@]} -eq 0 ]; then
        local manual_mod="${_HVE_COLOR_SOURCES_DIR:-}/manual_hypr.sh"
        if [ -f "$manual_mod" ]; then
            while IFS= read -r f; do
                [ -n "$f" ] && files+=("$f")
            done < <(bash -c 'source "$1" >/dev/null 2>&1; hve_colour_source_watch' _ "$manual_mod" 2>/dev/null)
        fi
    fi

    # Return unique files
    printf '%s\n' "${files[@]}" | sort -u
}

# Ensure log directory exists
mkdir -p "$(dirname "$LOG_FILE")"

# Singleton guard: only one color watcher per machine
# (odd/hide-idempotency-and-singleton-watcher W1). The lock is a non-blocking
# exclusive flock on a long-lived fd, held for the whole process lifetime.
# A rejected second instance logs one line and exits 0 — it is a no-op, not
# an error: exactly one instance keeps running assemble.sh / templates-apply.
# If flock itself is missing, FAIL OPEN: the guard cannot run, so losing the
# singleton guarantee must never silently disable the colour sync — log one
# distinct warning line and continue unguarded. The capability check is real
# (`command -v`), not an exit-code guess, so a missing dependency and a
# genuine lock contention stay distinguishable.
# Guard invariant: the singleton lock lives on fd 9. `exec 9>` does NOT set
# close-on-exec, so every child would inherit fd 9 — and a `flock` rides the
# open file description, not the process, so a killed watcher whose orphaned
# child still holds fd 9 keeps the lock held (the exact defect reproduced in
# odd/watcher-startup-resilience: the orphaned inotifywait rejected every new
# watcher for up to 30 s — or forever after an ungraceful HVE death).
# THEREFORE: any long-lived child MUST close fd 9 on its own invocation
# (9>&-); closing it in this shell would release the lock. Per-command 9>&-
# is a harmless no-op when fd 9 was never opened (the fail-open path below).
mkdir -p "$HVE_SAFE_DIR"
LOCK_FILE="$HVE_SAFE_DIR/color_watcher.lock"
if ! command -v flock >/dev/null 2>&1; then
    _log "Singleton guard unavailable: flock not found in PATH — continuing UNGUARDED (duplicate watchers will not be prevented)"
else
    exec 9>"$LOCK_FILE"
    if ! flock -n 9; then
        _log "Singleton guard: another color watcher holds $LOCK_FILE — exiting without starting"
        exit 0
    fi
fi

_log "Starting watcher"

WATCH_FILES=$(find_watch_files)

if [ -z "$WATCH_FILES" ]; then
    _log "ERROR: no color source files found, exiting"
    exit 1
fi

FILE_COUNT=$(echo "$WATCH_FILES" | wc -l)
_log "Monitoring $FILE_COUNT file(s):"
while IFS= read -r f; do
    _log "  - $f"
done <<< "$WATCH_FILES"

# Build initial hashes for all watched files
declare -A LAST_HASHES
while IFS= read -r file; do
    if [ -f "$file" ]; then
        LAST_HASHES["$file"]=$(md5sum "$file" 2>/dev/null | cut -d' ' -f1)
    fi
done <<< "$WATCH_FILES"

# Helper: write current colors for HVE UI signal file
_write_color_signal() {
    if [ -f "$GET_COLORS_SCRIPT" ]; then
        bash "$GET_COLORS_SCRIPT" 9>&- > "$COLOR_SIGNAL" 2>/dev/null
    fi
}

# Notify the running HVE app to refresh its in-memory theme
_notify_hve() {
    if [ -x "$HVE_SCRIPTS_DIR/hve-ipc" ]; then
        "$HVE_SCRIPTS_DIR/hve-ipc" 9>&- refresh-theme 2>/dev/null
    fi
}

# Ask the app to re-assert the applied theme's colour authority, exactly
# once per change burst. Capability routing
# (odd/tasks/hve-capability-routing.md, unit 1c3): bash only asks — the app
# routes `assert-color-authority` to the backend that owns the colours, so
# this shell never copies palettes, never calls a backend CLI and never
# rewrites backend files (no re-hash needed: bash wrote nothing). The small
# cooldown keeps a burst from spamming the socket; `force` (startup repair)
# bypasses it.
_reassert_theme_authority() {
    local force="${1:-}"
    hve_theme_owns_colours || return 0
    local safe_dir="${HVE_SAFE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/hve}"
    local stamp_file="$safe_dir/assert-color-authority.last"
    local now
    now=$(date +%s)
    if [ "$force" != "force" ] && [ -f "$stamp_file" ]; then
        local last
        last=$(cat "$stamp_file" 2>/dev/null)
        case "$last" in
            ''|*[!0-9]*) ;;
            *) [ $((now - last)) -lt "${HVE_THEME_ASSERT_COOLDOWN:-5}" ] && return 0 ;;
        esac
    fi
    mkdir -p "$safe_dir" 2>/dev/null
    printf '%s' "$now" > "$stamp_file"
    if [ -x "$HVE_SCRIPTS_DIR/hve-ipc" ]; then
        # Two verbs: a change burst asks `assert-color-authority`, which the
        # app gates on a monitor-event window (the palette file is the
        # detector, the monitor event is the permission —
        # odd/tasks/palette-defence-on-monitor-events.md). The startup repair
        # is NOT a palette change: it must never wait for a monitor event, so
        # it goes through the ungated `repair-color-authority`.
        if [ "$force" = "force" ]; then
            "$HVE_SCRIPTS_DIR/hve-ipc" 9>&- repair-color-authority 2>/dev/null || true
            _log "Asked HVE to repair colour authority (repair-color-authority)"
        else
            "$HVE_SCRIPTS_DIR/hve-ipc" 9>&- assert-color-authority 2>/dev/null || true
            _log "Asked HVE to re-assert colour authority (assert-color-authority)"
        fi
    fi
}

# Declared backend refresh (capability routing,
# odd/tasks/hve-capability-routing.md, unit 1d4): every changed path is
# offered to the module that declared it, and each owning module's refresh
# runs once per pass, before the overlay regeneration below. The core knows
# no backend tool or layout — the module owns both. A refresh that fails or
# has nothing to do is logged and the loop continues: the overlay
# regeneration afterwards is unconditional. Afterwards every watched hash is
# re-baselined, so writes the refresh itself made cannot re-trigger the loop.
_run_declared_refreshes() {
    local f mod id
    declare -A _refreshed_mods=()
    local ran_any=false
    for f in "$@"; do
        [ -n "$f" ] || continue
        mod=$(_hve_colour_module_for_path "$f" 9>&- 2>/dev/null) || continue
        [ -n "$mod" ] || continue
        if [ -n "${_refreshed_mods[$mod]:-}" ]; then
            continue
        fi
        _refreshed_mods[$mod]=1
        id=$(bash -c 'source "$1" >/dev/null 2>&1; hve_colour_source_id' _ "$mod" 9>&- 2>/dev/null) || id="$mod"
        _log "Running declared refresh for colour module '$id'..."
        if _hve_colour_module_refresh "$mod" 9>&- >> "$LOG_FILE" 2>&1; then
            _log "Declared refresh for colour module '$id' OK"
        else
            _log "Declared refresh for colour module '$id' failed or had nothing to do (continuing)"
        fi
        ran_any=true
    done
    if [ "$ran_any" = true ]; then
        local wf
        while IFS= read -r wf; do
            if [ -f "$wf" ]; then
                LAST_HASHES["$wf"]=$(md5sum "$wf" 2>/dev/null | cut -d' ' -f1)
            fi
        done <<< "$WATCH_FILES"
    fi
}

# Repair a hijack that happened while HVE was off, before the first paint.
_reassert_theme_authority force
# Force initial refresh: ensure overlay is up-to-date when watcher starts
_log "Initial overlay refresh..."
if bash "$ASSEMBLE_SCRIPT" 9>&- >> "$LOG_FILE" 2>&1; then
    _log "Initial overlay refresh OK"
    _write_color_signal
    _notify_hve
    _log "Color signal written"
else
    _log "WARN: initial overlay refresh had issues (see log above)"
fi

while true; do
    # Wait for close_write events on any watched file (timeout 30s for periodic re-check)
    # Using close_write instead of modify — fires AFTER the file is fully written
    # Using inotifywait directly (without xargs pipe which mangles exit codes)
    # shellcheck disable=SC2086
    inotifywait -q -e close_write -t 30 $WATCH_FILES 9>&- 2>/dev/null

    # Inner loop: keep processing while rapid changes are detected, so events
    # that happen during processing don't get lost (inotifywait is one-shot).
    while true; do
        # Check if any file's content actually changed (hash-based detection)
        changed=false
        changed_files=()
        while IFS= read -r file; do
            if [ -f "$file" ]; then
                current_hash=$(md5sum "$file" 2>/dev/null | cut -d' ' -f1)
                if [ "$current_hash" != "${LAST_HASHES[$file]}" ]; then
                    LAST_HASHES["$file"]="$current_hash"
                    changed=true
                    changed_files+=("$file")
                    _log "Change detected: $file"
                fi
            else
                _log "WARN: watched file no longer exists: $file"
            fi
        done <<< "$WATCH_FILES"

        if [ "$changed" = false ]; then
            break
        fi

        # Theme authority first: re-assert the applied theme's palette before
        # the declared refreshes below, so each module's refresh and
        # assemble.sh afterwards both render the theme's colours.
        _reassert_theme_authority

        # Declared backend refreshes, once per owning module, before the
        # overlay regeneration (the timing the per-tool branch had).
        _run_declared_refreshes "${changed_files[@]}"

        _log "Regenerating overlay..."
        # Run assemble.sh — stderr goes to log so we can see if it fails
        if bash "$ASSEMBLE_SCRIPT" 9>&- >> "$LOG_FILE" 2>&1; then
            _log "Overlay regenerated OK"
            _write_color_signal
            _notify_hve
            _log "Color signal written, IPC sent"
        else
            _log "ERROR: assemble.sh failed (see log above)"
        fi
    done
done
