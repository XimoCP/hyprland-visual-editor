# Archive Report: skwd-wallpaper-delegation-minimal

**Archived**: 2026-09-22

**Task state**: `tasks.md` is prose and carries no markdown checkboxes, which is why `gentle-ai sdd-status` reported this change blocked. Completion is therefore recorded from CODE, not from checkboxes:

- the function `force_noctalia_wallpaper_off` no longer exists anywhere in `src/`;
- the replacement behaviour test `settings_json_copied_verbatim` lives at `src/providers/noctalia.rs:1875` and passes (`cargo test settings_json_copied_verbatim` → 1 passed);
- the wallpaper-application chain was later refined by the background-authority work of 2026-09-20/21 (commits `50fe1ea`, `b4b52c7`, `261c81c`, `dedf5d6`, `7695131`), which supersedes this change's "best effort" delegation description while keeping its agnostic principle: HVE owns files and reload, and never assumes which daemon paints.

**Live spec created**: `openspec/specs/wallpaper-delegation/spec.md` — 5 requirements, 9 scenarios, generated from this change's delta with the editorial notes and section headers removed. Requirement text is verbatim.

**Historical bytes**: `tasks.md` was NOT rewritten into checkboxes; it stays exactly as it was written.
