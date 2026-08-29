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
    // Solid bright color distinct from the cover (surface-container 44,31,29)
    // so the curtain's dark cover is detectable via pixel count.
    let (w, h) = (320u32, 200u32);
    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
    for px in buf.make_mut_bytes().chunks_mut(4) {
        px[0] = 240;
        px[1] = 190;
        px[2] = 90;
        px[3] = 255;
    }
    slint::Image::from_rgba8(buf)
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
    win.set_gallery_mosaic_tiles(ModelRc::new(VecModel::from(tiles0)));
    win.set_gallery_mosaic_current_page(pg.current() as i32);
    win.set_gallery_mosaic_total_pages(pg.total_pages() as i32);
    win.set_gallery_mosaic_page_numbers(ModelRc::new(VecModel::from(
        pg.page_numbers().iter().map(|n| *n as i32).collect::<Vec<i32>>(),
    )));
    // Flush initial layout (no curtain pending — phase 1 settled)
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }

    // Flip to page 1 — this bumps flip-token inside MosaicView and starts the curtain
    assert!(pg.step(1), "must flip to page 1");
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

    // Mid-curtain: advance ~300ms (~18 ticks) → phase ~0.5, column stagger visible
    for _ in 0..18 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let mid = win.window().take_snapshot().expect("mid curtain snapshot");
    save_slice_png(mid.clone(), "/tmp/opencode/mosaic_curtain_mid.png");
    let mid_dark = count_cover_pixels(&mid);

    // Settled: advance past total curtain duration (600ms total → another 400ms)
    for _ in 0..25 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    let settled = win.window().take_snapshot().expect("settled snapshot");
    save_slice_png(settled.clone(), "/tmp/opencode/mosaic_curtain_settled.png");
    let settled_dark = count_cover_pixels(&settled);

    // Curtain contract: mid must show staggered covers (dark), settled must have none
    assert!(
        mid_dark > 800,
        "mid-curtain must show opaque covers (staggered wipe) — got {mid_dark} cover pixels, expected >800 (crossfade fallback would be ~0)"
    );
    assert!(
        settled_dark < 300,
        "settled must have no covers — got {settled_dark} cover pixels, expected <300"
    );
    assert!(
        mid_dark > settled_dark + 500,
        "mid vs settled must differ visibly — mid {mid_dark} vs settled {settled_dark}"
    );
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
    win.set_gallery_mosaic_tiles(ModelRc::new(VecModel::from(tiles0)));
    win.set_gallery_mosaic_current_page(pg.current() as i32);
    win.set_gallery_mosaic_total_pages(pg.total_pages() as i32);
    win.set_gallery_mosaic_page_numbers(ModelRc::new(VecModel::from(
        pg.page_numbers().iter().map(|n| *n as i32).collect::<Vec<i32>>(),
    )));
    for _ in 0..2 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
    }
    assert!(pg.step(1));
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
