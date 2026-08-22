# Apply Progress — theme-gallery (PR1 Foundation)

## Slice
PR1 Foundation — Phase 1 (7 tareas) — ThemeCard model + tokens

## Completed
- [x] 1.1 Scaffold gallery/mod.rs + views/mod.rs exports — `cargo test gallery -- --list` pass, commit 3509699
- [x] 1.2 RED ThemeCard mapping empty/active/providers — 5 tests, commit 17bed84 (RED then GREEN)
- [x] 1.3 GREEN ThemeCard types ColorScheme/Border — GalleryColors, BorderConfig, Background, FALLBACK_ACCENT #4fc3f7, commit 17bed84
- [x] 1.4 ThemeGalleryModel themes/focus/style — from_infos, with_style, len/is_empty/focused_card, commit d07abca
- [x] 1.5 Focus clamp S13 no-wrap — move_focus clamped 0..len-1, empty guard, set_focused_index, tests clamp_bounds, commit d07abca
- [x] 1.6 Tokens fallback-accent #4fc3f7 — verified ui/tokens.slint already contains fallback-accent, radius 2-40, spacing 2-20, anim-expand 350ms, Roboto Condensed — cargo check pass, commit 7a8758d
- [x] 1.7 ThemeCardDelegate — ui/gallery/ThemeCardDelegate.slint with GalleryCardData, dots, border preview, shader badge, active overlay, SkwdTokens — cargo check pass, commit 7a8758d

## Verification
- Focused test: `cargo test gallery --` → 11 passed
- Full suite: `cargo test` → 256 passed; 0 failed; 1 ignored
- Type check: `cargo check` → pass (Slint compile ok, delegate placeholder validated via temporary import)
- Engine sealed: no modifications to src/engine.rs, config.rs, settings.rs, theme_manager.rs, app_state.rs, watcher.rs, utils.rs, providers/*
- Window mutation preserved: Shell, SizePolicy, NavState intact

## Commits (stacked-to-main, work-unit)
1. 3509699 — scaffold gallery module
2. 17bed84 — ThemeCard types + mapping
3. d07abca — ThemeGalleryModel + focus clamp S13
4. 7a8758d — ThemeCardDelegate + tokens verification

## Scope Guard
- No UI Slint beyond delegate placeholder (PR2+)
- No providers/engine changes
- No slot registration yet (PR4)

## Next
PR2 Slice carousel (Phase 2) — SliceView GalleryView, width 108↔768 anim 350 OutCubic, handle_key, SliceDelegate skew, flip, video, preheat.
Budget: this PR1 is 535 lines (exceeds 400). Recommend size:exception (tests+delegate) or split delegate into PR2 if reviewer prefers. Next PRs remain stacked-to-main: PR1 → PR2 → PR3 → PR4.

## Artifacts
- src/shell/gallery/mod.rs
- src/shell/gallery/model.rs
- src/shell/gallery/views/mod.rs
- src/shell/mod.rs (gallery decl)
- ui/gallery/ThemeCardDelegate.slint
- ui/tokens.slint (verified, no change)
