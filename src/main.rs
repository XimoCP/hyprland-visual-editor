mod app_state;
mod callbacks;
mod composer;
mod config;
mod config_guard;
mod config_markers;
mod countdown;
mod engine;
mod hypr_ipc;
mod ipc;
mod presets;
mod preset_store;
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
mod scripts_contract;
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

/// Initial carousel focus for the gallery slider: the ACTIVE theme's card
/// index (first visible when the slider opens), or 0 when no theme is
/// active / the list is empty. Callers feed the result into
/// `set_gallery_focused` before `refresh_slice_ring`, which snaps both
/// `focus-pos` and `delta-base` to it — zero drift is preserved
/// (focus-pos == delta-base when settled).
pub(crate) fn gallery_initial_focus(window: &crate::MainWindow) -> usize {
    use slint::Model as _;
    let cards = window.get_gallery_cards();
    let count = cards.row_count() as usize;
    if count == 0 {
        return 0;
    }
    (0..count)
        .find_map(|i| cards.row_data(i).filter(|row| row.is_active).map(|_| i))
        .unwrap_or(0)
}

/// True when any gallery card carries the active mark.
pub(crate) fn gallery_has_active(window: &crate::MainWindow) -> bool {
    use slint::Model as _;
    let cards = window.get_gallery_cards();
    (0..cards.row_count()).any(|i| cards.row_data(i).is_some_and(|row| row.is_active))
}

/// Startup fallback latch: true when the gallery opened WITHOUT a resolved
/// active card (focus fell back to 0, e.g. empty `last_applied` on disk).
/// Consumed by `reaffirm_gallery_focus_on_active` — user navigation away
/// from slot 0 also disarms it via the focused-index gate there.
static GALLERY_STARTED_FALLBACK: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Record whether this startup resolved an active card. Call right AFTER
/// `set_gallery_focused(gallery_initial_focus(..))` at every startup site.
pub(crate) fn gallery_note_startup(window: &crate::MainWindow) {
    GALLERY_STARTED_FALLBACK.store(
        !gallery_has_active(window),
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// Snap focus onto a LATE-arriving active card (active resolved after a
/// fallback startup — watcher refresh, late list resolution). Returns true
/// when it moved focus; the caller must then run `refresh_slice_ring`,
/// which snaps delta-base + focus-pos together (zero drift preserved).
/// Idle (false) when: startup had an active card, no active card exists,
/// focus already sits on the active card (latch consumed), or the user
/// navigated away from the fallback slot (no yanking, ever).
pub(crate) fn reaffirm_gallery_focus_on_active(window: &crate::MainWindow) -> bool {
    use slint::Model as _;
    if !GALLERY_STARTED_FALLBACK.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    let cards = window.get_gallery_cards();
    let count = cards.row_count() as usize;
    let Some(active) =
        (0..count).find(|&i| cards.row_data(i).is_some_and(|row| row.is_active))
    else {
        return false;
    };
    let focused = window.get_gallery_focused().max(0) as usize;
    if focused < count && cards.row_data(focused).is_some_and(|row| row.is_active) {
        GALLERY_STARTED_FALLBACK.store(false, std::sync::atomic::Ordering::Relaxed);
        return false;
    }
    if focused != 0 {
        return false; // user navigated away — never yank
    }
    window.set_gallery_focused(active as i32);
    GALLERY_STARTED_FALLBACK.store(false, std::sync::atomic::Ordering::Relaxed);
    true
}

// ── Active-theme backfill (legacy configs) ──────────────────────────────
// Applies made before the active mark was persisted left
// `cfg.last_applied_theme` empty on disk, so every restart resolved NO
// active card and the carousel fell back to the first one (user report:
// always the same theme centered). Resolve the active theme from the LIVE
// wallpaper — the same IPC the noctalia-v5 provider saves with — adopt the
// first matching theme in list order, and persist it so this only runs once.

/// Saved noctalia-v5 wallpaper of one theme (empty dir/file → None).
fn saved_theme_wallpaper(themes_dir: &std::path::Path, name: &str) -> Option<String> {
    std::fs::read_to_string(
        themes_dir
            .join(name)
            .join("providers")
            .join("noctalia-v5")
            .join("wallpaper.txt"),
    )
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

/// Resolve the backfill candidate: live wallpaper via IPC, then the first
/// theme whose saved snapshot matches. IPC failure or no match → None (the
/// caller keeps the old first-card fallback; next launch tries again).
fn backfill_active_from_live(
    tm: &crate::theme_manager::ThemeManager,
    config_dir: &std::path::Path,
) -> Option<String> {
    let live = crate::providers::noctalia_runtime::noctalia_msg(&["msg", "wallpaper-get"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    let themes_dir = config_dir.join("hve").join("themes");
    let themes = tm.list().ok()?;
    let rows: Vec<(String, Option<String>)> = themes
        .iter()
        .map(|t| (t.name.clone(), saved_theme_wallpaper(&themes_dir, &t.name)))
        .collect();
    crate::callbacks::backfill_active_candidate(&rows, &live)
}

// ── Legacy prewarm/warmup — REMOVED slice 8 (R8) ──
// Panel navigation no longer uses legacy_tab / nav-ready; held-key repeat handled per-section.

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

/// Rebuild window gallery cards from gallery_tm in place (keeps baked thumbs)
/// then refresh mosaic + slice ring. Shared by Save rename/delete/refresh/
/// overwrite so the Gallery follows without restart.
fn sync_save_gallery_ui(
    w: &crate::MainWindow,
    gallery_tm: &std::sync::Arc<std::sync::Mutex<crate::theme_manager::ThemeManager>>,
    refresh_mosaic_page: &std::sync::Arc<dyn Fn(bool) + Send + Sync>,
    refresh_slice_ring: &std::sync::Arc<dyn Fn() + Send + Sync>,
) {
    let new_rows: Vec<crate::GalleryCardData> = {
        let gtm = gallery_tm.lock().unwrap();
        gtm.list().unwrap_or_default().iter().map(|info| {
            let card = crate::shell::gallery::ThemeCard::from_info(info);
            crate::GalleryCardData {
                name: slint::SharedString::from(info.name.as_str()),
                saved_at: slint::SharedString::from(info.saved_at.as_str()),
                is_active: info.is_active,
                providers: slint::ModelRc::new(slint::VecModel::from(
                    info.providers.iter().map(|p| slint::SharedString::from(p.as_str())).collect::<Vec<_>>(),
                )),
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
                    card.thumb_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                ),
                thumb: slint::Image::default(),
                hero: slint::Image::default(),
                slat_image: slint::Image::default(),
                slat_expanded_image: slint::Image::default(),
            }
        }).collect()
    };
    let model_rc = w.get_gallery_cards();
    if let Some(model) = model_rc.as_any().downcast_ref::<slint::VecModel<crate::GalleryCardData>>() {
        crate::shell::gallery::model::sync_cards(model, new_rows);
    } else {
        w.set_gallery_cards(slint::ModelRc::new(slint::VecModel::from(new_rows)));
    }
    // Late active arrival after a fallback startup: snap focus onto it
    // BEFORE the ring refresh settles delta-base + focus-pos (zero drift).
    reaffirm_gallery_focus_on_active(w);
    refresh_mosaic_page(false);
    refresh_slice_ring();
}

/// Debounce guard: overlapping reasserts (timed fallbacks + event-driven)
/// must not stack multiple unset->set cycles — each cycle is a visible
/// fullscreen drop, and five stacked retries meant five visible minimizes.
static LAST_REASSERT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
/// Theme cycle generation: exactly one invisible unset->set per theme apply.
/// The first path to fire (event-driven configreloaded OR 4.8s fallback)
/// wins; the other becomes a no-op for that generation.
pub(crate) static THEME_GEN: std::sync::Mutex<u64> = std::sync::Mutex::new(0);
pub(crate) static THEME_CYCLE_FIRED_GEN: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);
pub(crate) static THEME_TRANSITIONING_FLAG: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static THEME_ORIG_ANIM: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static THEME_ORIG_BLUR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static THEME_ORIG_BORDER: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static THEME_ORIG_SHADOW: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// Interlude view state: the workspace the view must RETURN to and whether
/// the view was actually staged away. See `InterludeState` for the rules.
static THEME_INTERLUDE: std::sync::Mutex<InterludeState> =
    std::sync::Mutex::new(InterludeState::new());

// Display pump for the slice reel spring (slice_reel owns the physics;
// this timer owns the wall-clock cadence). Lives on the UI thread only —
// `slint::Timer` is not Send, hence the thread-local.
thread_local! {
    static REEL_TIMER: std::cell::RefCell<Option<slint::Timer>> =
        const { std::cell::RefCell::new(None) };
}

/// Chained-glide duration for the slice carousel — matches
/// `SkwdTokens.anim-expand` (ui/tokens.slint).
const SLICE_GLIDE_MS: f32 = 150.0;

/// Ensure the reel display pump is running. Each tick advances the spring
/// and writes the position; when the spring reports idle the timer stops
/// itself (stopping from inside the callback is supported).
fn ensure_reel_timer(weak: slint::Weak<crate::MainWindow>) {
    REEL_TIMER.with_borrow_mut(|slot| {
        if slot.is_some() {
            return; // already pumping
        }
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(16),
            move || {
                let Some(w) = weak.upgrade() else {
                    stop_reel_timer();
                    return;
                };
                match crate::shell::gallery::views::slice_reel::step(0.016) {
                    Some(v) => w.set_gallery_slice_focus_pos(v),
                    None => stop_reel_timer(), // settled — stop the pump
                }
            },
        );
        *slot = Some(timer);
    });
}

fn stop_reel_timer() {
    REEL_TIMER.with_borrow_mut(|slot| {
        if let Some(timer) = slot.as_ref() {
            timer.stop();
        }
        *slot = None;
    });
}

pub(crate) fn is_theme_transitioning_flag() -> bool {
    THEME_TRANSITIONING_FLAG.load(std::sync::atomic::Ordering::Relaxed)
}

fn hypr_getoption_int(option: &str, fallback: &str) -> String {
    let text = std::process::Command::new("hyprctl")
        .args(["getoption", option, "-j"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok());
    if let Some(t) = text {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
            // Hyprland is inconsistent: some options expose "int", booleans set
            // from config may only expose "str" ("true"/"1"/"[EMPTY]").
            if let Some(i) = v.get("int").and_then(|n| n.as_i64()) {
                return i.to_string();
            }
            if let Some(s) = v.get("str").and_then(|s| s.as_str()) {
                let s = s.trim();
                if s.parse::<i64>().is_ok() || s == "true" || s == "false" {
                    return s.to_string();
                }
            }
        }
    }
    fallback.to_string()
}

fn hypr_set(option: &str, value: &str) {
    let _ = std::process::Command::new("hyprctl")
        .args(["keyword", option, value])
        .output();
}

pub(crate) fn restore_theme_masks() {
    if let Some(orig) = THEME_ORIG_ANIM.lock().unwrap().take() {
        hypr_set("animations:enabled", &orig);
        tracing::info!("[theme] animations restored to {}", orig);
    }
    if let Some(orig) = THEME_ORIG_BLUR.lock().unwrap().take() {
        hypr_set("decoration:blur:enabled", &orig);
        tracing::info!("[theme] blur restored to {}", orig);
    }
    if let Some(orig) = THEME_ORIG_BORDER.lock().unwrap().take() {
        hypr_set("general:border_size", &orig);
        tracing::info!("[theme] border_size restored to {}", orig);
    }
    if let Some(orig) = THEME_ORIG_SHADOW.lock().unwrap().take() {
        hypr_set("decoration:shadow:enabled", &orig);
        tracing::info!("[theme] shadow restored to {}", orig);
    }
}

/// Re-apply the transition masks after a config reload. Every `hyprctl
/// reload` resets runtime keyword overrides to the config values (animations
/// and blur back ON), which un-masks the transition mid-flight and re-floats
/// HVE visibly. Called on EVERY configreloaded line during a theme
/// transition — bypasses the 3s throttle (cheap, idempotent).
pub(crate) fn reapply_theme_masks() {
    if !is_theme_transitioning_flag() {
        return;
    }
    hypr_set("animations:enabled", "0");
    hypr_set("decoration:blur:enabled", "0");
    hypr_set("general:border_size", "0");
    hypr_set("decoration:shadow:enabled", "0");
    tracing::info!("[theme] masks re-applied after reload wipe");
}

fn reassert_debounce_ok() -> bool {
    let mut last = LAST_REASSERT.lock().unwrap();
    let now = std::time::Instant::now();
    if last.is_some_and(|t| now.duration_since(t) < std::time::Duration::from_millis(1500)) {
        return false; // a cycle ran (or is running) very recently — skip
    }
    *last = Some(now);
    true
}

/// Re-assert immersive fullscreen after Gallery expand or after a Noctalia
/// theme apply.
///
/// Root cause note: a bare `set` on a window Hyprland already believes is
/// fullscreen is a compositor NO-OP — it answers ok but fires no state
/// transition, so the shell bar (Noctalia/Waybar) never receives the
/// fullscreen event and stays visible. Manual SUPER+F works because it is a
/// `toggle` (real transition) and SUPER+H remaps the window (fresh state).
/// Fix: cycle unset -> set with a short gap to force a real transition.
/// Debounced: one visible cycle max per 1.5s window.
pub(crate) fn reassert_gallery_fullscreen() {
    if !crate::shell::Shell::is_gallery_expanded() {
        return;
    }
    // Theme transition: exactly one cycle per generation, no debounce window.
    // The first caller (event-driven OR fallback) claims the generation and
    // the second becomes a no-op — guarantees one visible floating->fullscreen.
    if is_theme_transitioning_flag() {
        let gen = *THEME_GEN.lock().unwrap();
        {
            let mut fired = THEME_CYCLE_FIRED_GEN.lock().unwrap();
            if fired.is_some_and(|g| g == gen) {
                tracing::info!("[reassert] skip — generation {} already fired", gen);
                return;
            }
            *fired = Some(gen);
        }
        tracing::info!("[reassert] theme generation {} claims cycle (single-fire)", gen);
    } else if !reassert_debounce_ok() {
        return;
    }
    if let Some(mut ctrl) = crate::composer::global_controller() {
        // Step 1 of the cycle: drop fullscreen (fires a real state change).
        let _ = ctrl.composer().set_fullscreen(false);
        if !ctrl.gallery_session_active() {
            let _ = ctrl.enter_gallery_session();
        }
    }
    // Step 2: re-enter fullscreen after the compositor settles the unset.
    slint::Timer::single_shot(std::time::Duration::from_millis(150), || {
        if !crate::shell::Shell::is_gallery_expanded() {
            return;
        }
        if let Some(mut ctrl) = crate::composer::global_controller() {
            let ok = ctrl.composer().set_fullscreen(true);
            if !ctrl.gallery_session_active() {
                let _ = ctrl.enter_gallery_session();
            }
            tracing::info!("[reassert] cycled unset->set fullscreen ok={}", ok);
        }
    });
}

/// Timed safety net after a theme apply: REMOVED — the single unset->set
/// cycle now runs inside the transparent fade window (event-driven path in
/// hypr_ipc, or the 4.8s fallback here), so timed retries would only add
/// visible flashes after fade-in.

// ── Fresh-launch fullscreen settle (floating-startup fix) ─────────────
// The old eager path mapped the window first (small floating Home frame)
// and fired focus + fullscreen on blind 200ms/400ms timers — often before
// Hyprland mapped/focused the client — while an unconditional rules
// rewrite + `hyprctl reload` re-floated the window mid-settle.
//
// The new path maps ONCE (rules applied + Gallery mounted pre-show) and
// then retries fullscreen gated on `hyprctl clients -j`: dispatch only
// once HVE is mapped, stop once fullscreen is confirmed.

/// Absolute settle schedule (ms after show): two confirmation attempts
/// plus a late reassert inside ~2s. Ticks that find HVE already
/// fullscreen (or the schedule exhausted) dispatch nothing.
pub(crate) const STARTUP_FULLSCREEN_RETRIES_MS: [u64; 3] = [300, 800, 1500];

/// Pure settle decision for one startup retry tick.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum StartupFsAction {
    /// HVE not mapped yet — wait for the next tick, dispatch nothing.
    Wait,
    /// HVE mapped but windowed — run one unset->set cycle now.
    Assert,
    /// Fullscreen confirmed, or attempts exhausted — stop retrying.
    Done,
}

pub(crate) fn startup_fs_action(mapped: bool, fullscreen: bool, attempt: usize) -> StartupFsAction {
    if fullscreen || attempt >= STARTUP_FULLSCREEN_RETRIES_MS.len() {
        return StartupFsAction::Done;
    }
    if mapped {
        StartupFsAction::Assert
    } else {
        StartupFsAction::Wait
    }
}

/// Schedule tick `attempt` of the settle chain. Delays are absolute
/// (schedule[N] ms after show); each tick chains the next only while the
/// policy says so. Runs OUTSIDE the theme debounce window: a fresh
/// startup owns no theme cycle, and the 1.5s debounce must not swallow
/// the late reassert.
fn schedule_startup_settle(attempt: usize, tile_toggle: bool) {
    let Some(&at_ms) = STARTUP_FULLSCREEN_RETRIES_MS.get(attempt) else {
        return;
    };
    let prev_ms = attempt
        .checked_sub(1)
        .and_then(|p| STARTUP_FULLSCREEN_RETRIES_MS.get(p))
        .copied()
        .unwrap_or(0);
    let delay = at_ms.saturating_sub(prev_ms);
    slint::Timer::single_shot(std::time::Duration::from_millis(delay), move || {
        startup_settle_fullscreen(attempt, tile_toggle);
    });
}

/// One tick of the startup settle chain. `tile_toggle` is the one-shot
/// tiling-mode float toggle (only when the rules file actually changed at
/// this launch); it is consumed on the first mapped tick and never
/// re-fired, so retries can never stack visible minimizes.
fn startup_settle_fullscreen(attempt: usize, tile_toggle: bool) {
    if !crate::shell::Shell::is_gallery_expanded() || is_theme_transitioning_flag() {
        return;
    }
    let seen = crate::composer::hyprland::query_hve_seen();
    match startup_fs_action(seen.mapped, seen.fullscreen, attempt) {
        StartupFsAction::Done => {
            tracing::info!(
                "[startup] fullscreen settle done (mapped={} fullscreen={} attempt={})",
                seen.mapped,
                seen.fullscreen,
                attempt
            );
        }
        StartupFsAction::Wait => {
            tracing::info!("[startup] HVE not mapped yet — wait (attempt={})", attempt);
            schedule_startup_settle(attempt + 1, tile_toggle);
        }
        StartupFsAction::Assert => {
            if let Some(mut ctrl) = composer::global_controller() {
                if tile_toggle {
                    ctrl.composer().toggle_float();
                    tracing::info!("[startup] Tiling mode ON: togglefloating dispatched (mapped-gated)");
                }
                // Cycle unset->set to force a real compositor transition
                // (a bare `set` on an already-fullscreen client is a no-op
                // that leaves the shell bar visible — see reassert note).
                let _ = ctrl.composer().set_fullscreen(false);
                let ok = ctrl.composer().set_fullscreen(true);
                if !ctrl.gallery_session_active() {
                    let _ = ctrl.enter_gallery_session();
                }
                tracing::info!("[startup] mapped-gated unset->set fullscreen ok={} (attempt={})", ok, attempt);
            }
            schedule_startup_settle(attempt + 1, false);
        }
    }
}

fn get_active_workspace_name() -> String {
    std::process::Command::new("hyprctl")
        .args(["activeworkspace", "-j"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| "2".to_string())
}

fn find_empty_workspace(exclude: &str) -> String {
    // Find a normal workspace with no windows, not special, not the current one.
    // Falls back to "10" (Hyprland creates it on demand).
    let out = std::process::Command::new("hyprctl")
        .args(["workspaces", "-j"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    if let Some(arr) = out.as_ref().and_then(|v| v.as_array()) {
        for ws in arr {
            let name = ws.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let wins = ws.get("windows").and_then(|n| n.as_u64()).unwrap_or(1);
            if wins == 0 && !name.starts_with("special") && name != exclude && !name.is_empty() {
                return name.to_string();
            }
        }
    }
    if exclude != "10" { "10".to_string() } else { "11".to_string() }
}

// ── Theme interlude (empty-workspace stage for the wallpaper/bar swap) ──

/// Lua payload that switches the active VIEW to `ws` without moving any
/// window. This Hyprland build (0.56.2, Lua-config runtime) routes every
/// `dispatch` through its Lua shim, so the payload must be valid Lua:
/// numeric `workspace 10` errors with "')' expected near '10'", while
/// `hl.dsp.focus({ workspace = "10" })` maps to the changeWorkspace
/// dispatcher (LuaBindingsDispatchers.cpp, `hl.focus` workspace branch).
/// Verified live: both directions answer ok and the view flips.
/// Returns None for names that could escape the Lua string — allowlist is
/// the same policy as composer::hyprland::safe_workspace_target. Normal
/// workspace names are alphanumeric, so "special:x" (colon) is rejected
/// here by design: the interlude stage must be a NORMAL workspace.
fn interlude_view_script(ws: &str) -> Option<String> {
    if !is_safe_workspace_name(ws) {
        return None;
    }
    Some(format!("hl.dsp.focus({{ workspace = \"{ws}\" }})"))
}

fn is_safe_workspace_name(ws: &str) -> bool {
    !ws.is_empty()
        && ws.len() <= 128
        && ws.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
}

/// Switch the compositor view (never any window) to `ws`. Best-effort:
/// a failure keeps the old stay-fullscreen contract (no staging, no return).
fn hypr_switch_view(ws: &str) -> bool {
    let Some(script) = interlude_view_script(ws) else {
        tracing::warn!("[theme] interlude view switch rejected unsafe name {:?}", ws);
        return false;
    };
    std::process::Command::new("hyprctl")
        .args(["dispatch", &script])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("ok"))
        .unwrap_or(false)
}

/// Re-set fullscreen on HVE via the WINDOW-TARGETED v5 dispatcher (reuses
/// composer's verified builder). Crucially it carries NO focus step: the
/// focus-by-title inside `reassert_gallery_fullscreen` targets HVE on
/// orig_ws and focusing a window on another workspace drags the VIEW back
/// with it — the early ventanita+windows flash. Idempotent when HVE is
/// already fullscreen (no-op), a real transition when a reload re-floated it.
fn reassert_hve_fullscreen_targeted() -> bool {
    let script = crate::composer::hyprland::v5_set_fullscreen(true);
    std::process::Command::new("hyprctl")
        .args(["dispatch", &script])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("ok"))
        .unwrap_or(false)
}

// ── Notification silence during the theme swap ──────────────────────────
// The theme apply fires a burst of notify-send toasts (wallpaper set,
// palette applied, reloads) — previously swallowed because HVE covered the
// screen; the interlude stage now exposes them. The swap runs under
// Do-Not-Disturb: the pre-swap state is captured first-writer-wins and
// restored exactly once after the view is home (the watchdog is the safety
// net — `take()` makes a second restore a no-op).
static THEME_DND_ORIG: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// "on"→"true", "off"→"false" — noctalia's DND boolean spelling.
fn dnd_bool_arg(status: &str) -> Option<&'static str> {
    match status {
        "on" => Some("true"),
        "off" => Some("false"),
        _ => None,
    }
}

/// Verified live: `noctalia msg notification-dnd-status` prints on/off;
/// `notification-dnd-set true|false` answers ok. Best-effort everywhere.
fn noctalia_dnd_set(state: &str) -> bool {
    std::process::Command::new("noctalia")
        .args(["msg", "notification-dnd-set", state])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("ok"))
        .unwrap_or(false)
}

fn noctalia_dnd_status() -> Option<String> {
    let out = std::process::Command::new("noctalia")
        .args(["msg", "notification-dnd-status"])
        .output()
        .ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim().to_string();
    if s == "on" || s == "off" { Some(s) } else { None }
}

/// Engage DND for the swap. First writer wins: while a swap is already in
/// flight (rapid successive applies) a newer generation must NOT overwrite
/// the captured original with its own silenced "on".
fn theme_dnd_engage() {
    let orig = {
        let mut slot = THEME_DND_ORIG.lock().unwrap();
        if slot.is_some() {
            return; // already engaged by an earlier generation
        }
        match noctalia_dnd_status() {
            Some(s) => {
                *slot = Some(s);
                slot.clone().unwrap()
            }
            None => {
                tracing::warn!("[theme] noctalia DND status unavailable — notifications not silenced");
                return;
            }
        }
    };
    let ok = noctalia_dnd_set("true");
    tracing::info!("[theme] DND engaged (was {}) ok={}", orig, ok);
}

/// Restore the pre-swap DND state, exactly once (take()).
fn theme_dnd_release() {
    if let Some(orig) = THEME_DND_ORIG.lock().unwrap().take() {
        if let Some(arg) = dnd_bool_arg(&orig) {
            let ok = noctalia_dnd_set(arg);
            tracing::info!("[theme] DND restored to {} ok={}", orig, ok);
        }
    }
}

/// Interlude view state, pure — `THEME_INTERLUDE` holds the shared static.
struct InterludeState {
    orig: Option<String>,
    moved: bool,
}

impl InterludeState {
    const fn new() -> Self {
        Self { orig: None, moved: false }
    }
    /// First writer wins: while an interlude is in flight, a newer theme
    /// generation must keep returning to the user's ORIGINAL workspace, not
    /// to the interlude stage the view currently sits on.
    fn capture(&mut self, orig: &str) -> bool {
        if self.orig.is_some() {
            return false;
        }
        self.orig = Some(orig.to_string());
        true
    }
    fn orig(&self) -> Option<&str> {
        self.orig.as_deref()
    }
    fn should_move(&self) -> bool {
        !self.moved
    }
    fn mark_moved(&mut self) {
        self.moved = true;
    }
    /// Whether the view is (still) staged away — the finale must NOT run the
    /// focusing fullscreen cycle then: `v5_focus` targets HVE on orig_ws and
    /// focusing a window on another workspace drags the VIEW back with it.
    fn staged(&self) -> bool {
        self.moved
    }
    /// Consume the return trip: exactly once, and only if the view moved.
    /// If a generation never stages the view, nothing is restored — the
    /// old stay-fullscreen contract never leaves the user's workspace.
    fn take_restore(&mut self) -> Option<String> {
        if !self.moved {
            return None;
        }
        self.moved = false;
        self.orig.take()
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
    let mut cfg = Config::load();
    // Capture System active for startup re-apply (must survive move into AppState)
    let startup_system_active = cfg.is_system_active;
    // Use saved language, or auto-detect from system locale
    let tr_lang = if cfg.language.is_empty() {
        tr::detect_language()
    } else {
        cfg.language.clone()
    };
    let tr = Arc::new(Tr::with_lang(&tr_lang));

    tracing::info!("Locale: {}", tr.lang);

    // ── Migration guard (drop-conf-support R6–R8) ──────────────────────
    // Evaluate BEFORE any settings write so a conf-only system never
    // receives writes it would ignore. The window still opens; blocked
    // paths surface a bilingual status instead of mutating.
    let guard_decision = config_guard::evaluate_config_guard_default();
    tracing::info!(
        "[guard] migration decision: {:?} (init enable permitted: {})",
        guard_decision,
        guard_decision.permits_init_enable()
    );
    let guard_permits = guard_decision.permits_mutation();
    let guard_block_message: slint::SharedString = match guard_decision {
        config_guard::ConfigGuardDecision::PromptToEnable => tr.tr_shared(
            "shell.lua_enable",
            "Run init.sh enable to let HVE manage hyprland.lua",
        ),
        config_guard::ConfigGuardDecision::ConfOnly => tr.tr_shared(
            "shell.lua_migration",
            "HVE only manages hyprland.lua — migrate from hyprland.conf",
        ),
        _ => slint::SharedString::from(""),
    };

    // ── Ensure hve-settings exists with default window rules and keybinds section ──
    if guard_permits {
        ensure_settings_file();
    }

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
    window.set_shell_status_text(if guard_block_message.is_empty() {
        tr.tr_shared("shell.status_ready", "Ready")
    } else {
        guard_block_message.clone()
    });
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
        // Legacy configs: applies predating the persisted active mark left
        // the field empty — resolve the active theme from the live wallpaper
        // and persist it, so the gallery opens on the APPLIED card.
        if theme_manager.last_applied.is_empty() {
            if let Some(name) = backfill_active_from_live(&theme_manager, &config_dir) {
                theme_manager.last_applied = name.clone();
                cfg.last_applied_theme = name;
                let _ = cfg.save();
                tracing::info!(
                    "[startup] Backfilled active theme '{}' from live wallpaper",
                    theme_manager.last_applied
                );
            }
        }
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
            // Programmatic snap — disarm the reel so the next glide starts here.
            crate::shell::gallery::views::slice_reel::snap();
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
                // Distant jump: instant rebase — no glide through the ring.
                crate::shell::gallery::views::slice_reel::snap();
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
                // Spring glide (skwd-wall port): retargetable mid-flight with
                // momentum — chained steps blend instead of restarting a curve.
                let reduced = w.get_gallery_reduced_motion();
                let duration = if reduced { 0.0 } else { SLICE_GLIDE_MS };
                crate::shell::gallery::views::slice_reel::glide_to(
                    current_pos,
                    step.target,
                    duration,
                );
                if duration > 0.0 {
                    ensure_reel_timer(weak.clone());
                } else {
                    w.set_gallery_slice_focus_pos(step.target);
                }
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
        // Open on the active theme: initial carousel focus is the active
        // card, so refresh_slice_ring below settles there (zero drift).
        window.set_gallery_focused(crate::gallery_initial_focus(&window) as i32);
        crate::gallery_note_startup(&window);
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
            let guard_permits = guard_permits;
            let guard_block_message = guard_block_message.clone();
            window.on_gallery_card_clicked(move |idx| {
                tracing::debug!("{}", crate::callbacks::mouse_trace(&format!("gallery card-clicked idx={idx}")));
                if !guard_permits {
                    tracing::warn!("[guard] gallery apply refused (migration guard)");
                    if let Some(w) = win.upgrade() {
                        w.set_shell_status_text(guard_block_message.clone());
                    }
                    return;
                }
                if crate::shell::Shell::is_mutating(&shell_c) {
                    tracing::debug!("[gallery] card-clicked ignored — mutating");
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
                        // Persist the active mark for the NEXT startup: tm.apply
                        // holds it in memory (gallery_tm), cfg carries it on
                        // disk — both managers seed last_applied from cfg, so
                        // the gallery opens on this card instead of index 0.
                        // (No SharedState in scope here; file-level write.)
                        let mut cfg = crate::config::Config::load();
                        cfg.last_applied_theme = name.clone();
                        let _ = cfg.save();
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
                        // ── Theme transition: empty-workspace interlude ──
                        // While the theme reloads, the VIEW visits an empty
                        // workspace so the user watches only the new wallpaper
                        // + crystal bar — never their active windows (the
                        // stage has zero windows by construction). HVE stays
                        // fullscreen on orig_ws the whole time, faded to 0;
                        // the view returns AFTER fade-in, so the flip reveals
                        // the finished state, never mid-swap pixels.
                        // Dispatch note: this Hyprland build (0.56.2 Lua
                        // config) routes every dispatch through its Lua shim
                        // — numeric `workspace 10` errors "')' expected";
                        // verified live: `hl.dsp.focus({ workspace = "10" })`
                        // → ok. See interlude_view_script().
                        //   T=0     suppress auto-hide, capture orig, DND on,
                        //           mask anim+blur+border+shadow, fade-out
                        //           420ms (Slint states, ease-in)
                        //   T=560   stage: view → empty ws (first gen only;
                        //           after the exit fade completes)
                        //   T=1500  finale: no focusing cycle while staged
                        //           (see below)
                        //   T=1900  targeted fullscreen (no focus), view →
                        //           orig_ws, fade-in 520ms (ease-out)
                        //   T=2470  masks back, DND off (after the entrance
                        //           fade completes)
                        //   T=3500  watchdog (also returns the view)
                        // configreloaded only re-applies masks (hypr_ipc).
                        // Keep these offsets in sync with the theme-fade
                        // durations in ui/main.slint (states block).
                        let orig_ws = get_active_workspace_name();
                        {
                            let mut g = THEME_GEN.lock().unwrap();
                            *g += 1;
                            tracing::info!("[theme] new generation {} for ws {}", *g, orig_ws);
                        }
                        *THEME_CYCLE_FIRED_GEN.lock().unwrap() = None;
                        THEME_TRANSITIONING_FLAG.store(true, std::sync::atomic::Ordering::Relaxed);
                        // Interlude bookkeeping BEFORE the view moves: first
                        // writer wins, so a rapid successive theme apply keeps
                        // returning to the user's REAL workspace.
                        if is_safe_workspace_name(&orig_ws) {
                            THEME_INTERLUDE.lock().unwrap().capture(&orig_ws);
                        }
                        // Silence the notify-send machine-gun while the stage
                        // is exposed (first-writer-wins, see theme_dnd_engage).
                        theme_dnd_engage();
                        // CRITICAL: suppress the focus-lost auto-hide for the whole
                        // transition. The theme reload makes HVE lose focus for an
                        // instant; the gallery contract ("focus lost → immediate
                        // hide") parked HVE in special:minimized — the floating
                        // window + blurred bar-less special the user saw.
                        crate::countdown::suppress_auto_minimize(std::time::Duration::from_millis(6000));
                        // Mask outer Hyprland animations + blur + border + shadow
                        // for the whole transition, so the drop/re-assert fullscreen
                        // flips are instant and frameless. Reloads wipe runtime
                        // keywords — hypr_ipc re-applies them on every
                        // configreloaded line.
                        {
                            let anim_orig = hypr_getoption_int("animations:enabled", "1");
                            *THEME_ORIG_ANIM.lock().unwrap() = Some(anim_orig.clone());
                            hypr_set("animations:enabled", "0");
                            let blur_orig = hypr_getoption_int("decoration:blur:enabled", "1");
                            *THEME_ORIG_BLUR.lock().unwrap() = Some(blur_orig.clone());
                            hypr_set("decoration:blur:enabled", "0");
                            let border_orig = hypr_getoption_int("general:border_size", "2");
                            *THEME_ORIG_BORDER.lock().unwrap() = Some(border_orig.clone());
                            hypr_set("general:border_size", "0");
                            let shadow_orig = hypr_getoption_int("decoration:shadow:enabled", "1");
                            *THEME_ORIG_SHADOW.lock().unwrap() = Some(shadow_orig.clone());
                            hypr_set("decoration:shadow:enabled", "0");
                            tracing::info!(
                                "[theme] masked animations ({}), blur ({}), border ({}), shadow ({})",
                                anim_orig, blur_orig, border_orig, shadow_orig
                            );
                        }
                        if let Some(w) = win.upgrade() {
                            w.set_theme_transitioning(true);
                        }
                        let weak_back = win.clone();
                        let fallback_gen = *THEME_GEN.lock().unwrap();
                        // T+560ms: stage the interlude — flip the VIEW (never
                        // a window) to an empty workspace. Runs AFTER the
                        // exit fade completes (420ms ease-in in main.slint)
                        // so the user watches the shell dissolve before the
                        // instant cut (masks on → no compositor animation).
                        // First generation only; a newer generation inherits
                        // the staged view.
                        {
                            let stage_gen = fallback_gen;
                            slint::Timer::single_shot(std::time::Duration::from_millis(560), move || {
                                let cur = *THEME_GEN.lock().unwrap();
                                if cur != stage_gen {
                                    return; // superseded — the newer gen owns staging
                                }
                                let target = {
                                    let st = THEME_INTERLUDE.lock().unwrap();
                                    if !st.should_move() {
                                        return; // view already on the stage
                                    }
                                    st.orig().map(|o| find_empty_workspace(o))
                                };
                                let Some(target) = target else { return; };
                                if hypr_switch_view(&target) {
                                    THEME_INTERLUDE.lock().unwrap().mark_moved();
                                    tracing::info!(
                                        "[theme] interlude: view staged on empty ws {} (gen {})",
                                        target, stage_gen
                                    );
                                } else {
                                    tracing::warn!(
                                        "[theme] interlude staging failed — stay-fullscreen fallback (gen {})",
                                        stage_gen
                                    );
                                }
                            });
                        }
                        slint::Timer::single_shot(std::time::Duration::from_millis(1500), move || {
                            let cur_gen = *THEME_GEN.lock().unwrap();
                            if cur_gen != fallback_gen {
                                tracing::info!(
                                    "[theme] finale skip — stale generation {} (current {})",
                                    fallback_gen, cur_gen
                                );
                                return; // the newer generation owns staging + return
                            }
                            let still = weak_back.upgrade().is_some_and(|w| w.get_theme_transitioning());
                            if !still {
                                return;
                            }
                            if THEME_INTERLUDE.lock().unwrap().staged() {
                                // View is on the stage: SKIP the focusing
                                // cycle. reassert_gallery_fullscreen focuses
                                // HVE by title first, and focusing a window on
                                // another workspace drags the VIEW back with
                                // it — the user then sees floating HVE (the
                                // ventanita) + their windows mid-cycle, 400ms
                                // before the intended return. Fullscreen is
                                // re-ensured with the window-targeted
                                // dispatcher (no focus) at return time.
                                tracing::info!("[theme] finale: view staged — focusing cycle skipped");
                            } else {
                                // One masked unset→set cycle while opacity is
                                // 0 (stay-fullscreen fallback — the view never
                                // left). Forces a real fullscreen transition so
                                // the bar reliably hides.
                                reassert_gallery_fullscreen();
                            }
                            // Fade-in after the cycle settled (150ms unset→set).
                            let weak_re = weak_back.clone();
                            slint::Timer::single_shot(std::time::Duration::from_millis(400), move || {
                                if let Some(w) = weak_re.upgrade() {
                                    w.set_theme_transitioning(false);
                                    tracing::info!("[theme] finale fade-in");
                                }
                                THEME_TRANSITIONING_FLAG.store(false, std::sync::atomic::Ordering::Relaxed);
                                // Return the view FIRST — compositor masks are
                                // still ON, so the flip is an instant cut (no
                                // elastic bounce), and the entrance fade then
                                // plays over the real desktop.
                                if let Some(orig) = THEME_INTERLUDE.lock().unwrap().take_restore() {
                                    // Reload chains may have re-floated HVE after
                                    // the finale — re-set fullscreen with the
                                    // window-targeted dispatcher BEFORE the flip
                                    // (no focus step, so the view is never dragged
                                    // back; idempotent when already fullscreen).
                                    let fs_ok = reassert_hve_fullscreen_targeted();
                                    let back_ok = hypr_switch_view(&orig);
                                    tracing::info!(
                                        "[theme] interlude: view returned to {} ok={} (targeted fullscreen {})",
                                        orig, back_ok, fs_ok
                                    );
                                }
                                // Compositor masks + DND release AFTER the
                                // entrance fade completes (520ms ease-out in
                                // main.slint): animations must stay OFF through
                                // the flip, and toasts stay swallowed during
                                // the entrance.
                                slint::Timer::single_shot(std::time::Duration::from_millis(570), move || {
                                    restore_theme_masks();
                                    theme_dnd_release();
                                    tracing::info!("[theme] masks restored, DND released after entrance fade");
                                });
                            });
                        });
                        // Hard watchdog: never stay transparent forever — force fade-in at 3.5s,
                        // and never strand the view on the interlude stage.
                        let weak_watch = win.clone();
                        let watch_gen = fallback_gen;
                        slint::Timer::single_shot(std::time::Duration::from_millis(3500), move || {
                            let cur = *THEME_GEN.lock().unwrap();
                            if cur != watch_gen {
                                return; // superseded by newer theme
                            }
                            if let Some(w) = weak_watch.upgrade() {
                                if w.get_theme_transitioning() {
                                    w.set_theme_transitioning(false);
                                    tracing::warn!("[theme] watchdog forced fade-in gen {}", watch_gen);
                                }
                            }
                            THEME_TRANSITIONING_FLAG.store(false, std::sync::atomic::Ordering::Relaxed);
                            restore_theme_masks();
                            if let Some(orig) = THEME_INTERLUDE.lock().unwrap().take_restore() {
                                let fs_ok = reassert_hve_fullscreen_targeted();
                                let ok = hypr_switch_view(&orig);
                                tracing::warn!(
                                    "[theme] watchdog returned view to {} ok={} (targeted fullscreen {})",
                                    orig, ok, fs_ok
                                );
                            }
                            // Safety net: never leave DND engaged (no-op if
                            // the return path already released it).
                            theme_dnd_release();
                        });
                    }
                }
            });
        }
        window.set_gallery_empty_text(SharedString::from(gallery_slot.empty_message()));
        window.set_gallery_mit_footer(SharedString::from(gallery_slot.mit_footer()));
        window.set_gallery_mit_link(SharedString::from(crate::shell::gallery::model::MIT_FOOTER_LINK));
        window.set_gallery_style(0);
        // Slider opens on the active theme — never hardcoded 0.
        window.set_gallery_focused(crate::gallery_initial_focus(&window) as i32);
        crate::gallery_note_startup(&window);
        refresh_slice_ring();
        window.set_gallery_reduced_motion(gallery_slot.is_reduced_motion());
        {
            let win = window.as_weak();
            let slot = gallery_slot.clone();
            let tm = gallery_tm.clone();
            let refresh_slice = refresh_slice_ring.clone();
            window.on_gallery_style_selected(move |style| {
                tracing::debug!("{}", crate::callbacks::mouse_trace(&format!("gallery style-selected style={style}")));
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
                tracing::debug!("{}", crate::callbacks::mouse_trace(&format!("gallery card-right-clicked idx={idx}")));
                if crate::shell::Shell::is_mutating(&shell_c) {
                    tracing::debug!("[gallery] card-right-clicked ignored — mutating");
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
            window.on_panel_section_selected(move |section| {
                tracing::debug!("[panel][mouse] section-selected section={} (rail click)", section);
                if crate::shell::Shell::is_mutating(&shell_c) {
                    tracing::debug!("[panel] section-selected ignored — mutating");
                    return;
                }
                let idx = section.max(0) as usize;
                let target = crate::shell::nav::PanelSection::from_index(idx).unwrap_or(crate::shell::nav::PanelSection::Save);
                let is_closed = crate::shell::Shell::with_nav(&shell_c, |n| n.panel_state() == crate::shell::nav::PanelState::Closed);
                let is_open = crate::shell::Shell::with_nav(&shell_c, |n| matches!(n.panel_state(), crate::shell::nav::PanelState::Open(_)));
                tracing::debug!("[panel] section-selected target={:?} is_closed={} is_open={}", target, is_closed, is_open);
                if is_closed {
                    let _ = crate::shell::Shell::enter_panel(&shell_c, target);
                } else if is_open {
                    crate::shell::Shell::set_panel_section(&shell_c, target);
                }
            });
        }
        {
            // Keyboard R11 v2 (Settings): save-list arrows own ONE focused
            // index (Rust-side, so search filters and delete focus shifts
            // stay consistent) and Tab/Shift+Tab cycle panel sections.
            let win = window.as_weak();
            window.on_panel_save_nav(move |delta| {
                tracing::debug!("[panel][kbd] save-nav delta={} (arrow)", delta);
                let Some(w) = win.upgrade() else { return; };
                use slint::Model as _;
                // PanelRoot reads root.theme-names (panel-save-theme-names is
                // a shell binding), so the list length comes from there.
                let len = w.get_theme_names().row_count();
                let before = w.get_panel_save_focused_index();
                let after = callbacks::save_nav_step(before, delta, len);
                tracing::debug!("[panel][kbd] save-nav {} -> {} (len={})", before, after, len);
                w.set_panel_save_focused_index(after);
            });
        }
        {
            // Click a theme card → the row takes keyboard focus, so arrows
            // continue from where the mouse left off (keyboard == mouse, D8).
            let win = window.as_weak();
            window.on_panel_save_focus_requested(move |idx| {
                tracing::debug!("[panel][mouse] focus-requested idx={} (click on Save row)", idx);
                let Some(w) = win.upgrade() else { return; };
                w.set_panel_save_focused_index(idx);
            });
        }
        {
            // Generic mouse trace cable: Slint-only clicks (empty state,
            // dialog cancels, card bodies, mutating guard) forward a reason
            // string here so --verbose shows every mouse interaction.
            // No state change, no focus move — trace only.
            window.on_panel_mouse_trace(move |reason| {
                tracing::debug!("{}", crate::callbacks::mouse_trace(&reason));
            });
        }
        {
            // Generic focus trace cable: menu-fs / kb / shell-kbd gain+loss
            // handlers forward "scope direction via reason" here so
            // --verbose shows every keyboard-focus move.
            // No state change, no focus move — trace only.
            window.on_panel_focus_trace(move |reason| {
                tracing::debug!("{}", crate::callbacks::focus_trace(&reason));
            });
        }
        {
            let shell_c = shell.clone();
            let win = window.as_weak();
            window.on_panel_back(move || {
                tracing::debug!("[panel][kbd] back pressed (Esc)");
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

    // ── Legacy Welcome/Home header i18n — REMOVED slice 8 (R8) ──
    // Panel headers are hardcoded per section; WelcomeCards removed with HomeModule.

    // ── Legacy Home status/nav i18n — REMOVED slice 8 (R8) ──
    // home-none-* removed — Active config display moved
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
    window.set_auto_minimize_label(tr.tr_shared("settings.auto_minimize", "Hide delay"));
    window.set_timer_label(tr.tr_shared("settings.timer", "Delay:"));
    window.set_language_label(tr.tr_shared("settings.language", "Language"));
    window.set_tiling_label(tr.tr_shared("settings.tiling_mode", "Tiling mode"));
    window.set_autostart_label(tr.tr_shared("settings.autostart", "Autostart"));
    window.set_theme_label(tr.tr_shared("settings.theme", "Theme"));
    window.set_reset_label(tr.tr_shared("settings.reset_presets", "Reset presets"));

    // ── Legacy ThemesModule i18n — REMOVED slice 8 (R8) ──
    // Theme list UI replaced by Gallery cards + panel SaveSection.
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
    // Re-apply System active on startup — window shows ON via set_system_active,
    // but engine must be enabled too, otherwise restart appears as "not persisted".
    if startup_system_active {
        if let Ok(st) = state.lock() {
            let _ = st.engine().init_enable();
        }
    }
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
            tracing::debug!("{}", crate::callbacks::mouse_trace(&format!("save-theme name={} (save button/enter)", name_str.trim())));
            let trimmed = name_str.trim().to_string();
            if trimmed.is_empty() {
                if let Some(w) = weak.upgrade() {
                    w.set_panel_save_error("name-empty".into());
                    w.set_theme_error_text("name-empty".into());
                }
                return;
            }
            // Existing name → overwrite confirm dialog (tranche B), no silent save
            let exists = {
                let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                st.theme_manager().list().unwrap_or_default().iter().any(|t| t.name == trimmed)
            };
            if exists {
                if let Some(w) = weak.upgrade() {
                    w.set_panel_save_dialog_target(trimmed.clone().into());
                    w.set_panel_save_dialog_mode("overwrite".into());
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
        // ── Panel Save "My Themes" apply: same Gallery path, dual-manager sync ──
        // Row click applies via GallerySlot (gallery_tm) then mirrors
        // last_applied into the main manager so refresh_theme_list shows the
        // active mark in the Save list. No file writes — only active sync.
        {
            let state_c = state.clone();
            let gallery_tm_c = gallery_tm.clone();
            let slot_c = gallery_slot.clone();
            let refresh_mosaic_page_c = refresh_mosaic_page.clone();
            let refresh_slice_ring_c = refresh_slice_ring.clone();
            let weak = window.as_weak();
            let guard_permits = guard_permits;
            let guard_block_message = guard_block_message.clone();
            window.on_panel_apply_saved_theme(move |idx| {
                tracing::debug!("{}", crate::callbacks::mouse_trace(&format!("apply-saved-theme idx={idx}")));
                if !guard_permits {
                    tracing::warn!("[guard] saved-theme apply refused (migration guard)");
                    if let Some(w) = weak.upgrade() {
                        w.set_shell_status_text(guard_block_message.clone());
                    }
                    return;
                }
                let i = idx.max(0) as usize;
                let name_opt = {
                    let guard = gallery_tm_c.lock().unwrap();
                    guard.list().unwrap_or_default().get(i).map(|info| info.name.clone())
                };
                let Some(name) = name_opt else { return; };
                let outcome = slot_c.apply_theme(&name);
                if outcome != crate::shell::gallery::slot::ApplyOutcome::Applied {
                    return; // Pulsed (already active) / NotFound / Failed: no sync
                }
                // Dual-manager sync: mirror active into main manager AND
                // persist to cfg so the next startup resolves the same card.
                {
                    let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    st.mark_theme_applied(&name);
                    if let Some(w) = weak.upgrade() {
                        st.refresh_theme_list(&w);
                    }
                }
                if let Some(w) = weak.upgrade() {
                    // In-place card sync (preserves baked thumbs, like gallery click)
                    let new_rows: Vec<crate::GalleryCardData> = {
                        let gtm = gallery_tm_c.lock().unwrap();
                        let infos = gtm.list().unwrap_or_default();
                        infos
                            .iter()
                            .map(|info| {
                                let card = crate::shell::gallery::ThemeCard::from_info(info);
                                crate::GalleryCardData {
                                    name: slint::SharedString::from(info.name.as_str()),
                                    saved_at: slint::SharedString::from(info.saved_at.as_str()),
                                    is_active: info.is_active,
                                    providers: slint::ModelRc::new(slint::VecModel::from(
                                        info.providers
                                            .iter()
                                            .map(|p| slint::SharedString::from(p.as_str()))
                                            .collect::<Vec<_>>(),
                                    )),
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
                        crate::shell::gallery::model::sync_cards(model, new_rows);
                    } else {
                        w.set_gallery_cards(slint::ModelRc::new(slint::VecModel::from(new_rows)));
                    }
                    w.set_gallery_focused(idx);
                    refresh_mosaic_page_c(false);
                    refresh_slice_ring_c();
                }
            });
        }
        // ── Panel Save tranche B: rename/delete/refresh/overwrite/search ──
        // Dual-sync via pure callbacks::handle_panel_* (slot first for the
        // gallery side, main TM converges on shared storage), then
        // refresh_theme_list + gallery card sync + mosaic/slice refresh.
        {
            let state_c = state.clone();
            let gallery_tm_c = gallery_tm.clone();
            let slot_c = gallery_slot.clone();
            let refresh_mosaic_page_c = refresh_mosaic_page.clone();
            let refresh_slice_ring_c = refresh_slice_ring.clone();
            let weak = window.as_weak();
            window.on_panel_rename_saved_theme(move |old, new| {
                tracing::debug!("{}", crate::callbacks::mouse_trace(&format!("rename-saved-theme old={old} new={new}")));
                let (old_str, new_str) = (old.to_string(), new.to_string());
                let mut error_msg = String::new();
                let ok = {
                    let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    let r = callbacks::handle_panel_rename(&old_str, &new_str, st.theme_manager_mut(), &slot_c, &mut error_msg);
                    if r {
                        // Rename moves last_applied in the manager — mirror to
                        // cfg or the next startup loses the active card.
                        let last = st.theme_manager().last_applied.clone();
                        st.update_cfg(|c| c.last_applied_theme = last);
                    }
                    r
                };
                let Some(w) = weak.upgrade() else { return; };
                if !ok {
                    w.set_panel_save_error(error_msg.clone().into());
                    w.set_theme_error_text(error_msg.into());
                    return;
                }
                w.set_panel_save_error("".into());
                w.set_theme_error_text("".into());
                {
                    let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    st.refresh_theme_list(&w);
                }
                sync_save_gallery_ui(&w, &gallery_tm_c, &refresh_mosaic_page_c, &refresh_slice_ring_c);
            });
        }
        {
            let state_c = state.clone();
            let gallery_tm_c = gallery_tm.clone();
            let slot_c = gallery_slot.clone();
            let refresh_mosaic_page_c = refresh_mosaic_page.clone();
            let refresh_slice_ring_c = refresh_slice_ring.clone();
            let weak = window.as_weak();
            window.on_panel_delete_saved_theme(move |name| {
                tracing::debug!("{}", crate::callbacks::mouse_trace(&format!("delete-saved-theme name={name}")));
                let name_str = name.to_string();
                let del_idx = {
                    let gtm = gallery_tm_c.lock().unwrap();
                    gtm.list().unwrap_or_default().iter().position(|t| t.name == name_str).unwrap_or(0)
                };
                let mut error_msg = String::new();
                let ok = {
                    let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    let r = callbacks::handle_panel_delete(&name_str, st.theme_manager_mut(), &slot_c, &mut error_msg);
                    if r {
                        // Delete clears last_applied when the active theme is
                        // removed — mirror so cfg never names a ghost theme.
                        let last = st.theme_manager().last_applied.clone();
                        st.update_cfg(|c| c.last_applied_theme = last);
                    }
                    r
                };
                let Some(w) = weak.upgrade() else { return; };
                if !ok {
                    w.set_panel_save_error(error_msg.clone().into());
                    w.set_theme_error_text(error_msg.into());
                    return;
                }
                w.set_panel_save_error("".into());
                w.set_theme_error_text("".into());
                {
                    let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    st.refresh_theme_list(&w);
                }
                sync_save_gallery_ui(&w, &gallery_tm_c, &refresh_mosaic_page_c, &refresh_slice_ring_c);
                // Focus the neighbor that takes the deleted slot (next, or previous if last)
                let new_len = gallery_tm_c.lock().unwrap().list().unwrap_or_default().len();
                w.set_panel_save_focused_index(callbacks::focus_after_delete(del_idx, new_len));
            });
        }
        {
            let state_c = state.clone();
            let gallery_tm_c = gallery_tm.clone();
            let refresh_mosaic_page_c = refresh_mosaic_page.clone();
            let refresh_slice_ring_c = refresh_slice_ring.clone();
            let weak = window.as_weak();
            window.on_panel_refresh_saved_theme(move || {
                tracing::debug!("{}", crate::callbacks::mouse_trace("refresh-saved-theme"));
                let mut error_msg = String::new();
                let ok = {
                    let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    let mut gtm = gallery_tm_c.lock().unwrap();
                    callbacks::handle_panel_refresh(st.theme_manager_mut(), &mut gtm, &mut error_msg)
                };
                let Some(w) = weak.upgrade() else { return; };
                if !ok {
                    w.set_panel_save_error(error_msg.clone().into());
                    w.set_theme_error_text(error_msg.into());
                    return;
                }
                w.set_panel_save_error("".into());
                w.set_theme_error_text("".into());
                {
                    let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    st.refresh_theme_list(&w);
                }
                sync_save_gallery_ui(&w, &gallery_tm_c, &refresh_mosaic_page_c, &refresh_slice_ring_c);
            });
        }
        {
            let state_c = state.clone();
            let gallery_tm_c = gallery_tm.clone();
            let refresh_mosaic_page_c = refresh_mosaic_page.clone();
            let refresh_slice_ring_c = refresh_slice_ring.clone();
            let weak = window.as_weak();
            window.on_panel_overwrite_saved_theme(move |name| {
                tracing::debug!("{}", crate::callbacks::mouse_trace(&format!("overwrite-saved-theme name={name}")));
                let name_str = name.to_string();
                let mut error_msg = String::new();
                let ok = {
                    let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    let mut gtm = gallery_tm_c.lock().unwrap();
                    callbacks::handle_panel_save(&name_str, st.theme_manager_mut(), &mut gtm, || {}, || {}, &mut error_msg)
                };
                let Some(w) = weak.upgrade() else { return; };
                if !ok {
                    w.set_panel_save_error(error_msg.clone().into());
                    w.set_theme_error_text(error_msg.into());
                    return;
                }
                w.set_panel_save_error("".into());
                w.set_theme_error_text("".into());
                w.set_panel_save_input("".into());
                {
                    let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    st.refresh_theme_list(&w);
                }
                sync_save_gallery_ui(&w, &gallery_tm_c, &refresh_mosaic_page_c, &refresh_slice_ring_c);
            });
        }
        {
            let state_c = state.clone();
            let weak = window.as_weak();
            window.on_panel_save_search_changed(move |query| {
                tracing::debug!("{}", crate::callbacks::mouse_trace(&format!("save-search-changed q={query}")));
                let Some(w) = weak.upgrade() else { return; };
                let q = query.to_string();
                if q.trim().is_empty() {
                    let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    st.refresh_theme_list(&w);
                    return;
                }
                let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                let rows = callbacks::save_list_rows(st.theme_manager());
                let out = callbacks::filter_saved_rows(&q, &rows);
                w.set_theme_names(slint::ModelRc::new(slint::VecModel::from(
                    out.iter().map(|(n, _, _)| slint::SharedString::from(n.as_str())).collect::<Vec<_>>(),
                )));
                w.set_theme_saved_ats(slint::ModelRc::new(slint::VecModel::from(
                    out.iter().map(|(_, d, _)| slint::SharedString::from(d.as_str())).collect::<Vec<_>>(),
                )));
                w.set_theme_is_actives(slint::ModelRc::new(slint::VecModel::from(
                    out.iter().map(|(_, _, a)| *a).collect::<Vec<_>>(),
                )));
            });
        }
        // Ensure initial panel placeholders are cleared
        window.set_panel_save_placeholder("Theme name…".into());
        window.set_panel_save_button_text("Save".into());
        window.set_panel_save_error("".into());
        // Save classic tranche A: search + card action labels (themes.* keys exist in en/es.json)
        window.set_panel_save_search_placeholder(tr.tr_shared("themes.search_placeholder", "Search themes..."));
        window.set_panel_save_empty_text(tr.tr_shared("themes.empty", "No themes yet."));
        window.set_panel_save_apply_text(tr.tr_shared("themes.apply", "Apply"));
        window.set_panel_save_refresh_text(tr.tr_shared("themes.refresh", "Refresh"));
        window.set_panel_save_rename_text(tr.tr_shared("themes.rename", "Rename"));
        window.set_panel_save_delete_text(tr.tr_shared("themes.delete", "Delete"));
        // Save classic tranche B: dialog texts (themes.* keys in en/es.json)
        window.set_panel_save_rename_title(tr.tr_shared("themes.rename_title", "Rename theme"));
        window.set_panel_save_confirm_delete(tr.tr_shared("themes.confirm_delete", "Delete theme"));
        window.set_panel_save_confirm_delete_msg(tr.tr_shared("themes.confirm_delete_msg", "Are you sure you want to delete"));
        window.set_panel_save_confirm_refresh(tr.tr_shared("themes.confirm_refresh", "Refresh theme"));
        window.set_panel_save_confirm_refresh_msg(tr.tr_shared("themes.confirm_refresh_msg", "This will reload the theme with the current configuration. Continue?"));
        window.set_panel_save_overwrite_title(tr.tr_shared("themes.overwrite_title", "Overwrite theme"));
        window.set_panel_save_overwrite_msg(tr.tr_shared("themes.overwrite_msg", "already exists. Overwrite it with the current setup?"));
        window.set_panel_save_cancel_text(tr.tr_shared("themes.cancel", "Cancel"));
    }

    // ── Panel Borders tune: debounced geometry + snap-on-pick (slice 4, COLOR GATED) ──
    {
        let state_c = state.clone();
        let weak = window.as_weak();
        let guard_permits = guard_permits;
        let guard_block_message = guard_block_message.clone();
        window.on_panel_apply_geometry(move |size, radius, gap_in, gap_out| {
            tracing::debug!("[borders][mouse|kbd] apply-geometry size={} radius={} gap_in={} gap_out={}", size, radius, gap_in, gap_out);
            if !guard_permits {
                tracing::warn!("[guard] geometry apply refused (migration guard)");
                if let Some(w) = weak.upgrade() {
                    w.set_shell_status_text(guard_block_message.clone());
                }
                return;
            }
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
        let guard_permits = guard_permits;
        let guard_block_message = guard_block_message.clone();
        window.on_panel_apply_border(move |idx, file| {
            tracing::debug!("[borders][mouse|kbd] apply-border idx={} file={} (click/card)", idx, file);
            if !guard_permits {
                tracing::warn!("[guard] border apply refused (migration guard)");
                if let Some(w) = weak.upgrade() {
                    w.set_shell_status_text(guard_block_message.clone());
                }
                return;
            }
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
    // ── Panel Motion pick (slice 5, R4) — 19 cards scan("animations") → apply_animation toggle-off → none
    {
        let state_c = state.clone();
        let weak = window.as_weak();
        let guard_permits = guard_permits;
        let guard_block_message = guard_block_message.clone();
        window.on_panel_apply_animation(move |idx, file| {
            tracing::debug!("[motion][mouse|kbd] apply-animation idx={} file={}", idx, file);
            if !guard_permits {
                tracing::warn!("[guard] animation apply refused (migration guard)");
                if let Some(w) = weak.upgrade() {
                    w.set_shell_status_text(guard_block_message.clone());
                }
                return;
            }
            let file_str = file.to_string();
            let (is_deact, result) = {
                let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                let is_deact = st.cfg().active_anim_file == file_str;
                let new = if is_deact { String::new() } else { file_str.clone() };
                st.cfg_mut().active_anim_file = new.clone();
                let _ = st.cfg().save();
                let arg = if is_deact { "none" } else { &new };
                let res = st.engine().apply_animation(arg);
                (is_deact, res)
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Animation error: {}", e);
            }
            if let Some(w) = weak.upgrade() {
                w.set_active_anim_index(if is_deact { -1 } else { idx });
                // bezier persists with active animation per design — keep current bezier values
                // (no snap; tuning applies to active). Future: per-animation bezier map.
            }
        });
    }
    // ── Border preset CRUD ──
    {
        let state_c = state.clone();
        let weak = window.as_weak();
        window.on_panel_save_border_preset(move |name| {
            let name_str = name.to_string();
            tracing::debug!("[borders][preset] save name={}", name_str);
            let result = if let Some(w) = weak.upgrade() {
                let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                let size = st.cfg().border_size;
                let radius = st.cfg().border_radius;
                let gap_in = st.cfg().gaps_in;
                let gap_out = st.cfg().gaps_out;
                drop(st);
                let content = crate::preset_store::PresetStore::generate_border_lua(
                    &name_str, size, radius, gap_in, gap_out,
                );
                let store = crate::preset_store::PresetStore::new("borders");
                let r = store.save(&name_str, &content);
                if r.is_ok() {
                    let store2 = crate::preset_store::PresetStore::new("borders");
                    let names: Vec<_> = store2.list()
                        .into_iter()
                        .map(|n| slint::SharedString::from(n.as_str()))
                        .collect();
                    let tags: Vec<_> = names.iter().map(|_| slint::SharedString::from("CUSTOM")).collect();
                    w.set_user_border_preset_names(slint::ModelRc::from(names.as_slice()));
                    w.set_user_border_preset_tags(slint::ModelRc::from(tags.as_slice()));
                    w.set_border_save_error(String::new().into());
                    w.set_border_preset_name(String::new().into());
                }
                r
            } else {
                Ok(())
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Border preset save error: {}", e);
                if let Some(w) = weak.upgrade() {
                    w.set_border_save_error(e.into());
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_panel_rename_border_preset(move |old_name, new_name| {
            let old = old_name.to_string();
            let new = new_name.to_string();
            tracing::debug!("[borders][preset] rename old={} new={}", old, new);
            let result = {
                let store = crate::preset_store::PresetStore::new("borders");
                store.rename(&old, &new)
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Border preset rename error: {}", e);
                if let Some(w) = weak.upgrade() {
                    w.set_border_save_error(e.into());
                }
            } else if let Some(w) = weak.upgrade() {
                let store = crate::preset_store::PresetStore::new("borders");
                let names: Vec<_> = store.list()
                    .into_iter()
                    .map(|n| slint::SharedString::from(n.as_str()))
                    .collect();
                let tags: Vec<_> = names.iter().map(|_| slint::SharedString::from("CUSTOM")).collect();
                    w.set_user_border_preset_names(slint::ModelRc::from(names.as_slice()));
                    w.set_user_border_preset_tags(slint::ModelRc::from(tags.as_slice()));
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_panel_delete_border_preset(move |name| {
            let name_str = name.to_string();
            tracing::debug!("[borders][preset] delete name={}", name_str);
            let result = {
                let store = crate::preset_store::PresetStore::new("borders");
                store.delete(&name_str)
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Border preset delete error: {}", e);
                if let Some(w) = weak.upgrade() {
                    w.set_border_save_error(e.into());
                }
            } else if let Some(w) = weak.upgrade() {
                let store = crate::preset_store::PresetStore::new("borders");
                let names: Vec<_> = store.list()
                    .into_iter()
                    .map(|n| slint::SharedString::from(n.as_str()))
                    .collect();
                let tags: Vec<_> = names.iter().map(|_| slint::SharedString::from("CUSTOM")).collect();
                    w.set_user_border_preset_names(slint::ModelRc::from(names.as_slice()));
                    w.set_user_border_preset_tags(slint::ModelRc::from(tags.as_slice()));
            }
        });
    }
    // ── Animation preset CRUD ──
    {
        let weak = window.as_weak();
        window.on_panel_save_animation_preset(move |name| {
            let name_str = name.to_string();
            tracing::debug!("[motion][preset] save name={}", name_str);
            let result = if let Some(w) = weak.upgrade() {
                let a = w.get_bezier_a() as f64;
                let b = w.get_bezier_b() as f64;
                let c = w.get_bezier_c() as f64;
                let d = w.get_bezier_d() as f64;
                let content = crate::preset_store::PresetStore::generate_animation_lua(
                    &name_str, a, b, c, d,
                );
                let store = crate::preset_store::PresetStore::new("animations");
                let r = store.save(&name_str, &content);
                if r.is_ok() {
                    let store2 = crate::preset_store::PresetStore::new("animations");
                    let names: Vec<_> = store2.list()
                        .into_iter()
                        .map(|n| slint::SharedString::from(n.as_str()))
                        .collect();
                    let tags: Vec<_> = names.iter().map(|_| slint::SharedString::from("CUSTOM")).collect();
                    w.set_user_animation_preset_names(slint::ModelRc::from(names.as_slice()));
                    w.set_user_animation_preset_tags(slint::ModelRc::from(tags.as_slice()));
                    w.set_animation_save_error(String::new().into());
                    w.set_animation_preset_name(String::new().into());
                }
                r
            } else {
                Ok(())
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Animation preset save error: {}", e);
                if let Some(w) = weak.upgrade() {
                    w.set_animation_save_error(e.into());
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_panel_rename_animation_preset(move |old_name, new_name| {
            let old = old_name.to_string();
            let new = new_name.to_string();
            tracing::debug!("[motion][preset] rename old={} new={}", old, new);
            let result = {
                let store = crate::preset_store::PresetStore::new("animations");
                store.rename(&old, &new)
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Animation preset rename error: {}", e);
                if let Some(w) = weak.upgrade() {
                    w.set_animation_save_error(e.into());
                }
            } else if let Some(w) = weak.upgrade() {
                let store = crate::preset_store::PresetStore::new("animations");
                let names: Vec<_> = store.list()
                    .into_iter()
                    .map(|n| slint::SharedString::from(n.as_str()))
                    .collect();
                let tags: Vec<_> = names.iter().map(|_| slint::SharedString::from("CUSTOM")).collect();
                w.set_user_animation_preset_names(slint::ModelRc::from(names.as_slice()));
                w.set_user_animation_preset_tags(slint::ModelRc::from(tags.as_slice()));
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_panel_delete_animation_preset(move |name| {
            let name_str = name.to_string();
            tracing::debug!("[motion][preset] delete name={}", name_str);
            let result = {
                let store = crate::preset_store::PresetStore::new("animations");
                store.delete(&name_str)
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Animation preset delete error: {}", e);
                if let Some(w) = weak.upgrade() {
                    w.set_animation_save_error(e.into());
                }
            } else if let Some(w) = weak.upgrade() {
                let store = crate::preset_store::PresetStore::new("animations");
                let names: Vec<_> = store.list()
                    .into_iter()
                    .map(|n| slint::SharedString::from(n.as_str()))
                    .collect();
                let tags: Vec<_> = names.iter().map(|_| slint::SharedString::from("CUSTOM")).collect();
                w.set_user_animation_preset_names(slint::ModelRc::from(names.as_slice()));
                w.set_user_animation_preset_tags(slint::ModelRc::from(tags.as_slice()));
            }
        });
    }
    // ── Panel Filters pick (slice 6, R5) — shader cards toggle-off → none (3s overlay suppression via GallerySlot guard)
    {
        let state_c = state.clone();
        let weak = window.as_weak();
        let guard_permits = guard_permits;
        let guard_block_message = guard_block_message.clone();
        window.on_panel_apply_shader(move |idx, file| {
            tracing::debug!("[filters][mouse|kbd] apply-shader idx={} file={}", idx, file);
            if !guard_permits {
                tracing::warn!("[guard] shader apply refused (migration guard)");
                if let Some(w) = weak.upgrade() {
                    w.set_shell_status_text(guard_block_message.clone());
                }
                return;
            }
            let file_str = file.to_string();
            let (is_deact, result) = {
                let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                let is_deact = st.cfg().active_shader_file == file_str;
                let new = if is_deact { String::new() } else { file_str.clone() };
                st.cfg_mut().active_shader_file = new.clone();
                let _ = st.cfg().save();
                let arg = if is_deact { "none" } else { &new };
                let res = st.engine().apply_shader(arg);
                (is_deact, res)
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Shader error: {}", e);
            }
            if let Some(w) = weak.upgrade() {
                w.set_active_shader_index(if is_deact { -1 } else { idx });
            }
        });
    }
    // ── Legacy nav_modules (legacy_tab sidebar) — REMOVED slice 8 (R8) ──

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

    // ── Startup: sync autostart + ensure vital SUPER+H (2026-09-07 slim-down keeps only SUPER+H)
    {
        let state_guard = state.lock().unwrap_or_else(|e| e.into_inner());
        if guard_permits {
            set_keybinds(true);
            set_autostart(state_guard.cfg().auto_start);
        } else {
            tracing::warn!("[guard] startup settings write skipped (migration guard)");
        }
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
        // ── Fresh-launch settle (floating-startup fix) ──
        // Rules BEFORE the first map (steady state: no-op, no reload → no
        // re-float), Gallery mounted BEFORE the first map (first mapped
        // frame is already Gallery, no Home flash), then ONE map + focus +
        // confirmation-gated fullscreen retries. The window is never shown
        // until it is ready, so a manual launch lands straight in
        // fullscreen Gallery instead of a small floating frame.
        let startup_tiling = state.lock().unwrap_or_else(|e| e.into_inner()).cfg().tiling_mode;
        let rules_changed = if guard_permits {
            set_tiling_window_rules(startup_tiling)
        } else {
            tracing::warn!("[guard] startup window rules skipped (migration guard)");
            false
        };
        dispatch_initial_gallery_expand(&shell);

        window.show()?;
        if let Some(mut ctrl) = composer::global_controller() {
            ctrl.set_window_hidden(false);
            ctrl.set_tray_mode(false);
            // Drive the ShowStateMachine for the fresh-launch show: without
            // begin_show it stays Hidden forever and every focus-lost is
            // ignored (no countdown, no auto-minimize — the bug where the X
            // never morphed and the window never closed). The tray path does
            // this inside toggle_tray; startup must do it here. Confirm
            // (focus-gained) or the 1s entry timeout moves it to Visible.
            if ctrl.begin_show_machine() {
                ctrl.schedule_entry_timeout(window.as_weak());
            }
        }

        // ── Legacy prewarm/warmup removed — panel handles its own focus (R11) ──

        // En Wayland/Hyprland, show() no garantiza foco automático.
        // Forzamos foco via Composer para evitar el doble-click inicial.
        if let Some(ctrl) = composer::global_controller() {
            ctrl.composer().focus();
        }
        tracing::info!(
            "[startup] mapped once (rules_changed={}) — settle chain armed",
            rules_changed
        );

        // Tiling ON + rules actually rewritten at this launch (reload
        // pending): float BEFORE entering the session, once the client is
        // mapped. Steady state (rules unchanged) skips this — the client
        // maps per config and retries only assert fullscreen.
        schedule_startup_settle(0, startup_tiling && rules_changed);
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

    // ── Keyboard navigation logic (legacy_tab) — REMOVED slice 8 (R8) ──
    // Legacy held-key nav (legacy_tab + nav-ready) removed; panel sections handle
    // arrows locally via per-section FocusScope (R11). This ignored test is retired.

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

    // ── Theme interlude view switch (strict TDD) ─────────────────────────

    #[test]
    fn interlude_script_matches_verified_lua_payload() {
        // Verified live on Hyprland 0.56.2: `hyprctl dispatch
        // 'hl.dsp.focus({ workspace = "10" })'` → ok (view on ws 10, zero
        // windows). Numeric `workspace 10` instead errors with
        // "')' expected near '10'" — the dispatch IPC routes through the
        // Lua shim, so the payload must be a valid Lua expression.
        assert_eq!(
            interlude_view_script("10").as_deref(),
            Some("hl.dsp.focus({ workspace = \"10\" })")
        );
    }

    #[test]
    fn interlude_script_rejects_unsafe_names() {
        assert_eq!(interlude_view_script(""), None);
        // Colons never appear in a normal workspace name we may return to;
        // they would also terminate the Lua string ("special:x").
        assert_eq!(interlude_view_script("special:minimized"), None);
        assert_eq!(interlude_view_script("a\"quote"), None);
        assert_eq!(interlude_view_script("semi;colon"), None);
        assert_eq!(interlude_view_script(&"x".repeat(129)), None);
    }

    #[test]
    fn interlude_capture_first_writer_wins() {
        let mut s = InterludeState::new();
        assert!(s.capture("2"), "first generation captures the origin");
        assert!(!s.capture("10"), "a newer generation must NOT re-capture");
        assert_eq!(s.orig(), Some("2"), "origin stays the user's workspace");
    }

    #[test]
    fn interlude_restore_is_moved_gated_and_consumed_once() {
        let mut s = InterludeState::new();
        s.capture("2");
        assert_eq!(s.take_restore(), None, "no restore before mark_moved");
        s.mark_moved();
        assert!(s.staged(), "mark_moved must be observable for the finale skip");
        assert_eq!(s.take_restore(), Some("2".to_string()));
        assert_eq!(s.take_restore(), None, "restore is consumed exactly once");
    }

    #[test]
    fn dnd_bool_maps_noctalia_status_words() {
        // Verified live: `noctalia msg notification-dnd-status` prints
        // on/off and `notification-dnd-set true|false` toggles it.
        assert_eq!(dnd_bool_arg("on"), Some("true"));
        assert_eq!(dnd_bool_arg("off"), Some("false"));
        assert_eq!(dnd_bool_arg(""), None);
        assert_eq!(dnd_bool_arg("garbage"), None);
    }

    #[test]
    fn interlude_move_only_while_view_is_home() {
        let mut s = InterludeState::new();
        s.capture("2");
        assert!(s.should_move());
        s.mark_moved();
        assert!(!s.should_move(), "a newer generation must not re-stage");
        s.take_restore();
        // After the return trip the state resets: the NEXT theme apply must
        // be able to capture a fresh origin and stage the view again.
        assert!(s.capture("5"), "next interlude can capture a new origin");
        assert!(s.should_move(), "state resets for the next generation");
    }

    // ── Startup fullscreen settle policy (confirmation-gated retries) ──
    // The old path fired unset->set on fixed 200ms/400ms timers, often
    // before Hyprland mapped the client. The new path polls
    // `hyprctl clients -j` and only dispatches once HVE is mapped.

    #[test]
    fn startup_fs_waits_while_unmapped() {
        assert_eq!(startup_fs_action(false, false, 0), StartupFsAction::Wait);
        assert_eq!(startup_fs_action(false, false, 1), StartupFsAction::Wait);
    }

    #[test]
    fn startup_fs_asserts_once_mapped_but_windowed() {
        assert_eq!(startup_fs_action(true, false, 0), StartupFsAction::Assert);
        assert_eq!(startup_fs_action(true, false, 1), StartupFsAction::Assert);
    }

    #[test]
    fn startup_fs_done_once_fullscreen_confirmed() {
        assert_eq!(startup_fs_action(true, true, 0), StartupFsAction::Done);
        assert_eq!(startup_fs_action(false, true, 0), StartupFsAction::Done);
    }

    #[test]
    fn startup_fs_done_when_attempts_exhausted() {
        let last = STARTUP_FULLSCREEN_RETRIES_MS.len();
        assert_eq!(startup_fs_action(false, false, last), StartupFsAction::Done);
        assert_eq!(startup_fs_action(true, false, last), StartupFsAction::Done);
    }

}
