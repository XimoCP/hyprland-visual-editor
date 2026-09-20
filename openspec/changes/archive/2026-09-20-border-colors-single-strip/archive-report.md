# Archive Report — `border-colors-single-strip`

- **Change**: `border-colors-single-strip`
- **Archived**: 2026-09-20 → `openspec/changes/archive/2026-09-20-border-colors-single-strip/`
- **Artifact store**: openspec (repo-local; no Engram mirrors written)
- **Status at close**: tasks 30/30 complete; no verify report was ever produced for this change.

## Final State (AT CLOSE — terminal record)

The change shipped as 4 chained PRs on branch `hve2-visual-rewrite`, all merged into
that branch. All of the following commits were corroborated in the branch history
during archive:

- PR1 — stop-count formula fix in `callbacks.rs` (strip-as-single-stop counting).
- PR2 — commit `0d271a1`: strip UI + picker card + compact rows; `BorderColorSlot.slint` deleted.
- PR3 — commit `b4c6e2a`: keyboard sub-navigation — `strip-cycle` on `BordersSection`,
  forwarded from `PanelRoot`, `tune-enter` on stop 3, fixed index map.
- PR4 — commits `63f64ad` + `476a442`: picker card always mounted with
  `states`/`transitions`, plus render verification by reading PNGs.
- Follow-up defect fixes on the same branch: `1d7c3d0` (re-seed the active border
  marker on section entry), `8115232` (resolve a missing tertiary colour instead of
  aliasing the secondary), `5f91b27` + `4363e7a` + `e9aed85` + `0747f2c` + `beedf42`
  (quality findings from an independent cross-model review: strip ring clamp, ghost
  stops at zero colours, pinned glow middle stop, corrected picker comment).

## R5 Deviation — Resolved 2026-09-20 by Keeper Decision

R5 originally promised a vertical slide (`y: -8px → 0px` entry, `0px → -8px` exit)
that the implementation cannot perform, because the picker card is an in-flow child
of the `picker-block` VerticalLayout and its `y` is owned by the layout. The keeper
decided to document the behaviour that exists and keep the current animation. R5 was
corrected to describe the real entry (height 0px → 380px, opacity 0 → 1,
transform-scale-x/y 0.95 → 1.0, 250ms `ease-in-out-back`) and exit (height 380px →
0px, opacity 1 → 0, 150ms `ease-in`). Design Open Question #3 was closed with that
resolution. This correction was UNCOMMITTED in the worktree at archive time and was
archived as-is (the staged rename carries the worktree modification; see `git
status` `RM` entries observed during archive).

## Design Open Questions — REMAIN OPEN (not blockers)

- **#1**: `transform-scale-x/y` is GPU-only and therefore outside the
  software-renderer visual gate.
- **#2**: whether `strip-cycle` forwarding from `PanelRoot` should be replaced by a
  `strip-engaged` boolean.

Both are preserved as open in the archived `design.md`. They were not resolved and
are not treated as blockers.

## Verification

No verify report was ever produced for this change. This is recorded honestly; no
findings are invented. Render verification evidence cited in the final-state facts
(PR4, PNG reading) comes from the orchestrator's launch prompt and commit history,
not from a persisted `verify-report` artifact.

## Spec Sync

- No canonical spec existed at `openspec/specs/border-color-strip/spec.md`, so the
  delta spec was a full spec. It was copied mechanically (`cp` + `diff -r` readback,
  empty diff) to the new source of truth:
  - `openspec/specs/border-color-strip/spec.md`
- The synced spec contains requirements **R1 through R15**:
  R1 Horizontal Chip Strip · R2 Chip Identity and Hover/Focus · R3 In-Flow Picker
  Card (Open) · R4 In-Flow Picker Card (Close) · R5 Picker Entry Animation
  (corrected entry/exit, no `y` animation) · R6 Compact Inactive Colour Row ·
  R7 Compact Glow Colour Rows · R8 Keyboard: Strip as Single Stop ·
  R9 Keyboard: Sub-Navigation Within Strip · R10 Scroll-to-Strip When Picker Opens ·
  R11 Shared Picker State via editing-slot · R12 BorderColorSlot Deprecation ·
  R13 Visual Render Verification · R14 Test Rewrite · R15 Picker Content: Tokens + HSV.
- Native delta-spec composition (`sdd-archive-compose`) was not applicable: there was
  no pre-existing canonical spec to compose into, and the delta carried no
  ADDED/MODIFIED/REMOVED/RENAMED sections.
- No `rules.archive` exist in `openspec/config.yaml` (no `rules:` section); nothing
  further to apply.

## Archive Contents (observed, not inferred)

- `proposal.md`: present (moved byte-identical).
- `specs/border-color-strip/spec.md`: present, including the uncommitted R5 correction
  (moved byte-identical).
- `design.md`: present, OQ #3 closed, OQs #1–#2 open (moved byte-identical).
- `tasks.md`: present, **30/30 tasks complete, 0 unfinished**. SHA-256
  `f417cc4b5293151cf396e491ad2817a4cbbc4576564091db3e7ae6f6da924bb1` identical
  before and after the move. Historical bytes preserved; no checkboxes repaired or
  rewritten.
- `verify-report.md`: absent — never produced. `apply-progress`: absent.
- Active `openspec/changes/` no longer contains `border-colors-single-strip`.

## Mechanical Evidence

- Step 2 copy readback `diff -r` (delta spec vs temp copy): empty — pass.
- Step 3 move readback `diff -r` (pre-move snapshot vs archived tree): empty — pass.
- This `archive-report.md` is additive-only and was written after the move, so it is
  excluded from the move comparison.

## SDD Cycle Complete

Implementation: complete per tasks artifact (30/30) and merged branch commits.
Verification: no persisted verify report; render verification happened during PR4 per
commit history. Unfinished tasks: none. Unresolved findings: design open questions
#1 and #2 remain open (non-blocking).
