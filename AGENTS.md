# HVE Project — Agent Instructions

## Project Skills

- `hve2` — Plano maestro del proyecto HVE 2 (visual rewrite). Load it FIRST for any session on the `hve2-visual-rewrite` branch: `.opencode/skills/hve2/SKILL.md`.

## Hard Rules

- HVE 2 is a VISUAL-ONLY editor. Monitors/HDR/display layout belongs to hyprmod — never plan or build it here.
- Preserve the engine intact: `src/engine.rs`, `src/config.rs`, `src/settings.rs`, `src/theme_manager.rs`, `src/app_state.rs`, `src/watcher.rs`, `src/utils.rs`, all `src/providers/` are reused, not rewritten.
- Single window that MUTATES; never stack windows.
- Visual styles 1:1 with skwd-wall (parallelogram, grid, hexagon).
- One module at a time; each closes with tests green before the next starts.
- User approves each phase before the next phase launches.
- Engineered artifacts (code, specs, docs, comments, UI copy) default to ENGLISH. Chat replies follow the user's language (Spanish/rioplatense).
- Strict TDD: write the failing test first, then implement. Test runner: `cargo test`.
- Never copy GPL code from hyprmod; translate MIT ideas from skwd-wall with credit.
- VISUAL VERIFICATION (mandatory for any `.slint` / visual change): before reporting work done, run the headless render — `cargo test slice_focus_flow_renders` — then READ the PNGs it saves under `/tmp/opencode/` (you have vision: use the read tool on the image files) and confirm the geometry/look with your own inspection. Extend that test (or add sibling render tests) to cover any new visual state you touch. A UI change verified only by "tests green + build clean" is NOT verified.

## Context

- Stack: Rust 2021, Slint 1.17.1, MIT license, binary `hve`. Config: `openspec/config.yaml`.
- Completed: `rewrite-compositor-driven` change decoupled the Hyprland driver behind a `Composer` trait (see `openspec/changes/rewrite-compositor-driven/`).
- Active branch for HVE 2 work: `hve2-visual-rewrite`.
- See `.opencode/skills/hve2/references/vision.md` for the full product vision, module tree, external references, and risks.