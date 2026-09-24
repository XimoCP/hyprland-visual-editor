#!/bin/bash
# HVE reload coalescer: one `hyprctl reload` per change-burst, never lost.
#
# Two modes, one file:
#   sourced  — defines `hve_reload_queue` (called by assemble.sh)
#   `drain`  — runs the single-owner background drainer loop
#
# Protocol (see odd/tasks/coalesce-config-reloads.md and
# odd/tasks/reload-drainer-idle-exit.md):
#   - `reload.requested` (atomic) is "a reload is owed". It is written BEFORE
#     the drainer spawn, so a losing spawn exits knowing the winning drainer
#     will pick it up.
#   - Only ONE drainer runs (flock on `reload.drain.lock`); it fires once the
#     burst has been quiet for the drain window, then keeps draining until it
#     has been idle for `HVE_RELOAD_IDLE_TICKS` (default 20, ≈12 s at the
#     default drain window) — it does not outlive its usefulness.
#   - Invariant: an EXTRA reload is always safe, a LOST one never is. The
#     pending evidence must not disappear before a reload has actually fired:
#     the drainer CLAIMS the marker (`mv` → `reload.claimed`, atomic) before
#     the drain window and consumes it (`rm`) only AFTER a successful fire.
#     A drainer killed inside the window — or facing a down Hyprland — leaves
#     the claim behind, and the next drainer pass fires it. Worst outcomes:
#     a reload that arrives LATER, or (a crash right after a successful fire)
#     ONE redundant reload on the next pass — the intended trade.
#   - Idle exit safety: the idle counter counts ONLY ticks in which NEITHER
#     the marker NOR a claim exists — a pending claim (Hyprland down) keeps
#     the drainer alive for however long the retry takes — and the exit is
#     ordered against marker writes by the `reload.request.lock` handshake:
#     the queue holds that lock (blocking) around writing the marker +
#     spawning, and the drainer takes it (blocking) only when it has decided
#     to exit, re-checks the marker under the lock, and either keeps draining
#     or exits while holding it. "Exiting at the same instant a request
#     arrives" can therefore never lose the reload.
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

    # Handshake (the correctness core): hold `reload.request.lock` (BLOCKING)
    # around "write the marker" + "spawn the drainer". The drainer takes the
    # same lock only when it has decided to exit, and re-checks the marker
    # under it — so a request arriving at the same instant a drainer exits
    # can never be lost. Held for microseconds; the spawn returns at once.
    # The lock file itself stays behind (flock semantics: the lock lives on
    # the open descriptor and dies when its every copy closes).
    exec 8>"$HVE_SAFE_DIR/reload.request.lock"
    flock 8
    echo "$(date +%s%N)" > "$HVE_SAFE_DIR/reload.requested.tmp"
    mv "$HVE_SAFE_DIR/reload.requested.tmp" "$HVE_SAFE_DIR/reload.requested"

    # Spawn the drainer detached with its OWN stdio: callers may be the
    # engine capturing pipes (`Command::output`), and a drainer holding a
    # capture pipe would block the caller until the drainer exits. A losing
    # spawn (flock taken) exits 0 instantly; the winner drains the marker.
    # The wrapper's first act drops the `reload.request.lock` descriptor this
    # spawn inherits from the handshake above: the lock must live only while
    # the queue holds it (and while the drainer's exit path re-takes it),
    # never while the drainer runs — an inherited copy would block every
    # later queue until this drainer (idle-)exits.
    nohup bash -c 'exec 8>&-; exec bash "$1" drain' _ "$HVE_SCRIPTS_DIR/reload_coalescer.sh" >/dev/null 2>&1 &
    exec 8>&-
}

# Single-owner drainer loop. Exactly one instance per machine: losers exit 0.
hve_reload_drain() {
    # Defensive: drop any `reload.request.lock` descriptor inherited from a
    # caller that spawned us under that lock (the queue already does this via
    # the wrapper, but a stray spawn must not hold the lock either).
    exec 8>&-
    exec 9>"$HVE_SAFE_DIR/reload.drain.lock"
    if ! flock -n 9; then
        exit 0
    fi
    echo $$ > "$HVE_SAFE_DIR/reload.drain.pid"
    # Pidfile hygiene: a NORMAL exit (the idle exit below) removes this
    # drainer's pidfile, so consumers never trust a stale one. SIGKILL cannot
    # run a trap — a SIGKILLed drainer necessarily leaves the pidfile behind,
    # which is why every consumer must verify /proc/<pid> liveness before
    # signalling (see the Rust harness). The trap goes ONLY after the flock
    # win: a losing drainer must never remove the WINNER's pidfile.
    trap 'rm -f "$HVE_SAFE_DIR/reload.drain.pid"' EXIT

    local drain_secs idle_ticks idle=0
    drain_secs=$(awk -v ms="$HVE_RELOAD_DRAIN_MS" 'BEGIN { printf "%.3f", ms / 1000 }')
    # Idle bound as a TICK COUNT (default 20 → ≈12 s at the default 600 ms
    # drain window), independent of the drain window, so tests can set it
    # small. The drainer exits ONLY after this many consecutive idle ticks —
    # and never while a marker or a claim is owed.
    idle_ticks="${HVE_RELOAD_IDLE_TICKS:-20}"

    while true; do
        if [ -f "$HVE_SAFE_DIR/reload.requested" ] || [ -f "$HVE_SAFE_DIR/reload.claimed" ]; then
            # Owed work exists — reset the idle budget: a drainer that just
            # worked gets a FULL idle bound before it may exit, and a pending
            # claim (Hyprland down) keeps the drainer alive for however long
            # the retry takes. Counting a claim as idle would abandon it.
            idle=0
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
            # Idle tick: NEITHER the marker NOR a claim exists. ONLY this
            # path may exit the drainer, and only after the handshake below
            # proves no request raced the decision.
            idle=$((idle + 1))
            if [ "$idle" -lt "$idle_ticks" ]; then
                sleep "$drain_secs"
                continue
            fi

            # Handshake (the correctness core): take `reload.request.lock`
            # (BLOCKING). The queue holds it only around writing the marker
            # and spawning, so waiting is microseconds — and while we hold
            # the DRAIN lock no spawn can win, so ours is the only drainer
            # that can serve the marker. Re-check under the lock:
            #   - marker/claim present → a request raced the exit: keep
            #     draining (reset the budget, drop the lock; the next tick
            #     serves it);
            #   - nothing → EXIT while holding the lock. The process exit
            #     releases both locks and the EXIT trap removes the pidfile;
            #     the queue blocked on this lock then writes its marker and
            #     spawns a fresh drainer that wins the now-free drain lock.
            # No deadlock: the queue never waits for the drain lock while
            # holding the request lock (its spawn is a non-blocking attempt).
            exec 8>"$HVE_SAFE_DIR/reload.request.lock"
            flock 8
            if [ -f "$HVE_SAFE_DIR/reload.requested" ] || [ -f "$HVE_SAFE_DIR/reload.claimed" ]; then
                idle=0
                exec 8>&-
                continue
            fi
            exit 0
        fi
    done
}

if [ "${1:-}" = "drain" ]; then
    hve_reload_drain
fi