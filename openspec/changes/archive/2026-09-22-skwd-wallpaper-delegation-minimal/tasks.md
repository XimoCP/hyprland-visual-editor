# Tasks: skwd-wallpaper-delegation-minimal

Minimal agnostic fix — no deep sync. Each task closes with `cargo test` green.

## Task 1 — Remove Sep-6 force-off rule (agnostic restore)

**Goal:** HVE copies `settings.json` verbatim again; wallpaper IPC unconditional.

**Changes:**
- In `src/providers/noctalia.rs`:
  - Delete `force_noctalia_wallpaper_off()` function (lines 165-179) and its comment block (153-161)
  - Delete `noctalia_wallpaper_module_enabled()` (184-192)
  - In `NoctaliaV4Provider::apply()` (378-391): replace settings.json special-case with plain `fs::copy` like other source files
  - In `NoctaliaV4Provider::post_apply()` (433-461): remove `wallpaper_module_on` gate and the `else` branch that skips IPC; keep only the IPC loop (has_named logic stays)
  - Remove / update tests `settings_patch_forces_wallpaper_module_off_and_keeps_the_rest`, `settings_patch_creates_wallpaper_object_when_missing`, `settings_patch_never_corrupts_malformed_json`, `wallpaper_module_enabled_reads_live_settings_conservatively` — replace with passthrough test: `settings_json_copied_verbatim`
- No UI, no other providers touched

**Tests (TDD first):**
- `test_settings_json_copied_verbatim`: create temp theme with `settings.json` containing `wallpaper.enabled=true`, apply via temp config dir, assert live `settings.json` still has `enabled:true` (not forced false)
- Existing `test_noctalia_v4_provider_new` etc. stay green
- Run `cargo test` — all green

**Done when:** `grep -r force_noctalia_wallpaper_off src/` empty and wallpaper IPC runs without gate.

---

## Task 2 — Best-effort delegation to skwd-walld (silent, agnostic)

**Goal:** After successful Noctalia wallpaper IPC, forward same path to skwd-walld if its socket exists; never fail apply.

**Discovery spike (time-box 30m):**
- Confirm socket exists: `/run/user/1000/skwd-wall-v2/wall.sock` (found in explore) and payload format
- Try `echo -n "/path/to/wall.jpg" | socat - UNIX-CONNECT:/run/user/1000/skwd-wall-v2/wall.sock` and `nc -U` variants; check skwd-walld source or `--help` for expected JSON vs plain path
- If undocumented and cannot be sniffed quickly, still land Task 1 alone as usable slice; document fallback.

**Changes (if protocol found):**
- In `src/providers/noctalia.rs` (both `NoctaliaV4Provider::post_apply` after IPC loop, and `NoctaliaV5Provider::apply` after `wallpaper-set` IPC at line 864):
  - Add helper `fn delegate_to_skwd_walld(wallpaper_path: &Path)`:
    - Resolve socket path via `XDG_RUNTIME_DIR` or `$HOME` fallback: `format!("{}/skwd-wall-v2/wall.sock", runtime_dir)`
    - If socket missing → return Ok(())
    - Try `UnixStream::connect` with 500ms timeout, write path + newline, flush, drop; any error → `tracing::warn!` and return Ok(())
    - Never propagate error to caller
  - Call helper best-effort after each successful wallpaper IPC (iterate over same `entries` / `wp` path)
- Keep agnostic: helper probes socket, no hard dependency

**Tests (TDD first):**
- `test_delegate_noop_when_socket_absent`: set env `XDG_RUNTIME_DIR` to empty temp dir, call helper with dummy path, assert Ok(()) and no panic
- `test_delegate_swallow_failure`: create temp socket path that is a regular file (not socket), call helper, assert Ok(()) (error swallowed)
- `test_delegate_success_with_fake_socket` (optional if spike succeeds): spawn `UnixListener` on temp path, call helper, assert listener received path string
- All `cargo test` green

**Done when:** With socket present wallpaper visible via skwd-walld; with socket absent behavior unchanged; failures never bubble.

---

## Task 3 — Verify agnostic file flow + reload

**Goal:** Prove HVE still owns only files + reload, no daemon coupling.

**Checks:**
- `cargo test` full suite green (including new tests from Task 1-2)
- Manual: create theme with wallpaper A, apply — live files restored + visible wallpaper changes (via Noctalia or skwd-walld)
- Manual: stop skwd-walld (`systemctl --user stop skwd-walld`), apply theme with wallpaper B — still applies via Noctalia, no error
- Manual: change wallpaper externally via `skwd-walld` direct, list themes — HVE still shows its own theme `last_applied`, no sync (ignored as spec)

**Deliverable:** `cargo test` output + brief manual notes in verify report.

---

## Ordering

Task 1 → Task 2 (depends on IPC being unconditional) → Task 3 (verification). Task 1 alone is shippable if Task 2 spike blocked.

## Review Budget

Estimated changed lines: ~80 (Task 1 deletions) + ~60 (Task 2 helper + tests) = <200 lines, well under 800 budget. Single PR with work-unit commits per task.
