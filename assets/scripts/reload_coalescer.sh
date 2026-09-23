#!/bin/bash
# HVE reload coalescer: one `hyprctl reload` per change-burst, never lost.
#
# Two modes, one file:
#   sourced  — defines `hve_reload_queue` (called by assemble.sh)
#   `drain`  — runs the single-owner background drainer loop
#
# Protocol (see odd/tasks/coalesce-config-reloads.md):
#   - `reload.requested` (atomic) is the "a reload is owed" marker. It is
#     written BEFORE the drainer spawn, so a losing spawn exits knowing the
#     winning drainer will pick it up.
#   - Only ONE drainer runs (flock on `reload.drain.lock`); it fires once the
#     burst has been quiet for the drain window, then loops forever.
#   - NEVER LOST: a marker survives a killed drainer; the next request's
#     drainer consumes it. The worst acceptable failure is a reload that
#     arrives LATER, never one that never arrives.
#   - Disable: HVE_RELOAD_COALESCE=0 → the queue performs today's exact
#     inline reload. Fail open: a missing `flock` → same immediate reload,
#     with one warning line (precedent: color_watcher.sh).

# --- PATHS ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
# shellcheck source=/dev/null
source "$SCRIPT_DIR/utils.sh"

# Drain window in milliseconds (default 600). Bursts shorter than the window
# fold into ONE reload; longer ones split into several — safe either way,
# because splitting means MORE reloads, never FEWER.
HVE_RELOAD_DRAIN_MS="${HVE_RELOAD_DRAIN_MS:-600}"

# Queue one reload. Called by assemble.sh after the overlay write.
hve_reload_queue() {
    # Disabled: today's exact inline reload (request-time pgrep guard kept).
    if [ "${HVE_RELOAD_COALESCE:-1}" = "0" ]; then
        if pgrep -x "Hyprland" > /dev/null 2>&1; then
            hyprctl reload > /dev/null 2>&1
        fi
        return 0
    fi

    # Fail open when the single-owner guarantee cannot run: a missing flock
    # must never silence the reloads. Warning, then today's immediate reload.
    if ! command -v flock >/dev/null 2>&1; then
        echo "[HVE] reload coalescer unavailable: flock not found in PATH — reloading immediately" >&2
        if pgrep -x "Hyprland" > /dev/null 2>&1; then
            hyprctl reload > /dev/null 2>&1
        fi
        return 0
    fi

    # The marker is the durable memory of the reload being owed. Write it
    # atomically BEFORE spawning so no ordering can strand it.
    mkdir -p "$HVE_SAFE_DIR"
    echo "$(date +%s%N)" > "$HVE_SAFE_DIR/reload.requested.tmp"
    mv "$HVE_SAFE_DIR/reload.requested.tmp" "$HVE_SAFE_DIR/reload.requested"

    # Spawn the drainer detached with its OWN stdio: callers may be the
    # engine capturing pipes (`Command::output`), and a drainer holding a
    # capture pipe would block the caller until the drainer exits. A losing
    # spawn (flock taken) exits 0 instantly; the winner drains the marker.
    nohup bash "$HVE_SCRIPTS_DIR/reload_coalescer.sh" drain >/dev/null 2>&1 &
}

# Single-owner drainer loop. Exactly one instance per machine: losers exit 0.
hve_reload_drain() {
    exec 9>"$HVE_SAFE_DIR/reload.drain.lock"
    if ! flock -n 9; then
        exit 0
    fi
    echo $$ > "$HVE_SAFE_DIR/reload.drain.pid"

    local drain_secs
    drain_secs=$(awk -v ms="$HVE_RELOAD_DRAIN_MS" 'BEGIN { printf "%.3f", ms / 1000 }')

    while true; do
        if [ -f "$HVE_SAFE_DIR/reload.requested" ]; then
            rm -f "$HVE_SAFE_DIR/reload.requested"
            sleep "$drain_secs"
            # A request that landed during the grace period gets its own
            # drain (this is the burst-folding window: quiet-then-fire).
            if [ -f "$HVE_SAFE_DIR/reload.requested" ]; then
                continue
            fi
            # Fire-time guard: Hyprland may have died during the window —
            # same skip semantics as the request-time guard (today's code).
            if pgrep -x "Hyprland" > /dev/null 2>&1; then
                hyprctl reload > /dev/null 2>&1
            fi
        else
            sleep "$drain_secs"
        fi
    done
}

if [ "${1:-}" = "drain" ]; then
    hve_reload_drain
fi