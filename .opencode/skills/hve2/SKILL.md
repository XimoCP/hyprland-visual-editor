---
name: hve2
description: "Trigger: HVE 2, reescritura visual, galería de temas, taller curvas, estilo skwd, ventana transformable. Plano maestro del proyecto HVE 2."
license: MIT
metadata:
  author: "XimoCP"
  version: "1.0"
---

# HVE 2 — Plano Maestro

## Activation Contract

Load this skill when working on HVE 2: visual rewrite, theme gallery, curve/border/color workshop, skwd-wall-inspired UI, window transformation, or any session on the `hve2-visual-rewrite` branch.

## Hard Rules

- HVE 2 is a VISUAL-ONLY editor. Monitors/HDR/display layout belongs to hyprmod — NEVER plan or build it here.
- Preserve the engine intact: `src/engine.rs`, `src/config.rs`, `src/settings.rs`, `src/theme_manager.rs`, `src/app_state.rs`, `src/watcher.rs`, `src/utils.rs`, all `src/providers/` are NOT rewritten, only reused.
- Single window that MUTATES. Never stack windows on top of each other; the card expands into the settings panel inside the same window.
- Visual styles are 1:1 with skwd-wall: parallelogram carousel, grid, and hexagon presentations.
- Every module closes (tests green, reviewed) before the next one starts. Tree assembly, one branch at a time.
- User approves each phase before the next phase launches (Interactive mode).
- Engineered artifacts (code, specs, docs, comments, UI copy) default to ENGLISH. Chat replies follow the user's language (Spanish/rioplatense).
- TDD is STRICT: write the failing test first, then implement.

## Decision Gates

| Situation | Decision |
|-----------|----------|
| New feature idea | Add to vision doc first; implement only after user approval |
| skwd-wall feature wanted | Translate VISUAL IDEA to Slint with credit; never copy GPL code (skwd-wall is GPL-3.0, same as hyprmod) |
| Engine change needed | STOP: engine is sealed; negotiate scope with user before touching it |
| >400 changed lines in one step | STOP: split into chained PRs or ask for explicit size exception |

## Execution Steps

1. Read this skill and `references/vision.md` at session start on this branch.
2. Check current phase status in `openspec/` (or Engram topic keys) before acting.
3. Work the dependency chain: proposal -> specs -> design -> tasks -> apply -> verify -> archive.
4. For apply/verify, follow STRICT TDD with `cargo test` as the test runner.
5. After each phase, report result to the user and wait for approval (Interactive).
6. Commit work in reviewable units; keep tests and docs with their code.

## Output Contract

Return: `status`, `executive_summary`, `artifacts` (paths/topic keys), `next_recommended`, `risks`, `skill_resolution`.

## References

- `references/vision.md` — product vision, agreed scope, module tree order, and external references (skwd-wall, hyprmod, HyprShades).
- `openspec/config.yaml` — project stack and strict TDD configuration.
- `openspec/changes/rewrite-compositor-driven/` — completed engine decoupling work (Composer trait).