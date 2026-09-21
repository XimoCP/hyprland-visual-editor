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
- Never copy GPL code from hyprmod or skwd-wall (both GPL-3.0); translate visual ideas only, with credit.
- VISUAL VERIFICATION (mandatory for any `.slint` / visual change): before reporting work done, run the headless render — `cargo test slice_focus_flow_renders` — then READ the PNGs it saves under `/tmp/opencode/` (you have vision: use the read tool on the image files) and confirm the geometry/look with your own inspection. Extend that test (or add sibling render tests) to cover any new visual state you touch. A UI change verified only by "tests green + build clean" is NOT verified.

## Model Roles

- **Writing and verification are always different models.** Verifying with the same model that wrote the code — even with fresh context — does not count as verification: independent context without an independent model shares the writer's blind spots.
- Concrete assignments live ONLY in `opencode.json` (`agent.<name>.model`). Never repeat model names in this file, in skills, or in docs. They change often; the rule does not. Swapping models is a one-line config change — nothing else needs rewriting.
- At session start, read the assignments and compare the writing set (implementation/phase agents) against the verification set (review, judge, verification agents). If they overlap, tell the user BEFORE starting and propose a different model for verification — preferably from another family.
- Every delegated verification must override the model explicitly. A verification launched on the default worker model is not a verification.
- Report which model verified each deliverable, so a missing cross-model check is visible instead of assumed.

## Review Proportionality (mandatory)

Verification is proportional to DANGER, never to diff size. A five-line change that deletes files is dangerous; a five-hundred-line documentation change is not.

- **Tier 1 — no external verification** (orchestrator self-check: run the suite, read the diff): comments, wording, names, docs, build warnings, formatting, and new tests that change no production behaviour.
- **Tier 2 — orchestrator review, no second model**: single-line logic changes with no outside effect, and new cases of behaviour that already exists. Queue these and run ONE batch review per agreed volume or per closed stage, instead of a round per change.
- **Tier 3 — cross-model verification**, where the rules above apply in full: anything that deletes, moves or writes files; path and permission handling; threads and concurrency; timeouts and bounds; state machines; anything touching another subsystem (colour, wallpaper, window management); and anything the author cannot verify live.
- The keeper's own live test is the strongest check for visible behaviour. Do not duplicate it with a ceremonial round.
- Report the tier used for each deliverable, so an over-reviewed or under-reviewed change is visible instead of guessed.

## Context

- Stack: Rust 2021, Slint 1.17.1, MIT license, binary `hve`. Config: `openspec/config.yaml`.
- Completed: `rewrite-compositor-driven` change decoupled the Hyprland driver behind a `Composer` trait (see `openspec/changes/rewrite-compositor-driven/`).
- Active branch for HVE 2 work: `hve2-visual-rewrite`.
- See `.opencode/skills/hve2/references/vision.md` for the full product vision, module tree, external references, and risks.