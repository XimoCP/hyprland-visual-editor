#!/usr/bin/env bash
# HVE live verification — W6 (colour watcher survives HVE's death) + W7 (reload
# drainer exits when idle, never losing a reload).
#
# Run it from your OWN interactive session (it needs your Wayland environment):
#     bash /tmp/opencode/hve-live-verify.sh
#
# It launches the DEV binary from the repo tree (the installed one still has the
# pre-W6 code), exercises both units, prints PASS/FAIL per step, and cleans up
# after itself. It never touches your installed HVE or ~/.local/bin.
set -u

REPO=/home/ximo/Proyectos/hve
HVE="$REPO/target/release/hve"
CACHE="$HOME/.cache/hve"
WLOG="$CACHE/color_watcher.log"
LOCK="$CACHE/color_watcher.lock"
LOG=/tmp/opencode/hve-live-verify.log
PASS=0; FAIL=0

ok()  { printf '  PASS  %s\n' "$*"; PASS=$((PASS+1)); }
bad() { printf '  FAIL  %s\n' "$*"; FAIL=$((FAIL+1)); }
step(){ printf '\n== %s ==\n' "$*"; }

watchers() { pgrep -f 'assets/scripts/color_watcher.sh' 2>/dev/null | wc -l; }
drainers() { pgrep -f 'reload_coalescer.sh drain' 2>/dev/null | wc -l; }
markers()  { ls "$CACHE" 2>/dev/null | grep -cE '^reload\.(requested|claimed)$'; }
hve_pid()  { pgrep -f "^$HVE" 2>/dev/null | head -1; }

stop_hve() {
    local pid; pid=$(hve_pid)
    [ -n "$pid" ] && { kill -9 "$pid" 2>/dev/null; sleep 0.5; }
}

cleanup() {
    step "CLEANUP"
    stop_hve
    pkill -f "$HVE" 2>/dev/null
    echo "  (HVE stopped; nothing else is left running)"
}
trap cleanup EXIT

step "0. Preconditions"
if [ ! -x "$HVE" ]; then bad "dev binary missing: $HVE"; exit 1; fi
echo "  dev binary: $HVE ($(md5sum "$HVE" | cut -c1-8))"
if [ -n "$(hve_pid)" ]; then bad "HVE is already running — close it first"; exit 1; fi
ok "HVE is not running"

# A stale pre-fix drainer holds the drain lock forever and would make the W7
# check meaningless. Safe to remove only when nothing is owed.
if [ "$(drainers)" -gt 0 ]; then
    if [ "$(markers)" -eq 0 ]; then
        pkill -9 -f 'reload_coalescer.sh drain' 2>/dev/null; sleep 0.3
        ok "removed a stale pre-fix drainer ($(drainers) left), no reload was owed"
    else
        bad "a drainer is alive WITH pending markers — not touching it; stop and tell me"; exit 1
    fi
else
    ok "no stale drainer"
fi
: > "$LOG" 2>/dev/null || true
WLOGSZ=$(stat -c %s "$WLOG" 2>/dev/null || echo 0)

step "1. Start HVE (dev binary, repo tree)"
( cd "$REPO" && setsid "$HVE" >>"$LOG" 2>&1 & )
for _ in $(seq 1 40); do [ "$(watchers)" -gt 0 ] && break; sleep 0.5; done
if [ "$(watchers)" -eq 1 ]; then ok "exactly ONE colour watcher is alive"; else bad "expected 1 watcher, found $(watchers)"; fi
if grep -q 'Color watcher started' "$LOG"; then ok "HVE logged the watcher as started"; else bad "no 'Color watcher started' in the HVE log"; fi
if tail -c +$((WLOGSZ+1)) "$WLOG" 2>/dev/null | grep -q 'Singleton guard.*exiting'; then
    bad "the watcher was REJECTED by the singleton guard"
else
    ok "the watcher was not rejected"
fi

step "2. Kill HVE ungracefully (kill -9) — W6's whole point"
PID=$(hve_pid)
if [ -n "$PID" ]; then kill -9 "$PID"; ok "sent SIGKILL to HVE (pid $PID)"; else bad "could not find the HVE pid"; fi
GONE=no
for _ in $(seq 1 20); do [ "$(watchers)" -eq 0 ] && { GONE=yes; break; }; sleep 0.25; done
if [ "$GONE" = yes ]; then ok "the watcher died with HVE (within ~5 s)"; else bad "the watcher SURVIVED HVE — PDEATHSIG not working"; fi
if flock -n "$LOCK" -c true 2>/dev/null; then ok "the singleton lock is FREE after HVE died"; else bad "the lock is still held (an orphan kept it)"; fi

step "3. Start HVE again — must get a watcher on the first attempt"
: > "$LOG"
( cd "$REPO" && setsid "$HVE" >>"$LOG" 2>&1 & )
for _ in $(seq 1 40); do [ "$(watchers)" -gt 0 ] && break; sleep 0.5; done
if [ "$(watchers)" -eq 1 ]; then ok "exactly ONE watcher again"; else bad "expected 1 watcher, found $(watchers)"; fi
if grep -q 'failed to start after' "$LOG"; then bad "HVE reported a FAILED start (retries exhausted)"; else ok "no failed-start error in the HVE log"; fi

step "4. Trigger one reload burst and watch the drainer — W7"
WLOGSZ2=$(stat -c %s "$WLOG" 2>/dev/null || echo 0)
bash "$REPO/assets/scripts/assemble.sh" >/dev/null 2>&1
SPAWNED=no
for _ in $(seq 1 20); do [ "$(drainers)" -gt 0 ] && { SPAWNED=yes; break; }; sleep 0.25; done
if [ "$SPAWNED" = yes ]; then ok "a drainer spawned for the burst"; else bad "no drainer spawned (queue not reached?)"; fi
FIRED=no
for _ in $(seq 1 40); do [ "$(markers)" -eq 0 ] && { FIRED=yes; break; }; sleep 0.25; done
if [ "$FIRED" = yes ]; then ok "the owed reload was consumed (no marker left behind)"; else bad "a marker is still pending after the burst"; fi
echo "  (waiting for the idle bound, ~15 s, so the drainer can exit on its own)"
EXITED=no
for _ in $(seq 1 60); do [ "$(drainers)" -eq 0 ] && { EXITED=yes; break; }; sleep 0.5; done
if [ "$EXITED" = yes ]; then ok "the drainer exited on its own (no immortal child left)"; else bad "a drainer is STILL alive after the idle bound"; fi

step "RESULT"
printf '  PASSED: %d   FAILED: %d\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] && echo "  VERDICT: both units behave as designed." || echo "  VERDICT: something did not behave — report the FAIL lines above."
