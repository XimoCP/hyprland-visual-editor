# Feature: temporary colour-authority yield around a theme apply

**Locator**: `odd/tasks/skwd-policy-toggle-around-apply.md`
**Engram mirror**: topic `odd/skwd-policy-toggle-around-apply/tasks`
**Repo**: `/home/ximo/Proyectos/hve` — branch `hve2-visual-rewrite` (do NOT switch branches)
**Checkpoint before this work**: `ee93935` (clean tree; roll back here and the patch leaves no trace)
**Status**: PLANNED — keeper-authorized 2026-09-22, awaiting his go on the work units
**TDD**: strict — runner `cargo test`
**Delivery budget forecast**: ~150-250 authored changed lines (source + tests + this document)

## Objective

Applying a theme must not show the wallpaper engine's auto-derived colours. Today it does, for ~4 s.

## Problem, with the measured evidence

`skwd-wall` (third-party — NOT ours) owns the colour scheme: `~/.config/skwd-wall-v2/config.json` carries `theme.policy == "wallpaper"`, so it regenerates a wallpaper-derived palette **asynchronously after** HVE's synchronous apply steps and re-imposes `custom skwd-wall`. HVE detects that ownership and re-asserts the theme's palette, but deliberately waits first (settle 2 s, then up to 3 sets 2 s apart, cap 8 s — `src/providers/noctalia.rs:900-903`), because re-asserting earlier would simply lose again.

Measured live on 2026-09-22 16:41 (apply of generation 4, from `/tmp/opencode/hve-live.log`):

| Moment | Event |
|---|---|
| +0.00 s | wallpaper painted (`Restored wallpaper: joker3.png`) |
| +0.88 s | HVE already wrote the theme palette (`Set active scheme: custom JokerTheme`) |
| +3.18 s | HVE's reveal finished (view returned, targeted fullscreen true) |
| +4.10 s | the theme palette finally wins (`re-asserted and verified`) |

So the keeper sees the engine's colour for ~4.1 s, of which ~1.8 s are **after** the desktop is revealed — exactly when he is looking.

## Intent (the keeper's own design)

Temporarily yield the engine's colour authority for the duration of one apply: before handing the wallpaper over, set `theme.policy` to `off` in the engine's config file; apply the theme and let the theme palette land; then put the value back **exactly** as it was.

Explicit constraints from the keeper:

- Do NOT leave the switch off permanently.
- Do NOT depend on the engine's command interface (`skwd-helm`) — a third-party program whose updates would break us.
- Do NOT change anything inside `skwd-wall`.
- A checkpoint commit first, so a failure leaves no trace.

## Verified premises (read from source, 2026-09-22)

1. **The engine reloads its config when the file changes.** It stores the file's mtime and reloads on change: `skwd-deck/crates/skwd-wall-core/src/infrastructure/config/store.rs:8-23` (`mtime` field + `reload_if_changed`), called from `crates/skwd-wall-core/src/composition/state/model.rs:119` and `crates/skwd-walld/src/infrastructure/image_optimization.rs:199`. The daemon holds that store: `crates/skwd-walld/src/composition/context.rs:18`. An external edit IS observed (on its internal tick, not instantly).
2. **`theme.policy` accepts exactly three values**: `wallpaper | fixed | off` (`crates/skwd-config/src/settings.rs:116-119`; key name at `crates/skwd-config/src/key_catalog.rs:380`).
3. **`noctalia.themeMode` is read by nobody**: absent from the key catalog and from the engine source; only HVE reads it (`src/providers/noctalia.rs:887-892`), out of caution.
4. **This is not a new dependency class**: HVE already reads that same file for ownership detection (`src/providers/noctalia.rs:857-893`).
5. **Two wallpapers carry a pinned per-wallpaper profile** with `theme.policy: "wallpaper"` (`theme.wallpaperProfiles`, keys `static:Cars/car7.jpg` and `static:wallp9.jpg`). Those override the general switch.

## Scope

In scope (HVE only):

- Flip `theme.policy` to `off` before the wallpaper hand-off, restore the previous value once the theme palette is verified.
- Preserve every other key of that file, its remaining formatting, and its permissions (currently `0600`).
- Re-read the file before restoring, so a concurrent engine write is never clobbered with a stale copy.
- Crash safety: if HVE dies between the flip and the restore, restore on the next start.
- Honest logging of every step and of every failure to write; a failed write must never abort the apply.

Out of scope (recorded, not done here):

- Anything inside `skwd-wall`; the `skwd-helm` command interface.
- The pinned per-wallpaper profiles (residual — decide after the general case works).
- `skwd-helm apply ... timed out after 500ms`, logged on **every** apply (separate smell, worth its own investigation).
- The permanent-config variant (the keeper rejected leaving it off).

## Design constraints (house pattern)

- Pure decisions plus a thin impure shell, the same shape as `hide_plan` and `blur_decision`.
- Pure: whether to yield at all; which value to restore; the exact edited JSON text, given the original text and the previous value.
- Impure: read, write, wait, log.
- Never panic. A missing or unparsable config means "no ownership": behave exactly as today.

## Work units

**W1 — yield and restore the value (pure plan + impure write).**
Read the engine config; if `theme.policy == "wallpaper"`, produce the edited text with `off` and remember the previous value; restore it afterwards. Preserve every other key, keep the file's permissions, re-read before restoring.

**W2 — wire it around the apply, with crash recovery.**
Flip before the wallpaper hand-off; restore when the palette is verified (or at the bounded cap); on startup, if a marker says we left it off, restore and log.

## Acceptance criteria

1. With `theme.policy == "wallpaper"`, an apply writes `off` BEFORE the wallpaper hand-off and restores `wallpaper` after the palette is verified (or at the cap).
2. With any other value (`fixed`, `off`, missing file, unparsable file), HVE writes nothing at all and behaves exactly as today.
3. The write preserves every other key and the file's permissions; re-reading our own output yields the original document except for that one value.
4. If the process dies between the flip and the restore, the next start restores the previous value and logs it.
5. Every write failure is logged and never aborts the apply.

## Checks

- `cargo test` — strict TDD, RED first.
- All new tests run against a sandboxed config path: the existing `SKWD_WALL_V2_CONFIG` seam already redirects it, and the tests must NEVER touch the keeper's real config.
- Live confirmation belongs to the keeper: apply a theme and watch for the flash.

## Risks / residuals (to watch)

1. **Restoring `wallpaper` may re-arm the engine.** If it then regenerates, the theme palette could be stomped again — the same problem, later. Must be observed live; if it happens, the restore point is wrong.
2. **The two pinned profiles** still flash until decided.
3. **The engine writing while we hold `off`** (keeper touching its settings mid-apply) could race our restore; mitigated by re-reading before restoring, and the window is milliseconds wide.
4. **The engine's reload is tick-based**, not instant: the flip needs a moment to be observed before the wallpaper is handed over. The wait must be measured, not guessed.

## Verification record

| Check | Who | Result |
|---|---|---|
| Full suite | orchestrator (re-ran it, did not trust the writer's report) | `cargo test` → **918 passed / 0 failed** (~33 s), 0 build warnings. Baseline before W1: 903. |
| RED first (strict TDD) | writer, observed before implementing | 14 of the 15 new tests failed against `todo!()` stubs; the single green was the mechanical seam test, as expected. |
| Cross-model independent verification (read-only, adversarial) | **GLM 5.3 Flash** — the writer ran on **DeepSeek V4 Flash** | **PASS WITH FINDINGS** — no BLOCKING, no MAJOR, three MINOR (below). Independently confirmed: the keeper's real config is unreachable from production code (the `SKWD_WALL_V2_CONFIG` seam is the only redirect, and every test uses it); the marker never lands in the engine's config directory; the ambiguity refusal is total (all planning is pure and happens before any write, so there can be no partial write and no stray marker); byte preservation holds for every shape tested, including the dotted `"theme.policy"` profile keys; zero panics in the production half; no existing test weakened or deleted. |
| Full suite (W2) | orchestrator (re-ran it, did not trust the writer's report) | `cargo test` → **934 passed / 0 failed** (~34 s), 0 build warnings. Baseline before W2: 918. |
| RED first (W2, strict TDD) | writer, observed before implementing | Three staged RED runs, **regenerated** because the first writer session was interrupted mid-flight and left uncommitted work: (1) primitives inert → 12 failed in `skwd_policy` + 3 in `noctalia` + 1 in `main`; (2) yield active but the guard's `Drop` disabled → 3 failed, proving the guard is what restores; (3) ownership evaluated AFTER the yield → 1 failed against the new trap fixture, proving the documented trap is really exercised. |
| Cross-model independent verification (W2, read-only, adversarial) | **GLM 5.3 Flash** — the writer ran on **DeepSeek V4 Flash** | **PASS WITH FINDINGS** — no BLOCKING, no MAJOR, two MINOR and two NIT (below). Independently confirmed: no reachable path leaves the switch `off` (every branch either restores through the guard's `Drop` or hands ownership to the worker, which restores on all of its exits); the ownership snapshot really is taken before the flip, and the trap fixture pins that regression; the marker-first ordering is correct and startup repair runs only in the lock-owning instance, so it can never repair under a live yield; all three W1 findings are closed; no test weakened or deleted; no panics in the production halves. |
| Live behaviour | **NOT RUN** | Needs the keeper: apply a theme with the engine owning colours and watch for the flash. This is also what tunes the 250 ms observe wait — the elapsed time is logged for exactly that. |

## Findings from the W2 verification (open)

1. **MINOR — the observe wait blocks the UI thread.** The 250 ms wait between the flip and the wallpaper hand-off uses `std::thread::sleep` on HVE's event-loop thread (`noctalia.rs:946-956`), once per apply while the engine owns colours. It is bounded and lands during a theme transition, so it is not a regression to be scared of — but the cleaner shape is to perform the wait inside the spawned worker. To be judged against the keeper's live feel.
2. **MINOR — the repair can clear the marker without repairing.** `recover_crashed_yield` clears the marker even when `plan_restore` refuses (it needs a unique anchor). The switch would then stay `off` with no retry, and the log line ("value already restored, or changed by the keeper") would be inaccurate. Narrow — it requires the anchor to become ambiguous between the yield and the restore — but the marker exists precisely to retry.

NIT: `src/providers/skwd_policy.rs` ends without a trailing newline.
NIT: the delivery forecast was badly missed — the document predicted ~150-250 authored lines; W2 alone is ~800, mostly tests. See the delivery note below.

## Delivery note (open)

W1 + W2 together are far past the ~400 authored-lines-per-slice budget (the W1 module alone is 816 lines; W2 adds ~824 across three files). The repository has **no remote configured** and the branch is hundreds of commits ahead of `master`, so the vehicle for delivery is a separate, later decision — recorded here so it is not discovered at PR time.

Findings from the W1 verification — **all three closed by W2** (all MINOR; folded into the next work unit rather than opening a separate correction round):

1. `write_atomic` leaves its `.tmp` file behind when the write or the sync fails; only the rename-failure path cleans up. A later write truncates it, but a failed write should leave nothing.
2. The temp file name is fixed, so two concurrent invocations would collide. Not reachable while W2 keeps a single caller, but it should be unique.
3. `write_marker` does not `sync_all`, unlike the config write. A power cut at that instant could leave a corrupt marker, and recovery refuses to clear a marker it cannot parse — so the switch could stay off until a later startup repairs it. The marker exists precisely to prevent that state, so it must be as durable as the write it protects.

Coverage gaps (NIT): no test for the `"policy" : "wallpaper"` spacing variant, and none for a bare `"policy"` nested in another object. The logic covers both; the coverage is unexercised.

## Progress

- [x] W1 — yield/restore the value (pure plan + impure write) — shipped as a
  work-unit commit on 2026-09-23 (hash reported by the orchestrator, not
  self-referential on purpose). Primitives live in
  `src/providers/skwd_policy.rs`: pure `plan_yield`/`plan_restore` (single
  bare `"policy"` anchor, refuses ambiguity, byte-preserving), impure
  `yield_color_authority`/`restore_color_authority` (same-dir atomic write,
  mode preserved, marker-first protocol), and `recover_crashed_yield`.
  The path resolver is now exactly one: `noctalia::skwd_wall_config_path`
  delegates to `skwd_policy::config_path`.
- [x] W2 — wire around the apply + crash recovery (**includes closing the three W1 findings above**) — shipped as a work-unit commit on 2026-09-23 (hash reported by the orchestrator, not self-referential). Wiring: `noctalia` yields via `skwd_policy::yield_color_authority` BEFORE the wallpaper hand-off (a 250 ms bounded observe wait — `COLOR_YIELD_OBSERVE_WAIT`, cadence measured from skwd-deck source on 2026-09-23, elapsed time logged so the live test can tune it), holds the previous value in a RAII `YieldGuard`, and the bounded re-assert worker restores it once the palette is verified (or at the cap). The ownership decision is snapshotted BEFORE the flip (the W2 trap: after the flip the config reads `off`); every non-worker exit path restores via the guard's Drop, which is disarmed only when the worker takes ownership. `startup_recover_crashed_yield()` runs once at startup, after the single-instance lock, and logs repaired / nothing / failed. W1 findings closed: temp cleanup guard on every failure path, unique per-invocation temp names with an orphan sweep, marker fsync. Test (a) strengthened with a trap fixture (`OWNER_TRAP_JSON`, policy `wallpaper` without `themeMode: follow`) so the post-yield-ownership regression is actually exercised. Full suite **934 passed / 0 failed**, 0 build warnings. Live confirmation still pending (keeper).
- [ ] Live confirmation (keeper)
