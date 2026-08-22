# Archive Report: hve2-trunk — State-Driven Navigation Shell

## Change Summary

**Change**: hve2-trunk
**Description**: HVE 2 state-driven navigation shell — skwd-wall (MIT) concept translation to Slint. Single mutating window, NavState, SlotRegistry, SizePolicy, shell chrome, i18n (ES/EN), keyboard+mouse navigation, and headless integration tests.
**Branch**: hve2-visual-rewrite
**Archive Date**: 2026-08-21
**Artifact Store**: hybrid (OpenSpec + Engram)

## Final-State Facts

Per the Final-State Authority hierarchy, the following facts represent the state of the change at close, not earlier intermediate snapshots:

### Verification
- **Test suite**: 245 passed, 0 failed, 1 ignored (`cargo test`)
- **Build**: `cargo check` passed with zero warnings
- **Spec compliance**: 9/9 requirements, 23/23 scenarios — all PASS
- **Critical findings**: 0
- **Blockers**: 0
- **Native validation**: `gentle-ai.verify-result/v1` passed (verdict: pass)

### D5 Remediation
- `Shell::sync_after_show` implemented in `src/shell/mod.rs` (UI-thread global registration)
- Production show paths wired in `src/composer/hyprland.rs` (v5 special-workspace, v5 fallback remap, v4 special-workspace, v4 fallback remap, HyprMode::None)
- Regression test: `integration_production_show_sync_restores_expanded_state`

### Engine & Composer Contracts
- Sealed engine files untouched (verified by `git diff`)
- Composer trait unchanged; shell wired via existing seams

### Scope
- No gallery implementation (module 2) — stub slots only
- No workshop implementation (module 4) — stub slots only
- No skwd-wall visual styles (module 3) — design tokens only
- No engine/config/settings changes

## Task Completion Gate

- **Total tasks**: 18
- **Completed**: 18/18 (all checked `[x]`)
- **Source**: `openspec/changes/archive/2026-08-21-hve2-trunk/tasks.md`
- **Gate**: PASS — no unchecked implementation tasks

## Native Review Receipt Gate

- `reviewGate`: structurally absent (review mode disabled for this repository)
- **Gate**: PASS — ordinary repository policy applies, no receipt to validate

## Specs Synced

| Domain | Action | Details |
|--------|--------|---------|
| nav-shell | Created | 5 requirements, 14 scenarios — copied as main spec (no prior main spec existed) |
| base-window | Created | 4 requirements, 9 scenarios — copied as main spec (no prior main spec existed) |

Both delta specs were full specs (not deltas against existing main specs). No merge was needed; they were copied mechanically to `openspec/specs/{domain}/spec.md`.

## Main Specs Updated

- `openspec/specs/nav-shell/spec.md` — created (source of truth for nav-shell domain)
- `openspec/specs/base-window/spec.md` — created (source of truth for base-window domain)

## Archive Contents

- `proposal.md` ✅
- `specs/nav-shell/spec.md` ✅
- `specs/base-window/spec.md` ✅
- `design.md` ✅
- `tasks.md` ✅ (18/18 tasks complete)
- `verify-report.md` ✅

## Archived To

`openspec/changes/archive/2026-08-21-hve2-trunk/`

## Engram Observations Read

| Artifact | Observation ID | Topic Key |
|----------|---------------|-----------|
| proposal | #528 | `sdd/hve2-trunk/proposal` |
| spec | #529 | `hve2-trunk spec` |
| design | #530 | `hve2-trunk design (skwd-wall translation)` |
| tasks | #531 | `hve2-trunk tasks` |
| verify-report | #545 | `sdd/hve2-trunk verify report — PASS (strict TDD)` |

## Design Decisions (archived)

- D1: NavState in Rust (single owner, UI read-only)
- D2: Rapid-nav FIFO VecDeque
- D3: Slot trait + HashMap registry
- D4: Stepped Timer 16ms for window sizing
- D5: Composer sync_after_show (UI-thread global)
- D6: SkwdTokens export global
- D7: Canvas 2D (module 3 seam, N/A in trunk)
- D8: Keyboard = mouse (single NavCommand)

## Risks at Close

1. **UI-thread global lifetime**: Shell is thread-local, must initialize on Slint UI thread before compositor show operations. Production initialization verified in all show paths.
2. **No GPL contamination**: Verified — no GPL code from hyprmod used. All visual ideas translated from skwd-wall (MIT) with credit headers.

## SDD Cycle Status

**Formally closed.** The change has been fully planned, implemented, verified, and archived. Ready for the next change (module 2: theme gallery).
