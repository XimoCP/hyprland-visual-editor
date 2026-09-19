# Feature: single-instance handoff (second launch raises the running window)

**Locator**: `odd/tasks/single-instance-handoff.md`
**Engram mirror**: topic `odd/single-instance-handoff/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite`
**Status**: IMPLEMENTED (commit `a56016e`) — parent gate + rebuild/install pending
**TDD**: strict — runner `cargo test`

## Problem

HVE allows a single instance, which is correct: two UIs must never fight over the same desktop state. But the current refusal is a dead end.

Evidence of current behaviour:
- `src/main.rs:1072-1079` takes a non-blocking exclusive `flock` on `~/.cache/hve/hve.lock`; on `WouldBlock` it logs `Another instance of HVE is already running.` and calls `std::process::exit(1)`.
- The default log filter is `info` (`src/main.rs:1048-1054`), so the message does appear when launched from a terminal — and is invisible when launched from the desktop.
- The already-running instance is typically the one autostarted at login, which runs in **tray mode** (`--tray`): its window is hidden, so the user cannot see it and does not connect the error with a live app.
- Daily effect: `cargo run` fails while developing, with a message that names no way out but the shell.

What already exists to build on: a live IPC socket at `$XDG_RUNTIME_DIR/hve.sock`, with a dispatcher in `src/ipc.rs` (`dispatch_command`) that already handles `pause-restart`, `next-anim`, `next-border`, `next-shader`, `toggle-tray`, `status`, `quit`, `refresh-theme`. The user's `SUPER + H` keybind uses `hve-ipc toggle-tray`, a Python client at `~/.local/bin/hve-ipc` that speaks the same protocol.

## Required behaviour

1. **New idempotent `show` command** in the IPC dispatcher: makes the running instance's window visible and focused. It must NOT hide the window when it is already visible (unlike `toggle-tray`, which flips state) and must work when the instance sits in tray mode.
2. **Handoff on lock refusal**: when the new process finds the lock held (`WouldBlock`), it connects to the socket, sends `show`, and exits without creating a second UI or spawning a window. Print a clear, human message stating that HVE is already running and that its window was brought forward.
3. **Honest fallback**: if the socket is unreachable, refuses, or times out, keep the process's single-instance guarantee and print an actionable error naming both exits — `pkill -x hve` to close it, or `SUPER + H` to show it — then exit non-zero.
4. **No regression**: the single-instance guarantee is unchanged (never two UIs, never a stolen lock); `toggle-tray`, `status`, `quit` and the other commands keep their current semantics.

## Design constraints

- Reuse the existing socket/protocol; do not invent a second channel. Follow how `src/ipc.rs` frames and reads commands, and how `invoke_on_main` runs work on the Slint event-loop thread.
- `show` must be safe to call repeatedly and at any moment (during the show/hide state machine, while mutating, when already visible).
- Keep the wake-up fast: the handoff must not hang the launching process; bound the wait and fall back to the honest error.
- No change to the tray-mode decision itself: the autostarted instance still starts without a window by design.

## Acceptance criteria

1. With an instance running in tray mode (window hidden), launching a second one makes the running window visible and the second process exits without a second UI — covered by an automated test that fails before the change.
2. `show` is idempotent: invoking it on an already-visible window leaves it visible (no accidental hide), covered by a test.
3. When the socket is unavailable, the second process still exits non-zero with the actionable message; no lock is stolen and no ghost UI appears.
4. Full suite green (`cargo test`), `cargo check --all-targets` without warnings.
5. Manual verification: `pkill -x hve` then launch the tray instance, then launch a second one and observe the window coming forward.

## Out of scope

- Changing the single-instance rule or the lock file semantics.
- Changing `toggle-tray` or any other existing IPC command.
- The recorded debt: duplicated index map, the picker `y` slide-in, the scanner `@Color`/`icon` gap, `sync_preset_indices`, the flaky `providers::noctalia` test.
- Anything about the autostart block in `hve-settings.lua` (already correct and awaiting the user's next login).

## Delivery

Work-unit commit(s) with Conventional Commit messages, tests and docs alongside the behaviour, no AI attribution. Parent re-runs the full suite, then rebuilds and installs `~/.local/bin/hve` (backup first) so the user can verify live.

## Implementation

- `src/composer/mod.rs` — `Controller::show_window`: raise the window (show when hidden, focus when visible, never hide). Idempotent by construction (`ShowStateMachine` refuses a second `begin_show`); `toggle_tray`'s show branch now delegates to it so the two paths cannot drift.
- `src/ipc.rs` — `"show"` dispatcher arm + `cmd_show`; handoff client (`request_show`, `request_show_with_timeout`, `handoff_show`, `handoff_show_running`); `HandoffOutcome` / `HandoffReport`; `HANDOFF_TIMEOUT = 3s` and `HANDOFF_FALLBACK_EXIT_CODE = 1`.
- `src/main.rs` — lock refusal (`WouldBlock`) now runs the handoff before any window exists and exits with the report's code; other lock errors keep their old logging.
- `README.md` — `show` row in the IPC table and the CLI list, plus the second-instance note in the tray section.

### RED (before implementation)

`cargo test --bin hve 'composer::tests::test_show_window'` — 3 failed:
```
assertion `left == right` failed: an already-visible show must bring the window forward via focus
  left: []   right: ["focus"]
a hidden window is in the special workspace: fast path
assertion `left == right` failed
  left: Hidden
 right: Entering
test result: FAILED. 0 passed; 3 failed; 0 ignored; 0 measured; 761 filtered out; finished in 0.06s
```

`cargo test --bin hve 'ipc::tests'` — 7 failed for the intended reasons:
```
test_dispatch_routes_show_to_its_handler_not_unknown ... FAILED
  `show` must have a dispatcher arm, got: error: unknown command 'show'
test_request_show_sends_the_show_command_and_reports_success ... FAILED
  left: Unreachable("not implemented")   right: Shown("ok")
test_request_show_reports_unreachable_when_the_instance_refuses ... FAILED
  the server refusal must be surfaced, got: not implemented
test_handoff_show_success_reports_exit_zero_and_a_forwarded_message ... FAILED
  a positive reply reports success
test_handoff_show_unreachable_exits_nonzero_and_names_both_exits ... FAILED
  the message must explain the refusal, got:
test_request_show_is_bounded_when_the_instance_never_answers ... FAILED   the client must connect
test_request_show_reports_unreachable_when_the_instance_closes_without_reply ... FAILED   the client must connect
test result: FAILED. 14 passed; 7 failed; 0 ignored; 0 measured; 743 filtered out; finished in 2.00s
```

### GREEN (after implementation)

```
cargo test --bin hve 'test_show_window'  -> 3 passed; 0 failed
cargo test --bin hve 'ipc::tests'        -> 21 passed; 0 failed
cargo test                               -> 764 passed; 0 failed; 0 ignored
cargo check --all-targets                -> Finished `dev` profile, no warnings
```

First full run after the change reported `763 passed; 1 failed`: the pre-recorded flaky `providers::shell::tests::test_noctalia_v5_paths_config_dir` (out of scope). It passes in isolation (`1 passed`), touches no file from this change, and the second full run is `764 passed; 0 failed`.

### Coverage notes (honest gaps)

- Acceptance 1 and 2 are covered by `composer::tests::test_show_window_*`.
- Acceptance 3 (socket-unavailable path) is covered by the handoff report tests plus the bounded-wait test; "no UI is spawned" is guaranteed by construction — the handoff runs in the lock-refusal branch *before* `MainWindow::new()`. There is no headless assertion for that ordering; it is the call-site position, not a testable precondition.
- The `invoke_on_main` event-loop hop and the `global_controller()` lookup inside `cmd_show` are not executed headlessly: installing the process-global controller in a test would make it visible to every parallel test that reads `global_controller()`. The dispatch arm itself is pinned by `test_dispatch_routes_show_to_its_handler_not_unknown`.
- No `.slint` or render-visible state changed: no headless render was required or run.

### Rollback boundary

Revert `a56016e` alone: it removes the `show` arm, `cmd_show`, the handoff client and `show_window`, restoring the previous "already running" dead end. No config, lock-file or schema migration is involved.

## Progress

- 2026-09-20 — Created from the user's report that `cargo run` with the app alive shows `Another instance of HVE is already running`, plus the parent's verification of the lock path (`src/main.rs:1072-1079`), the log level (`:1048-1054`) and the existing IPC command surface (`src/ipc.rs`). No source changed yet.
- 2026-09-20 — Strict TDD apply complete: RED observed (3 composer + 7 ipc failures), GREEN observed (3 + 21 focused, 763 full), `cargo check --all-targets` clean. Committed as `a56016e`.

## Next step

Parent gate: re-run `cargo test` + `cargo check --all-targets`, then rebuild and install `~/.local/bin/hve` (backup first) and run the live manual check (acceptance 5): `pkill -x hve`, launch the tray instance, launch a second one, confirm the window comes forward and the second process exits 0; then stop the instance and confirm the fallback message names both exits.
