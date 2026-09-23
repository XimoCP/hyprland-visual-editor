# Feature: idempotent scratchpad hide + single colour watcher

**Locator**: `odd/tasks/hide-idempotency-and-singleton-watcher.md`
**Engram mirror**: topic `odd/hide-idempotency-and-singleton-watcher/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches)
**Status**: IMPLEMENTED + VERIFIED — LIVE CONFIRMED 2026-09-22, AWAITING KEEPER SIGN-OFF (W1 `88c10b1`, W2 `ab932a7`, doc `098ed20`, flock fail-open `8a17f2c`). The binary was built and installed from this branch on 2026-09-22 and the keeper ran the live confirmation himself: four consecutive applies, no bar above HVE, no dimmed desktop.
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~300-400 authored changed lines was the forecast; the ACTUAL authored total is 781 across the four commits (W1 237, W2 234, doc 159, flock fail-open 159). The code+tests part alone is **621** (598 added, 23 deleted; `src/` + `assets/`), i.e. OVER the ~400 review budget; the remaining 160 lines are this document. Not padding: the bulk is the test code this document mandates (the sandboxed integration test plus the parser/plan tests). Decision open for the keeper at PR time: `size:exception` vs two chained PRs (strategy `ask-on-risk`).

## Source (verified evidence, 2026-09-22 — read-only investigation, keeper's live session)

### S1 — The keeper's report

> "despues de aplicar dos o tres veces un fondo vuelve el fondo desenfocado y la barra de noctalia vuelve a ganarnos la partida y se pone por encima de nuestra aplicación"

Two symptoms, one mechanism, and both regress on the 2nd/3rd apply.

### S2 — Symptom A: the noctalia bar renders above HVE

- HVE's ONLY lever against layer-shell surfaces is real fullscreen mode 2. The code says so itself: `src/composer/hyprland.rs:369` — "Fallback to legacy real fullscreen (2, covers layer-shell)", and `src/main.rs:883-889` documents the forced unset→set cycle for the same reason.
- Live evidence (`hyprctl layers`): `noctalia-bar-default` (level 2, top) for pid 110429; live `hyprctl clients` put HVE in workspace `special:minimized` with `fullscreen=2`.
- Inside an OPEN special workspace overlay HVE is NOT in fullscreen mode 2, so the level-2 bar is drawn on top of it. That is exactly the report.

### S3 — Symptom B: the background reads as "out of focus"

- Hyprland's `decoration:dim_special` is `0.2` live (its default), so whenever a special workspace is OPEN the desktop behind it is dimmed 20%. Measured live: `decoration:blur:special = false`, `decoration:dim_inactive = false`, all four theme-transition masks at their normal values (`animations:enabled=true`, `decoration:blur:enabled=true`, `general:border_size=3`, `decoration:shadow:enabled=true`). So the wash is `dim_special`, not a leaked blur mask.
- The scratchpad is currently CLOSED on both monitors (`specialWorkspace: {id: 0, name: ""}`) — the healthy state. The failure is intermittent, which is why it needs the 2nd/3rd apply.

### S4 — Root cause: `hide()` blind-toggles the scratchpad

`src/composer/hyprland.rs:169-202` (`HyprlandComposer::hide`, V5 arm):

```rust
let s1 = v5_move_to_special();
let did_move = self.hypr_dispatch_v5(&s1);
if did_move {
    let s2 = v5_toggle_special();      // hl.dsp.workspace.toggle_special("minimized")
    let _ = self.hypr_dispatch_v5(&s2); // <- result DISCARDED
    true
}
```

Three defects in five lines:

1. `toggle_special` is a **toggle**, i.e. state-dependent and NOT idempotent. It does not "close", it *inverts*.
2. Its result is **discarded** (`let _ =`) and never verified.
3. `hide()` has **no idempotency guard**, while `show()` does: `show()` asks `if self.hve_in_special()` first (`src/composer/hyprland.rs:230`). The asymmetry is the bug.

Failure case (matches "2 or 3 times"): a second `hide()` arrives while HVE is already parked and the scratchpad is already closed. The targeted move is a no-op, but the toggle **re-opens** the scratchpad → HVE becomes visible inside an open special overlay → symptom A (bar on top) + symptom B (dimmed desktop behind).

Why extra `hide()` calls arrive after an apply:
- `assets/scripts/assemble.sh:92-94` runs `hyprctl reload` unconditionally, and HVE's own comment (`src/main.rs:857-861`) states every reload re-floats HVE and re-enables blur. Each reload is another focus flap.
- The reload loop is fed by the watcher(s) AND by the IPC listener on every `configreloaded` (`src/hypr_ipc.rs`).
- `src/countdown.rs:519-562` hides on focus loss, gated only by `SUPPRESS_UNTIL` (6 s, re-armed per apply) and `THEME_TRANSITIONING_FLAG`; `src/countdown.rs:183-186` says the delay exists precisely "so you can return from the other monitor before it hides" (explains the keeper's monitor suspicion).

### S5 — Root cause: colour watcher instances accumulate

- `src/main.rs:3689` does `drop(_color_watcher)`. Dropping a `std::process::Child` in Rust does **not** kill the child: the bash script is orphaned and keeps running forever.
- `assets/scripts/color_watcher.sh` has **no** singleton guard (no pidfile, no lock): it logs `Starting watcher` and enters `while true`.
- Live evidence: `lsof ~/.cache/hve/color_watcher.log` shows TWO bash processes writing the same file — PID 51553 (`/home/ximo/Proyectos/hve/assets/scripts/color_watcher.sh`, the dev tree) and PID 113789 (`/home/ximo/.local/bin/assets/scripts/color_watcher.sh`, the installed tree). The log has 23 `Starting watcher` lines historically and every line duplicated.
- Consequence in the log: both run `assemble.sh` over the SAME temp path, producing
  `mv: no se puede efectuar 'stat' sobre '/home/ximo/.cache/hve/overlay.tmp': No existe el fichero`.
- Each extra watcher doubles the `templates-apply` / `assemble.sh` / `hyprctl reload` cadence, which widens the window in which S4's extra `hide()` lands.

### S6 — Discarded (measured, so the fix does not chase them)

- No blur-mask leak right now (all four masks at config values).
- Background image `joker1.png` is 2616x1472 for DP-3 at 2560x1440 — no upscale, so no "soft because rescaled".
- The image `joker1.png` is the real wallpaper in play (not a baked thumbnail).

### S7 — Observation gap (NOT in scope, recorded for follow-up)

`tracing_subscriber::fmt()` (`src/main.rs:1254`) writes to stdout, and the live `hve --tray` process has `fd 1` and `fd 2` pointed at `/dev/null`. Every `[theme] blur restored to …` and `[countdown] Gallery focus lost` line is discarded, which is why only the watcher log is readable. Out of scope for this change; listed as a follow-up.

## Work units

### W1 — one colour watcher per machine (kill on exit + singleton guard)

Files: `src/watcher.rs`, `src/main.rs`, `assets/scripts/color_watcher.sh`.

1. **Script guard** (`assets/scripts/color_watcher.sh`): acquire a non-blocking exclusive lock BEFORE the first `_log "Starting watcher"`, right after `mkdir -p "$(dirname "$LOG_FILE")"`. Use `flock -n` on a dedicated lock file under `HVE_SAFE_DIR` (e.g. `color_watcher.lock`), held via a long-lived fd for the whole process lifetime. On failure: log one line naming the singleton guard and `exit 0` (a rejected second instance is not an error).
2. **Rust guard** (`src/watcher.rs`): replace the raw `Option<Child>` return with an owned guard type (e.g. `ColorWatcher`) whose `Drop` sends `kill()` and then `wait()` (reaping matters: a killed-but-unreaped child stays a zombie). Keep the existing spawn semantics, script-path resolution and log target unchanged.
3. **Wire it** (`src/main.rs`): keep the binding alive for the whole event loop and drop it on exit so the guard actually fires (replace the current `drop(_color_watcher)` no-op with the guard's real teardown; keep the existing binding name/scope).

**Acceptance criteria**
- A second instance of the real script, started while a first one holds the lock, exits 0 promptly and logs the guard message instead of doing work.
- Exactly one instance performs `assemble.sh` / `templates-apply` per event, observable in the log.
- Dropping the Rust guard terminates AND reaps the child: after drop, `/proc/<pid>` is gone.
- Script-path resolution (`proj/assets/scripts/color_watcher.sh`) and the log target (`cache/hve/color_watcher.log`, `/tmp` fallback) are unchanged.

**Tests (strict TDD — RED first)**
- Rust unit: the guard kills a real placeholder child (`sleep`) and leaves no `/proc/<pid>` entry after drop.
- Rust sandboxed integration for the REAL script (must not touch the keeper's live session):
  copy the real `color_watcher.sh` + the real `utils.sh` into a temp `assets/scripts/`, place STUBS for `assemble.sh`, `get_colors.sh` and `hve-ipc` beside them (so `utils.sh` resolves `HVE_SCRIPTS_DIR` into the temp tree), point `HOME` and `HVE_CACHE_DIR` at the temp dir, create a minimal watched file (e.g. `$HOME/.cache/wal/colors.json`) so `find_watch_files` is non-empty, and put a `sleep`-based `inotifywait` stub (plus a `noctalia` stub) on `PATH`. Then: start instance 1 (must hold the lock), start instance 2, assert instance 2 exited and logged the guard message, and assert the lock file exists.
  **Forbidden in tests**: running the real `assemble.sh`, calling the real `hyprctl`, touching `$HOME/.cache/hve`, or invoking the real `noctalia`.

### W2 — hiding is idempotent and verified (never re-opens the scratchpad)

Files: `src/composer/hyprland.rs` (plus tests where the existing composer tests live, `src/composer/mod.rs` and/or the `hyprland.rs` test module).

Follow the established house pattern: pure, testable decisions/builders plus a thin impure shell (the same shape as `blur_decision` in `src/countdown.rs` and the `v5_*` builders).

1. **New pure parser**: read the live scratchpad state from `hyprctl monitors -j` — a monitor whose `specialWorkspace.name` is non-empty (or `id != 0`) means the scratchpad is OPEN. Keep it pure over the JSON text so it is unit-testable with fixtures, and document that `{id: 0, name: ""}` is the closed state (verified live).
2. **New pure hide plan**: a function that, given `parked` (HVE already in the special workspace — reuse the existing `hve_in_special` semantics) and `scratchpad_open`, returns the ordered dispatch list:
   - `parked && !scratchpad_open` → **empty** (already hidden: this is the regression case that must never emit a toggle).
   - `parked && scratchpad_open` → close only.
   - `!parked` → targeted move to special, then close (the move opens the overlay; the code comment at `hyprland.rs:179-182` states this).
3. **Impure execution**: `hide()` queries state, builds the plan, dispatches it in order, then **verifies** the scratchpad ended closed. If it is still open, dispatch at most ONE corrective close and log the outcome (bounded retry — no loop). If the plan is empty, return `true` without dispatching anything.
4. Keep the existing `win.window().hide()` fallback paths and the V4 arm's semantics intact; only the V5 arm gets the plan. Do not touch `show()`.

**Acceptance criteria**
- A repeated `hide()` while parked with the scratchpad closed dispatches NOTHING (no toggle, no move) and reports success.
- `hide()` from a non-parked state still ends with the scratchpad closed, verified by a fresh query rather than by trusting the toggle.
- When the scratchpad is still open after the planned close, exactly one corrective close is dispatched and the result is logged; never more than one.
- `show()` behaviour and the `show_fast` dispatch order test remain unchanged and green.

**Tests (strict TDD — RED first)**
- Parser: fixtures for closed (`{id:0,name:""}`, both monitors) and open (`special:minimized`) states.
- Plan: the three cases above, asserting the exact dispatch list and that the regression case emits an empty plan.
- No test may call the real `hyprctl`: the plan and parser are pure, and the impure wiring stays thin (assert the ORDER of the pure plan, the same way the existing `test_show_fast_path_moves_before_focus_avoids_special_overlay_blur` asserts move-before-focus).

## Verification

- `cargo test` — full suite green (baseline before this change: 893 passed / 0 failed).
- Cross-model independent verification of the finished diff, on a DIFFERENT model from the writer (`glm-5.3-flash`), read-only, against this document's acceptance criteria. Report which model verified.
- Live check is the keeper's: the fix cannot be proven from the suite alone (the failure is compositor state). After the commits, the keeper restarts HVE and repeats the 2-3 applies.
- AGENTS.md VISUAL VERIFICATION applies only if a `.slint` or visual file changes — this change touches none.

## Operational note (outside the code change)

The two orphan watchers alive RIGHT NOW (PIDs 51553 dev-tree and 113789 installed) predate the guard and will ignore it. Until they are killed (or the machine restarts), the live session keeps the duplicated cadence. Killing them leaves no watcher until HVE restarts, because HVE only spawns one at startup.

## Progress

Observed evidence (writer model, TDD runs):
- RED W1 (unit + sandboxed, before any guard): `cargo test --bin hve watcher::tests` — sandboxed test failed with "first instance never acquired the singleton lock — flock guard missing?" after the 30s hard timeout.
- RED W2 (parser + plan, before implementation): `cannot find function scratchpad_open` (E0425).
- GREEN W1: `cargo test --bin hve watcher::tests` — 5 passed (2 new: `color_watcher_guard_kills_and_reaps_the_child_on_drop`, `sandboxed_second_watcher_instance_is_rejected_while_first_holds_the_lock`).
- GREEN W2: `cargo test --bin hve 'composer::'` — 53 passed (7 new parser/plan tests), then full suite `cargo test`: **902 passed / 0 failed** (baseline 893 + 9 new).
- Full-suite command: `cargo test` (result: 902 passed, 0 failed, ~32s).
- Cross-model independent verification (read-only) reproduced a fail-closed edge: with `flock` missing from PATH the guard rejected the FIRST instance too ("another color watcher holds …", `exit 0`, zero work). Fixed with a real capability check (`command -v flock`) that FAILS OPEN — one distinct "guard unavailable / continuing UNGUARDED" warning line, watcher keeps running. Covered by the sandboxed `sandboxed_watcher_fails_open_when_flock_is_missing` (work-unit commit `fix(scripts): fail open when flock is missing so a missing dep never disables the watcher`).

### W1 — one colour watcher per machine
- [x] Script guard (flock) + guard log line — `assets/scripts/color_watcher.sh` (flock on `HVE_SAFE_DIR/color_watcher.lock`, acquired before the first `_log "Starting watcher"`)
- [x] Rust owned guard with kill+reap on drop — `src/watcher.rs` (`ColorWatcher`)
- [x] Wire the guard in `main.rs` (real teardown) — the existing `drop(_color_watcher)` now kills + reaps
- [x] RED test: guard kills and reaps a placeholder child — observed (E0422 before the type existed), green after
- [x] RED test: sandboxed second real-script instance is rejected — observed (guard missing), green after
- [x] Work-unit commit — `88c10b1 fix(scripts): make the colour watcher a singleton and kill+reap it on exit`
- [x] Cross-model finding closed: a missing `flock` must FAIL OPEN, never reject the first instance — real `command -v flock` capability check + `sandboxed_watcher_fails_open_when_flock_is_missing` (commit `8a17f2c`)

### W2 — idempotent verified hide
- [x] Pure scratchpad-open parser + fixtures — `scratchpad_open()` over `hyprctl monitors -j` (`{id:0,name:""}` = closed, verified live)
- [x] Pure hide plan (3 cases, empty plan for the regression case) — `hide_plan(parked, scratchpad_open)` + `HideStep`
- [x] `hide()` uses the plan + bounded verified close — V5 arm: query → plan → dispatch in order → fresh-query verification → at most ONE corrective close + outcome log
- [x] RED tests for parser and plan — observed (E0425), green after (7 tests)
- [x] Work-unit commit — `ab932a7 fix(composer): make hide idempotent and verify the scratchpad ends closed`

## Verification record (2026-09-22)

| Check | Who | Result |
|---|---|---|
| Full suite | orchestrator (re-ran it, did not trust the writer's report) | `cargo test` → **903 passed / 0 failed** (~32 s) |
| Cross-model independent verification (read-only, adversarial) | **GLM 5.3 Flash** — writer ran on **DeepSeek V4 Flash** | **PASS WITH FINDINGS**: confirmed the suite count, no existing test weakened or deleted (`git diff -- '*test*'` empty), scope exactly as authorized, and that the new tests are not passing for the wrong reason (both false-approval paths checked and discarded). |
| Both guard branches, end-to-end on the REAL script | orchestrator, own sandbox probe (`/tmp/opencode/probe_guard_branches.sh`) | **flock present**: second instance rejected with exit 0, exactly 1 "Starting watcher", 1 rejection line, 0 unguarded warnings, exactly 1 `assemble.sh` run, lock file present. **flock absent** (`flock` stripped from PATH): distinct "continuing UNGUARDED" warning, 0 false rejections, exactly 1 `assemble.sh` run, first instance alive. Both as designed. |
| Visible behaviour / live | **the keeper, on his own desktop** (the only valid check for compositor state) | **PASS**. Binary built from this branch and installed 2026-09-22 (hash-verified atomic replace); HVE relaunched with its stdout captured to `/tmp/opencode/hve-live.log`. Four consecutive theme applies logged (generations 1-4, 16:41:30-16:41:51 local). The keeper's own verdict: *"el blur ya no está y la barra queda siempre por debajo de HVE"*. Zero WARN or ERROR lines in the whole log. Side-evidence for W1: the pre-fix watcher survived the old binary's IPC `quit` (the orphan defect reproduced on demand) and was retired by hand; the new instance runs exactly one guarded watcher with its lock file present. |

Proportionality note: the two work-unit commits got the full cross-model round above. The small flock fail-open delta (one capability check plus one test) got orchestrator review plus the two-branch end-to-end probe, not a second cross-model round — said plainly rather than implied.

## Residuals (known, recorded, NOT fixed here)

1. **Per-monitor scratchpad semantics (MINOR).** The scratchpad state is per monitor, but the close dispatch acts on the monitor that currently has focus. With a dual-monitor setup, if focus has already moved to the other monitor when the countdown decides to hide, the *query* and the *dispatch* can talk about different monitors — and the corrective close could close the wrong one. Pre-existing semantics (the show path has the same ambiguity), not worsened by this change, and not closable without a live dual-monitor test.
2. **Corrective-close window (MINOR).** If an external agent (a Hyprland keybind) opens the scratchpad between the close and the verification, the single corrective close can re-open it; there is no second verification, only a log line. Milliseconds-wide; the next `hide()` self-repairs with the `[Close]` plan.
3. **The most plausible residual path to the keeper's original symptom** is the external interleaving in (1)/(2) rather than the pure logic, which is now total for the tested cases.
4. **Out of scope, unchanged:** `assemble.sh:92-94` still runs `hyprctl reload` unconditionally (the reload loop that feeds the extra focus flaps, see S4). HVE's `tracing` output is still discarded when the app is started from the tray (S7); the live check below captured it by relaunching with stdout redirected to a file, which is a diagnostic workaround, not a fix — the app's own log remains unreadable in normal use.
5. **V4 fallback arm** still blind-toggles `togglespecialworkspace` in the `abandoned_v5` and pure-V4 paths — authorized by this document ("keep the V4 arm's semantics intact"), only reachable when the Lua (V5) dispatch fails or on a conf-based Hyprland.

## Next step (keeper)

1. **The keeper's sign-off on this change** (still pending by his own instruction; pinned in Engram as `hve2/pending/keeper-review-hide-idempotency`). The live confirmation he asked for is now DONE and PASSED — see the verification record.
2. At PR time, decide `size:exception` vs two chained PRs (621 authored code+test lines vs the ~400 budget).

