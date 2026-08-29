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
            slat_image: crate::shell::gallery::slat_image::baked_slat_image(src.clone(), false),
            slat_expanded_image: crate::shell::gallery::slat_image::baked_slat_image(src, true),
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

    // Settled: focus position exactly at the focused index (frac 0).
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
    win.set_gallery_slice_rebasing(true);
    win.set_gallery_slice_focus_pos(6.5); // frac = −0.5
    let mid = win.window().take_snapshot().expect("midflight snapshot");
    save_slice_png(mid, "/tmp/opencode/slice_midflight.png");
    win.set_gallery_slice_rebasing(false);
}
