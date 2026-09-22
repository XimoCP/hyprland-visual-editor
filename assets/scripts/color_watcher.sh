#!/bin/bash
# --- HVE Color Watcher ---
# Watches color source files for changes and triggers overlay regeneration.
# Supports: Noctalia, pywal, matugen, manual Hyprland config scan.
# Runs as a background process while HVE is active.
#
# Changes:
#   - Uses inotifywait -e close_write (fires AFTER file is fully written)
#   - Logs to ~/.cache/hve/color_watcher.log
#   - Errors from assemble.sh are visible in the log

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

HVE_HYPR_DIR="$HOME/.config/hypr"
ASSEMBLE_SCRIPT="$HVE_SCRIPTS_DIR/assemble.sh"
LOG_FILE="$HOME/.cache/hve/color_watcher.log"
COLOR_SIGNAL="$HOME/.cache/hve/colors.json"
GET_COLORS_SCRIPT="$HVE_SCRIPTS_DIR/get_colors.sh"
NOCTALIA_V5_SETTINGS="$HOME/.local/state/noctalia/settings.toml"

_log() {
    echo "[HVE Watcher] $(date '+%H:%M:%S') $*" >> "$LOG_FILE"
}

# Find which color tool is active and return the files to watch
find_watch_files() {
    local files=()

    # Noctalia v4: rendered Lua output in noctalia/ subdirectory
    local noctalia_lua="$HVE_HYPR_DIR/noctalia/noctalia-colors.lua"
    if [ -f "$noctalia_lua" ]; then
        files+=("$noctalia_lua")
    fi

    # Noctalia v5: rendered Lua output directly in the hypr dir
    local noctalia_v5_lua="$HVE_HYPR_DIR/noctalia.lua"
    if [ -f "$noctalia_v5_lua" ]; then
        files+=("$noctalia_v5_lua")
    fi

    # Noctalia v5: settings.toml changes when user modifies colors in Noctalia's own UI.
    # We watch it so HVE can detect external color changes and refresh.
    local noctalia_v5_settings="$HOME/.local/state/noctalia/settings.toml"
    if [ -f "$noctalia_v5_settings" ] && command -v noctalia &>/dev/null; then
        files+=("$noctalia_v5_settings")
    fi

    # Noctalia palette files: watch custom palettes and community palettes so HVE
    # picks up direct palette edits (individual color changes within a scheme).
    local noctalia_palettes_dir="$HOME/.config/noctalia/palettes"
    if [ -d "$noctalia_palettes_dir" ]; then
        while IFS= read -r pf; do
            files+=("$pf")
        done < <(find "$noctalia_palettes_dir" -name "*.json" -type f 2>/dev/null)
    fi
    local noctalia_community_palettes_dir="$HOME/.local/state/noctalia/community-palettes"
    if [ -d "$noctalia_community_palettes_dir" ]; then
        while IFS= read -r pf; do
            files+=("$pf")
        done < <(find "$noctalia_community_palettes_dir" -name "*.json" -type f 2>/dev/null)
    fi

    # Pywal
    local wal_json="$HOME/.cache/wal/colors.json"
    if [ -f "$wal_json" ]; then
        files+=("$wal_json")
    fi

    # Matugen: watch the config to detect output file changes
    local matugen_config="$HOME/.config/matugen/config.toml"
    if [ -f "$matugen_config" ]; then
        files+=("$matugen_config")
        local output_file
        output_file=$(grep -A5 'hyprland' "$matugen_config" 2>/dev/null | grep 'output' | head -1 | sed 's/.*= *//' | tr -d '"' | sed "s|~|$HOME|")
        if [ -n "$output_file" ] && [ -f "$output_file" ]; then
            files+=("$output_file")
        fi
    fi

    # Manual fallback: scan hypr Lua config files for color definitions
    if [ ${#files[@]} -eq 0 ]; then
        while IFS= read -r f; do
            files+=("$f")
        done < <(find "$HVE_HYPR_DIR" -maxdepth 2 -name "*.lua" -type f 2>/dev/null)
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
mkdir -p "$HVE_SAFE_DIR"
LOCK_FILE="$HVE_SAFE_DIR/color_watcher.lock"
exec 9>"$LOCK_FILE"
if ! flock -n 9; then
    _log "Singleton guard: another color watcher holds $LOCK_FILE — exiting without starting"
    exit 0
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
        bash "$GET_COLORS_SCRIPT" > "$COLOR_SIGNAL" 2>/dev/null
    fi
}

# Notify the running HVE app to refresh its in-memory theme
_notify_hve() {
    if [ -x "$HVE_SCRIPTS_DIR/hve-ipc" ]; then
        "$HVE_SCRIPTS_DIR/hve-ipc" refresh-theme 2>/dev/null
    fi
}

# Force initial refresh: ensure overlay is up-to-date when watcher starts
_log "Initial overlay refresh..."
if bash "$ASSEMBLE_SCRIPT" >> "$LOG_FILE" 2>&1; then
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
    inotifywait -q -e close_write -t 30 $WATCH_FILES 2>/dev/null

    # Inner loop: keep processing while rapid changes are detected, so events
    # that happen during processing don't get lost (inotifywait is one-shot).
    while true; do
        # Check if any file's content actually changed (hash-based detection)
        changed=false
        noctalia_settings_changed=false
        while IFS= read -r file; do
            if [ -f "$file" ]; then
                current_hash=$(md5sum "$file" 2>/dev/null | cut -d' ' -f1)
                if [ "$current_hash" != "${LAST_HASHES[$file]}" ]; then
                    LAST_HASHES["$file"]="$current_hash"
                    changed=true
                    _log "Change detected: $file"
                    # Track if this was Noctalia v5 settings
                    [ "$file" = "$NOCTALIA_V5_SETTINGS" ] && noctalia_settings_changed=true
                fi
            else
                _log "WARN: watched file no longer exists: $file"
            fi
        done <<< "$WATCH_FILES"

        if [ "$changed" = false ]; then
            break
        fi

        # Noctalia v5: if settings.toml changed, run templates-apply FIRST so the
        # rendered Lua files (noctalia.lua / noctalia-colors.lua) reflect the new
        # palette before assemble.sh reads them. This bridges the gap where v5's
        # color-scheme-set only persists the setting but does NOT auto-run
        # templates-apply.
        if [ "$noctalia_settings_changed" = true ] && [ -f "$NOCTALIA_V5_SETTINGS" ] && command -v noctalia &>/dev/null; then
            _log "Noctalia v5 settings changed — applying templates..."
            if noctalia msg templates-apply >> "$LOG_FILE" 2>&1; then
                _log "Templates applied"
                # Update hashes of rendered files so they don't trigger a second pass
                # v4: noctalia/noctalia-colors.lua
                # v5: noctalia.lua
                for f in \
                    "$HVE_HYPR_DIR/noctalia/noctalia-colors.lua" \
                    "$HVE_HYPR_DIR/noctalia.lua"; do
                    [ -f "$f" ] && LAST_HASHES["$f"]=$(md5sum "$f" 2>/dev/null | cut -d' ' -f1)
                done
            else
                _log "templates-apply failed (noctalia may not be available)"
            fi
        fi

        _log "Regenerating overlay..."
        # Run assemble.sh — stderr goes to log so we can see if it fails
        if bash "$ASSEMBLE_SCRIPT" >> "$LOG_FILE" 2>&1; then
            _log "Overlay regenerated OK"
            _write_color_signal
            _notify_hve
            _log "Color signal written, IPC sent"
        else
            _log "ERROR: assemble.sh failed (see log above)"
        fi
    done
done
