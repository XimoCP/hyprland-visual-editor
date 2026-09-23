#!/bin/bash
# HVE reload coalescer: one `hyprctl reload` per change-burst, never lost.
#
# Two modes, one file:
#   sourced  — defines `hve_reload_queue` (called by assemble.sh)
#   `drain`  — runs the single-owner background drainer loop
#
# Protocol (see odd/tasks/coalesce-config-reloads.md):
#   - `reload.requested` (atomic) is "a reload is owed". It is written BEFORE
#     the drainer spawn, so a losing spawn exits knowing the winning drainer
#     will pick it up.
#   - Only ONE drainer runs (flock on `reload.drain.lock`); it fires once the
#     burst has been quiet for the drain window, then loops forever.
#   - Invariant: an EXTRA reload is always safe, a LOST one never is. The
#     pending evidence must not disappear before a reload has actually fired:
#     the drainer CLAIMS the marker (`mv` → `reload.claimed`, atomic) before
#     the drain window and consumes it (`rm`) only AFTER a successful fire.
#     A drainer killed inside the window — or facing a down Hyprland — leaves
#     the claim behind, and the next drainer pass fires it. Worst outcomes:
#     a reload that arrives LATER, or (a crash right after a successful fire)
#     ONE redundant reload on the next pass — the intended trade.
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
        if [ -f "$HVE_SAFE_DIR/reload.requested" ] || [ -f "$HVE_SAFE_DIR/reload.claimed" ]; then
            # Claim: fold any new marker into the in-flight claim with an
            # atomic mv. From this point until AFTER a successful fire the
            # claim is the durable memory of the owed reload — a kill at ANY
            # point in between leaves evidence the next drainer pass fires.
            if [ -f "$HVE_SAFE_DIR/reload.requested" ]; then
                mv -f "$HVE_SAFE_DIR/reload.requested" "$HVE_SAFE_DIR/reload.claimed"
            fi
            sleep "$drain_secs"
            # A request that landed during the grace period gets its own
            # drain (this is the burst-folding window: quiet-then-fire).
            if [ -f "$HVE_SAFE_DIR/reload.requested" ]; then
                continue
            fi
            # Fire-time guard: Hyprland may have died during the window —
            # same skip semantics as the request-time guard (today's code).
            # While it is down the claim STAYS pending and retries next pass:
            # a delayed reload is harmless, a lost one never is.
            if pgrep -x "Hyprland" > /dev/null 2>&1; then
                if hyprctl reload > /dev/null 2>&1; then
                    # Consume ONLY after a successful fire. A crash between
                    # the fire and this removal leaves the claim: the next
                    # drainer pass fires one redundant reload — the intended
                    # trade (an extra reload is safe, a lost one never is).
                    rm -f "$HVE_SAFE_DIR/reload.claimed"
                fi
            fi
        else
            sleep "$drain_secs"
        fi
    done
}

if [ "${1:-}" = "drain" ]; then
    hve_reload_drain
fi