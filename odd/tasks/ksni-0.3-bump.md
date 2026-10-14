# ksni 0.2.2 → 0.3 bump (HVE)

Branch `hve2-visual-rewrite`. Test runner `cargo test`; the UI renders need
`HVE_RENDER_DIR=<dir> cargo test --bin hve <test> -- --nocapture` and the frames are read by eye.
Delivery: ONE work-unit commit (Cargo.toml + tray + its callers + tests + docs together), because the
Cargo.toml line alone does not compile.
Deferred from Lane C (S5 / T5, `odd/tasks/lane-c-health.md`), which holds the recon this plan was built from.

## Specs

- **S1 — the dependency outcome.** `ksni` is `0.3` with `default-features = false, features = ["blocking", "async-io"]`.
  The build graph loses `ksni 0.2.2`, `dbus 0.9.11`, `dbus-tree 0.9.2`, the BUILD-time `dbus-codegen 0.9.1` and with it
  `clap 2.34.0` (+ `ansi_term`, `atty`, `strsim 0.8`, `textwrap`, `vec_map`), and gains NOTHING: `ksni 0.3.5` is
  already in the graph on `async-io` (slint's own dependency) and `tokio` must not appear in the lock. Measured, not
  assumed — the claim is the DELTA, since the absolute package count depends on the toolchain and the flags:
  **16 packages leave `Cargo.lock` and none enter** (the lock keeps `thiserror 1.0.69` for its other consumers), and
  the Linux build tree loses 12. The feature set is forced, not chosen: `tokio` + `async-io` together trips ksni's
  `compile_error!`, and no runtime feature at all trips the other one (`compat.rs:1-5`).
- **S2 — "no tray" means HVE keeps running, with a warning.** `start_tray` returns `Option<TrayHandle>`. On a failure
  it logs `tracing::warn!` with the ksni error and the app continues without a tray. Nothing on HVE's failure paths
  panics, in any thread (ksni's own internal `assert`/`expect` calls, e.g. the per-PID `NameTaken` assert, are not
  reachable from those paths — checked against the crate's sources).
  Today's behaviour on the same failure is a panic inside 0.2's spawned thread (`thread::spawn(|| self.run().unwrap())`)
  that the user never sees; HVE keeps running there too, so this is the behaviour-preserving option, not a new choice.
- **S3 — a panel that starts LATER still gets the icon.** The service is spawned with `assume_sni_available(true)`,
  so a missing `org.kde.StatusNotifierWatcher` is a soft error routed to `Tray::watcher_offline` (default: keep
  running, which HVE does not override) and the service re-registers when the watcher appears. This is required to
  match 0.2: `ksni 0.2.2` subscribes to `NameOwnerChanged` and calls `register_status_notifier_item` when the watcher
  appears (`service.rs:180-217`), while 0.3 WITHOUT the flag returns `Err` immediately and never retries.
- **S4 — the tray the user sees does not change.** Same 48×48 BGRA icon from the theme, same menu (title, Toggle
  System, Next Animation/Border/Shader, Hide/Show Window, Quit), same labels from `Tr`, the Hide/Show label still
  refreshes, and the IPC paths (`update_global_icon`, `refresh_global_menu`) still reach a live tray. No icon that
  appears today may stop appearing.
- **S5 — one owner for `system_active`.** `main.rs` creates the `Arc<AtomicBool>`, passes it into `start_tray`, and
  passes the SAME Arc to `setup_callbacks` no matter what the tray did. The tray no longer hands one back.
- **S6 — checks.** A permanent deterministic test proves the no-tray path returns `None` and does not panic without a
  session bus; a TEMPORARY probe run under `dbus-run-session` proves the success path on a private bus and is deleted
  before the commit (a permanent one would register a second StatusNotifierItem on the keeper's live bus and put a
  duplicate icon in his panel). `cargo check --all-targets` clean; full suite green; clippy must not gain warnings.
  Note for anyone repeating this: with BOTH bus variables unset, zbus 5.16 falls back to `/run/user/<euid>/bus`
  (`address/mod.rs:74-75`), which on a logged-in machine IS the live session bus — a real "no bus" probe needs its own
  namespace (`unshare -r`), not just an unset variable.
- **S7 — the docs say what happens now.** `docs/wiki/Tray-and-Automation.md` states that when the tray cannot be
  created, HVE warns and keeps running without it.
- **S8 — the bump must not make HVE hang when it is hidden.** Hiding the window (Escape / the tray's Hide item /
  the Toggle callback) must return, and the app must keep answering the compositor. Concretely: no caller on the UI
  thread may wait for `ksni::blocking::Handle::update` (0.3's version is `compat::block_on`, it WAITS for the
  service, where 0.2 only enqueued), and the tray thread must never wait for the composer's controller mutex
  (`crate::composer::global_controller()`), which the UI thread holds while it asks for that refresh. The tray's
  `menu()` reads the hidden state through `try_global_controller()` instead; a stale label for one paint is the
  accepted price and the next refresh corrects it.
- **S9 — the shape of the fix is pinned by a test.** A deterministic test fails if the production body of
  `src/tray.rs` contains `block_on`, if `TrayHandle`'s own methods reach `.update(` at all (the worker has to stay
  its only owner — counting call sites alone cannot see a call MOVED into a caller), or if the part of `menu()` the
  tray thread runs — everything before its `vec!`, since the item closures are deferred to the UI thread through
  `slint::invoke_from_event_loop` — waits for the controller lock. The test scans the PRODUCTION body only (it names
  the forbidden thing itself, exactly like `architecture_contract` and `checks_build` do).

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1-S7 | inline (parent writes; high risk → independent verifier after) | DONE, verified by `opencode-go/mimo-v2.6-flash` (no blockers) | `deps(tray): bump ksni 0.2 -> 0.3 and survive a missing tray` |
| T2 | S8-S9 | inline (fix written 2026-10-14; checks, install and record completed 2026-10-15) | DONE, verified by `opencode/mimo-v2.6-flash-free` (no blockers; 4 advisories) | `fix(tray): stop the ksni 0.3 update from deadlocking the hide path` |
| T3 | S9 | inline, Tier 1 (test-only; no production behaviour changes) | DONE, self-checked (RED proved against `e9396df`, suite green) | `test(tray): pin where the blocking update is called, not how often` |

### T1 evidence

- **RED (compile level, and it has to be)**: `cargo test --bin hve start_tray_without_a_session_bus` failed with
  `error[E0061]: this function takes 5 arguments but 6 arguments were supplied` and
  `error[E0599]: no method named `is_none` found for struct `TrayHandle``. A runtime RED is not expressible here:
  the failure path does not exist in the old API at all (0.2 panics inside a thread nobody watches), which is the
  whole point of the unit.
- **GREEN**: same test → `test tray::tests::start_tray_without_a_session_bus_returns_none ... ok` (0.09 s).
- **S3 proved load-bearing, not decorative**: the temporary probe run under a private bus with no watcher
  (`dbus-run-session -- cargo test --bin hve probe_live_bus_with_no_watcher -- --nocapture`) passed with
  `assume_sni_available(true)` — verbatim output `PROBE RESULT: spawn -> Ok, update -> Some, icon + menu refresh
  clean` — and the SAME probe with the flag removed (`tray.spawn()`) **FAILED** on that same bus. That is the
  "HVE started before the panel" case, and it would have been a silent regression against 0.2.
- **Probe deleted before the commit**: `grep -c 'probe_live_bus' src/tray.rs` → 0. Kept, it would run on the
  keeper's live bus and register a second StatusNotifierItem in his panel.
- **S1 measured**: `cargo tree --prefix none | sort -u | wc -l` → **456 before, 444 after** (−12 in the Linux
  tree, same workspace, same flags; an independent verifier re-measured the same delta as 593 → 581 from a detached
  baseline worktree, so the DELTA is the durable claim and the absolute numbers are environment-specific).
  `Cargo.lock`: **−16 packages, +0**, verbatim removed names `ansi_term, atty, clap, dbus, dbus-codegen,
  dbus-tree, hermit-abi, ksni, libdbus-sys, strsim, textwrap, unicode-width, vec_map, winapi,
  winapi-i686-pc-windows-gnu, winapi-x86_64-pc-windows-gnu`. `ksni v0.2.2` is gone from `cargo tree`; only
  `ksni v0.3.5` remains (reached from `i-slint-core` and from `hve`); **`tokio` is still absent**. Corrected after
  independent verification: `thiserror 1.0.69` was NOT removed (its other consumers keep it) — S1 no longer claims it.
- **Checks on the frozen bytes**: `cargo check --all-targets` clean (no warning, no error); `cargo clippy
  --all-targets` → **67 warnings, the same count Lane C left** (none new); `cargo fmt --check` clean;
  full suite **1786 passed / 0 failed / 0 ignored** in 167.13 s (it was 1785 before this unit — the +1 is this
  unit's own test).
- **NOT verified here, and it cannot be**: that the icon actually appears in the keeper's notification area. That
  needs a live `StatusNotifierHost` in his session, so it is his check after installing the new build. The private
  bus can prove registration happens; it cannot prove his panel draws it.

### Install (2026-10-14, with the keeper's authorization)

- **Return point first**: tag `pre-ksni-0.3-20261014` at `6fd7faf`, and the installed binary copied to
  `target/backup/hve-pre-ksni-0.3-6fd7faf`, md5 `56142fae465844d4f093b5ad5e1cc26c` — identical to
  `~/.local/bin/hve` at that moment (the Lane C build).
- `cargo build --release` → `target/release/hve`, md5 **`379412a71f9d19dd1a4fd38cb9539cb5`**, 2m10s. A second
  `cargo build --release` reports nothing to recompile, which is the proof that the installed bytes are committed
  HEAD (`ca5b6bf`).
- Installed by RENAME (temp file + `mv`), not `cp`: the keeper's HVE was RUNNING and `cp` over a live executable
  fails with `Text file busy`. Installed md5 verified EQUAL to the build; `~/.local/bin/hve --version` → `hve 1.0.0`.
- **No assets were re-copied**, deliberately: this change touches no shipped asset (unlike Lane C's install, which
  found four stale files under the installed assets directory).
- **What only the keeper can check**: close and reopen HVE — the window he had open was the old build — and look at
  the notification area: the HVE icon must be there, and the menu's Hide/Show Window label must follow the window.
  If the tray does NOT appear, the new warning line is the first thing to read, because it now says why.


### Independent verification (2026-10-14, after the commit)

- **Verifier: `opencode-go/mimo-v2.6-flash`** — a different model from the writer (`deepseek-v4.1-flash`), launched
  as a fresh subagent with no shared context, explicitly asked to REFUTE the change.
- **Result: no blockers.** S1..S7 each returned SOUND, with the strongest claims re-proved from scratch rather than
  read: it built a **python fake `org.kde.StatusNotifierWatcher` on a `dbus-run-session` private bus** and observed
  the item register late (`RegisterStatusNotifierItem(org.kde.StatusNotifierItem-<pid>-1)`), plus a live readback of
  `Title`, `Id`, `IconPixmap` (48×48, 9216 bytes) and the full menu label list — that is S3 and S4 proved
  behaviourally, not by argument. It also ran the same probe with the flag removed and saw `Err(ServiceUnknown)`
  with zero registrations, confirming the flag is load-bearing.
- **Advisories it raised, and what was done with each**: (1) the lock loses 16 packages, not 17 — corrected in S1
  and here; (2) `thiserror 1.0.69` stays in the graph — S1 corrected; (3) absolute tree counts are
  environment-brittle — S1 now states the delta as the claim; (4) ksni's own internal `assert`/`expect` calls make
  "nothing panics" too absolute — S2 scoped to HVE's failure paths; (5) the zbus `/run/user/<euid>/bus` fallback
  makes "no session bus" loose — recorded in S6. None of them is a code defect.
- **What it could not verify**: whether the keeper's own panel draws the icon (no live host), and the baseline's
  clippy count (that would need a full rebuild of the old dbus stack).
- **Workspace left clean by the verifier**: `git status --porcelain` empty, all its probes and scratch deleted, no
  new files and no worktrees left behind.

## T2 — the hang the bump introduced (2026-10-15)

Root cause, proven in code: `ksni 0.3`'s blocking `Handle::update` is `compat::block_on(...)` — it waits for the
tray service, where 0.2 only enqueued. The hide path (`src/main.rs`: `toggle_tray` + `refresh_global_menu()`) runs
on the UI thread while HOLDING the composer's controller mutex (`global_controller()` returns the locked guard), and
the tray's `menu()` — on the tray thread — needs that same lock. Both sides waited for each other: the UI thread
stopped answering Hyprland's pings, the compositor showed its ANR dialog (`hyprland-dialog`, driven by
`enable_anr_dialog` / `anr_missed_pings`), and choosing close SIGKILLed the process. The same wedge explains the
keeper's report that the tray had no menu.

- **Fix**: (1) one worker thread owns the blocking `update`; `TrayHandle::update_icon`/`refresh_menu` only enqueue a
  `TrayTask` over an `mpsc` channel; (2) `HveTray::menu()` reads the hidden state with `try_global_controller()`;
  (3) the pin in S9. If the worker thread cannot start, `start_tray` warns and returns `None` rather than leaving
  callers that can block.
- **RED/GREEN**: the pin `tray::tests::tray_updates_never_block_the_caller` failed first against the pre-fix body
  (which called `update` directly on the caller and `global_controller()` in `menu()`); GREEN after the rewrite →
  `test tray::tests::tray_updates_never_block_the_caller ... ok`.
- **Checks on the frozen bytes**: `cargo fmt --check` clean (it caught one stray blank line, removed);
  `cargo clippy --all-targets` → **67 warnings, the same count Lane C and T1 left** (none from `src/tray.rs`);
  full suite **1789 passed / 0 failed / 0 ignored** in 164.83 s. This unit adds exactly ONE test.
- **Install (2026-10-15)**: `cargo build --release` → md5 **`ee79e79031ba2da6bf0da8c57b523364`**, installed by
  RENAME (temp file + `mv`) into `~/.local/bin/hve`; installed md5 equals the build; `~/.local/bin/hve --version`
  → `hve 1.0.0`. The previously installed bytes (`0841f9b10cb2c71a4b18790846c8284c`, the i18n build) did NOT contain
  this fix, which is why the keeper's app still hung on hide.
- **Only the keeper can check the rest**: press Escape to hide — HVE must stay alive and answer (no ANR dialog), and
  the tray's Hide/Show label must follow the window.

### T2 independent verification (2026-10-15)

- **Verifier: `opencode/mimo-v2.6-flash-free`** — a different model from the writer (`opencode-go/deepseek-v4.1-flash`,
  and free, on the keeper's instruction while credit is tight), fresh context, explicitly asked to REFUTE.
- **Result: SOUND, no blockers.** It re-proved the root cause from the dependency's own source rather than from the
  commit message: `ksni-0.3.5/src/blocking.rs:206-207` (`update` = `compat::block_on(self.0.update(f))`, resolving to
  `async_io::block_on` under HVE's features), `lib.rs:518-531` (the async update waits for the ack),
  `service.rs:169-173` (the ack is sent only AFTER `service.update(...)`, which rebuilds the menu —
  `service.rs:344-347` → `:291` `menu_flatten(self.tray.menu())`), and `ksni-0.2.2/src/lib.rs:182-186` (0.2 only set
  a `need_update` flag and returned). That is the whole wedge: under 0.3 every update the UI thread made while
  holding the controller lock waited for a menu rebuild that needed the same lock.
- **It also confirmed the call-site map**: `update_icon`/`refresh_menu` have no caller outside the UI thread, no
  direct ksni handle exists outside `src/tray.rs`, every `HveTray` method that ksni calls on the tray thread is pure
  except `menu()`, and the one remaining blocking `global_controller()` (`tray.rs:335`) sits inside a closure handed
  to `slint::invoke_from_event_loop`, so it runs on the UI thread.
- **Advisories it raised, and what was done with each**: (1) the `.update(` count pin cannot see WHERE a call sits —
  a call moved into a caller keeps the count at 2, so it would stay green through a regression: **fixed in T3**;
  (2) the 1200-character window past `fn menu(&self)` was blind to a blocking read added deeper inside `menu()`:
  **fixed in T3** (the pin now scopes to the part of `menu()` the tray thread actually runs, up to its `vec!`, and
  says why the closures are out of scope); (3) a worker thread that panicked would stop tray updates without a log
  line: **accepted**, not a defect this commit caused, and it needs a panic that cannot now happen on that thread
  (its two arms are a struct-field assignment and a no-op); (4) "the next refresh corrects it" is loose wording for
  the stale label — a rebuild that computes the same label emits no change signal, so it corrects on the next
  TOGGLE: **accepted**, recorded in S8 as the price of not waiting.
- **What it could not verify, and said so**: whether the keeper's panel draws the icon and whether his ANR dialog is
  gone — both need his live session. It ran no live-bus probe on purpose (it would register a second
  StatusNotifierItem in his panel, the reason the project's temporary probe is deleted before each commit).
- **Checks it re-ran on `287528a`**: full suite `1789 passed / 0 failed` (165.24 s), `cargo fmt --check` clean,
  `cargo clippy --all-targets` 67 warnings with none from `src/tray.rs` — all matching the writer's claims.
- **Workspace left clean**: `git status --porcelain` empty, no new file, no worktree, scratch dir deleted.
- **Not a blocker but worth keeping**: the pin's RED is carried by its menu assertions, not by the count — on the old
  body the old file already had exactly two `.update(` sites, so the count assertion passed while the shape was
  wrong. T3 is the direct answer to that.

## Log

- **L1 (2026-10-14, the keeper, verbatim)**: `arranca con ksni 2.2`
- **L2 (2026-10-14, the parent, recon)**: read the plan in `odd/tasks/lane-c-health.md` ("T5 recon") and the Engram
  record `deps/ksni-bump`; then read BOTH crates' sources to settle what the plan left open:
  `ksni 0.2.2 service.rs:91-95` (`thread::spawn(|| self.run().unwrap())` — a failure is an invisible thread panic) and
  `:180-217` (it waits for the watcher via `NameOwnerChanged`); `ksni 0.3.5 blocking.rs:176-192` (`spawn_with_options`
  makes one synchronous registration and only then spawns the thread, so the caller gets a `Result`),
  `service.rs:84-97` and `:114-120` (`assume_sni_available` is what turns a missing watcher into a soft error, and is
  also what skips the `WontShow` check), and `Cargo.lock` (ksni 0.3.5 already resolves on `async-executor`/`async-io`/
  `async-lock`/`futures-*` + `zbus`, with NO `tokio`, so the feature choice must be `async-io` or ksni's own
  `compile_error!` "Features tokio and async-io cannot be enabled at the same time" fires).
  Baseline before the change: `cargo tree` = **456 unique packages**, 6 lines matching dbus/dbus-tree/dbus-codegen/
  clap v2.34, `ksni v0.2.2` reached only from `hve`, `ksni v0.3.5` only from `i-slint-core`.
- **L3 (2026-10-14, the parent, decisions)**: (1) "no tray" = warn and continue, because it is what today's code
  already does observably — a panic nobody sees does not stop the editor. (2) `assume_sni_available(true)` is NOT
  slipped in as a preference: without it the bump regresses the case where HVE starts before the panel.
  (3) `system_active` moves to the caller (S5) so the `None` case does not need a fake Arc.
- **L4 (2026-10-14, the keeper, verbatim)**: `cada vez que aprieto Escape y lo mando a ocultar me mata HVE`,
  plus `se ha quedado pillado, hay que matarlo` and the report that the tray had lost its menu.
- **L5 (2026-10-15, the keeper, verbatim)**: `arregle hve anda` — read as: finish the fix and hand back a working
  app. What was still missing at that point: the worker rewrite was WRITTEN in the working tree but unverified,
  uncommitted and NOT in the installed binary.
- **L6 (2026-10-15, the parent, decisions)**: (1) the fix restores 0.2's ENQUEUE shape on 0.3 rather than moving the
  whole hide path off the lock — smaller blast radius, and the lock's owner stays where it is. (2) `menu()` accepts a
  momentarily stale Hide/Show label instead of waiting: the label is cosmetic, the wedge is fatal. (3) The report
  `HVE en el tray ahora no tiene el menú` is treated as the SAME defect, not a second one, because the tray thread
  was blocked on the controller lock the UI thread held. (4) STILL OPEN, the keeper's decision, unanswered since
  2026-10-14: whether to disable `enable_anr_dialog` (or raise `anr_missed_pings`) in his Hyprland config, so a slow
  app is not killed while the compositor waits.
- **L7 (2026-10-15, the parent, T3)**: the independent verifier's two pin advisories were taken as real and fixed,
  because the pin is the ONLY thing standing between the keeper and the same hang coming back: the assertion now
  scopes to the `TrayHandle` impl (no `.update(` may appear there at all) and to the part of `menu()` the tray thread
  runs (up to its `vec!`, since the item closures are deferred to the UI thread). RED re-proved by running the same
  algorithm over `git show e9396df:src/tray.rs`: on the old body the count assertion PASSED (exactly two sites) while
  the three shape assertions failed, which is exactly the hole the count left. GREEN after: full suite
  **1789 passed / 0 failed**, `cargo fmt --check` clean. The rest of the advisories were accepted with a reason, not
  silently dropped.
