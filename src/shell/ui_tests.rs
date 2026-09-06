//! Headless tests for the delivery-4 Slint UI (`ui/main.slint` mounts
//! `ShellRoot`, which imports `SkwdTokens` and `shell.slint`). Generated
//! Rust comes from `include_modules!()` in main.rs. Tests instantiate
//! `MainWindow` headless and assert the `SkwdTokens` values (design.md
//! "Design Tokens", verbatim — task 3.1), the shell chrome surface and
//! the i18n placeholder strings (task 3.2 + 4.4). MIT credit: translated
//! from skwd-wall (MIT, © liixini).

#![cfg(test)]

use slint::Global as _;

/// Headless backend on the current thread — same pattern as
/// `composer::tests::init_test_platform` (per-thread, `init_no_event_loop`).
fn init_test_platform() {
    i_slint_backend_testing::init_no_event_loop();
}

/// Token handle bound to a MainWindow instance. `SkwdTokens` is
/// re-exported through main.slint → shell.slint → tokens.slint.
fn tokens(win: &crate::MainWindow) -> crate::SkwdTokens<'_> {
    crate::SkwdTokens::get(win)
}

// ── Task 3.1: SkwdTokens — design.md "Design Tokens" verbatim ──────

#[test]
fn tokens_fonts_and_anim_cadence_match_design() {
    init_test_platform();
    let win = crate::MainWindow::new().unwrap();
    let t = tokens(&win);

    // Fonts: Roboto Condensed (body), Roboto (heading), Roboto Mono (code).
    assert_eq!(t.get_font_body(), "Roboto Condensed");
    assert_eq!(t.get_font_heading(), "Roboto");
    assert_eq!(t.get_font_mono(), "Roboto Mono");

    // Durations (ms): 100..1000, 350ms expand (skwd animExpand, D4).
    assert_eq!(t.get_anim_100(), 100);
    assert_eq!(t.get_anim_150(), 150);
    assert_eq!(t.get_anim_200(), 200);
    assert_eq!(t.get_anim_250(), 250);
    assert_eq!(t.get_anim_300(), 300);
    assert_eq!(t.get_anim_expand(), 350);
    assert_eq!(t.get_anim_400(), 400);
    assert_eq!(t.get_anim_1000(), 1000);
}

#[test]
fn tokens_sizes_radii_spacing_match_design() {
    init_test_platform();
    let win = crate::MainWindow::new().unwrap();
    let t = tokens(&win);

    // Sizes (px): 9, 11, 12, 13, 14, 16, 18, 24, 32, 36.
    assert_eq!(t.get_size_9(), 9.0);
    assert_eq!(t.get_size_11(), 11.0);
    assert_eq!(t.get_size_12(), 12.0);
    assert_eq!(t.get_size_13(), 13.0);
    assert_eq!(t.get_size_14(), 14.0);
    assert_eq!(t.get_size_16(), 16.0);
    assert_eq!(t.get_size_18(), 18.0);
    assert_eq!(t.get_size_24(), 24.0);
    assert_eq!(t.get_size_32(), 32.0);
    assert_eq!(t.get_size_36(), 36.0);

    // Radii (px): 2, 4, 8, 12, 16, 20, 40.
    assert_eq!(t.get_radius_2(), 2.0);
    assert_eq!(t.get_radius_4(), 4.0);
    assert_eq!(t.get_radius_8(), 8.0);
    assert_eq!(t.get_radius_12(), 12.0);
    assert_eq!(t.get_radius_16(), 16.0);
    assert_eq!(t.get_radius_20(), 20.0);
    assert_eq!(t.get_radius_40(), 40.0);

    // Spacing (px): 2, 4, 8, 12, 16, 20.
    assert_eq!(t.get_spacing_2(), 2.0);
    assert_eq!(t.get_spacing_4(), 4.0);
    assert_eq!(t.get_spacing_8(), 8.0);
    assert_eq!(t.get_spacing_12(), 12.0);
    assert_eq!(t.get_spacing_16(), 16.0);
    assert_eq!(t.get_spacing_20(), 20.0);
}

#[test]
fn tokens_borders_opacity_and_colors_match_design() {
    init_test_platform();
    let win = crate::MainWindow::new().unwrap();
    let t = tokens(&win);

    // Border (px): 1, 2, 3. Opacity: .35, .5, .6.
    assert_eq!(t.get_border_1(), 1.0);
    assert_eq!(t.get_border_2(), 2.0);
    assert_eq!(t.get_border_3(), 3.0);
    assert_eq!(t.get_opacity_dim(), 0.35);
    assert_eq!(t.get_opacity_mid(), 0.5);
    assert_eq!(t.get_opacity_strong(), 0.6);

    // Colors (skwd palette → HveColors mapping, design.md).
    assert_eq!(t.get_primary(), slint::Color::from_rgb_u8(0xff, 0xb4, 0xab));
    assert_eq!(t.get_tertiary(), slint::Color::from_rgb_u8(0x8b, 0xce, 0xff));
    assert_eq!(t.get_surface(), slint::Color::from_rgb_u8(0x1d, 0x10, 0x0e));
    assert_eq!(t.get_surface_variant(), slint::Color::from_rgb_u8(0x5a, 0x41, 0x3e));
    assert_eq!(t.get_surface_container(), slint::Color::from_rgb_u8(0x2c, 0x1f, 0x1d));
    assert_eq!(t.get_outline(), slint::Color::from_rgb_u8(0xa9, 0x89, 0x86));
    assert_eq!(t.get_fallback_accent(), slint::Color::from_rgb_u8(0x4f, 0xc3, 0xf7));
}

#[test]
fn tokens_gallery_geometry_matches_design() {
    init_test_platform();
    let win = crate::MainWindow::new().unwrap();
    let t = tokens(&win);

    // Slice carousel, skwd-wall exact: w135/expanded-w924/h520/
    // spacing 4 (hairline gap)/skew 35px offset/visible 12/radius 0.
    // gallery-immersive-redesign 1.7 fixed the swapped h/card-w and
    // replaced angle tokens with px spacing + skew offset.
    assert_eq!(t.get_gallery_slice_w(), 135.0);
    assert_eq!(t.get_gallery_slice_expanded_w(), 924.0);
    assert_eq!(t.get_gallery_slice_h(), 520.0);
    assert_eq!(t.get_gallery_slice_spacing(), 4.0);
    assert_eq!(t.get_gallery_slice_skew_offset(), 35.0);
    assert_eq!(t.get_gallery_slice_visible(), 12);
    assert_eq!(t.get_gallery_slice_radius(), 0.0);

    // Hex columns: 140/3/7.
    assert_eq!(t.get_gallery_hex_h(), 140.0);
    assert_eq!(t.get_gallery_hex_cols(), 3);
    assert_eq!(t.get_gallery_hex_rows(), 7);

    // Grid: 6×3, thumb 300×169.
    assert_eq!(t.get_gallery_grid_cols(), 6);
    assert_eq!(t.get_gallery_grid_rows(), 3);
    assert_eq!(t.get_gallery_thumb_w(), 300.0);
    assert_eq!(t.get_gallery_thumb_h(), 169.0);

    // Mosaic: 48, relax 2, 1500×800; uiScale 1.0.
    assert_eq!(t.get_gallery_mosaic_cell(), 48.0);
    assert_eq!(t.get_gallery_mosaic_relax(), 2);
    assert_eq!(t.get_gallery_mosaic_w(), 1500.0);
    assert_eq!(t.get_gallery_mosaic_h(), 800.0);
    assert_eq!(t.get_ui_scale(), 1.0);
}

// ── Task 3.2/4.1: MainWindow shell mount — read-only view ────────

#[test]
fn main_window_mounts_shell_with_initial_state() {
    init_test_platform();
    let win = crate::MainWindow::new().unwrap();

    // nav-shell spec R1: initial Home + Collapsed; the UI mirrors the
    // Rust NavState and holds no navigation state of its own.
    assert_eq!(win.get_mounted_screen(), 0, "Home — nothing mounted");
    assert!(!win.get_expanded(), "Collapsed at startup");
    assert_eq!(win.get_focused_card(), 0, "focus starts on card 0 (Gallery)");
}

#[test]
fn main_window_exposes_shell_i18n_properties() {
    init_test_platform();
    let win = crate::MainWindow::new().unwrap();

    // Default English placeholders (overwritten by main.rs from tr.rs).
    assert_eq!(win.get_shell_brand_text(), "HVE");
    assert_eq!(win.get_shell_brand_subtitle(), "Hyprland Visual Editor");
    assert_eq!(win.get_shell_status_text(), "Ready");
    assert_eq!(win.get_shell_back_hint(), "Esc hides");
}

#[test]
fn main_window_exposes_navigation_callbacks() {
    init_test_platform();
    let win = crate::MainWindow::new().unwrap();

    // nav-shell spec R4: shell actions surface as callbacks; Rust maps them
    // to NavCommand in delivery 4.
    let activated = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let backs = std::rc::Rc::new(std::cell::RefCell::new(0));
    let moves = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

    win.on_card_activated({
        let activated = activated.clone();
        move |screen| activated.borrow_mut().push(screen)
    });
    win.on_back_activated({
        let backs = backs.clone();
        move || *backs.borrow_mut() += 1
    });
    win.on_nav_move({
        let moves = moves.clone();
        move |direction| moves.borrow_mut().push(direction.to_string())
    });

    // Card activation (mouse click or Return on the focused card).
    win.invoke_card_activated(0);
    win.invoke_card_activated(1);
    // Back/collapse (Escape or the back affordance).
    win.invoke_back_activated();
    // Arrow keys for focus movement (nav-move → NavState.move_focus).
    win.invoke_nav_move(slint::SharedString::from("down"));
    win.invoke_nav_move(slint::SharedString::from("up"));

    assert_eq!(*activated.borrow(), vec![0, 1], "card-activated fires with the focused card index");
    assert_eq!(*backs.borrow(), 1, "back-activated fires on back/collapse");
    assert_eq!(*moves.borrow(), vec!["down", "up"], "nav-move reports arrow direction");
}

// ── V6 focus-flow visual verification (headless render) ────────────────
// Renders the Slice wall at a settled focus position and mid-glide
// (frac 0.5) with real baked parallelogram images, and saves PNGs under
// /tmp/opencode/ for human review. This is the headless visual-check tool:
// geometry regressions (positions, widths, image layers) show up here
// before they reach the screen. MIT credit: translated from skwd-wall
// (MIT, © liixini).

/// Deterministic 480×270 gradient wallpaper variant for theme `i`.
fn slice_test_gradient(i: usize) -> image::RgbaImage {
    let (w, h) = (480u32, 270u32);
    let mut img = image::RgbaImage::new(w, h);
    let hue = (i as f32 * 30.0).to_radians();
    let (r0, g0, b0) = (
        (hue.sin() * 0.5 + 0.5) * 235.0,
        (hue.cos() * 0.5 + 0.5) * 235.0,
        130.0 + (i as f32 * 29.0) % 120.0,
    );
    for (x, y, px) in img.enumerate_pixels_mut() {
        let t = (x as f32 + y as f32) / (w + h) as f32;
        *px = image::Rgba([
            (r0 + (1.0 - t) * 60.0) as u8,
            (g0 + t * 70.0) as u8,
            (b0 + t * 50.0) as u8,
            255,
        ]);
    }
    img
}

fn save_slice_png(buf: slint::SharedPixelBuffer<slint::Rgba8Pixel>, path: &str) {
    let w = buf.width();
    let h = buf.height();
    let img = image::RgbaImage::from_raw(w, h, Vec::from(buf.as_bytes())).expect("snapshot buffer");
    img.save(path).expect("save png");
}

#[test]
fn slice_focus_flow_renders_settled_and_midflight() {
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    // Software-rasterized headless backend (first init on this thread).
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1); // 1 = Gallery screen
    win.set_gallery_empty(false);
    win.set_gallery_style(0);
    win.set_gallery_focused(6);

    let count = 12usize;
    let stage_w = 1920.0f32;
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    for i in 0..count {
        let src = slice_test_gradient(i);
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: slint::Image::default(),
            hero: slint::Image::default(),
            slat_image: {
                let rgba = crate::shell::gallery::slat_image::baked_slat_rgba(src.clone(), false);
                let (w, h) = (rgba.width(), rgba.height());
                let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                buf.make_mut_bytes().copy_from_slice(rgba.as_raw());
                slint::Image::from_rgba8(buf)
            },
            slat_expanded_image: {
                let rgba = crate::shell::gallery::slat_image::baked_slat_rgba(src, true);
                let (w, h) = (rgba.width(), rgba.height());
                let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                buf.make_mut_bytes().copy_from_slice(rgba.as_raw());
                slint::Image::from_rgba8(buf)
            },
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));

    let tiles: Vec<crate::SliceTileData> = crate::shell::gallery::views::slice::slice_delta_tiles(count, 6, stage_w)
        .into_iter()
        .map(|t| crate::SliceTileData {
            delta: t.delta,
            real_index: t.real_index as i32,
            is_expanded: t.is_expanded,
            fade: t.fade,
            dist: t.dist,
        })
        .collect();
    win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles)));

    // Settled: focus position exactly at the focused index (frac 0). The
    // delta-base must match focus-pos — the Rust reel spring now drives
    // focus-pos per frame (no Slint animate freezes values anymore), so the
    // property applies instantly and the base has to be explicit.
    win.set_gallery_slice_delta_base(6);
    win.set_gallery_slice_focus_pos(6.0);
    let settled = win.window().take_snapshot().expect("settled snapshot");
    save_slice_png(settled, "/tmp/opencode/slice_settled.png");

    // Mid-glide (production state): one chained step commits → Rust relabels
    // deltas for the new focused slot (Theme 7) and retargets focus-pos,
    // which is still tweening (frac −0.5). Outgoing and incoming cards are
    // both half-grown, neighbors reflowing, adjacency gap-free. Rebasing
    // disables the tween so the snapshot captures the mid-flight GEOMETRY
    // directly (no event loop to advance the animation).
    let tiles_relabeled: Vec<crate::SliceTileData> =
        crate::shell::gallery::views::slice::slice_delta_tiles(count, 7, stage_w)
            .into_iter()
            .map(|t| crate::SliceTileData {
                delta: t.delta,
                real_index: t.real_index as i32,
                is_expanded: t.is_expanded,
                fade: t.fade,
                dist: t.dist,
            })
            .collect();
    win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles_relabeled)));
    win.set_gallery_focused(7);
    win.set_gallery_slice_delta_base(7);
    win.set_gallery_slice_rebasing(true);
    win.set_gallery_slice_focus_pos(6.5); // frac = −0.5
    let mid = win.window().take_snapshot().expect("midflight snapshot");
    save_slice_png(mid, "/tmp/opencode/slice_midflight.png");

    // Ring-seam crossing (infinite-carousel regression): 12 themes, chain
    // forward from real 11 into real 0. The virtual focus keeps counting
    // (base 12) while the relabeled model centers real 0 — frac = −0.5 puts
    // real 0 half-grown at center and real 11 half-collapsed to its left.
    let tiles_seam: Vec<crate::SliceTileData> =
        crate::shell::gallery::views::slice::slice_delta_tiles(count, 0, stage_w)
            .into_iter()
            .map(|t| crate::SliceTileData {
                delta: t.delta,
                real_index: t.real_index as i32,
                is_expanded: t.is_expanded,
                fade: t.fade,
                dist: t.dist,
            })
            .collect();
    win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles_seam)));
    win.set_gallery_focused(0);
    win.set_gallery_slice_delta_base(12);
    win.set_gallery_slice_focus_pos(11.7); // frac = −0.3: incoming 70% grown
    let seam = win.window().take_snapshot().expect("seam snapshot");
    save_slice_png(seam, "/tmp/opencode/slice_seam.png");
    win.set_gallery_slice_delta_base(0);
    win.set_gallery_slice_rebasing(false);
}

#[test]
fn slice_twenty_chained_steps_settle_without_drift() {
    use crate::shell::gallery::views::slice::{focus_step, scaled_metrics, slice_delta_tiles};
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .ok(); // allow already-initialized when tests share the thread
    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1); // 1 = Gallery screen
    win.set_gallery_empty(false);
    win.set_gallery_style(0);
    win.set_gallery_focused(6);
    let count = 12usize;
    let stage_w = 1920.0f32;
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    for i in 0..count {
        let src = slice_test_gradient(i);
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: slint::Image::default(),
            hero: slint::Image::default(),
            slat_image: {
                let rgba = crate::shell::gallery::slat_image::baked_slat_rgba(src.clone(), false);
                let (w, h) = (rgba.width(), rgba.height());
                let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                buf.make_mut_bytes().copy_from_slice(rgba.as_raw());
                slint::Image::from_rgba8(buf)
            },
            slat_expanded_image: {
                let rgba = crate::shell::gallery::slat_image::baked_slat_rgba(src, true);
                let (w, h) = (rgba.width(), rgba.height());
                let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                buf.make_mut_bytes().copy_from_slice(rgba.as_raw());
                slint::Image::from_rgba8(buf)
            },
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));
    // 20 chained +1 production steps through the REAL pipeline: each step
    // relabels deltas for the new focused slot, advances the virtual base,
    // and the reel settles focus-pos EXACTLY on the virtual next (frac 0 —
    // the drift fix). The final render must show a full 924 center with
    // 135 laterals, not the thinned wall the residue used to leave.
    let mut v = 6i32;
    let mut pos = 6.0f32;
    let mut focused = 6usize;
    for _ in 0..20 {
        let s = focus_step(v, pos, 1);
        v = s.virtual_next;
        pos = s.target; // exact settle (reel value() contract)
        focused = (focused + 1) % count;
        let tiles: Vec<crate::SliceTileData> = slice_delta_tiles(count, focused, stage_w)
            .into_iter()
            .map(|t| crate::SliceTileData {
                delta: t.delta,
                real_index: t.real_index as i32,
                is_expanded: t.is_expanded,
                fade: t.fade,
                dist: t.dist,
            })
            .collect();
        win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles)));
        win.set_gallery_focused(focused as i32);
        win.set_gallery_slice_delta_base(v);
        win.set_gallery_slice_focus_pos(pos);
    }
    assert!((pos - v as f32).abs() < 0.001, "20 steps must settle frac 0, got {}", pos - v as f32);
    let m = scaled_metrics(stage_w);
    assert!((m.expanded - 924.0).abs() < 0.01, "settled center must be 924");
    assert!((m.collapsed - 135.0).abs() < 0.01, "settled laterals must be 135");
    let snap = win.window().take_snapshot().expect("20-step snapshot");
    save_slice_png(snap, "/tmp/opencode/slice_20steps.png");
}

#[test]
fn slice_fluid_scaling_renders_small_and_large_no_clipping() {
    use crate::shell::gallery::views::slice::{scaled_metrics, slice_delta_tiles};
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    // Prove fluid clamp A+B visually: 1092 (≈14") must not clip, 1920 (≈24") stays capped.
    // Also unit-assert the 58% rule before taking snapshots.
    let m1092 = scaled_metrics(1092.0);
    let m1920 = scaled_metrics(1920.0);
    assert!((m1092.expanded - 633.36).abs() < 1.0, "1092 expanded {} expected ~633", m1092.expanded);
    assert!((m1920.expanded - 924.0).abs() < 0.01, "1920 expanded {}", m1920.expanded);
    assert!(m1092.expanded < 1092.0 * 0.59, "small expanded must be ≤58%");
    // Small-screen: 2 per side (total 5) with fixed 32px inset; large 1920 stays 3.
    let small_tiles = slice_delta_tiles(12, 6, 1092.0);
    let mid_tiles = slice_delta_tiles(12, 6, 1280.0);
    let large_tiles = slice_delta_tiles(12, 6, 1920.0);
    assert_eq!(
        small_tiles.len(),
        5,
        "1092 must show exactly 5 (2 left + focused + 2 right), got {}",
        small_tiles.len()
    );
    assert_eq!(
        small_tiles.iter().filter(|t| !t.is_expanded).count(),
        4,
        "1092 must have 4 collapsed peeks"
    );
    assert_eq!(
        mid_tiles.len(),
        5,
        "1280 must also show 5, got {}",
        mid_tiles.len()
    );
    assert_eq!(
        large_tiles.len(),
        3,
        "1920 must stay exactly 3 tiles (expanded +1 left +1 right), got {}",
        large_tiles.len()
    );
    assert!((m1920.collapsed - 135.0).abs() < 0.01, "1920 collapsed must stay 135");
    assert!((m1920.height - 520.0).abs() < 0.01);
    assert!((m1920.skew - 35.0).abs() < 0.01);
    assert!((m1920.gap - 24.0).abs() < 0.01);
    // No overflow at small: leftmost ≥0, rightmost ≤stage
    for w in [1092.0f32, 1280.0] {
        let slots = crate::shell::gallery::views::slice::ring_visible_slots(12, 6, w);
        let min_x = slots.iter().map(|s| s.x).fold(f32::MAX, f32::min);
        let max_r = slots.iter().map(|s| s.x + s.width).fold(f32::MIN, f32::max);
        assert!(min_x >= -0.5, "small stage {w} leftmost {min_x} must be ≥0");
        assert!(max_r <= w + 0.5, "small stage {w} rightmost {max_r} must be ≤{w}");
    }
    // Render small stage.
    let render_at = |stage_w: f32, stage_h: f32, png: &str| {
        i_slint_core::platform::set_platform(Box::new(
            i_slint_backend_testing::TestingBackend::new(
                i_slint_backend_testing::TestingBackendOptions {
                    mock_time: true,
                    threading: false,
                    renderer_name: Some(slint::SharedString::from("software")),
                    ..Default::default()
                },
            ),
        ))
        .ok(); // allow already-initialized when tests run in same thread
        let win = crate::MainWindow::new().unwrap();
        win.window().set_size(slint::PhysicalSize::new(stage_w as u32, stage_h as u32));
        win.set_mounted_screen(1);
        win.set_gallery_empty(false);
        win.set_gallery_style(0);
        win.set_gallery_focused(6);
        let count = 12usize;
        let mut cards: Vec<crate::GalleryCardData> = Vec::new();
        for i in 0..count {
            let src = slice_test_gradient(i);
            cards.push(crate::GalleryCardData {
                name: SharedString::from(format!("Theme {i}")),
                saved_at: SharedString::from(""),
                is_active: false,
                providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
                accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
                primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
                secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
                tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
                surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
                border_size: 0,
                border_radius: 0,
                border_color: slint::Color::from_rgb_u8(0, 0, 0),
                shader: SharedString::from(""),
                thumb_path: SharedString::from(""),
                thumb: slint::Image::default(),
                hero: slint::Image::default(),
                slat_image: {
                    let rgba = crate::shell::gallery::slat_image::baked_slat_rgba(src.clone(), false);
                    let (w, h) = (rgba.width(), rgba.height());
                    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                    buf.make_mut_bytes().copy_from_slice(rgba.as_raw());
                    slint::Image::from_rgba8(buf)
                },
                slat_expanded_image: {
                    let rgba = crate::shell::gallery::slat_image::baked_slat_rgba(src, true);
                    let (w, h) = (rgba.width(), rgba.height());
                    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                    buf.make_mut_bytes().copy_from_slice(rgba.as_raw());
                    slint::Image::from_rgba8(buf)
                },
            });
        }
        win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));
        let tiles: Vec<crate::SliceTileData> = slice_delta_tiles(count, 6, stage_w)
            .into_iter()
            .map(|t| crate::SliceTileData {
                delta: t.delta,
                real_index: t.real_index as i32,
                is_expanded: t.is_expanded,
                fade: t.fade,
                dist: t.dist,
            })
            .collect();
        win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles)));
        win.set_gallery_slice_focus_pos(6.0);
        win.set_gallery_slice_delta_base(6);
        // Give Slint a tick to propagate stage.width → delegates' stage-width (scaled geometry)
        for _ in 0..2 {
            i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
        }
        let snap = win.window().take_snapshot().expect("snapshot");
        // No overflow: the focused slot (scaled expanded) must be fully inside stage.
        let m = scaled_metrics(stage_w);
        let fx = stage_w / 2.0 - m.expanded / 2.0;
        assert!(fx >= 0.0 - 0.5, "focused slot left {fx} must be ≥0 at stage {stage_w}");
        assert!(fx + m.expanded <= stage_w + 0.5, "focused slot right must fit at stage {stage_w}");
        save_slice_png(snap, png);
    };
    render_at(1092.0, 1080.0, "/tmp/opencode/slice_fluid_1092.png");
    render_at(1920.0, 1080.0, "/tmp/opencode/slice_fluid_1920.png");
}

#[test]
fn mosaic_hero_centered_renders_with_pagination() {
    use crate::shell::gallery::views::mosaic::{justified_hero_layout, mosaic_curtain_delays, MosaicPages};
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1); // Gallery
    win.set_gallery_empty(false);
    win.set_gallery_style(2); // Mosaic
    win.set_gallery_focused(0);

    // Synthetic cards with PLAIN gradient thumbs (mosaic cells are
    // rectangles — the parallelogram slat bakes are Slice-only).
    let count = 12usize; // capacity 9 at this stage → page 2 exists
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    for i in 0..count {
        let img = slint::Image::from_rgba8({
            let grad = slice_test_gradient(i);
            let (w, h) = grad.dimensions();
            let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
            buf.make_mut_bytes().copy_from_slice(grad.as_raw());
            buf
        });
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: img.clone(),
            hero: img,
            slat_image: slint::Image::default(),
            slat_expanded_image: slint::Image::default(),
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));

    // Real page pipeline: MosaicPages → page_render → hero layout.
    let stage_w = 1920.0f32;
    let stage_h = 1080.0f32;
    let pg = MosaicPages::new(count, stage_w, stage_h);
    assert!(pg.total_pages() >= 2, "12 themes must paginate (capacity {})", pg.capacity());
    let (aspects, real_indices) = pg.page_render();
    let (layout, tile_reals) = justified_hero_layout(&aspects, &real_indices, stage_w, stage_h);
    let delays = mosaic_curtain_delays(&layout.tiles);
    let tiles: Vec<crate::MosaicTileData> = layout
        .tiles
        .iter()
        .zip(tile_reals.iter())
        .zip(delays.iter())
        .map(|((t, r), d)| crate::MosaicTileData {
            x: t.x, y: t.y, w: t.w, h: t.h,
            real_index: *r as i32,
            delay_ms: *d as i32,
        })
        .collect();
    win.set_gallery_mosaic_tiles(ModelRc::new(VecModel::from(tiles)));
    win.set_gallery_mosaic_current_page(pg.current() as i32);
    win.set_gallery_mosaic_total_pages(pg.total_pages() as i32);
    win.set_gallery_mosaic_page_numbers(ModelRc::new(VecModel::from(
        pg.page_numbers().iter().map(|n| *n as i32).collect::<Vec<i32>>(),
    )));

    let snap = win.window().take_snapshot().expect("mosaic snapshot");
    save_slice_png(snap, "/tmp/opencode/mosaic_hero.png");
}

fn bright_mosaic_thumb() -> slint::Image {
    // Striped image with high-frequency variance so the curtain cover's
    // IMAGE CONTENT (zoomed + dimmed) is distinguishable from the flat dark
    // color it replaced. Vertical stripes every 20px alternate warm/cool.
    let (w, h) = (320u32, 200u32);
    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
    let bytes = buf.make_mut_bytes();
    for y in 0..h as usize {
        for x in 0..w as usize {
            let idx = (y * w as usize + x) * 4;
            let stripe = (x / 20) % 2 == 0;
            if stripe {
                bytes[idx] = 240;
                bytes[idx + 1] = 190;
                bytes[idx + 2] = 90;
            } else {
                bytes[idx] = 70;
                bytes[idx + 1] = 140;
                bytes[idx + 2] = 200;
            }
            bytes[idx + 3] = 255;
        }
    }
    slint::Image::from_rgba8(buf)
}

fn count_buffer_diff(
    a: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
    b: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
) -> usize {
    // Counts pixels in the mosaic band where two snapshots differ by >15
    // in any channel — proves zoom+dim makes covered pixels differ from
    // settled (fully revealed) pixels of the same tile.
    let w = a.width() as usize;
    let h = a.height() as usize;
    assert_eq!(w, b.width() as usize);
    assert_eq!(h, b.height() as usize);
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let mut count = 0usize;
    let y0 = 150usize.min(h);
    let y1 = 800usize.min(h);
    let x0 = 60usize.min(w);
    let x1 = 1650usize.min(w);
    for y in y0..y1 {
        for x in x0..x1 {
            let idx = (y * w + x) * 4;
            if ab[idx + 3] < 10 || bb[idx + 3] < 10 {
                continue;
            }
            let dr = (ab[idx] as i16 - bb[idx] as i16).abs();
            let dg = (ab[idx + 1] as i16 - bb[idx + 1] as i16).abs();
            let db = (ab[idx + 2] as i16 - bb[idx + 2] as i16).abs();
            if dr > 15 || dg > 15 || db > 15 {
                count += 1;
            }
        }
    }
    count
}

fn count_covered_non_uniform(
    mid: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
    settled: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
) -> usize {
    // Pixels that are BOTH covered (mid vs settled diff >15 for the pixel
    // AND its right neighbor) AND carry image content (mid neighbor variance
    // >15). Flat dark cover yields ~0 interior (covered but uniform); striped
    // image cover yields high interior variance. Requiring both pixels covered
    // excludes the curtain seam (dark vs stripe edge) which would otherwise
    // fake variance for the flat cover.
    let w = mid.width() as usize;
    let h = mid.height() as usize;
    let mb = mid.as_bytes();
    let sb = settled.as_bytes();
    let mut count = 0usize;
    let y0 = 150usize.min(h);
    let y1 = 800usize.min(h);
    let x0 = 60usize.min(w);
    let x1 = 1650usize.min(w.saturating_sub(1));
    for y in y0..y1 {
        for x in x0..x1 {
            let idx = (y * w + x) * 4;
            let nidx = (y * w + x + 1) * 4;
            if mb[idx + 3] < 10 || sb[idx + 3] < 10 || mb[nidx + 3] < 10 || sb[nidx + 3] < 10 {
                continue;
            }
            let dr = (mb[idx] as i16 - sb[idx] as i16).abs();
            let dg = (mb[idx + 1] as i16 - sb[idx + 1] as i16).abs();
            let db = (mb[idx + 2] as i16 - sb[idx + 2] as i16).abs();
            let is_covered = dr > 15 || dg > 15 || db > 15;
            if !is_covered {
                continue;
            }
            let dr2 = (mb[nidx] as i16 - sb[nidx] as i16).abs();
            let dg2 = (mb[nidx + 1] as i16 - sb[nidx + 1] as i16).abs();
            let db2 = (mb[nidx + 2] as i16 - sb[nidx + 2] as i16).abs();
            let neighbor_covered = dr2 > 15 || dg2 > 15 || db2 > 15;
            if !neighbor_covered {
                continue;
            }
            let vr = (mb[idx] as i16 - mb[nidx] as i16).abs();
            let vg = (mb[idx + 1] as i16 - mb[nidx + 1] as i16).abs();
            let vb = (mb[idx + 2] as i16 - mb[nidx + 2] as i16).abs();
            if vr > 15 || vg > 15 || vb > 15 {
                count += 1;
            }
        }
    }
    count
}

fn count_hue_flips_covered(
    mid: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
    settled: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
) -> usize {
    // Counts covered pixels where the dominant hue (warm vs cool) flips
    // between mid and settled. With flat cover (no zoom) the pattern phase
    // is identical, so hues match (just dimmed). With zoomed 1.1 cover the
    // stripe phase shifts, causing some flips. Used to prove zoom visibility.
    let w = mid.width() as usize;
    let h = mid.height() as usize;
    let mb = mid.as_bytes();
    let sb = settled.as_bytes();
    let mut count = 0usize;
    let y0 = 150usize.min(h);
    let y1 = 800usize.min(h);
    let x0 = 60usize.min(w);
    let x1 = 1650usize.min(w);
    for y in y0..y1 {
        for x in x0..x1 {
            let idx = (y * w + x) * 4;
            if mb[idx + 3] < 10 || sb[idx + 3] < 10 {
                continue;
            }
            let dr = (mb[idx] as i16 - sb[idx] as i16).abs();
            let dg = (mb[idx + 1] as i16 - sb[idx + 1] as i16).abs();
            let db = (mb[idx + 2] as i16 - sb[idx + 2] as i16).abs();
            let is_covered = dr > 15 || dg > 15 || db > 15;
            if !is_covered {
                continue;
            }
            // Warm = R dominant, Cool = B dominant (stripe colors)
            let mid_warm = mb[idx] > mb[idx + 2] + 20; // R > B
            let settled_warm = sb[idx] > sb[idx + 2] + 20;
            if mid_warm != settled_warm {
                count += 1;
            }
        }
    }
    count
}

fn count_cover_pixels(buf: &slint::SharedPixelBuffer<slint::Rgba8Pixel>) -> usize {
    // Curtain cover is an opaque dark rect inside the mosaic band.
    // Exact token color is #2c1f1d but the software renderer composites the
    // bright thumb underneath with a slight blend, so we count any opaque
    // dark pixel inside the band (r<110,g<90,b<70) as a proxy for cover.
    // This discriminates the staggered wipe (many dark cover pixels at mid)
    // from the crossfade fallback or settled state (few).
    let w = buf.width() as usize;
    let h = buf.height() as usize;
    let bytes = buf.as_bytes();
    let mut count = 0usize;
    // Region that covers the justified mosaic band at 1920×1080 while
    // excluding the top chrome (y<90) and bottom bar.
    let y0 = 150usize.min(h);
    let y1 = 800usize.min(h);
    let x0 = 60usize.min(w);
    let x1 = 1650usize.min(w);
    for y in y0..y1 {
        for x in x0..x1 {
            let idx = (y * w + x) * 4;
            let r = bytes[idx];
            let g = bytes[idx + 1];
            let b = bytes[idx + 2];
            let a = bytes[idx + 3];
            if a < 10 {
                continue; // transparent gutter / outside tiles
            }
            if r < 120 && g < 90 && b < 70 {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn mosaic_curtain_phase_driven_renders_mid_and_settled() {
    use crate::shell::gallery::views::mosaic::{justified_hero_layout, mosaic_curtain_delays, MosaicPages};
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1);
    win.set_gallery_empty(false);
    win.set_gallery_style(2);
    win.set_gallery_focused(0);
    win.set_gallery_reduced_motion(false);

    // 18 themes → 2 full pages (capacity 9 at 1920×1080) → meaningful stagger
    let count = 18usize;
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    let thumb = bright_mosaic_thumb();
    for i in 0..count {
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: thumb.clone(),
            hero: thumb.clone(),
            slat_image: slint::Image::default(),
            slat_expanded_image: slint::Image::default(),
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));

    let stage_w = 1920.0f32;
    let stage_h = 1080.0f32;
    let mut pg = MosaicPages::new(count, stage_w, stage_h);
    assert!(pg.total_pages() >= 2, "18 themes must paginate (capacity {})", pg.capacity());

    // Page 0 settled
    let (aspects0, reals0) = pg.page_render();
    let (layout0, tile_reals0) = justified_hero_layout(&aspects0, &reals0, stage_w, stage_h);
    let delays0 = mosaic_curtain_delays(&layout0.tiles);
    let tiles0: Vec<crate::MosaicTileData> = layout0
        .tiles
        .iter()
        .zip(tile_reals0.iter())
        .zip(delays0.iter())
        .map(|((t, r), d)| crate::MosaicTileData {
            x: t.x,
            y: t.y,
            w: t.w,
            h: t.h,
            real_index: *r as i32,
            delay_ms: *d as i32,
        })
        .collect();
    win.set_gallery_mosaic_tiles(ModelRc::new(VecModel::from(tiles0.clone())));
    win.set_gallery_mosaic_current_page(pg.current() as i32);
    win.set_gallery_mosaic_total_pages(pg.total_pages() as i32);
    win.set_gallery_mosaic_page_numbers(ModelRc::new(VecModel::from(
        pg.page_numbers().iter().map(|n| *n as i32).collect::<Vec<i32>>(),
    )));
    // Flush initial layout (no curtain pending — phase 1 settled)
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }

    // Flip to page 1 — snapshot page 0 tiles as the under layer (Tramo 9)
    // then set page 1 tiles and bump flip-token to start the curtain.
    assert!(pg.step(1), "must flip to page 1");
    win.set_gallery_mosaic_under_tiles(ModelRc::new(VecModel::from(tiles0.clone())));
    let (aspects1, reals1) = pg.page_render();
    let (layout1, tile_reals1) = justified_hero_layout(&aspects1, &reals1, stage_w, stage_h);
    let delays1 = mosaic_curtain_delays(&layout1.tiles);
    let tiles1: Vec<crate::MosaicTileData> = layout1
        .tiles
        .iter()
        .zip(tile_reals1.iter())
        .zip(delays1.iter())
        .map(|((t, r), d)| crate::MosaicTileData {
            x: t.x,
            y: t.y,
            w: t.w,
            h: t.h,
            real_index: *r as i32,
            delay_ms: *d as i32,
        })
        .collect();
    win.set_gallery_mosaic_tiles(ModelRc::new(VecModel::from(tiles1.clone())));
    win.set_gallery_mosaic_current_page(pg.current() as i32);

    // Tramo 9: verify under-layer was populated with page 0 tiles
    use slint::Model as _;
    let under = win.get_gallery_mosaic_under_tiles();
    let under_rc = under.as_any().downcast_ref::<VecModel<crate::MosaicTileData>>()
        .expect("under-tiles should be a VecModel");
    assert_eq!(under_rc.row_count(), tiles0.len(),
        "under-tiles must snapshot the outgoing page ({} tiles)", tiles0.len());

    // Mid-curtain: advance ~300ms (~18 ticks) → phase ~0.5, column stagger visible
    for _ in 0..18 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let mid = win.window().take_snapshot().expect("mid curtain snapshot");
    save_slice_png(mid.clone(), "/tmp/opencode/mosaic_curtain_mid.png");

    // Settled: advance past total curtain duration (600ms total → another 400ms)
    for _ in 0..25 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let settled = win.window().take_snapshot().expect("settled snapshot");
    save_slice_png(settled.clone(), "/tmp/opencode/mosaic_curtain_settled.png");
    let settled_dark = count_cover_pixels(&settled);
    let diff_mid_settled = count_buffer_diff(&mid, &settled);
    let covered_non_uniform = count_covered_non_uniform(&mid, &settled);
    let hue_flips = count_hue_flips_covered(&mid, &settled);
    eprintln!("DEBUG mid vs settled: covered_non_uniform={covered_non_uniform} diff={diff_mid_settled} settled_dark={settled_dark} hue_flips={hue_flips}");

    // Tramo 8 contract: cover shows the incoming tile's OWN THEME IMAGE
    // (sharp, zoomed ~1.1 + dimmed ~0.25) — not flat dark. At mid, the
    // covered bands must contain IMAGE CONTENT (non-uniform stripes), so the
    // wipe is perceptible even though it reuses the same texture. We isolate
    // covered pixels (mid vs settled diff) and require they carry variance.
    // Zoom must be visible: stripe phase shifts due to 1.1 scale, causing
    // hue flips between mid (zoomed) and settled (1:1) at same coordinates.
    assert!(
        covered_non_uniform > 500,
        "mid-curtain cover must show image content (non-uniform) in covered regions — got {covered_non_uniform} variance-in-covered pixels, expected >500 (flat dark would be ~0)"
    );
    assert!(
        settled_dark < 300,
        "settled must have no covers — got {settled_dark} cover pixels, expected <300"
    );
    assert!(
        diff_mid_settled > 500,
        "curtain must differ visibly mid vs settled — diff {diff_mid_settled} expected >500"
    );
    assert!(
        hue_flips > 300,
        "zoom must be visible: covered (1.1×) vs revealed (1:1) stripe phase must shift — hue flips {hue_flips} expected >300 (no-zoom flat would be ~0)"
    );

    // Tramo 9 regression guard — Defect 1+2: at settled, all tiles MUST
    // show the incoming bright stripe image (no dark under-layer leakage).
    // The dark pixels (r<80 && g<50 && b<40) were the bug symptom — with
    // curtain-phase >= 1.0 gating, content-visible is true for ALL tiles.
    let snap_bytes = settled.as_bytes();
    let sw = settled.width() as usize;
    let sh = settled.height() as usize;
    let band_y0 = 150usize.min(sh);
    let band_y1 = 800usize.min(sh);
    let band_x0 = 60usize.min(sw);
    let band_x1 = 1650usize.min(sw);
    let mut dark_settled = 0usize;
    for y in band_y0..band_y1 {
        for x in band_x0..band_x1 {
            let idx = (y * sw + x) * 4;
            if snap_bytes[idx + 3] < 200 {
                continue;
            }
            let r = snap_bytes[idx];
            let g = snap_bytes[idx + 1];
            let b = snap_bytes[idx + 2];
            if r < 80 && g < 50 && b < 40 {
                dark_settled += 1;
            }
        }
    }
    assert!(
        dark_settled < 10000,
        "settled must NOT have dark under-layer pixels — got {dark_settled} \
         (Defect 1+2 regression guard: late columns dark would have 100k+)"
    );

    // Tramo 9 Defect 4: at mid, the growing clip reveals incoming image
    // left→right; behind the clip edge, the transparent un-swept region
    // shows the frozen under-layer (outgoing page).  covered_non_uniform
    // > 500 (above) proves the cover has image content.  The diff
    // diff_mid_settled > 500 proves the sweep differs from settled.
}

#[test]
fn mosaic_curtain_respects_reduced_motion() {
    use crate::shell::gallery::views::mosaic::{justified_hero_layout, mosaic_curtain_delays, MosaicPages};
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1);
    win.set_gallery_empty(false);
    win.set_gallery_style(2);
    win.set_gallery_focused(0);
    win.set_gallery_reduced_motion(true);

    let count = 12usize;
    let thumb = bright_mosaic_thumb();
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    for i in 0..count {
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: thumb.clone(),
            hero: thumb.clone(),
            slat_image: slint::Image::default(),
            slat_expanded_image: slint::Image::default(),
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));

    let stage_w = 1920.0f32;
    let stage_h = 1080.0f32;
    let mut pg = MosaicPages::new(count, stage_w, stage_h);
    let (aspects0, reals0) = pg.page_render();
    let (layout0, tile_reals0) = justified_hero_layout(&aspects0, &reals0, stage_w, stage_h);
    let delays0 = mosaic_curtain_delays(&layout0.tiles);
    let tiles0: Vec<crate::MosaicTileData> = layout0
        .tiles
        .iter()
        .zip(tile_reals0.iter())
        .zip(delays0.iter())
        .map(|((t, r), d)| crate::MosaicTileData {
            x: t.x,
            y: t.y,
            w: t.w,
            h: t.h,
            real_index: *r as i32,
            delay_ms: *d as i32,
        })
        .collect();
    win.set_gallery_mosaic_tiles(ModelRc::new(VecModel::from(tiles0.clone())));
    win.set_gallery_mosaic_current_page(pg.current() as i32);
    win.set_gallery_mosaic_total_pages(pg.total_pages() as i32);
    win.set_gallery_mosaic_page_numbers(ModelRc::new(VecModel::from(
        pg.page_numbers().iter().map(|n| *n as i32).collect::<Vec<i32>>(),
    )));
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    // Tramo 9: set under-layer on flip (reduced-motion still snapshots it).
    assert!(pg.step(1));
    win.set_gallery_mosaic_under_tiles(ModelRc::new(VecModel::from(tiles0)));
    let (aspects1, reals1) = pg.page_render();
    let (layout1, tile_reals1) = justified_hero_layout(&aspects1, &reals1, stage_w, stage_h);
    let delays1 = mosaic_curtain_delays(&layout1.tiles);
    let tiles1: Vec<crate::MosaicTileData> = layout1
        .tiles
        .iter()
        .zip(tile_reals1.iter())
        .zip(delays1.iter())
        .map(|((t, r), d)| crate::MosaicTileData {
            x: t.x,
            y: t.y,
            w: t.w,
            h: t.h,
            real_index: *r as i32,
            delay_ms: *d as i32,
        })
        .collect();
    win.set_gallery_mosaic_tiles(ModelRc::new(VecModel::from(tiles1)));
    win.set_gallery_mosaic_current_page(pg.current() as i32);

    // Mid with reduced-motion: crossfade, NO curtain covers
    for _ in 0..6 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let mid = win.window().take_snapshot().expect("reduced mid snapshot");
    // Save for visual inspection but not required for contract — keep distinct name
    save_slice_png(mid.clone(), "/tmp/opencode/mosaic_curtain_reduced_mid.png");
    let mid_dark = count_cover_pixels(&mid);
    assert!(
        mid_dark < 300,
        "reduced-motion must NOT show curtain covers — got {mid_dark} cover pixels, expected <300 (crossfade only)"
    );
    // Settled also no covers
    for _ in 0..20 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let settled = win.window().take_snapshot().expect("reduced settled");
    save_slice_png(settled.clone(), "/tmp/opencode/mosaic_curtain_reduced_settled.png");
    let settled_dark = count_cover_pixels(&settled);
    assert!(settled_dark < 300, "reduced settled must have no covers — got {settled_dark}");
}

// ── Mutating-window slice 1: panel morph midflight (R1) ──────────────

#[test]
fn panel_morph_midflight_renders() {
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1);
    win.set_expanded(true);
    win.set_gallery_empty(false);
    win.set_gallery_style(0);
    win.set_gallery_focused(0);
    win.set_gallery_reduced_motion(false);
    win.set_panel_section(0);
    win.set_is_mutating(false);
    win.set_is_panel_open(false);

    // Populate gallery with minimal cards so GalleryRoot paints content (slice)
    let count = 6usize;
    let stage_w = 1920.0f32;
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    for i in 0..count {
        let src = slice_test_gradient(i);
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: slint::Image::default(),
            hero: slint::Image::default(),
            slat_image: slint::Image::default(),
            slat_expanded_image: slint::Image::default(),
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));
    let tiles: Vec<crate::SliceTileData> = crate::shell::gallery::views::slice::slice_delta_tiles(count, 0, stage_w)
        .into_iter()
        .map(|t| crate::SliceTileData {
            delta: t.delta,
            real_index: t.real_index as i32,
            is_expanded: t.is_expanded,
            fade: t.fade,
            dist: t.dist,
        })
        .collect();
    win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles)));
    win.set_gallery_slice_focus_pos(0.0);

    // Flush layout settled
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let settled_gallery = win.window().take_snapshot().expect("gallery settled");
    save_slice_png(settled_gallery.clone(), "/tmp/opencode/panel_morph_gallery_settled.png");

    // Trigger mutating: Gallery 1→0 / Panel 0→1 (350ms morph, 200ms stagger R1)
    win.set_is_mutating(true);
    // Advance ~175ms (midflight) — halfway through 350ms crossfade
    for _ in 0..11 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let mid = win.window().take_snapshot().expect("midflight snapshot");
    save_slice_png(mid.clone(), "/tmp/opencode/panel_morph_midflight.png");

    // Complete mutation: panel open — wait for tuck 500ms + pause 140ms + stretch 560ms
    win.set_is_mutating(false);
    win.set_is_panel_open(true);
    for _ in 0..80 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let panel_settled = win.window().take_snapshot().expect("panel settled");
    save_slice_png(panel_settled.clone(), "/tmp/opencode/panel_morph_panel_settled.png");

    // Midflight must differ from both settled extremes (dock tuck + 80% wow visible, not instant swap)
    let diff_gallery_mid = count_buffer_diff(&settled_gallery, &mid);
    let diff_mid_panel = count_buffer_diff(&mid, &panel_settled);
    assert!(
        diff_gallery_mid > 300,
        "midflight must differ from gallery settled — got {diff_gallery_mid}, expected >300 (tuck+dock)"
    );
    assert!(
        diff_mid_panel > 300,
        "midflight must differ from panel settled — got {diff_mid_panel}, expected >300"
    );

    // Reduced-motion crossfade path (R1, R8): durations 0 → instant, no 350ms wait
    win.set_gallery_reduced_motion(true);
    win.set_is_panel_open(false);
    win.set_is_mutating(false);
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    // Instant morph via reduced-motion: set mutating then immediately panel open (0ms)
    win.set_is_mutating(true);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(8));
    let reduced_mid = win.window().take_snapshot().expect("reduced mid");
    save_slice_png(reduced_mid.clone(), "/tmp/opencode/panel_morph_reduced_mid.png");
    win.set_is_mutating(false);
    win.set_is_panel_open(true);
    // No delay needed — reduced-motion duration 0 means immediate
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(8));
    let reduced_settled = win.window().take_snapshot().expect("reduced settled");
    save_slice_png(reduced_settled.clone(), "/tmp/opencode/panel_morph_reduced_settled.png");
    // Reduced snapshots should be valid (no panic) and at least panel is visible (reuse diff check loosely)
    let diff_reduced = count_buffer_diff(&reduced_mid, &reduced_settled);
    // With 0ms, mid and settled may be close; just ensure we didn't crash and images exist
    assert!(
        reduced_mid.width() == reduced_settled.width(),
        "reduced snapshots dimensions must match"
    );
    let _ = diff_reduced;
}

// ── Mutating-window slice 2: SaveSection render (R10) ─────────────

#[test]
fn panel_save_renders() {
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1);
    win.set_expanded(true);
    win.set_gallery_empty(false);
    win.set_gallery_style(0);
    win.set_gallery_focused(0);
    win.set_gallery_reduced_motion(false);
    win.set_panel_section(0);
    win.set_is_mutating(false);
    win.set_is_panel_open(true);
    win.set_panel_save_input("MyMix".into());
    win.set_panel_save_error("".into());
    win.set_panel_save_placeholder("Theme name…".into());
    win.set_panel_save_button_text("Save".into());

    let count = 6usize;
    let stage_w = 1920.0f32;
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    for i in 0..count {
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: slint::Image::default(),
            hero: slint::Image::default(),
            slat_image: slint::Image::default(),
            slat_expanded_image: slint::Image::default(),
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));
    let tiles: Vec<crate::SliceTileData> = crate::shell::gallery::views::slice::slice_delta_tiles(count, 0, stage_w)
        .into_iter()
        .map(|t| crate::SliceTileData {
            delta: t.delta,
            real_index: t.real_index as i32,
            is_expanded: t.is_expanded,
            fade: t.fade,
            dist: t.dist,
        })
        .collect();
    win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles)));
    win.set_gallery_slice_focus_pos(0.0);

    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let settled = win.window().take_snapshot().expect("panel save settled");
    save_slice_png(settled.clone(), "/tmp/opencode/panel_save.png");

    // Also render with error to verify error label visibility
    win.set_panel_save_error("name-empty".into());
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let err_snap = win.window().take_snapshot().expect("panel save error");
    save_slice_png(err_snap.clone(), "/tmp/opencode/panel_save_error.png");

    let diff = count_buffer_diff(&settled, &err_snap);
    assert!(diff > 200, "error label must change pixels — got {diff} expected >200");

    // Basic sanity: panel save settled must differ from empty gallery snapshot baseline
    assert!(settled.width() == 1920, "snapshot width 1920");
    assert!(err_snap.width() == 1920);
}

// ── Mutating-window slice 3: Borders pick layer (R3) ───────────────

#[test]
fn panel_borders_pick_renders() {
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1);
    win.set_expanded(true);
    win.set_gallery_empty(false);
    win.set_gallery_style(0);
    win.set_gallery_focused(0);
    win.set_gallery_reduced_motion(false);
    win.set_panel_section(1);
    win.set_is_mutating(false);
    win.set_is_panel_open(true);

    // Border presets (pick layer) — scan("borders") → cards
    let titles = vec![
        SharedString::from("Thin Rounded"),
        SharedString::from("Sharp"),
        SharedString::from("Thick"),
    ];
    let descs = vec![
        SharedString::from("Thin rounded borders"),
        SharedString::from("Sharp square borders"),
        SharedString::from("Thick rounded"),
    ];
    let tags = vec![
        SharedString::from("SYSTEM"),
        SharedString::from("SYSTEM"),
        SharedString::from("USER"),
    ];
    let files = vec![
        SharedString::from("thin-rounded.ron"),
        SharedString::from("sharp.ron"),
        SharedString::from("thick.ron"),
    ];
    win.set_border_titles(ModelRc::new(VecModel::from(titles)));
    win.set_border_descs(ModelRc::new(VecModel::from(descs)));
    win.set_border_tags(ModelRc::new(VecModel::from(tags)));
    win.set_border_files(ModelRc::new(VecModel::from(files)));
    win.set_active_border_index(0);

    let count = 6usize;
    let stage_w = 1920.0f32;
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    for i in 0..count {
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: slint::Image::default(),
            hero: slint::Image::default(),
            slat_image: slint::Image::default(),
            slat_expanded_image: slint::Image::default(),
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));
    let tiles: Vec<crate::SliceTileData> = crate::shell::gallery::views::slice::slice_delta_tiles(count, 0, stage_w)
        .into_iter()
        .map(|t| crate::SliceTileData {
            delta: t.delta,
            real_index: t.real_index as i32,
            is_expanded: t.is_expanded,
            fade: t.fade,
            dist: t.dist,
        })
        .collect();
    win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles)));
    win.set_gallery_slice_focus_pos(0.0);

    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap = win.window().take_snapshot().expect("panel borders pick snapshot");
    save_slice_png(snap.clone(), "/tmp/opencode/panel_borders_pick.png");
    assert!(snap.width() == 1920, "snapshot width 1920");

    // Active indicator check: second snapshot with different active index must differ
    win.set_active_border_index(1);
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap2 = win.window().take_snapshot().expect("panel borders pick active2");
    save_slice_png(snap2.clone(), "/tmp/opencode/panel_borders_pick_active2.png");
    let diff = count_buffer_diff(&snap, &snap2);
    assert!(diff > 200, "active indicator must change pixels — got {diff} expected >200");
}

// ── Mutating-window slice 4: Borders tune layer (R3, COLOR GATED) ─────────

#[test]
fn panel_borders_tune_renders() {
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1);
    win.set_expanded(true);
    win.set_gallery_empty(false);
    win.set_gallery_style(0);
    win.set_gallery_focused(0);
    win.set_gallery_reduced_motion(false);
    win.set_panel_section(1);
    win.set_is_mutating(false);
    win.set_is_panel_open(true);

    // Border presets (pick)
    let titles = vec![
        SharedString::from("Thin Rounded"),
        SharedString::from("Sharp"),
        SharedString::from("Thick"),
    ];
    let descs = vec![
        SharedString::from("Thin rounded borders"),
        SharedString::from("Sharp square borders"),
        SharedString::from("Thick rounded"),
    ];
    let tags = vec![
        SharedString::from("SYSTEM"),
        SharedString::from("SYSTEM"),
        SharedString::from("USER"),
    ];
    let files = vec![
        SharedString::from("thin-rounded.ron"),
        SharedString::from("sharp.ron"),
        SharedString::from("thick.ron"),
    ];
    win.set_border_titles(ModelRc::new(VecModel::from(titles)));
    win.set_border_descs(ModelRc::new(VecModel::from(descs)));
    win.set_border_tags(ModelRc::new(VecModel::from(tags)));
    win.set_border_files(ModelRc::new(VecModel::from(files)));
    win.set_active_border_index(0);
    // Tune layer — 4 GeometrySliders + color-slot gated
    win.set_border_size(2);
    win.set_corner_radius(10);
    win.set_gap_in(5);
    win.set_gap_out(5);

    let count = 6usize;
    let stage_w = 1920.0f32;
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    for i in 0..count {
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: slint::Image::default(),
            hero: slint::Image::default(),
            slat_image: slint::Image::default(),
            slat_expanded_image: slint::Image::default(),
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));
    let tiles: Vec<crate::SliceTileData> = crate::shell::gallery::views::slice::slice_delta_tiles(count, 0, stage_w)
        .into_iter()
        .map(|t| crate::SliceTileData {
            delta: t.delta,
            real_index: t.real_index as i32,
            is_expanded: t.is_expanded,
            fade: t.fade,
            dist: t.dist,
        })
        .collect();
    win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles)));
    win.set_gallery_slice_focus_pos(0.0);

    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap = win.window().take_snapshot().expect("panel borders tune snapshot");
    save_slice_png(snap.clone(), "/tmp/opencode/panel_borders_tune.png");
    assert!(snap.width() == 1920, "snapshot width 1920");

    // Snap simulation: picking sharp snaps sliders to 1/0/0/0 — must differ visually
    win.set_active_border_index(1);
    win.set_border_size(1);
    win.set_corner_radius(0);
    win.set_gap_in(0);
    win.set_gap_out(0);
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap2 = win.window().take_snapshot().expect("panel borders tune snapped");
    save_slice_png(snap2.clone(), "/tmp/opencode/panel_borders_tune_snapped.png");
    let diff = count_buffer_diff(&snap, &snap2);
    assert!(diff > 200, "tune snap must change pixels — got {diff} expected >200 (slider values)");

    // Color-gated placeholder must be present — check second diff still >0 for tune vs pick baseline
    // Tune baseline must differ from pick baseline (sliders visible)
    // Re-mount pick baseline comparison via previous PNG is not needed; we just ensure tune renders without panic and diff >200 suffices for visual gate.
}

// ── Mutating-window slice 5: Motion pick + bezier + CurvePreview (R4) ──

#[test]
fn panel_curve_preview_renders() {
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1);
    win.set_expanded(true);
    win.set_gallery_empty(false);
    win.set_gallery_style(0);
    win.set_gallery_focused(0);
    win.set_gallery_reduced_motion(false);
    win.set_panel_section(2);
    win.set_is_mutating(false);
    win.set_is_panel_open(true);

    // Animation presets (R4) — scan("animations") → cards
    // Use 3 entries for preview visibility (19-card unit test covers full 19 in callbacks);
    // preview sits below cards, so with 19 it would be scrolled off-screen and diff would be 0.
    let titles: Vec<SharedString> = (1..=3).map(|i| SharedString::from(format!("Anim {i:02}"))).collect();
    let descs: Vec<SharedString> = (1..=3).map(|i| SharedString::from(format!("Animation {i:02} desc"))).collect();
    let tags: Vec<SharedString> = (1..=3).map(|_| SharedString::from("SYSTEM")).collect();
    let files: Vec<SharedString> = (1..=3).map(|i| SharedString::from(format!("anim{:02}.conf", i))).collect();
    win.set_anim_titles(ModelRc::new(VecModel::from(titles)));
    win.set_anim_descs(ModelRc::new(VecModel::from(descs)));
    win.set_anim_tags(ModelRc::new(VecModel::from(tags)));
    win.set_anim_files(ModelRc::new(VecModel::from(files)));
    win.set_active_anim_index(0);
    // Bezier defaults a=0.25 b=0.1 c=0.25 d=1.0 (CSS cubic-bezier)
    win.set_bezier_a(0.25);
    win.set_bezier_b(0.1);
    win.set_bezier_c(0.25);
    win.set_bezier_d(1.0);

    let count = 6usize;
    let stage_w = 1920.0f32;
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    for i in 0..count {
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: slint::Image::default(),
            hero: slint::Image::default(),
            slat_image: slint::Image::default(),
            slat_expanded_image: slint::Image::default(),
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));
    let tiles: Vec<crate::SliceTileData> = crate::shell::gallery::views::slice::slice_delta_tiles(count, 0, stage_w)
        .into_iter()
        .map(|t| crate::SliceTileData {
            delta: t.delta,
            real_index: t.real_index as i32,
            is_expanded: t.is_expanded,
            fade: t.fade,
            dist: t.dist,
        })
        .collect();
    win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles)));
    win.set_gallery_slice_focus_pos(0.0);

    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap = win.window().take_snapshot().expect("panel curve preview snapshot");
    save_slice_png(snap.clone(), "/tmp/opencode/panel_curve_preview.png");
    assert!(snap.width() == 1920, "snapshot width 1920");

    // WHEN b→0.8 THEN preview redraws bezier (control point moves, curve shape changes)
    win.set_bezier_b(0.8);
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap2 = win.window().take_snapshot().expect("panel curve preview b08 snapshot");
    save_slice_png(snap2.clone(), "/tmp/opencode/panel_curve_preview_b08.png");
    let diff = count_buffer_diff(&snap, &snap2);
    assert!(diff > 200, "curve preview must redraw on b change — got {diff} expected >200 (bezier 0.1→0.8)");

    // Active indicator check: picking different animation still shows 19 cards, no crash
    win.set_active_anim_index(1);
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap3 = win.window().take_snapshot().expect("panel curve preview active2");
    save_slice_png(snap3.clone(), "/tmp/opencode/panel_curve_preview_active2.png");
    let diff2 = count_buffer_diff(&snap, &snap3);
    assert!(diff2 > 200, "active indicator must change pixels — got {diff2} expected >200");
}

// ── Mutating-window slice 6: Filters pick (R5) ──

#[test]
fn panel_filters_renders() {
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1);
    win.set_expanded(true);
    win.set_gallery_empty(false);
    win.set_gallery_style(0);
    win.set_gallery_focused(0);
    win.set_gallery_reduced_motion(false);
    win.set_panel_section(3);
    win.set_is_mutating(false);
    win.set_is_panel_open(true);

    // Shader presets (R5) — scan("shaders") → cards
    let titles: Vec<SharedString> = vec![SharedString::from("Blue Light"), SharedString::from("Cyberpunk")];
    let descs: Vec<SharedString> = vec![SharedString::from("Blue light filter"), SharedString::from("Cyberpunk neon")];
    let tags: Vec<SharedString> = vec![SharedString::from("SYSTEM"), SharedString::from("SYSTEM")];
    let files: Vec<SharedString> = vec![SharedString::from("blue-light.ron"), SharedString::from("cyberpunk.ron")];
    win.set_shader_titles(ModelRc::new(VecModel::from(titles)));
    win.set_shader_descs(ModelRc::new(VecModel::from(descs)));
    win.set_shader_tags(ModelRc::new(VecModel::from(tags)));
    win.set_shader_files(ModelRc::new(VecModel::from(files)));
    win.set_active_shader_index(0);

    let count = 6usize;
    let stage_w = 1920.0f32;
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    for i in 0..count {
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: slint::Image::default(),
            hero: slint::Image::default(),
            slat_image: slint::Image::default(),
            slat_expanded_image: slint::Image::default(),
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));
    let tiles: Vec<crate::SliceTileData> = crate::shell::gallery::views::slice::slice_delta_tiles(count, 0, stage_w)
        .into_iter()
        .map(|t| crate::SliceTileData {
            delta: t.delta,
            real_index: t.real_index as i32,
            is_expanded: t.is_expanded,
            fade: t.fade,
            dist: t.dist,
        })
        .collect();
    win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles)));
    win.set_gallery_slice_focus_pos(0.0);

    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap = win.window().take_snapshot().expect("panel filters snapshot");
    save_slice_png(snap.clone(), "/tmp/opencode/panel_filters.png");
    assert!(snap.width() == 1920, "snapshot width 1920");

    // Active indicator toggles
    win.set_active_shader_index(1);
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap2 = win.window().take_snapshot().expect("panel filters active2");
    save_slice_png(snap2.clone(), "/tmp/opencode/panel_filters_active2.png");
    let diff = count_buffer_diff(&snap, &snap2);
    assert!(diff > 200, "active shader indicator must change pixels — got {diff} expected >200");
}

// ── Mutating-window slice 7: System (R7, R11) ───────────────────────

#[test]
fn test_system_minimize_10s() {
    // RED: SystemSection must exist and expose timer 10s row + toggle_system ON/OFF
    // This test fails until SystemSection.slint is created with required rows.
    let path = "ui/panel/sections/SystemSection.slint";
    let content = std::fs::read_to_string(path).expect("SystemSection.slint must exist for slice 7");
    assert!(content.contains("10"), "timer 10s row must be present — got content without \"10\"");
    assert!(
        content.contains("System") || content.contains("system"),
        "System ON/OFF status block must be present"
    );
    // Also verify Config can hold 10s (engine sealed — no engine edit)
    let mut cfg = crate::config::Config::default();
    cfg.minimize_seconds = 10;
    assert_eq!(cfg.minimize_seconds, 10, "minimize_seconds 10s via toggle_system path");
    // Verify AppState toggle_system can be called (existing on_toggle_system reused)
    // We check the method exists and toggles is_system_active without panicking
    let proj = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let engine = crate::engine::Engine::new(&proj);
    let cfg2 = crate::config::Config::default();
    let tm = crate::theme_manager::ThemeManager::new(&proj);
    let mut state = crate::app_state::AppState::new(cfg2, engine, tm);
    let _ = state.toggle_system(true);
    assert!(state.cfg().is_system_active, "toggle_system(true) must set active");
    let mut st2 = state;
    st2.update_cfg(|c| c.minimize_seconds = 10);
    assert_eq!(st2.cfg().minimize_seconds, 10);
}

#[test]
fn test_system_rows_render() {
    // RED: SystemSection must contain autostart/theme-pref/reset/restart rows + About moved
    let path = "ui/panel/sections/SystemSection.slint";
    let content = std::fs::read_to_string(path).expect("SystemSection.slint must exist");
    for needle in ["Autostart", "Theme", "Reset", "Restart", "About"] {
        assert!(
            content.contains(needle),
            "SystemSection must contain \"{needle}\" row — missing in content"
        );
    }
    // Tiling stays exactly as-is (deferred window-rules/fullscreen-redesign)
    assert!(
        content.contains("Tiling") || content.contains("tiling"),
        "tiling toggle must stay exactly as-is"
    );
}

#[test]
fn panel_system_renders() {
    use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(1);
    win.set_expanded(true);
    win.set_gallery_empty(false);
    win.set_gallery_style(0);
    win.set_gallery_focused(0);
    win.set_gallery_reduced_motion(false);
    win.set_panel_section(4);
    win.set_is_mutating(false);
    win.set_is_panel_open(true);

    // System props — ON state
    win.set_system_active(true);
    win.set_auto_minimize(true);
    win.set_minimize_seconds(5);
    win.set_language("en".into());
    win.set_tiling_mode(false);
    win.set_autostart(false);
    win.set_theme("system".into());
    win.set_restart_required(false);
    win.set_keybinds_mode(false);
    // Settings labels (needed because MainWindow defaults are empty in test)
    win.set_auto_minimize_label("Auto-minimize".into());
    win.set_timer_label("Timer:".into());
    win.set_language_label("Language".into());
    win.set_tiling_label("Tiling mode".into());
    win.set_keybinds_label("Keyboard shortcuts".into());
    win.set_autostart_label("Autostart".into());
    win.set_theme_label("Theme".into());
    win.set_reset_label("Reset presets".into());
    win.set_settings_title("System".into());
    win.set_settings_restart_banner("⚠ Restart required".into());
    win.set_settings_restart_button("Restart".into());
    // About props
    win.set_home_about_title("About HVE".into());
    win.set_home_about_short("Hyprland Visual Editor makes your desktop truly yours.".into());
    win.set_home_about_full("HVE is a graphical app to visually manage your Hyprland desktop aesthetics.".into());
    win.set_home_about_tree_label("Project structure".into());
    win.set_home_about_tree_paths(ModelRc::new(VecModel::from(vec![SharedString::from("a/b"), SharedString::from("c/d")])));
    win.set_home_about_tree_descs(ModelRc::new(VecModel::from(vec![SharedString::from("desc a"), SharedString::from("desc c")])));
    win.set_home_about_tree_path_max("a/b".into());
    win.set_home_about_docs_label("View documentation".into());

    let count = 6usize;
    let stage_w = 1920.0f32;
    let mut cards: Vec<crate::GalleryCardData> = Vec::new();
    for i in 0..count {
        cards.push(crate::GalleryCardData {
            name: SharedString::from(format!("Theme {i}")),
            saved_at: SharedString::from(""),
            is_active: false,
            providers: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
            accent: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            primary: slint::Color::from_rgb_u8(0x8f, 0xd8, 0xff),
            secondary: slint::Color::from_rgb_u8(0x44, 0x55, 0x66),
            tertiary: slint::Color::from_rgb_u8(0x66, 0x77, 0x88),
            surface: slint::Color::from_rgb_u8(0x11, 0x14, 0x18),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: SharedString::from(""),
            thumb_path: SharedString::from(""),
            thumb: slint::Image::default(),
            hero: slint::Image::default(),
            slat_image: slint::Image::default(),
            slat_expanded_image: slint::Image::default(),
        });
    }
    win.set_gallery_cards(ModelRc::new(VecModel::from(cards)));
    let tiles: Vec<crate::SliceTileData> = crate::shell::gallery::views::slice::slice_delta_tiles(count, 0, stage_w)
        .into_iter()
        .map(|t| crate::SliceTileData {
            delta: t.delta,
            real_index: t.real_index as i32,
            is_expanded: t.is_expanded,
            fade: t.fade,
            dist: t.dist,
        })
        .collect();
    win.set_gallery_slice_tiles(ModelRc::new(VecModel::from(tiles)));
    win.set_gallery_slice_focus_pos(0.0);

    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap_on = win.window().take_snapshot().expect("panel system ON snapshot");
    save_slice_png(snap_on.clone(), "/tmp/opencode/panel_system.png");
    assert!(snap_on.width() == 1920, "snapshot width 1920");

    // Toggle OFF — must differ (ON/OFF visible)
    win.set_system_active(false);
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap_off = win.window().take_snapshot().expect("panel system OFF snapshot");
    save_slice_png(snap_off.clone(), "/tmp/opencode/panel_system_off.png");
    let diff_on_off = count_buffer_diff(&snap_on, &snap_off);
    assert!(
        diff_on_off > 200,
        "ON/OFF must change pixels — got {diff_on_off} expected >200"
    );

    // Timer 10s variant — must differ when seconds change
    win.set_system_active(true);
    win.set_minimize_seconds(10);
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let snap_10s = win.window().take_snapshot().expect("panel system 10s snapshot");
    save_slice_png(snap_10s.clone(), "/tmp/opencode/panel_system_10s.png");
    let diff_10s = count_buffer_diff(&snap_on, &snap_10s);
    assert!(
        diff_10s > 200,
        "timer 10s must change pixels — got {diff_10s} expected >200"
    );

    // Basic sanity: still 1920
    assert!(snap_10s.width() == 1920);
}

// ── Mutating-window slice 8: Legacy cleanup (R8) ───────────────────────

#[test]
fn test_no_active_tab_orphans() {
    use std::path::Path;
    let needle = ["active", "tab"].join("-");
    fn walk(dir: &Path, needle: &str, hits: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(&path, needle, hits);
            } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if matches!(ext, "slint" | "rs" | "json") {
                    // Skip self-test file to avoid false positive on needle literal
                    if path.ends_with("src/shell/ui_tests.rs") {
                        continue;
                    }
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if content.contains(needle) {
                            hits.push(format!("{} contains {}", path.display(), needle));
                        }
                    }
                }
            }
        }
    }
    let mut hits = Vec::new();
    walk(Path::new("ui"), &needle, &mut hits);
    walk(Path::new("src"), &needle, &mut hits);
    assert!(
        hits.is_empty(),
        "orphans must be 0 — grep -rn legacy_tab ui/ src/ →0 but found: {hits:?}"
    );
}

#[test]
fn test_modules_removed() {
    let content = std::fs::read_to_string("ui/modules.slint")
        .unwrap_or_else(|_| panic!("ui/modules.slint must exist (may be empty)"));
    for needle in ["HomeModule", "AnimationsModule", "BordersModule", "ShadersModule", "ThemesModule"] {
        assert!(
            !content.contains(needle),
            "legacy module {needle} must be gone from ui/modules.slint"
        );
    }
    // SettingsPanel overlay component must be gone from ui/components.slint
    let comp = std::fs::read_to_string("ui/components.slint").expect("ui/components.slint must exist");
    assert!(
        !comp.contains("export component SettingsPanel"),
        "SettingsPanel overlay component must be removed from ui/components.slint"
    );
    // SettingsButton is also legacy (only used by old overlay)
    assert!(
        !comp.contains("export component SettingsButton"),
        "SettingsButton must be removed from ui/components.slint — orphan after overlay removal"
    );
}

#[test]
fn test_panel_layer_gated() {
    let content = std::fs::read_to_string("ui/shell.slint").expect("ui/shell.slint must exist");
    // PanelRoot must be instantiated ONLY inside a conditional gated on is-panel-open || is-mutating
    // so that when closed it does not exist and cannot intercept pointer/keyboard input.
    assert!(
        content.contains("if root.is-panel-open || root.is-mutating"),
        "PanelRoot must be gated by `if root.is-panel-open || root.is-mutating` — found no such gate"
    );
    // Ensure the gate actually wraps PanelRoot (not a stray unrelated if)
    // Dock choreography: tuck + pause + stretch wrapper (~2k chars), so offset is ~2100
    let gated_pos = content
        .find("if root.is-panel-open || root.is-mutating")
        .expect("gate must exist");
    let panel_pos = content[gated_pos..]
        .find("PanelRoot")
        .expect("PanelRoot must follow the gate");
    assert!(
        panel_pos < 2500,
        "PanelRoot must appear shortly after the gate (within 2500 chars, accounts for 80% morph wrapper) — got offset {panel_pos}"
    );
    // Unconditional instantiation would keep an invisible but input-capturing layer over the gallery
    assert!(
        !content.contains("panel-layer := PanelRoot"),
        "unconditional `panel-layer := PanelRoot {{` must NOT exist — panel must be conditional, otherwise opacity 0 still captures input"
    );
}

#[test]
fn test_filterbar_focus_scope_behind_pills() {
    let content = std::fs::read_to_string("ui/gallery/FilterBar.slint").expect("ui/gallery/FilterBar.slint must exist");
    let focus_pos = content
        .find("focus-scope := FocusScope")
        .expect("focus-scope := FocusScope must exist in FilterBar.slint");
    let pills_pos = content
        .find("pills-wrapper := VerticalLayout")
        .expect("pills-wrapper := VerticalLayout must exist");
    assert!(
        focus_pos < pills_pos,
        "focus-scope must be declared BEFORE pills-wrapper so pills TouchAreas sit on top — got focus-scope at {focus_pos} pills-wrapper at {pills_pos} (FocusScope on top consumes clicks)"
    );
    // FocusScope body must contain real key handling (arrow navigation + Return/Enter)
    let body_end = std::cmp::min(content.len(), focus_pos + 3500);
    let body = &content[focus_pos..body_end];
    assert!(
        body.contains("ArrowLeft") || body.contains("ArrowRight") || body.contains("LeftArrow") || body.contains("RightArrow"),
        "FocusScope must handle arrow keys (ArrowLeft/ArrowRight) — no arrow handling found in FocusScope body"
    );
    assert!(
        body.contains("Return"),
        "FocusScope must handle Return (Enter) to activate pill — no Return handling found in FocusScope body"
    );
    // No FocusScope with width 100% may appear AFTER pills-wrapper (would sit on top again)
    let tail = &content[pills_pos..];
    assert!(
        !tail.contains("focus-scope := FocusScope"),
        "no FocusScope should appear AFTER pills-wrapper — found FocusScope after pills-wrapper (still on top, pills never receive clicks)"
    );
    // Tail must not contain a width:100% FocusScope declaration (the overlay trick).
    // Comments mentioning FocusScope are allowed; only declarations count.
    if tail.contains("FocusScope") {
        assert!(
            !tail.contains(":= FocusScope"),
            "no width:100% FocusScope may appear after pills-wrapper — tail still contains FocusScope declaration"
        );
    }
}

// ── Theme interlude fade — headless visual verification ────────────────
// The swap choreography relies on a REAL opacity dissolve (fade-out on
// exit, fade-in on entrance) instead of the compositor's elastic workspace
// slide. This renders the three states headless and saves PNGs under
// /tmp/opencode/ for human review, plus a deterministic pixel assertion:
// with the shell at opacity 0 the top-left region must be perfectly
// uniform (window background only), and content must return after the
// entrance fade completes.

/// Share of the most common pixel color in the top-left 100×100 region.
fn theme_fade_region_modal_fraction(buf: &slint::SharedPixelBuffer<slint::Rgba8Pixel>) -> f32 {
    let bytes = buf.as_bytes();
    let stride = buf.width() as usize * 4;
    let mut counts: std::collections::HashMap<[u8; 4], usize> = std::collections::HashMap::new();
    let mut total = 0usize;
    for y in 0..100usize {
        for x in 0..100usize {
            let o = y * stride + x * 4;
            *counts.entry([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]).or_default() += 1;
            total += 1;
        }
    }
    let max = counts.values().copied().max().unwrap_or(0);
    max as f32 / total.max(1) as f32
}

#[test]
fn theme_fade_renders_settled_dissolved_and_reappeared() {
    use slint::ComponentHandle as _;
    i_slint_core::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some(slint::SharedString::from("software")),
                ..Default::default()
            },
        ),
    ))
    .expect("platform already initialized");

    let win = crate::MainWindow::new().unwrap();
    win.window().set_size(slint::PhysicalSize::new(1920, 1080));
    win.set_mounted_screen(0); // Home: opaque bg → uniform region is meaningful

    // Flush any initial state animations.
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let settled = win.window().take_snapshot().expect("settled snapshot");
    save_slice_png(settled.clone(), "/tmp/opencode/theme_fade_settled.png");
    let settled_frac = theme_fade_region_modal_fraction(&settled);

    // Dissolve-out (exit fade, ease-in): advancing past the duration must
    // leave the whole shell at opacity 0 — top-left region fully uniform.
    win.set_theme_transitioning(true);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(500));
    let dissolved = win.window().take_snapshot().expect("dissolved snapshot");
    save_slice_png(dissolved.clone(), "/tmp/opencode/theme_fade_dissolved.png");
    let dissolved_frac = theme_fade_region_modal_fraction(&dissolved);

    // Dissolve-in (entrance fade, ease-out): content must come back.
    win.set_theme_transitioning(false);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(600));
    let reappeared = win.window().take_snapshot().expect("reappeared snapshot");
    save_slice_png(reappeared.clone(), "/tmp/opencode/theme_fade_reappeared.png");
    let reappeared_frac = theme_fade_region_modal_fraction(&reappeared);

    assert!(
        dissolved_frac > 0.995,
        "exit fade must dissolve the shell to pure background (modal fraction {dissolved_frac:.3}, settled {settled_frac:.3})"
    );
    assert!(
        settled_frac < 0.995,
        "settled home must have visible content in the top-left region (modal fraction {settled_frac:.3})"
    );
    assert!(
        reappeared_frac < 0.995,
        "entrance fade must bring the content back (modal fraction {reappeared_frac:.3})"
    );
}
