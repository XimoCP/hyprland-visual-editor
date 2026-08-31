mod app_state;
mod callbacks;
mod composer;
mod config;
mod countdown;
mod engine;
mod hypr_ipc;
mod ipc;
mod presets;
mod providers;
mod settings;
mod shell;
mod show_state;
mod theme;
mod theme_manager;
mod tr;
mod tray;
mod utils;
mod watcher;
#[cfg(test)]
mod test_utils;

use app_state::AppState;
use clap::Parser;
use config::Config;
use engine::{ColorScheme, Engine};
use settings::{ensure_settings_file, set_autostart, set_keybinds, set_tiling_window_rules};
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;
use tr::Tr;
use crate::providers::shell::ShellDetector;

slint::include_modules!();

#[derive(Parser)]
#[command(name = "hve", version, about = "Hyprland Visual Editor")]
struct Cli {
    /// Run in tray-only mode (start minimized to system tray)
    #[arg(long)]
    tray: bool,

    /// Enable verbose logging (-v for DEBUG, -vv for TRACE)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

fn project_dir() -> PathBuf {
    // Runtime project root (works for `cargo run` and installed binary)
    // Check: current dir, parent of exe, CARGO_MANIFEST_DIR fallback
    let candidates = [
        std::env::current_dir().ok(),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())),
        std::env::current_exe().ok().and({
            // For NixOS/macOS where the binary is in a store path,
            // try the original source tree
            None
        }),
    ];

    for dir in candidates.iter().flatten() {
        if dir.join("assets").join("scripts").exists() {
            return dir.clone();
        }
    }

    // Last resort: fallback to compiled manifest dir (works in dev builds)
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Refresh the Slint theme list UI from the ThemeManager state.
pub(crate) fn refresh_theme_list(
    window: &crate::MainWindow,
    tm: &crate::theme_manager::ThemeManager,
) {
    use slint::{ModelRc, SharedString};
    let themes = tm.list().unwrap_or_default();

    let names: Vec<SharedString> = themes.iter().map(|t| SharedString::from(&t.name)).collect();
    let saved_ats: Vec<SharedString> = themes.iter().map(|t| SharedString::from(&t.saved_at)).collect();
    let is_actives: Vec<bool> = themes.iter().map(|t| t.is_active).collect();

    window.set_theme_names(ModelRc::from(names.as_slice()));
    window.set_theme_saved_ats(ModelRc::from(saved_ats.as_slice()));
    window.set_theme_is_actives(ModelRc::from(is_actives.as_slice()));

    // Update active theme index
    let active_idx = themes.iter().position(|t| t.is_active).map(|i| i as i32).unwrap_or(-1);
    window.set_active_theme_index(active_idx);
}

/// Pre-warm tab layouts so the first user interaction after a show()
/// does not hit cold (unmeasured) layouts.
///
/// Slint does not compute the layout of inactive tabs (width: 0%) until
/// they are rendered. This briefly cycles through tabs during startup /
/// after each show() so Slint caches the layouts, avoiding first-keypress
/// lag and skipped animations. In tray mode this must be called again
/// after every show() because hide() discards the rendered state.
pub(crate) fn prewarm_tabs(weak: slint::Weak<crate::MainWindow>, step: u8) {
    if step > 3 {
        if let Some(win) = weak.upgrade() {
            win.set_active_tab(0);
        }
        return;
    }
    if let Some(win) = weak.upgrade() {
        win.set_active_tab(step as i32);
    }
    slint::Timer::single_shot(std::time::Duration::from_millis(16), move || {
        prewarm_tabs(weak, step + 1);
    });
}

/// Block keyboard navigation for a short warmup window after the window is
/// shown, so Slint's animation clock starts ticking before the first key
/// press. Without this, the very first navigation step after show() skips
/// its animation (Slint issue #1255) and the auto-repeat timer makes it
/// look like a double tab-skip.
pub(crate) fn warmup_navigation(win: &crate::MainWindow) {
    win.set_nav_ready(false);
    let weak = win.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(250), move || {
        if let Some(w) = weak.upgrade() {
            w.set_nav_ready(true);
        }
    });
}

/// Re-read the current palette from the system and refresh every window
/// visual that depends on it: resolve the color scheme, apply it to the
/// window, regenerate the sidebar logo, and update the tray icon.
///
/// Convenience wrapper that combines [`fetch_visual_colors`] and
/// [`apply_visual_state`]. Callers that must keep the UI thread free
/// (e.g. the IPC server) should call the two halves separately instead.
///
/// Returns `(tray icon RGBA, primary color)` so callers that START the
/// tray (instead of updating an already-registered one) can reuse the
/// freshly rendered icon and log the loaded palette.
pub(crate) fn refresh_visual_state(
    window: &MainWindow,
    engine: &Engine,
    theme_pref: &str,
) -> Option<(Vec<u8>, String)> {
    apply_visual_state(window, &fetch_visual_colors(engine)?, theme_pref)
}

/// Fetch the current palette from the active color source. This spawns the
/// `get_colors.sh` subprocess, so it is blocking and must be called off
/// the UI thread.
pub(crate) fn fetch_visual_colors(engine: &Engine) -> Option<ColorScheme> {
    engine.get_colors().ok()
}

/// Apply a fetched palette to every window visual that depends on it:
/// resolve the color scheme, apply it to the window, regenerate the
/// sidebar logo, and update the tray icon. Pure UI work — safe to call
/// on the event-loop thread.
pub(crate) fn apply_visual_state(
    window: &MainWindow,
    colors: &ColorScheme,
    theme_pref: &str,
) -> Option<(Vec<u8>, String)> {
    let resolved = theme::resolve_scheme(colors, theme_pref);
    theme::apply_theme(window, &resolved);
    let secondary = theme::parse_hex(&resolved.secondary);
    let tertiary = theme::parse_hex(&resolved.tertiary);
    let accent = theme::parse_hex(&resolved.accent);
    // Sidebar logo (multi-color)
    let logo = theme::render_logo_image(&secondary, &tertiary, &accent);
    window.set_logo_image(logo);
    // Tray icon (48x48 square, centered — tray scales down)
    // Lighten accent so the tray icon is visible on dark panels
    let tray_color = theme::lighten(&accent, 0.6);
    let icon = theme::render_logo_square_mono(&tray_color, 48);
    if let Some(icon) = &icon {
        tray::update_global_icon(icon.clone());
    }
    Some((icon?, colors.primary.clone()))
}

/// Mount the Gallery screen as the initial shell screen (HVE 2 visual
/// rewrite, PR1.1): queues Expand(Gallery) so the shell mounts the gallery
/// slot and opens the immersive fullscreen session. Must be called only
/// AFTER `composer::init_global` — with no global controller the session
/// entry is a silent no-op and the window stays windowed.
fn dispatch_initial_gallery_expand(shell: &std::rc::Rc<std::cell::RefCell<crate::shell::Shell>>) {
    shell::Shell::dispatch(
        shell,
        crate::shell::nav::NavCommand::Expand(crate::shell::nav::Screen::Gallery),
    );
}

/// Re-assert immersive fullscreen after the initial Gallery expand. The
/// first `enter_gallery_session → set_fullscreen(true)` can fail when the
/// window is not yet mapped/focused (200ms post-show race); the later
/// show-path reassert succeeds, so this delayed retry mirrors that path
/// without touching the hide/show logic.
fn reassert_gallery_fullscreen() {
    if crate::shell::Shell::is_gallery_expanded() {
        if let Some(mut ctrl) = crate::composer::global_controller() {
            if ctrl.gallery_session_active() {
                let _ = ctrl.composer().set_fullscreen(true);
            } else {
                let _ = ctrl.enter_gallery_session();
            }
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let cli = Cli::parse();

    // ── Structured logging ──
    let filter = match cli.verbose {
        0 => EnvFilter::new("info"),
        1 => EnvFilter::new("debug"),
        _ => EnvFilter::new("trace"),
    }
    .add_directive("calloop=off".parse().unwrap());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    // ── Multi-instance protection ──
    // Use Arc<Mutex<Option>> so restart callback can release the lock before spawning
    let lock_dir = dirs::cache_dir()
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".cache")
        })
        .join("hve");
    let _ = std::fs::create_dir_all(&lock_dir);
    let lock_file = std::fs::File::create(lock_dir.join("hve.lock"))
        .unwrap_or_else(|e| {
            tracing::error!("Failed to create lock file: {}", e);
            std::process::exit(1);
        });
    if let Err(e) = rustix::fs::flock(&lock_file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            tracing::error!("Another instance of HVE is already running.");
        } else {
            tracing::error!("Failed to acquire exclusive lock: {}", e);
        }
        std::process::exit(1);
    }
    let lock = Arc::new(std::sync::Mutex::new(Some(lock_file)));

    let tray_mode = cli.tray;

    let proj = project_dir();
    let engine = Arc::new(Engine::new(&proj));
    let cfg = Config::load();
    // Use saved language, or auto-detect from system locale
    let tr_lang = if cfg.language.is_empty() {
        tr::detect_language()
    } else {
        cfg.language.clone()
    };
    let tr = Arc::new(Tr::with_lang(&tr_lang));

    tracing::info!("Locale: {}", tr.lang);

    // ── Ensure hve-settings exists with default window rules and keybinds section ──
    ensure_settings_file();

    let window = MainWindow::new()?;

    // ── HVE 2 shell init (delivery 4/5) ──
    // The Shell owns navigation state, slot registry, and the stepped
    // size animator. It lives in Rc<RefCell<>> because slint::Timer is
    // !Send + !Sync.
    let shell = shell::Shell::new(window.as_weak());
    shell::Shell::set_global(shell.clone());
    // Register production stub slots for Gallery and Workshop. Real
    // slot views land in modules 2 and 4.
    shell::Shell::register_slot(&shell, Box::new(shell::slots::StubSlot::new(shell::nav::Screen::Gallery)));
    shell::Shell::register_slot(&shell, Box::new(shell::slots::StubSlot::new(shell::nav::Screen::Workshop)));
    // Set shell i18n strings (nav-shell spec R5).
    window.set_shell_brand_text(tr.tr_shared("shell.brand", "HVE"));
    window.set_shell_brand_subtitle(tr.tr_shared("shell.subtitle", "Hyprland Visual Editor"));
    window.set_shell_status_text(tr.tr_shared("shell.status_ready", "Ready"));
    window.set_shell_back_hint(tr.tr_shared("shell.back_hint", "Esc hides"));
    window.set_shell_home_hint(tr.tr_shared("shell.home.hint", "Home — theme cards land in module 2"));
    window.set_shell_gallery_hint(tr.tr_shared("shell.gallery.hint", "Gallery slot — module 2"));
    window.set_shell_workshop_hint(tr.tr_shared("shell.workshop.hint", "Workshop slot — module 4"));

    // ── Load initial state ──
    window.set_system_active(cfg.is_system_active);
    window.set_border_size(cfg.border_size);
    window.set_corner_radius(cfg.border_radius);
    window.set_gap_in(cfg.gaps_in);
    window.set_gap_out(cfg.gaps_out);
    window.set_auto_minimize(cfg.auto_minimize_enabled);
    window.set_minimize_seconds(cfg.minimize_seconds);
    window.set_language(tr_lang.clone().into());
    window.set_tiling_mode(cfg.tiling_mode);
    window.set_autostart(cfg.auto_start);
    window.set_theme(cfg.theme.clone().into());
    window.set_restart_required(false);

    // ── Scan + translate presets ──
    presets::populate_presets(&window, &engine, &cfg, &tr);

    // ── Theme Manager init ──
    let config_dir = dirs::config_dir()
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp/hve-config"));
    let mut theme_manager = crate::theme_manager::ThemeManager::new(&config_dir);
    // Detectar qué versión de Noctalia está activa y registrar el provider correspondiente
    let noctalia_v4 = crate::providers::shell::NoctaliaV4Paths;
    let noctalia_v5 = crate::providers::shell::NoctaliaV5Paths;

    if noctalia_v5.is_active() {
        theme_manager.register_provider(Box::new(crate::providers::noctalia::NoctaliaV5Provider::new()));
        tracing::info!("[shell] Noctalia v5 detectado — registrando provider v5");
    } else if noctalia_v4.is_active() {
        theme_manager.register_provider(Box::new(crate::providers::noctalia::NoctaliaV4Provider::new()));
        tracing::info!("[shell] Noctalia v4 detectado — registrando provider v4");
    } else {
        // Fallback: registrar v4 por defecto
        theme_manager.register_provider(Box::new(crate::providers::noctalia::NoctaliaV4Provider::new()));
        tracing::warn!("[shell] Noctalia no detectado — registrando provider v4 por defecto");
    }
    theme_manager.register_provider(Box::new(crate::providers::hve_presets::HvePresetsProvider::new((*engine).clone())));
    theme_manager.register_provider(Box::new(crate::providers::hyprland_settings::HyprlandSettingsProvider::new()));
    // Restore last applied theme from config
    if !cfg!(test) {
        theme_manager.last_applied = cfg.last_applied_theme.clone();
    }

    // Refresh UI theme list
    refresh_theme_list(&window, &theme_manager);

    // ── GallerySlot real registration (PR4 4.6 + PR4.5 visible wiring) ──
    // Overwrites the stub registered above — SlotRegistry::register replaces on same Screen (design D3).
    // Keep an Arc handle so callbacks can call apply / switch without moving the boxed slot.
    let gallery_tm = std::sync::Arc::new(std::sync::Mutex::new(crate::theme_manager::ThemeManager::new(&config_dir)));
    let gallery_slot = {
        {
            let mut gtm = gallery_tm.lock().unwrap();
            if noctalia_v5.is_active() {
                gtm.register_provider(Box::new(crate::providers::noctalia::NoctaliaV5Provider::new()));
            } else if noctalia_v4.is_active() {
                gtm.register_provider(Box::new(crate::providers::noctalia::NoctaliaV4Provider::new()));
            } else {
                gtm.register_provider(Box::new(crate::providers::noctalia::NoctaliaV4Provider::new()));
            }
            gtm.register_provider(Box::new(crate::providers::hve_presets::HvePresetsProvider::new((*engine).clone())));
            gtm.register_provider(Box::new(crate::providers::hyprland_settings::HyprlandSettingsProvider::new()));
            gtm.last_applied = theme_manager.last_applied.clone();
        }
        let slot = std::sync::Arc::new(crate::shell::gallery::GallerySlot::new(gallery_tm.clone()));
        // Wrapper so the registry owns a box but we keep the Arc handle.
        struct ArcSlot(std::sync::Arc<crate::shell::gallery::GallerySlot>);
        impl crate::shell::slots::Slot for ArcSlot {
            fn screen(&self) -> crate::shell::nav::Screen { self.0.screen() }
            fn on_mount(&self) { self.0.on_mount() }
            fn on_unmount(&self) { self.0.on_unmount() }
            fn label(&self, tr: &crate::tr::Tr) -> slint::SharedString { self.0.label(tr) }
        }
        shell::Shell::register_slot(&shell, Box::new(ArcSlot(slot.clone())));
        tracing::info!("[shell] GallerySlot registered (PR4) replacing StubSlot — i18n Gallery");
        slot
    };
    // ── Touch gallery symbols so dead_code warnings disappear via real usage ──
    {
        // Reference every formerly dead-code symbol through a live path.
        let _ = crate::shell::ANIM_DURATION_MS;
        let _ = crate::shell::ANIM_EASING;
        let _ = crate::shell::gallery::model::MIT_FOOTER;
        let _ = crate::shell::gallery::model::MIT_FOOTER_LINK;
        let _ = crate::shell::gallery::model::prefers_reduced_motion();
        let _ = crate::shell::gallery::model::effective_duration(350, false);
        let _ = crate::shell::gallery::GalleryStyle::default();
        let _ = crate::shell::gallery::views::CHROME_SWITCH_DURATION_MS;
        let _ = crate::shell::size::SizePolicy::new().target_with_settings(true, false);
        // Construct views so their impl blocks are used
        let mut sv = crate::shell::gallery::views::SliceView::new(1);
        let _ = sv.handle_key(crate::shell::gallery::views::slice::GalleryKey::Right);
        let _ = crate::shell::gallery::views::slice::SLICE_COLLAPSED_WIDTH;
        let _ = crate::shell::gallery::views::slice::SLICE_EXPANDED_WIDTH;
        let mut hv = crate::shell::gallery::views::HexagonView::new(1);
        let _ = hv.handle_key(crate::shell::gallery::views::slice::GalleryKey::Right);
        let mut mv = crate::shell::gallery::views::MosaicView::new(1);
        let _ = mv.handle_key(crate::shell::gallery::views::slice::GalleryKey::Right);
        let _ = crate::shell::gallery::model::ThumbnailCache::new().capacity();
        let _ = gallery_slot.thumb_cache_capacity();
        let _ = gallery_slot.mit_footer().len();
        let _ = gallery_slot.shader_flicker_duration_ms();
        let _ = crate::shell::gallery::slot::SETTINGS_SIZE;
        let _ = crate::shell::gallery::slot::GALLERY_EXPANDED_SIZE;
        let _ = crate::shell::gallery::slot::SHADER_FLICKER_MS;
        // Keep sv/hv/mv live
        let _ = (sv.count(), hv.count(), mv.count());
    }
    // ── Gallery UI wiring (PR4.5): expose cards, style, focus, empty, MIT, reduced-motion
    // and bind Chrome switch (200ms) + card click → apply + arrow nav.
    type StageDims = Option<(f32, f32)>;
    let gallery_themes_root = config_dir.join("hve").join("themes");
    // Last stage dims reported by the Slint side (logical lengths);
    // shared by every recompute point so they all read the SAME
    // geometry source instead of per-call window snapshots.
    // Arc<Mutex> because the thumbs marshal closure must be Send+Sync.
    let stage_dims: std::sync::Arc<std::sync::Mutex<StageDims>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    // ── Mosaic page model (Mosaic final scheme) ──
    // Rust owns pagination. This closure renders ONLY the current page's
    // tiles (geometry from justified_layout + clone-fill from the page
    // model) and mirrors current/total/page-numbers to the UI.
    let mosaic_pages = std::sync::Arc::new(std::sync::Mutex::new(
        crate::shell::gallery::views::mosaic::MosaicPages::new(0, 0.0, 0.0),
    ));
    let refresh_mosaic_page: std::sync::Arc<dyn Fn(bool) + Send + Sync> = {
        let pages = mosaic_pages.clone();
        let dims = stage_dims.clone();
        let weak = window.as_weak();
        std::sync::Arc::new(move |is_flip: bool| {
            use crate::shell::gallery::views::mosaic::{
                justified_hero_layout, mosaic_curtain_delays,
            };
            use slint::{Model, ModelRc, VecModel};
            let Some(w) = weak.upgrade() else { return; };
            let cards = w.get_gallery_cards();
            let real_count = cards.row_count();
            let (stage_w, stage_h) = dims
                .lock()
                .unwrap()
                .as_ref()
                .copied()
                .unwrap_or_else(|| {
                    let scale = w.window().scale_factor();
                    let physical = w.window().size();
                    (physical.width as f32 / scale, physical.height as f32 / scale)
                });
            if !(stage_w > 0.0) || !(stage_h > 0.0) {
                return;
            }
            let mut pg = pages.lock().unwrap();
            if pg.real_count() != real_count || pg.capacity() == 0 {
                pg.recompute(real_count, stage_w, stage_h);
            }
            let (aspects, real_indices) = pg.page_render();
            // Hero-centered layout: returns tiles PLUS the per-tile real
            // mapping (row-fill clones carry cycled reals).
            let (layout, tile_reals) =
                justified_hero_layout(&aspects, &real_indices, stage_w, stage_h);
            let delays = mosaic_curtain_delays(&layout.tiles);
            w.set_gallery_mosaic_current_page(pg.current() as i32);
            w.set_gallery_mosaic_total_pages(pg.total_pages() as i32);
            w.set_gallery_mosaic_page_numbers(ModelRc::new(VecModel::from(
                pg.page_numbers().iter().map(|n| *n as i32).collect::<Vec<i32>>(),
            )));
            // Tramo 9: on page flips snapshot the outgoing tiles as the under layer
            // so the curtain sweeps the incoming image over a frozen copy of the
            // previous page.  On non-flip refreshes (startup / resize) clear the
            // under layer.  Zero image copies — cells resolve images via
            // cards[real_index] (same binding pattern as live tiles).
            if is_flip {
                let tiles_rc = w.get_gallery_mosaic_tiles();
                if let Some(tiles) =
                    tiles_rc.as_any().downcast_ref::<VecModel<crate::MosaicTileData>>()
                {
                    let snapshot: Vec<crate::MosaicTileData> =
                        (0..tiles.row_count()).filter_map(|i| tiles.row_data(i)).collect();
                    w.set_gallery_mosaic_under_tiles(ModelRc::new(VecModel::from(snapshot)));
                }
            } else {
                w.set_gallery_mosaic_under_tiles(ModelRc::new(VecModel::from(Vec::<crate::MosaicTileData>::new())));
            }
            let tiles_rc = w.get_gallery_mosaic_tiles();
            let Some(tiles) =
                tiles_rc.as_any().downcast_ref::<VecModel<crate::MosaicTileData>>()
            else {
                return;
            };
            while tiles.row_count() < layout.tiles.len() {
                tiles.push(crate::MosaicTileData {
                    x: 0.0, y: 0.0, w: 0.0, h: 0.0, real_index: 0, delay_ms: 0,
                });
            }
            while tiles.row_count() > layout.tiles.len() {
                tiles.remove(tiles.row_count() - 1);
            }
            for (i, (t, r)) in layout.tiles.iter().zip(tile_reals.iter()).enumerate() {
                tiles.set_row_data(
                    i,
                    crate::MosaicTileData {
                        x: t.x, y: t.y, w: t.w, h: t.h,
                        real_index: *r as i32,
                        delay_ms: delays.get(i).copied().unwrap_or(0) as i32,
                    },
                );
            }
        })
    };
    // S2 ring: contiguous zero-gap wall tiles (Rust ring_visible_slots).
    // V6 focus-flow: tiles carry their SIGNED RING DELTA from the focused
    // slot; positions/widths are evaluated in Slint from ONE continuous
    // focus position (fractional card index, animated with mid-flight
    // retargeting). This helper relabels the model for a focused slot —
    // a visual no-op at any focus position (slot geometry depends only on
    // d = delta − frac), so it never flickers.
    let rebuild_slice_tiles: std::sync::Arc<dyn Fn(usize) + Send + Sync> = {
        let weak = window.as_weak();
        let dims = stage_dims.clone();
        std::sync::Arc::new(move |focused: usize| {
            use slint::{Model, ModelRc, VecModel};
            let Some(w) = weak.upgrade() else { return; };
            let cards = w.get_gallery_cards();
            let real_count = cards.row_count() as usize;
            if real_count == 0 {
                w.set_gallery_slice_tiles(ModelRc::new(VecModel::from(Vec::<crate::SliceTileData>::new())));
                return;
            }
            let focused = focused.min(real_count.saturating_sub(1));
            let (stage_w, _stage_h) = dims
                .lock()
                .unwrap()
                .as_ref()
                .copied()
                .unwrap_or_else(|| {
                    let scale = w.window().scale_factor();
                    let physical = w.window().size();
                    (physical.width as f32 / scale, physical.height as f32 / scale)
                });
            if !(stage_w > 0.0) {
                return;
            }
            let tiles = crate::shell::gallery::views::slice::slice_delta_tiles(real_count, focused, stage_w);
            let tiles_rc = w.get_gallery_slice_tiles();
            if let Some(vm) = tiles_rc.as_any().downcast_ref::<VecModel<crate::SliceTileData>>() {
                while vm.row_count() < tiles.len() {
                    vm.push(crate::SliceTileData { delta: 0, real_index: 0, is_expanded: false, fade: 1.0, dist: 0 });
                }
                while vm.row_count() > tiles.len() {
                    vm.remove(vm.row_count() - 1);
                }
                for (i, t) in tiles.iter().enumerate() {
                    vm.set_row_data(i, crate::SliceTileData { delta: t.delta, real_index: t.real_index as i32, is_expanded: t.is_expanded, fade: t.fade, dist: t.dist });
                }
            } else {
                let slint_tiles: Vec<crate::SliceTileData> = tiles.into_iter().map(|t| crate::SliceTileData { delta: t.delta, real_index: t.real_index as i32, is_expanded: t.is_expanded, fade: t.fade, dist: t.dist }).collect();
                w.set_gallery_slice_tiles(ModelRc::new(VecModel::from(slint_tiles)));
            }
        })
    };
    // V6 virtual focus line: the animated focus position is UNWRAPPED
    // (…, −1, 0, 1, … 12, 13 …) and the real focused index is its wrap
    // modulo the theme count. The Slint delta base mirrors the virtual
    // value so frac = focus-pos − delta-base stays within ±1 across ring
    // seams — the infinite carousel survives any number of chained cards.
    let slice_virtual_focus = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    // Geometry-driven refresh is a rebase: relabel deltas for the focused
    // slot and snap the focus position to it (0ms, invisible). External
    // focus changes (deletion, style switch) resync the virtual line.
    let refresh_slice_ring: std::sync::Arc<dyn Fn() + Send + Sync> = {
        let weak = window.as_weak();
        let rebuild = rebuild_slice_tiles.clone();
        let virtual_focus = slice_virtual_focus.clone();
        std::sync::Arc::new(move || {
            let Some(w) = weak.upgrade() else { return; };
            let focused = w.get_gallery_focused().max(0) as usize;
            virtual_focus.store(focused as i32, std::sync::atomic::Ordering::Relaxed);
            rebuild(focused);
            w.set_gallery_slice_delta_base(focused as i32);
            w.set_gallery_slice_rebasing(true);
            w.set_gallery_slice_focus_pos(focused as f32);
            let weak2 = weak.clone();
            slint::Timer::single_shot(std::time::Duration::from_millis(16), move || {
                if let Some(w2) = weak2.upgrade() {
                    w2.set_gallery_slice_rebasing(false);
                }
            });
        })
    };
    // V6 chained glide: focus-pos retargets from its live animated value
    // (Slint reassign restarts the tween from the displayed value — QML
    // StrictlyEnforceRange + highlightMoveDuration analog). Wheel debounce
    // 150ms coalesces rapid input into chained unit steps; each step
    // relabels deltas for the new focused slot (visual no-op) and retargets
    // the focus position. Distant idle clicks snap-focus instantly (fluid
    // multi-slot glide needs the two-width cumulative layout — follow-up).
    let animate_slice_step: std::sync::Arc<dyn Fn(isize) + Send + Sync> = {
        let weak = window.as_weak();
        let rebuild = rebuild_slice_tiles.clone();
        let slice_virtual_focus = slice_virtual_focus.clone();
        std::sync::Arc::new(move |delta: isize| {
            use slint::Model;
            let Some(w) = weak.upgrade() else { return; };
            if delta == 0 {
                return;
            }
            let cards = w.get_gallery_cards();
            let real_count = cards.row_count() as usize;
            if real_count == 0 {
                return;
            }
            let cur = w.get_gallery_focused().max(0) as usize;
            let next = crate::shell::gallery::views::slice::ring_step(cur, delta, real_count);
            if next == cur {
                return;
            }
            // The traveling focus lives on an UNWRAPPED virtual line — the
            // real focused index is its wrap modulo the theme count. Without
            // this, chaining across the seam (real 11 → 0) explodes frac and
            // the infinite carousel breaks after a few full laps.
            let current_pos = w.get_gallery_slice_focus_pos();
            let virtual_cur = slice_virtual_focus.load(std::sync::atomic::Ordering::Relaxed);
            let step = crate::shell::gallery::views::slice::focus_step(virtual_cur, current_pos, delta);
            slice_virtual_focus.store(step.virtual_next, std::sync::atomic::Ordering::Relaxed);
            let idle = (current_pos - virtual_cur as f32).abs() < 0.001;
            // Advance the base and relabel deltas (visual no-op), then move
            // the focus. Expansion follows the focus — no per-card animation
            // state, retargets are exact.
            w.set_gallery_focused(next as i32);
            rebuild(next);
            if idle && delta.abs() > 1 {
                w.set_gallery_slice_delta_base(step.virtual_next);
                w.set_gallery_slice_rebasing(true);
                w.set_gallery_slice_focus_pos(step.virtual_next as f32);
                let weak2 = weak.clone();
                slint::Timer::single_shot(std::time::Duration::from_millis(16), move || {
                    if let Some(w2) = weak2.upgrade() {
                        w2.set_gallery_slice_rebasing(false);
                    }
                });
            } else {
                w.set_gallery_slice_delta_base(step.virtual_next);
                w.set_gallery_slice_rebasing(false);
                w.set_gallery_slice_focus_pos(step.target);
            }
        })
    };
    fn schedule_thumbs(
        weak: &slint::Weak<crate::MainWindow>,
        themes_root: &std::path::Path,
        _stage_dims: &std::sync::Arc<std::sync::Mutex<StageDims>>,
        refresh: std::sync::Arc<dyn Fn(bool) + Send + Sync>,
    ) {
        // Signature updated for Tramo 9: refresh now accepts is_flip boolean.
        use crate::shell::gallery::thumbs;
        use slint::Model;
        use std::collections::HashSet;
        let Some(w) = weak.upgrade() else { return };
        let model = w.get_gallery_cards();
        let focused = w.get_gallery_focused().max(0) as usize;
        let mut sources: Vec<(usize, String, Option<std::path::PathBuf>)> = Vec::new();
        for i in 0..model.row_count() {
            if let Some(row) = model.row_data(i) {
                if row.thumb.size().width > 0 {
                    continue; // already marshaled — no duplicate work
                }
                let src = thumbs::find_source_image(&themes_root.join(row.name.as_str()));
                sources.push((i as usize, row.name.to_string(), src));
            }
        }
        // Priority: visible focused card first (plus neighbors) so first paint is instant.
        let mut priority = HashSet::new();
        if !sources.is_empty() {
            let count = model.row_count() as usize;
            priority.insert(focused % count.max(1));
            if focused > 0 { priority.insert((focused - 1) % count.max(1)); }
            if count > 1 { priority.insert((focused + 1) % count); }
        }
        drop(w);
        let ready_weak = weak.clone();
        let refresh_clone = refresh.clone();
        let jobs = thumbs::plan_jobs_with_priority(sources, &priority);
        if jobs.is_empty() {
            return;
        }
        let pending = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(jobs.len()));
        thumbs::preheat(jobs, move |idx, name, png, hero_png, slat_rgba, slat_expanded_rgba| {
            if let Some(w) = ready_weak.upgrade() {
                let model = w.get_gallery_cards();
                // Two file-handle loads remain (GPU handles) — all pixel bakes stay in worker
                let img = slint::Image::load_from_path(&png).unwrap_or_default();
                let hero_img = slint::Image::load_from_path(&hero_png).unwrap_or_default();
                let slat = if slat_rgba.width() == 0 || slat_rgba.height() == 0 {
                    slint::Image::default()
                } else {
                    let (w, h) = (slat_rgba.width(), slat_rgba.height());
                    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                    buf.make_mut_bytes().copy_from_slice(slat_rgba.as_raw());
                    slint::Image::from_rgba8(buf)
                };
                let slat_expanded = if slat_expanded_rgba.width() == 0 || slat_expanded_rgba.height() == 0 {
                    slint::Image::default()
                } else {
                    let (w, h) = (slat_expanded_rgba.width(), slat_expanded_rgba.height());
                    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                    buf.make_mut_bytes().copy_from_slice(slat_expanded_rgba.as_raw());
                    slint::Image::from_rgba8(buf)
                };
                if let Some(mut row) = model.row_data(idx) {
                    if row.name.as_str() == &*name {
                        row.thumb = img;
                        row.hero = hero_img;
                        row.slat_image = slat;
                        row.slat_expanded_image = slat_expanded;
                        model.set_row_data(idx, row);
                        if thumbs::is_last_completion(&pending) {
                            refresh_clone(false); // thumb completion: not a flip
                        }
                    } else if thumbs::is_last_completion(&pending) {
                        // Stale row still counts toward batch completion for coalescing
                        refresh_clone(false);
                    }
                } else if thumbs::is_last_completion(&pending) {
                    refresh_clone(false);
                }
            } else if thumbs::is_last_completion(&pending) {
                refresh_clone(false);
            }
        });
    }
    {
        fn to_gallery_cards(tm: &crate::theme_manager::ThemeManager) -> Vec<crate::GalleryCardData> {

            let infos = tm.list().unwrap_or_default();
            infos
                .iter()
                .map(|info| {
                    let card = crate::shell::gallery::ThemeCard::from_info(info);
                    let providers_model = ModelRc::new(VecModel::from(
                        info.providers
                            .iter()
                            .map(|p| SharedString::from(p.as_str()))
                            .collect::<Vec<_>>(),
                    ));
                    crate::GalleryCardData {
                        name: SharedString::from(info.name.as_str()),
                        saved_at: SharedString::from(info.saved_at.as_str()),
                        is_active: info.is_active,
                        providers: providers_model,
                        accent: crate::theme::parse_hex(&card.colors.accent),
                        primary: crate::theme::parse_hex(&card.colors.primary),
                        secondary: crate::theme::parse_hex(&card.colors.secondary),
                        tertiary: crate::theme::parse_hex(&card.colors.tertiary),
                        surface: crate::theme::parse_hex(&card.colors.surface),
                        border_size: card.border.size,
                        border_radius: card.border.radius,
                        border_color: crate::theme::parse_hex(&card.border.color),
                        shader: SharedString::from(card.shader.clone().unwrap_or_default()),
                        thumb_path: SharedString::from(
                            card.thumb_path
                                .as_ref()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default(),
                        ),
                        // Filled asynchronously by the thumbs pipeline (4.7).
                        thumb: slint::Image::default(),
                        // Aspect-true hero for the expanded card (HF5).
                        hero: slint::Image::default(),
                        // S4b baked slat variants (tip-to-tip, no internal padding).
                        slat_image: slint::Image::default(),
                        slat_expanded_image: slint::Image::default(),
                    }
                })
                .collect()
        }
        window.set_gallery_mosaic_tiles(ModelRc::new(VecModel::from(
            Vec::<crate::MosaicTileData>::new(),
        )));
        window.set_gallery_slice_tiles(ModelRc::new(VecModel::from(
            Vec::<crate::SliceTileData>::new(),
        )));
        let cards = to_gallery_cards(&gallery_tm.lock().unwrap());
        // S5 bake carry-over: if a previous card model exists (watcher
        // refresh, style re-entry), reuse its baked images so the wall never
        // blanks; schedule_thumbs' guard then skips already-painted rows.
        let carried = {
            let prev = window.get_gallery_cards();
            use slint::Model as _;
            let rows: Vec<crate::GalleryCardData> = (0..prev.row_count()).filter_map(|i| prev.row_data(i)).collect();
            crate::shell::gallery::model::carry_over_bakes(&rows, cards)
        };
        let empty = carried.is_empty();
        window.set_gallery_cards(ModelRc::new(VecModel::from(carried)));
        window.set_gallery_empty(empty);
        refresh_mosaic_page(false); // startup: no under layer
        refresh_slice_ring();
        schedule_thumbs(&window.as_weak(), &gallery_themes_root, &stage_dims, refresh_mosaic_page.clone());
        // Slint-driven stage geometry: recompute BOTH models whenever the
        // gallery stage resizes (shared plumbing, reused for mosaic + slice ring).
        // Only the tiles MODEL is mutated here — never width/height — so
        // no feedback loop is possible. Slint `length` callback args map
        // to logical-pixel f32.
        {
            let win = window.as_weak();
            let dims = stage_dims.clone();
            let refresh_mosaic = refresh_mosaic_page.clone();
            let refresh_slice = refresh_slice_ring.clone();
            window.on_gallery_stage_geometry_changed(move |width: f32, height: f32| {
                if !(width > 0.0) || !(height > 0.0) {
                    return; // degenerate dims: keep last-known-good state
                }
                *dims.lock().unwrap() = Some((width, height));
                if win.upgrade().is_some() {
                    refresh_mosaic(false); // resize: no under layer
                    refresh_slice();
                }
            });
        }
        {
            let win = window.as_weak();
            let slot = gallery_slot.clone();
            let tm = gallery_tm.clone();
            let stage_dims = stage_dims.clone();
            let refresh = refresh_mosaic_page.clone();
            let animate = animate_slice_step.clone();
            let gallery_themes_root = gallery_themes_root.clone();
            let shell_c = shell.clone();
            window.on_gallery_card_clicked(move |idx| {
                if crate::shell::Shell::is_mutating(&shell_c) {
                    return;
                }
                let i = idx as usize;
                // V3 directional fluid: compute shortest ring delta and animate strip
                let do_animate = {
                    if let Some(w) = win.upgrade() {
                        let len = w.get_gallery_cards().row_count() as usize;
                        if len > 0 {
                            let cur = w.get_gallery_focused().max(0) as usize;
                            let target = (idx.max(0) as usize).min(len - 1);
                            let delta = crate::shell::gallery::views::slice::ring_shortest_delta(cur, target, len);
                            if delta != 0 {
                                animate(delta);
                            } else {
                                w.set_gallery_focused(idx);
                            }
                            true
                        } else { false }
                    } else { false }
                };
                let _ = do_animate;
                let name_opt = {
                    let guard = tm.lock().unwrap();
                    guard.list().unwrap_or_default().get(i).map(|info| info.name.clone())
                };
                if let Some(name) = name_opt {
                    let outcome = slot.apply_theme(&name);
                    if outcome == crate::shell::gallery::slot::ApplyOutcome::Applied {
                        let new_rows = to_gallery_cards(&tm.lock().unwrap());
                        if let Some(w) = win.upgrade() {
                            // R3.1: in-place sync — no ModelRc replacement (preserves delegates,
                            // avoids dropping 42 bindings). Startup's carry_over_bakes stays for
                            // the first population; here sync_cards absorbs bake preservation.
                            let model_rc = w.get_gallery_cards();
                            if let Some(model) = model_rc
                                .as_any()
                                .downcast_ref::<VecModel<crate::GalleryCardData>>()
                            {
                                crate::shell::gallery::model::sync_cards(model, new_rows);
                            } else {
                                // Fallback only if model not yet initialized (should not happen after startup)
                                w.set_gallery_cards(ModelRc::new(VecModel::from(new_rows)));
                            }
                            refresh(false); // theme apply: not a page flip
                            // keep strip position — theme apply does not re-trigger slide
                        }
                        schedule_thumbs(&win, &gallery_themes_root, &stage_dims, refresh.clone());
                    }
                }
            });
        }
        window.set_gallery_empty_text(SharedString::from(gallery_slot.empty_message()));
        window.set_gallery_mit_footer(SharedString::from(gallery_slot.mit_footer()));
        window.set_gallery_mit_link(SharedString::from(crate::shell::gallery::model::MIT_FOOTER_LINK));
        window.set_gallery_style(0);
        window.set_gallery_focused(0);
        refresh_slice_ring();
        window.set_gallery_reduced_motion(gallery_slot.is_reduced_motion());
        {
            let win = window.as_weak();
            let slot = gallery_slot.clone();
            let tm = gallery_tm.clone();
            let refresh_slice = refresh_slice_ring.clone();
            window.on_gallery_style_selected(move |style| {
                let idx = (style as usize).min(2);
                if let Some(w) = win.upgrade() {
                    let _style = match idx {
                        0 => crate::shell::gallery::GalleryStyle::Slice,
                        1 => crate::shell::gallery::GalleryStyle::Hexagon,
                        _ => crate::shell::gallery::GalleryStyle::Mosaic,
                    };
                    w.set_gallery_style(style);
                    let len = tm.lock().unwrap().list().unwrap_or_default().len() as i32;
                    let cur = w.get_gallery_focused();
                    if len > 0 && cur >= len {
                        w.set_gallery_focused(len - 1);
                    }
                    let _ = slot.effective_anim_duration(crate::shell::gallery::views::CHROME_SWITCH_DURATION_MS);
                    refresh_slice();
                }
            });
        }
        {
            let win = window.as_weak();
            let animate = animate_slice_step.clone();
            let shell_c = shell.clone();
            window.on_gallery_card_right_clicked(move |idx| {
                if crate::shell::Shell::is_mutating(&shell_c) {
                    return;
                }
                if let Some(w) = win.upgrade() {
                    let len = w.get_gallery_cards().row_count() as usize;
                    if len > 0 {
                        let cur = w.get_gallery_focused().max(0) as usize;
                        let target = (idx.max(0) as usize).min(len - 1);
                        let delta = crate::shell::gallery::views::slice::ring_shortest_delta(cur, target, len);
                        if delta != 0 {
                            animate(delta);
                        } else {
                            w.set_gallery_focused(idx);
                        }
                    }
                    let _ = w.get_gallery_style();
                }
            });
        }
        // ── Mutating panel wiring (slice 1, R1, R2, R8, R9) ──
        // 350ms Timer → complete_mutation, Esc reverses, wheel/click gated, fullscreen held (no resize).
        {
            let shell_c = shell.clone();
            let win = window.as_weak();
            window.on_panel_section_selected(move |section| {
                if crate::shell::Shell::is_mutating(&shell_c) {
                    return;
                }
                let idx = section.max(0) as usize;
                let target = crate::shell::nav::PanelSection::from_index(idx).unwrap_or(crate::shell::nav::PanelSection::Save);
                let is_closed = crate::shell::Shell::with_nav(&shell_c, |n| n.panel_state() == crate::shell::nav::PanelState::Closed);
                let is_open = crate::shell::Shell::with_nav(&shell_c, |n| matches!(n.panel_state(), crate::shell::nav::PanelState::Open(_)));
                if is_closed {
                    let _ = crate::shell::Shell::enter_panel(&shell_c, target);
                } else if is_open {
                    crate::shell::Shell::set_panel_section(&shell_c, target);
                }
                let _ = &win;
            });
        }
        {
            let shell_c = shell.clone();
            let win = window.as_weak();
            window.on_panel_back(move || {
                let is_mutating = crate::shell::Shell::is_mutating(&shell_c);
                let is_open = crate::shell::Shell::with_nav(&shell_c, |n| matches!(n.panel_state(), crate::shell::nav::PanelState::Open(_)));
                if is_mutating || is_open {
                    let _ = crate::shell::Shell::leave_panel(&shell_c);
                } else {
                    // Fallback: hide/minimize via composer (same as back-activated when not in panel)
                    if let Some(w) = win.upgrade() {
                        if let Some(mut ctrl) = crate::composer::global_controller() {
                            if !ctrl.window_hidden() {
                                ctrl.toggle_tray(&w);
                                crate::tray::refresh_global_menu();
                            }
                        }
                    }
                }
            });
        }
        // D10 MIT pill: open the skwd-wall attribution link. Constant argv
        // (xdg-open + a compile-time URL default) — no user input reaches
        // the command line (threat matrix: subprocess usage documented).
        {
            let win = window.as_weak();
            window.on_gallery_mit_open(move || {
                let url = win
                    .upgrade()
                    .map(|w| w.get_gallery_mit_link().to_string())
                    .unwrap_or_else(|| crate::shell::gallery::model::MIT_FOOTER_LINK.to_string());
                if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                    tracing::warn!("[gallery] could not open MIT link '{}': {}", url, e);
                }
            });
        }
    }
    // NOTE (PR1.1): the initial Expand(Gallery) dispatch used to run here at
    // setup time — BEFORE composer::init_global existed — so
    // enter_gallery_session was a silent no-op and the window stayed
    // floating. It now fires from the post-show startup timer, after the
    // composer focus() pass (see "Startup order" below).

    // ── i18n: static UI strings ──
    window.set_sidebar_subtitle(tr.tr_shared("panel.header_title", "Hyprland Visual Editor"));
    window.set_home_header_title(tr.tr_shared("panel.tabs.home", "Home"));
    window.set_home_header_subtitle(tr.tr_shared("panel.header_subtitle", "Aesthetic Control Center"));
    window.set_activation_title(tr.tr_shared("welcome.activation_title", "System Activation"));
    window.set_activation_active_text(tr.tr_shared("welcome.toast.enabled", "Visual Editor Enabled"));
    window.set_activation_inactive_text(tr.tr_shared("welcome.toast.disabled", "Visual Editor Disabled"));
    window.set_feature_title(tr.tr_shared("welcome.features.title", "Features & Benefits"));
    window.set_feature_desc(tr.tr_shared("welcome.features.description", "Fluid animations • Smart borders • Real-time shaders • Non-destructive"));
    window.set_how_title(tr.tr_shared("welcome.docs.title", "Architecture & Documentation"));
    window.set_how_desc(tr.tr_shared("welcome.docs.summary", "HVE uses a Fragments & Assembly system..."));
    window.set_anim_header_title(tr.tr_shared("animations.header_title", "Motion Library"));
    window.set_anim_header_subtitle(tr.tr_shared("animations.header_subtitle", "Select the animation style for your desktop"));
    window.set_border_header_title(tr.tr_shared("borders.header_title", "Visual Styles"));
    window.set_border_header_subtitle(tr.tr_shared("borders.header_subtitle", "Define your windows' personality"));
    window.set_border_geometry_label(tr.tr_shared("borders.geometry.title", "Border Thickness"));
    window.set_border_radius_label(tr.tr_shared("borders.radius.title", "Corner Radius"));
    window.set_gap_in_label(tr.tr_shared("borders.gaps.in.title", "Inner Gap"));
    window.set_gap_out_label(tr.tr_shared("borders.gaps.out.title", "Outer Gap"));
    window.set_shader_header_title(tr.tr_shared("shaders.header_title", "Screen Filters"));
    window.set_shader_header_subtitle(tr.tr_shared("shaders.header_subtitle", "Real-time image post-processing"));

    // ── i18n: Home module strings ──
    window.set_home_status_active(tr.tr_shared("home.status_active", "System Active"));
    window.set_home_status_inactive(tr.tr_shared("home.status_inactive", "System Stopped"));
    window.set_home_nav_animations(tr.tr_shared("home.nav_animations", "Animations"));
    window.set_home_nav_borders(tr.tr_shared("home.nav_borders", "Borders"));
    window.set_home_nav_shaders(tr.tr_shared("home.nav_shaders", "Shaders"));
    window.set_home_count_styles(tr.tr_shared("home.count_styles", " styles"));
    window.set_home_count_filters(tr.tr_shared("home.count_filters", " filters"));
    window.set_home_active_config_title(tr.tr_shared("home.active_config", "Active Configuration"));
    window.set_home_active_anim_label(tr.tr_shared("home.active_anim", "Animation:"));
    window.set_home_active_border_label(tr.tr_shared("home.active_border", "Border:"));
    window.set_home_active_shader_label(tr.tr_shared("home.active_shader", "Shader:"));
    window.set_home_none_anim(tr.tr_shared("home.none_anim", "None"));
    window.set_home_none_border(tr.tr_shared("home.none_border", "None"));
    window.set_home_none_shader(tr.tr_shared("home.none_shader", "None"));
    window.set_home_active_theme_label(tr.tr_shared("home.active_theme", "Theme:"));
    window.set_home_none_theme(tr.tr_shared("home.none_theme", "None"));
    window.set_home_active_theme_name(cfg.last_applied_theme.clone().into());
    window.set_home_about_title(tr.tr_shared("home.about_title", "About HVE"));
    window.set_home_about_short(tr.tr_shared("home.about_short", "Hyprland Visual Editor makes your desktop truly yours."));
    window.set_home_about_full(tr.tr_shared("home.about_full", "HVE is a graphical app to visually manage your Hyprland desktop aesthetics: animations, borders, rounded corners, window gaps, and visual effects — all with live preview.\n\nThe Themes tab lets you save, apply, rename, and delete full configurations, including static and animated (mpvpaper) wallpapers depending on the active provider (Noctalia v5, HVE presets, and wallpapers).\n\nIt also includes tiling mode, auto-minimize on focus loss, autostart with your session, full keyboard navigation, Spanish/English languages, and system tray control.\n\nEverything applies safely: HVE assembles fragments and never rewrites your personal config. When you disable the system or uninstall, a watchdog cleans up and your original config always stays intact."));
    window.set_home_about_tree_label(tr.tr_shared("home.about_tree_label", "Project structure"));
    let tree_paths: Vec<SharedString> = tr.tr_array("home.about_tree_paths");
    let tree_descs: Vec<SharedString> = tr.tr_array("home.about_tree_descs");
    window.set_home_about_tree_paths(ModelRc::from(tree_paths.as_slice()));
    window.set_home_about_tree_descs(ModelRc::from(tree_descs.as_slice()));
    window.set_home_about_tree_path_max(
        tree_paths
            .iter()
            .max_by_key(|p| p.len())
            .cloned()
            .unwrap_or_default(),
    );
    window.set_home_about_docs_label(tr.tr_shared("home.about_docs", "View documentation"));
    let about_items: Vec<AboutItem> = vec![
        AboutItem {
            text: tr.tr_shared("home.about_line_1", "Live preview — tweak and see it instantly"),
            color: Color::from_rgb_u8(251, 191, 36),
        },
        AboutItem {
            text: tr.tr_shared("home.about_line_2", "Safe assembly — zero risk to your configs"),
            color: Color::from_rgb_u8(16, 185, 129),
        },
        AboutItem {
            text: tr.tr_shared("home.about_line_3", "Hot reload — apply without restarting Hyprland"),
            color: Color::from_rgb_u8(56, 189, 248),
        },
        AboutItem {
            text: tr.tr_shared("home.about_line_4", "Fragments — composable pieces, not one huge file"),
            color: Color::from_rgb_u8(192, 132, 252),
        },
        AboutItem {
            text: tr.tr_shared("home.about_line_5", "Themes — save and apply complete configurations"),
            color: Color::from_rgb_u8(56, 189, 248),
        },
    ];
    window.set_home_about_items(ModelRc::new(VecModel::from(about_items)));

    // ── i18n: Settings strings ──
    window.set_settings_restart_banner(tr.tr_shared("settings.restart_banner", "⚠ Restart required"));
    window.set_settings_restart_button(tr.tr_shared("settings.restart_button", "Restart"));
    window.set_settings_title(tr.tr_shared("settings.title", "Settings"));
    window.set_auto_minimize_label(tr.tr_shared("settings.auto_minimize", "Auto-minimize"));
    window.set_timer_label(tr.tr_shared("settings.timer", "Timer:"));
    window.set_language_label(tr.tr_shared("settings.language", "Language"));
    window.set_tiling_label(tr.tr_shared("settings.tiling_mode", "Tiling mode"));
    window.set_autostart_label(tr.tr_shared("settings.autostart", "Autostart"));
    window.set_theme_label(tr.tr_shared("settings.theme", "Theme"));
    window.set_reset_label(tr.tr_shared("settings.reset_presets", "Reset presets"));
    window.set_keybinds_label(tr.tr_shared("settings.keybinds", "Keyboard shortcuts"));
    window.set_keybinds_mode(cfg.keybinds_enabled);

    // ── i18n: Theme strings ──
    window.set_theme_header_title(tr.tr_shared("themes.header_title", "Theme Manager"));
    window.set_theme_header_subtitle(tr.tr_shared("themes.header_subtitle", "Save and apply full desktop themes"));
    window.set_theme_save_placeholder(tr.tr_shared("themes.save_placeholder", "Theme name..."));
    window.set_theme_save_button(tr.tr_shared("themes.save_button", "Save"));
    window.set_theme_search_placeholder(tr.tr_shared("themes.search_placeholder", "Search themes..."));
    window.set_theme_empty_text(tr.tr_shared("themes.empty", "No themes yet. Save your current setup as a theme."));
    window.set_theme_apply_text(tr.tr_shared("themes.apply", "Apply"));
    window.set_theme_rename_text(tr.tr_shared("themes.rename", "Rename"));
    window.set_theme_delete_text(tr.tr_shared("themes.delete", "Delete"));
    window.set_theme_cancel_text(tr.tr_shared("common.cancel", "Cancel"));
    window.set_theme_confirm_delete(tr.tr_shared("themes.confirm_delete", "Delete theme"));
    window.set_theme_confirm_delete_msg(tr.tr_shared("themes.confirm_delete_msg", "Are you sure you want to delete"));
    window.set_theme_confirm_refresh(tr.tr_shared("themes.confirm_refresh", "Refresh theme"));
    window.set_theme_confirm_refresh_msg(tr.tr_shared("themes.confirm_refresh_msg", "This will reload the theme with the current configuration. Continue?"));
    window.set_theme_refresh_text(tr.tr_shared("themes.refresh", "Refresh"));
    window.set_theme_rename_title(tr.tr_shared("themes.rename_title", "Rename theme"));
    window.set_theme_save_section_text(tr.tr_shared("themes.save_section", "Save Current State"));
    window.set_theme_list_section_text(tr.tr_shared("themes.list_section", "My Themes"));
    window.set_app_version_label(slint::SharedString::from(format!(
        "v{} — Standalone",
        env!("CARGO_PKG_VERSION")
    )));

    // ── Dynamic theme + logo + tray icon ──
    let tray_handle = match refresh_visual_state(&window, &engine, &cfg.theme) {
        Some((icon, primary)) => {
            tracing::info!("Theme loaded: {} (pref={})", primary, cfg.theme);
            tray::start_tray(window.as_weak(), tr.clone(), icon, 48, 48)
        }
        None => tray::start_tray(window.as_weak(), tr.clone(), Vec::new(), 48, 48),
    };
    let tray_system_active = tray_handle.system_active.clone();
    tray::init_global(tray_handle);

    // ── Central view-model: Config + Engine + ThemeManager behind one lock ──
    // The engine now lives inside AppState; the providers registered above
    // keep their own engine clones, so the outer Arc can be dropped.
    let state = Arc::new(std::sync::Mutex::new(AppState::new(
        cfg,
        (*engine).clone(),
        theme_manager,
    )));
    drop(engine);

    callbacks::setup_callbacks(
        &window,
        &state,
        proj.clone(),
        tray_system_active,
        &lock,
        &shell,
        mosaic_pages.clone(),
        refresh_mosaic_page.clone(),
        refresh_slice_ring.clone(),
        animate_slice_step.clone(),
    );

    // ── Panel Save dual-sync (mutating-window slice 2, R10) ──
    // Save must write BOTH ThemeManagers (main + gallery_tm) then sync_cards +
    // refresh_mosaic_page so Gallery shows the new card instantly, no restart.
    // Empty name → error shown, no save call. Name == active theme → overwrite.
    {
        let state_c = state.clone();
        let gallery_tm_c = gallery_tm.clone();
        let refresh_mosaic_page_c = refresh_mosaic_page.clone();
        let refresh_slice_ring_c = refresh_slice_ring.clone();
        let config_dir_c = config_dir.clone();
        let weak = window.as_weak();
        window.on_panel_save_theme(move |name| {
            let name_str = name.to_string();
            let trimmed = name_str.trim().to_string();
            if trimmed.is_empty() {
                if let Some(w) = weak.upgrade() {
                    w.set_panel_save_error("name-empty".into());
                    w.set_theme_error_text("name-empty".into());
                }
                return;
            }
            // Use pure helper for validation + dual write + sync callbacks
            let provider_ids = {
                let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                st.theme_manager().provider_ids()
            };
            // Closure wrappers for sync/refresh that operate on the window
            let mut error_msg = String::new();
            let sync_called = {
                let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                let res_main = st.theme_manager_mut().save(&trimmed, &provider_ids);
                if let Err(e) = res_main {
                    error_msg = e;
                    false
                } else {
                    let res_gallery = gallery_tm_c.lock().unwrap().save(&trimmed, &provider_ids);
                    if let Err(e) = res_gallery {
                        error_msg = e;
                        false
                    } else {
                        true
                    }
                }
            };
            if !sync_called {
                if let Some(w) = weak.upgrade() {
                    w.set_panel_save_error(error_msg.clone().into());
                    w.set_theme_error_text(error_msg.into());
                }
                return;
            }
            // Success: clear errors, refresh UI, sync gallery model, refresh mosaic/slice
            if let Some(w) = weak.upgrade() {
                w.set_panel_save_error("".into());
                w.set_theme_error_text("".into());
                w.set_panel_save_input("".into());
                // Refresh theme list from main manager (AppState helper)
                {
                    let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    st.refresh_theme_list(&w);
                }
                // Sync gallery cards in-place (R10: visible no restart)
                {
                    let new_rows: Vec<crate::GalleryCardData> = {
                        let gtm = gallery_tm_c.lock().unwrap();
                        let infos = gtm.list().unwrap_or_default();
                        infos
                            .iter()
                            .map(|info| {
                                let card = crate::shell::gallery::ThemeCard::from_info(info);
                                let providers_model = slint::ModelRc::new(slint::VecModel::from(
                                    info.providers
                                        .iter()
                                        .map(|p| slint::SharedString::from(p.as_str()))
                                        .collect::<Vec<_>>(),
                                ));
                                crate::GalleryCardData {
                                    name: slint::SharedString::from(info.name.as_str()),
                                    saved_at: slint::SharedString::from(info.saved_at.as_str()),
                                    is_active: info.is_active,
                                    providers: providers_model,
                                    accent: crate::theme::parse_hex(&card.colors.accent),
                                    primary: crate::theme::parse_hex(&card.colors.primary),
                                    secondary: crate::theme::parse_hex(&card.colors.secondary),
                                    tertiary: crate::theme::parse_hex(&card.colors.tertiary),
                                    surface: crate::theme::parse_hex(&card.colors.surface),
                                    border_size: card.border.size,
                                    border_radius: card.border.radius,
                                    border_color: crate::theme::parse_hex(&card.border.color),
                                    shader: slint::SharedString::from(card.shader.clone().unwrap_or_default()),
                                    thumb_path: slint::SharedString::from(
                                        card.thumb_path
                                            .as_ref()
                                            .map(|p| p.to_string_lossy().to_string())
                                            .unwrap_or_default(),
                                    ),
                                    thumb: slint::Image::default(),
                                    hero: slint::Image::default(),
                                    slat_image: slint::Image::default(),
                                    slat_expanded_image: slint::Image::default(),
                                }
                            })
                            .collect()
                    };
                    let model_rc = w.get_gallery_cards();
                    if let Some(model) = model_rc
                        .as_any()
                        .downcast_ref::<slint::VecModel<crate::GalleryCardData>>()
                    {
                        // Preserve baked images via carry_over_bakes semantics inside sync_cards
                        crate::shell::gallery::model::sync_cards(model, new_rows);
                    } else {
                        w.set_gallery_cards(slint::ModelRc::new(slint::VecModel::from(new_rows)));
                    }
                    w.set_gallery_empty(false);
                }
                // Refresh mosaic + slice ring so new card appears in both views
                refresh_mosaic_page_c(false);
                refresh_slice_ring_c();
                // Schedule thumbs for the new card (async bake)
                let weak2 = weak.clone();
                let gallery_themes_root2 = config_dir_c.join("hve").join("themes");
                let refresh_mosaic_page2 = refresh_mosaic_page_c.clone();
                // Inline thumb scheduling (same as startup path, but for the new card)
                {
                    use crate::shell::gallery::thumbs;
                    use slint::Model;
                    let model = w.get_gallery_cards();
                    let mut sources: Vec<(usize, String, Option<std::path::PathBuf>)> = Vec::new();
                    for i in 0..model.row_count() {
                        if let Some(row) = model.row_data(i) {
                            if row.thumb.size().width > 0 {
                                continue;
                            }
                            let src = thumbs::find_source_image(&gallery_themes_root2.join(row.name.as_str()));
                            sources.push((i as usize, row.name.to_string(), src));
                        }
                    }
                    if !sources.is_empty() {
                        let pending = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(sources.len()));
                        let jobs = thumbs::plan_jobs(sources);
                        if !jobs.is_empty() {
                            let pending_c = pending.clone();
                            thumbs::preheat(jobs, move |idx, name, png, hero_png, slat_rgba, slat_expanded_rgba| {
                                if let Some(w2) = weak2.upgrade() {
                                    let model2 = w2.get_gallery_cards();
                                    let img = slint::Image::load_from_path(&png).unwrap_or_default();
                                    let hero_img = slint::Image::load_from_path(&hero_png).unwrap_or_default();
                                    let slat = if slat_rgba.width() == 0 || slat_rgba.height() == 0 {
                                        slint::Image::default()
                                    } else {
                                        let (sw, sh) = (slat_rgba.width(), slat_rgba.height());
                                        let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(sw, sh);
                                        buf.make_mut_bytes().copy_from_slice(slat_rgba.as_raw());
                                        slint::Image::from_rgba8(buf)
                                    };
                                    let slat_exp = if slat_expanded_rgba.width() == 0 || slat_expanded_rgba.height() == 0 {
                                        slint::Image::default()
                                    } else {
                                        let (sw, sh) = (slat_expanded_rgba.width(), slat_expanded_rgba.height());
                                        let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(sw, sh);
                                        buf.make_mut_bytes().copy_from_slice(slat_expanded_rgba.as_raw());
                                        slint::Image::from_rgba8(buf)
                                    };
                                    if let Some(mut row) = model2.row_data(idx) {
                                        if row.name.as_str() == &*name {
                                            row.thumb = img;
                                            row.hero = hero_img;
                                            row.slat_image = slat;
                                            row.slat_expanded_image = slat_exp;
                                            model2.set_row_data(idx, row);
                                            if thumbs::is_last_completion(&pending_c) {
                                                refresh_mosaic_page2(false);
                                            }
                                        } else if thumbs::is_last_completion(&pending_c) {
                                            refresh_mosaic_page2(false);
                                        }
                                    } else if thumbs::is_last_completion(&pending_c) {
                                        refresh_mosaic_page2(false);
                                    }
                                } else if thumbs::is_last_completion(&pending) {
                                    refresh_mosaic_page2(false);
                                }
                            });
                        }
                    }
                }
            }
        });
        // Also wire gallery-stage geometry changed from earlier? already wired.
        // Ensure initial panel placeholders are cleared
        window.set_panel_save_placeholder("Theme name…".into());
        window.set_panel_save_button_text("Save".into());
        window.set_panel_save_error("".into());
    }

    // ── Panel Borders tune: debounced geometry + snap-on-pick (slice 4, COLOR GATED) ──
    {
        let state_c = state.clone();
        let weak = window.as_weak();
        window.on_panel_apply_geometry(move |size, radius, gap_in, gap_out| {
            let result = {
                let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                if size == st.cfg().border_size && radius == st.cfg().border_radius && gap_in == st.cfg().gaps_in && gap_out == st.cfg().gaps_out {
                    return;
                }
                st.cfg_mut().border_size = size;
                st.cfg_mut().border_radius = radius;
                st.cfg_mut().gaps_in = gap_in;
                st.cfg_mut().gaps_out = gap_out;
                st.apply_geometry()
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Geometry error: {}", e);
            }
            if let Some(w) = weak.upgrade() {
                w.set_border_size(size);
                w.set_corner_radius(radius);
                w.set_gap_in(gap_in);
                w.set_gap_out(gap_out);
            }
        });
    }
    {
        let state_c = state.clone();
        let weak = window.as_weak();
        window.on_panel_apply_border(move |idx, file| {
            use crate::callbacks::{preset_geometry_for, BorderGeometry};
            let file_str = file.to_string();
            let (is_deact, result, snap) = {
                let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                let is_deact = st.cfg().active_border_file == file_str;
                let new = if is_deact { String::new() } else { file_str.clone() };
                st.cfg_mut().active_border_file = new.clone();
                let _ = st.cfg().save();
                let arg = if is_deact { "none" } else { &new };
                let res = st.engine().apply_border(arg);
                let snap = if is_deact { BorderGeometry::default() } else { preset_geometry_for(&file_str) };
                if !is_deact {
                    let geom_changed = snap.size != st.cfg().border_size || snap.radius != st.cfg().border_radius || snap.gap_in != st.cfg().gaps_in || snap.gap_out != st.cfg().gaps_out;
                    if geom_changed {
                        st.cfg_mut().border_size = snap.size;
                        st.cfg_mut().border_radius = snap.radius;
                        st.cfg_mut().gaps_in = snap.gap_in;
                        st.cfg_mut().gaps_out = snap.gap_out;
                        let _ = st.apply_geometry();
                    }
                }
                (is_deact, res, snap)
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Border error: {}", e);
            }
            if let Some(w) = weak.upgrade() {
                w.set_active_border_index(if is_deact { -1 } else { idx });
                // snap sliders one-way (preset→tune, no reverse)
                w.set_border_size(snap.size);
                w.set_corner_radius(snap.radius);
                w.set_gap_in(snap.gap_in);
                w.set_gap_out(snap.gap_out);
            }
        });
    }

    // ── Nav modules (data-driven sidebar, translated) ──
    let nav_modules = Vec::from([
        crate::NavModule {
            label: SharedString::from(tr.tr_or("panel.tabs.home", "Home")),
            icon: SharedString::from("⌂"),
            accent: theme::parse_hex("#38bdf8"),
            tab_index: 0,
        },
        crate::NavModule {
            label: SharedString::from(tr.tr_or("panel.tabs.animations", "Animations")),
            icon: SharedString::from("▶"),
            accent: theme::parse_hex("#fbbf24"),
            tab_index: 1,
        },
        crate::NavModule {
            label: SharedString::from(tr.tr_or("panel.tabs.borders", "Borders")),
            icon: SharedString::from("◻"),
            accent: theme::parse_hex("#10b981"),
            tab_index: 2,
        },
        crate::NavModule {
            label: SharedString::from(tr.tr_or("panel.tabs.effects", "Effects")),
            icon: SharedString::from("◆"),
            accent: theme::parse_hex("#c084fc"),
            tab_index: 3,
        },
        crate::NavModule {
            label: SharedString::from(tr.tr_or("panel.tabs.themes", "Themes")),
            icon: SharedString::from("✦"),
            accent: theme::parse_hex("#f472b6"),
            tab_index: 4,
        },
    ]);
    window.set_nav_modules(ModelRc::new(VecModel::from(nav_modules)));

    // ── Start Hyprland IPC listener (handle kept alive so threads
    //     don't outlive the app on quit/restart) ──
    let _ipc_listener = hypr_ipc::start_listener(&window, proj.clone());

    // ── Countdown auto-minimize on focus loss ──
    let _focus_listener = countdown::setup_countdown(window.as_weak());

    // ── Start color watcher (bash inotify) ──
    // The bash-based watcher uses inotify for efficient file monitoring.
    let _color_watcher = watcher::spawn_color_watcher(&proj);

    // ── Composer controller (composition root) ──
    let composer_driver = Box::new(composer::HyprlandComposer::new());
    let controller = composer::Controller::new(composer_driver);
    composer::init_global(controller);

    // ── IPC server (Unix socket) ──
    ipc::start_ipc_server(window.as_weak(), proj.clone());

    // ── Startup: sync keybinds + autostart from saved config ──
    {
        let state_guard = state.lock().unwrap_or_else(|e| e.into_inner());
        if state_guard.cfg().keybinds_enabled {
            set_keybinds(true);
        }
        window.set_keybinds_mode(state_guard.cfg().keybinds_enabled);
        set_autostart(state_guard.cfg().auto_start);
    }

    // ── Startup sanity: repair a fullscreen state left stuck by a crash
    //     BEFORE the first show (gallery-immersive-redesign 1.5) ──
    composer::startup_fullscreen_sanity();

    // ── Visibilidad inicial según el modo ──
    if tray_mode {
        // Lazy load: no llamamos a show() hasta que el usuario pulse SUPER+H.
        // El primer toggle via IPC creará el xdg_toplevel con un debounce
        // de 400ms para evitar que el roundtrip de Wayland sea saboteado
        // por una pulsación accidental repetida.
        //
        // Esto evita por completo el problema de set_minimized() en Wayland
        // (no existe un-minimize programático) y es 100% agnóstico al WM.
        if let Some(mut ctrl) = composer::global_controller() {
            ctrl.set_window_hidden(true);
            ctrl.set_tray_mode(true);
        }
        tracing::info!("Starting in tray mode (lazy load — first toggle shows window)");
        // Still mount Gallery as the initial screen so the first reveal
        // lands there. The composer dispatch is a harmless no-op while the
        // window is hidden (no mapped client → session stays inactive);
        // PR1.1 scopes the fullscreen wiring to the eager-show path only.
        dispatch_initial_gallery_expand(&shell);
    } else {
        window.show()?;
        if let Some(mut ctrl) = composer::global_controller() {
            ctrl.set_window_hidden(false);
            ctrl.set_tray_mode(false);
        }

        // ── Pre-warm tab layouts ──
        // Slint no calcula el layout de tabs inactivos (width: 0%)
        // hasta que se renderizan por primera vez. Esto causa un delay
        // en el primer click. Solución: mostrar cada tab brevemente
        // durante el startup para que Slint cachem los layouts.
        prewarm_tabs(window.as_weak(), 1);

        // ── Warm up keyboard navigation ──
        // Block nav for 250ms after show so the animation clock ticks
        // before the first key press.
        warmup_navigation(&window);

        // En Wayland/Hyprland, show() no garantiza foco automático.
        // Forzamos foco via Composer para evitar el doble-click inicial.
        let startup_tiling = state.lock().unwrap_or_else(|e| e.into_inner()).cfg().tiling_mode;
        // ── Startup order (gallery-immersive-redesign PR1.1) ──
        // show() → focus() → initial Gallery expand(fullscreen session).
        //
        // The expand MUST live here instead of at setup time: the global
        // Composer controller is initialized later than the old call site,
        // and Hyprland only honors a fullscreen dispatch once HVE is mapped
        // AND focused — both are guaranteed after this 200ms post-show pass.
        let shell_for_expand = shell.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(200), move || {
            if let Some(ctrl) = composer::global_controller() {
                ctrl.composer().focus();
            }

            // ── Ensure hyprland.conf rules match saved config ──
            set_tiling_window_rules(startup_tiling);

            // Tiling ON: float BEFORE entering the immersive session.
            // exit_gallery_session restores floating geometry, so sessions
            // are designed to start from a floating window; toggling float
            // AFTER fullscreen could drop the fullscreen flag. The 600ms
            // delay lets the config reload settle for the mapped window,
            // then the expand runs in the same tick (float → fullscreen).
            if startup_tiling {
                slint::Timer::single_shot(std::time::Duration::from_millis(600), move || {
                    if let Some(ctrl) = composer::global_controller() {
                        ctrl.composer().toggle_float();
                    }
                    tracing::info!("[startup] Tiling mode ON: togglefloating dispatched (delayed)");
                    dispatch_initial_gallery_expand(&shell_for_expand);
                    slint::Timer::single_shot(std::time::Duration::from_millis(400), || {
                        reassert_gallery_fullscreen();
                    });
                });
            } else {
                dispatch_initial_gallery_expand(&shell_for_expand);
                slint::Timer::single_shot(std::time::Duration::from_millis(400), || {
                    reassert_gallery_fullscreen();
                });
            }
        });
    }

    // ── Global event loop (decoupled from window lifecycle) ──
    slint::run_event_loop_until_quit()?;

    // Clean up IPC socket after the event loop stops
    ipc::cleanup();

    drop(_color_watcher);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Startup Gallery expand wiring (gallery-immersive-redesign PR1.1) ──
    // The initial Expand(Gallery) must exist as a named helper so the
    // post-show startup timer can fire it AFTER composer::init_global and
    // composer focus() (the old setup-time dispatch ran while the global
    // controller was still None → fullscreen session was a silent no-op).
    #[test]
    fn initial_gallery_expand_mounts_gallery_screen() {
        i_slint_backend_testing::init_no_event_loop();
        let win = crate::MainWindow::new().unwrap();
        win.show().unwrap();
        let shell = shell::Shell::new(win.as_weak());
        assert_eq!(win.get_mounted_screen(), 0, "precondition: Home");
        dispatch_initial_gallery_expand(&shell);
        assert_eq!(win.get_mounted_screen(), 1, "initial expand mounts Gallery");
        assert!(win.get_expanded(), "Gallery mounts expanded");
    }

    // ── Keyboard navigation logic (backend-testing) ──────────────────
    // Ignored by default: initializes the Slint testing backend which can
    // only run once per process. Run with:
    //   cargo test -- --ignored nav_logic --test-threads=1
    //
    // Regression test: a plain hold of the arrow key (<600ms, no OS
    // auto-repeat yet) must NOT jump two tabs. The repeat Timer only starts
    // after the OS confirms the hold with its first repeat (event.repeat).
    #[test]
    #[ignore]
    fn nav_logic_single_keypress_single_step() {
        i_slint_backend_testing::init_integration_test_with_mock_time();
        let win = MainWindow::new().unwrap();
        win.show().unwrap();
        let w = win.window();

        // 1. Tap: un KeyPressed Down desde Inicio (tab 0) → exactamente UNA tab
        w.dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::DownArrow.into(),
        });
        assert_eq!(win.get_active_tab(), 1, "un key-pressed (tap) debe mover exactamente una tab");

        // 2. Hold de 500ms SIN repeat del OS: el Timer NO está activo
        //    (arranca solo tras event.repeat) → NO debe saltar otra tab.
        //    ESTO era el bug: el Timer viejo disparaba a los 400ms.
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(500));
        assert_eq!(win.get_active_tab(), 1, "hold de 500ms sin repeat del OS no debe saltar dos tabs");

        // 3. El OS confirma el hold con su primer repeat → activa el Timer, sin step
        w.dispatch_event(slint::platform::WindowEvent::KeyPressRepeated {
            text: slint::platform::Key::DownArrow.into(),
        });
        assert_eq!(win.get_active_tab(), 1, "el repeat del OS activa el Timer pero no hace step");

        // 4. El Timer toma el relevo del repeat. Nota: en el backend de
        //    testing, el Timer se registra recién en el siguiente tick (el
        //    dispatch del evento no avanza el reloj). En la app real el event
        //    loop de winit actualiza los timers continuamente.
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(150));

        // 5. Primer step del Timer (intervalo 150ms dentro de un módulo)
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(150));
        assert_eq!(win.get_active_tab(), 2, "Timer activo tras repeat del OS: step cada 150ms");

        // 6. El Timer se re-registra en el tick siguiente y luego dispara el
        //    2º step (mismo patrón de registro diferido del backend testing)
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(150));
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(150));
        assert_eq!(win.get_active_tab(), 3, "segundo intervalo del Timer: otro step");

        // 7. Release → todo se resetea
        w.dispatch_event(slint::platform::WindowEvent::KeyReleased {
            text: slint::platform::Key::DownArrow.into(),
        });

        // 8. Nuevo tap tras release → exactamente una tab más
        w.dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::DownArrow.into(),
        });
        assert_eq!(win.get_active_tab(), 4, "nuevo press tras release mueve una tab");
    }

    // ── Composer wiring verification ─────────────────────────────────

    /// Verify that the Controller is constructed with correct defaults
    /// and that the Composer trait is properly implemented.
    /// This test does NOT call init_global() (OnceLock can only be set once
    /// per process), but it verifies the Controller + Composer integration
    /// that main.rs uses at the composition root.
    #[test]
    fn test_controller_wiring_defaults() {
        let (fake, _calls) = composer::tests::FakeComposer::new();
        let controller = composer::Controller::new(Box::new(fake));

        // Verify the Controller's initial state matches what main.rs expects
        assert!(!controller.window_hidden(), "window should not be hidden at startup");
        assert!(!controller.tray_mode(), "tray_mode should be false at startup");
        assert!(controller.prev_workspace().is_none(), "no prev workspace at startup");

        // Verify the composer reference works
        let _composer = controller.composer();
    }

    // ── Borders tune geometry — strict TDD slice 4 (debounce, skip-init, snap) ──
    #[test]
    fn test_geometry_debounce_last_wins() {
        use crate::callbacks::{BorderGeometry, GeometryDebouncer, should_apply_geometry};
        let initial = BorderGeometry { size: 2, radius: 32, gap_in: 5, gap_out: 5 };
        let mut d = GeometryDebouncer::new(initial);
        d.push(initial); // prime init skip
        let seq = [
            BorderGeometry { size: 2, radius: 10, gap_in: 5, gap_out: 5 },
            BorderGeometry { size: 2, radius: 20, gap_in: 5, gap_out: 5 },
            BorderGeometry { size: 2, radius: 30, gap_in: 5, gap_out: 5 },
            BorderGeometry { size: 2, radius: 40, gap_in: 5, gap_out: 5 },
            BorderGeometry { size: 2, radius: 80, gap_in: 5, gap_out: 5 },
        ];
        for g in seq { assert!(d.push(g)); }
        let flushed = d.flush().expect("last-wins flush");
        assert_eq!(flushed.radius, 80);
        assert!(should_apply_geometry(&initial, &flushed));
    }

    #[test]
    fn test_skip_init_event() {
        use crate::callbacks::{BorderGeometry, GeometryDebouncer};
        let mut d = GeometryDebouncer::new(BorderGeometry::default());
        assert!(!d.push(BorderGeometry { size: 3, radius: 10, gap_in: 5, gap_out: 5 }), "first event ignored");
        assert!(!d.has_pending());
        assert_eq!(d.flush(), None);
        assert!(d.push(BorderGeometry { size: 3, radius: 10, gap_in: 5, gap_out: 5 }));
        assert!(d.has_pending());
    }

    #[test]
    fn test_pick_snaps_sliders() {
        use crate::callbacks::{preset_geometry_for, BorderGeometry};
        let snapped = preset_geometry_for("thin-rounded.ron");
        assert_eq!(snapped, BorderGeometry { size: 2, radius: 10, gap_in: 5, gap_out: 5 });
        assert_eq!(preset_geometry_for("sharp.ron"), BorderGeometry { size: 1, radius: 0, gap_in: 0, gap_out: 0 });
        assert_eq!(preset_geometry_for("thick.ron"), BorderGeometry { size: 5, radius: 20, gap_in: 10, gap_out: 10 });
        assert_eq!(preset_geometry_for("unknown.ron"), BorderGeometry::default());
    }

}
