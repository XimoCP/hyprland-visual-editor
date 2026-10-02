mod animation_preset;
mod app_state;
mod border_preset;
mod callbacks;
mod color_authority;
mod composer;
mod config;
mod config_guard;
mod config_markers;
mod countdown;
mod engine;
mod hypr_ipc;
mod ipc;
mod panel_i18n;
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
mod architecture_contract;
#[cfg(test)]
mod scripts_contract;
#[cfg(test)]
mod reload_coalescer;
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

/// D4: drop the hidden border draft and restore the active border (or no border
/// at all) by re-running the existing engine apply path, so the desktop never
/// keeps an unsaved draft behind the panel. Failures surface to the user.
fn teardown_border_draft(
    state: &std::sync::Mutex<AppState>,
    proj: &std::path::Path,
    weak: &slint::Weak<MainWindow>,
) {
    crate::preset_store::PresetStore::remove_draft(proj);
    let st = state.lock().unwrap_or_else(|e| e.into_inner());
    let active = st.cfg().active_border_file.clone();
    let arg = if active.is_empty() { "none".to_string() } else { active };
    if let Err(e) = st.engine().apply_border(&arg) {
        tracing::error!("[panel] re-apply after draft cleanup failed: {}", e);
        drop(st);
        if let Some(w) = weak.upgrade() {
            w.set_shell_status_text(slint::SharedString::from(&format!(
                "Border restore failed: {}",
                e
            )));
        }
    }
}

/// Default color cycle used by "add slot" so a freshly added slot is visibly
/// distinct from the previous ones (Hyprland allows 2..8 gradient colors).
const BORDER_SLOT_CYCLE: [&str; 6] = [
    "p:primary",
    "p:secondary",
    "p:tertiary",
    "p:error",
    "p:surface",
    "p:surface_lowest",
];

/// Read the `tune-active-colors` model back as plain encoded strings.
fn read_tune_slots(w: &crate::MainWindow) -> Vec<String> {
    let model = w.get_tune_active_colors();
    (0..model.row_count())
        .map(|i| model.row_data(i).unwrap_or_default().to_string())
        .collect()
}

/// Resolve one encoded tune colour into a real Slint colour. Slint cannot parse
/// "#rrggbbaa", so the pane needs actual colours to preview them and to seed
/// the colour picker. Token values reuse the `token-*` properties the theme
/// already pushed onto the window, so no palette round-trip is needed.
fn resolve_encoded_color(w: &crate::MainWindow, v: &str) -> slint::Color {
    use crate::border_preset::{BorderColor, PaletteToken as P};
    use crate::preset_store::PresetStore;
    match PresetStore::decode_colors(v).into_iter().next() {
        Some(BorderColor::Token(P::Primary)) => w.get_token_primary(),
        Some(BorderColor::Token(P::Secondary)) => w.get_token_secondary(),
        Some(BorderColor::Token(P::Tertiary)) => w.get_token_tertiary(),
        // A2: the engine's ColorScheme has no `error` field, so display falls
        // back to the accent value. Presets still store the bare token.
        Some(BorderColor::Token(P::Error)) => w.get_token_error(),
        Some(BorderColor::Token(P::Surface)) => w.get_token_surface(),
        Some(BorderColor::Token(P::SurfaceLowest)) => w.get_token_surface_lowest(),
        Some(BorderColor::Custom { r, g, b, a }) => slint::Color::from_argb_u8(a, r, g, b),
        None => slint::Color::from_argb_u8(0, 0, 0, 0),
    }
}

/// Fixed-size resolved-colour model so the pane can index it blindly without
/// bounds checks: 0..7 = active gradient slots, 8 = inactive, 9 = glow colour,
/// 10 = glow inactive colour. Rebuilt on load/add/remove and after a successful
/// tune apply. That last one matters: the picker only commits on pointer-up (or
/// on a discrete keypress), never per drag step, so the seed it re-reads is
/// always the value it just wrote — it cannot fight a colour being dragged,
/// which was the original reason this was rebuilt on load only.
const RESOLVED_COLORS_LEN: usize = 11;

fn refresh_resolved_colors(w: &crate::MainWindow) {
    let transparent = slint::Color::from_argb_u8(0, 0, 0, 0);
    let slots = read_tune_slots(w);
    let mut out: Vec<slint::Color> = Vec::with_capacity(RESOLVED_COLORS_LEN);
    for i in 0..8 {
        out.push(match slots.get(i) {
            Some(s) => resolve_encoded_color(w, s),
            None => transparent,
        });
    }
    out.push(resolve_encoded_color(w, &w.get_tune_inactive_color().to_string()));
    out.push(resolve_encoded_color(w, &w.get_tune_glow_color().to_string()));
    out.push(resolve_encoded_color(w, &w.get_tune_glow_color_inactive().to_string()));
    w.set_tune_colors_resolved(slint::ModelRc::from(out.as_slice()));
}

/// Sync the Borders tune pane from a border preset file — the ONE shared
/// path every border apply uses to populate the right-hand tune controls
/// (colour chips, gradient angle, inactive colour, glow, animation leaves,
/// and the geometry sliders).
///
/// Callers: the manual card click (`on_panel_apply_border`, which the IPC
/// `next-border` path routes through), both theme-apply paths (gallery
/// card click, panel Save), and the entry/startup seed
/// (`seed_borders_pane_from_persisted`). Sharing this body is what keeps the
/// theme paths from drifting behind the manual one again.
///
/// `border_file` is the preset file name with extension (e.g.
/// `"13_the_joker.lua"`); empty means deactivated and clears the pane,
/// exactly like a manual deselect. `geometry` is the PERSISTED geometry the
/// caller read from `Config` (`BorderGeometry::from_config`) — the same values
/// Hyprland renders. The pane must show those, not a per-preset lookup: a
/// border preset's identity is not its geometry, and the old table's keys
/// matched no shipped preset. This only touches the window: engine/config
/// writes stay with the callers (the manual apply owns its toggle; theme
/// applies arrive with the provider's own apply already persisted on disk,
/// which the caller reloaded first).
pub(crate) fn sync_border_tune_pane(
    w: &crate::MainWindow,
    proj: &std::path::Path,
    border_file: &str,
    geometry: crate::callbacks::BorderGeometry,
) {
    // Picking a preset gives the tune pane its apply target, so the
    // "select a border preset first" refusal hint no longer applies.
    if w.get_border_save_error() == TUNE_NEEDS_ACTIVE_BORDER {
        w.set_border_save_error(String::new().into());
    }
    if border_file.is_empty() {
        // Deselect: clear tune properties. Geometry is deliberately left
        // alone — it is persisted state, not part of the border identity.
        w.set_tune_color_count(0);
        w.set_tune_active_colors(slint::ModelRc::default());
        w.set_tune_angle(90);
        w.set_tune_inactive_color(String::new().into());
        w.set_tune_glow(String::new().into());
        w.set_tune_glow_enabled(false);
        w.set_tune_glow_range(20);
        w.set_tune_glow_render_power(4);
        w.set_tune_glow_color("p:primary".into());
        w.set_tune_glow_color_inactive("p:surface_lowest".into());
        w.set_tune_anim_borderangle_enabled(false);
        w.set_tune_anim_borderangle_speed(30);
        w.set_tune_anim_borderangle_bezier("default".into());
        w.set_tune_anim_borderangle_style("".into());
        w.set_tune_anim_border_enabled(false);
        w.set_tune_anim_border_speed(30);
        w.set_tune_anim_border_bezier("default".into());
        w.set_tune_anim_border_style("".into());
        w.set_tune_anim_fadeshadow_enabled(false);
        w.set_tune_anim_fadeshadow_speed(30);
        w.set_tune_anim_fadeshadow_bezier("default".into());
        w.set_tune_anim_fadeshadow_style("".into());
        w.set_tune_anim_curves(slint::ModelRc::default());
        w.set_tune_rule_enabled(false);
        // Nothing is selected, so there is no unsaved state to flag.
        w.set_tune_dirty(false);
        w.set_tune_animations(String::new().into());
        refresh_resolved_colors(w);
        return;
    }
    // Geometry one-way (persisted state → sliders, never the reverse).
    w.set_border_size(geometry.size);
    w.set_corner_radius(geometry.radius);
    w.set_gap_in(geometry.gap_in);
    w.set_gap_out(geometry.gap_out);
    // Task 2.2: read .lua → parse → bulk-set tune properties
    let preset_name = border_file.strip_suffix(".lua").unwrap_or(border_file);
    if let Ok(content) = crate::preset_store::PresetStore::read_border_file(preset_name, proj) {
        let params = crate::border_preset::parse(&content);
        use crate::preset_store::PresetStore;
        let count = params.active_colors.len() as i32;
        w.set_tune_color_count(count);
        let color_entries: Vec<slint::SharedString> = params.active_colors.iter().map(|c| {
            slint::SharedString::from(PresetStore::encode_colors(&[c.clone()]).as_str())
        }).collect();
        w.set_tune_active_colors(slint::ModelRc::from(color_entries.as_slice()));
        w.set_tune_angle(params.angle);
        w.set_tune_inactive_color(slint::SharedString::from(PresetStore::encode_inactive(&params.inactive).as_str()));
        w.set_tune_glow(slint::SharedString::from(PresetStore::encode_glow(params.glow.as_ref()).as_str()));
        // Set individual glow properties for the UI
        if let Some(ref glow) = params.glow {
            w.set_tune_glow_enabled(glow.enabled);
            w.set_tune_glow_range(glow.range);
            w.set_tune_glow_render_power(glow.render_power);
            w.set_tune_glow_color(slint::SharedString::from(PresetStore::encode_colors(&[glow.color.clone()]).as_str()));
            w.set_tune_glow_color_inactive(slint::SharedString::from(PresetStore::encode_colors(&[glow.color_inactive.clone()]).as_str()));
        } else {
            w.set_tune_glow_enabled(false);
            w.set_tune_glow_range(20);
            w.set_tune_glow_render_power(4);
            w.set_tune_glow_color("p:primary".into());
            w.set_tune_glow_color_inactive("p:surface_lowest".into());
        }
        w.set_tune_rule_enabled(params.rule_enabled);
        // Freshly loaded state is the saved state: clear the
        // D4 unsaved marker the pane shows in its header.
        w.set_tune_dirty(false);
        w.set_tune_animations(slint::SharedString::from(PresetStore::encode_animations(&params.animations).as_str()));
        // Set individual animation leaf properties
        for leaf in &params.animations {
            match leaf.leaf.as_str() {
                "borderangle" => {
                    w.set_tune_anim_borderangle_enabled(leaf.enabled);
                    w.set_tune_anim_borderangle_speed(leaf.speed.unwrap_or(30));
                    w.set_tune_anim_borderangle_bezier(slint::SharedString::from(leaf.bezier.as_deref().unwrap_or("default")));
                    w.set_tune_anim_borderangle_style(slint::SharedString::from(leaf.style.as_deref().unwrap_or("")));
                }
                "border" => {
                    w.set_tune_anim_border_enabled(leaf.enabled);
                    w.set_tune_anim_border_speed(leaf.speed.unwrap_or(30));
                    w.set_tune_anim_border_bezier(slint::SharedString::from(leaf.bezier.as_deref().unwrap_or("default")));
                    w.set_tune_anim_border_style(slint::SharedString::from(leaf.style.as_deref().unwrap_or("")));
                }
                "fadeShadow" => {
                    w.set_tune_anim_fadeshadow_enabled(leaf.enabled);
                    w.set_tune_anim_fadeshadow_speed(leaf.speed.unwrap_or(30));
                    w.set_tune_anim_fadeshadow_bezier(slint::SharedString::from(leaf.bezier.as_deref().unwrap_or("default")));
                    w.set_tune_anim_fadeshadow_style(slint::SharedString::from(leaf.style.as_deref().unwrap_or("")));
                }
                _ => {}
            }
        }
        // Set file-local curves
        let curve_names: Vec<slint::SharedString> = params.curves.iter()
            .map(|s| slint::SharedString::from(s.as_str()))
            .collect();
        w.set_tune_anim_curves(slint::ModelRc::from(curve_names.as_slice()));
        // Real colours for the pane's previews / picker seeds.
        refresh_resolved_colors(w);
        // D2: a `border_size` inside the preset `.lua` stays unread — the
        // persisted geometry set above is the pane's truth.
    }
}

/// Seed the Borders tune pane and its active-card marker from the persisted
/// state — the one funnel the section-entry callback (`on_panel_section_selected`)
/// and the startup seed both call. Extractable so the entry behaviour is
/// testable without running `main`: a fresh launch followed by Settings ->
/// Borders was showing Slint defaults because `sync_border_tune_pane` ran only
/// on apply paths.
///
/// It receives no state or engine handle: it can only push values into the
/// window. No apply, no config write, no watcher burst (acceptance criterion).
pub(crate) fn seed_borders_pane_from_persisted(
    w: &crate::MainWindow,
    proj: &std::path::Path,
    active_border_file: &str,
    geometry: crate::callbacks::BorderGeometry,
) {
    crate::presets::reseed_active_border_index(w, active_border_file);
    crate::sync_border_tune_pane(w, proj, active_border_file, geometry);
}

/// Whether a `section-selected(Borders)` event is a real entry that should seed
/// the tune pane. PanelMenu fires `selected` on every click, including a click
/// on the section that is already open, so a plain re-select arrives here too —
/// and seeding on that would clear the pane's dirty marker (and repaint it from
/// persisted state) while the user's live draft fragment is still assembled.
/// `is_panel_closed` covers the panel being opened onto Borders, where the pane
/// has nothing current to preserve; a different `prev_section` is a normal
/// switch.
pub(crate) fn borders_entry_should_seed(
    is_panel_closed: bool,
    prev_section: Option<crate::shell::nav::PanelSection>,
) -> bool {
    is_panel_closed || prev_section != Some(crate::shell::nav::PanelSection::Borders)
}

/// Build `BorderParams` from the window's current tune state (D4 live path).
/// `size` comes from the sealed config because the tune pane no longer owns a
/// geometry model of its own.
fn tune_params_from_window(w: &crate::MainWindow, size: i32) -> crate::border_preset::BorderParams {
    use crate::border_preset::{BorderColor, PaletteToken};
    use crate::preset_store::PresetStore;
    crate::border_preset::BorderParams {
        active_colors: PresetStore::decode_colors(&read_tune_slots(w).join(",")),
        angle: w.get_tune_angle(),
        inactive: PresetStore::decode_colors(&w.get_tune_inactive_color().to_string())
            .into_iter()
            .next()
            .unwrap_or(BorderColor::Token(PaletteToken::SurfaceLowest)),
        border_size: Some(size),
        glow: PresetStore::decode_glow(&w.get_tune_glow().to_string()),
        rule_enabled: w.get_tune_rule_enabled(),
        animations: PresetStore::decode_animations(&w.get_tune_animations().to_string()),
        curves: Vec::new(),
    }
}

/// Shown in the tune pane when an edit is refused because no border preset is
/// active. The pane is fully operable in that state (every control moves, the
/// dirty dot lights), so a silent no-op is indistinguishable from a broken
/// knob — which is exactly how the maintainer read it. The refusal is by
/// design (an unsaved draft must never become the desktop's border); saying so
/// out loud is the fix.
pub(crate) const TUNE_NEEDS_ACTIVE_BORDER: &str =
    "Select a border preset first - tune changes apply live to the active border.";

/// True when the tune pane's live draft has a border to apply to.
fn has_active_border(state: &std::sync::Mutex<AppState>) -> bool {
    let st = state.lock().unwrap_or_else(|e| e.into_inner());
    !st.cfg().active_border_file.is_empty()
}

/// Write the hidden live draft and re-assemble it (D4). No-op when no border is
/// active, so an unsaved draft can never become the desktop's border.
fn apply_live_draft(
    state: &std::sync::Mutex<AppState>,
    proj: &std::path::Path,
    params: &crate::border_preset::BorderParams,
) -> Result<(), String> {
    use crate::preset_store::PresetStore;
    if !has_active_border(state) {
        return Ok(());
    }
    let content = PresetStore::generate_border_lua_full("draft", params);
    PresetStore::write_draft(&content, proj).map_err(|e| format!("Draft write failed: {}", e))?;
    let st = state.lock().unwrap_or_else(|e| e.into_inner());
    st.engine().run_script("assemble.sh", &[]).map(|_| ()).map_err(|e| {
        // Drop the draft so a later successful assemble cannot pick up this
        // failed attempt's stale state.
        PresetStore::remove_draft(proj);
        format!("Live preview failed: {}", e)
    })
}

/// Push a slot list back to the UI (model + count) and refresh the live draft.
/// Shared by slot add/remove so both keep the pane and the draft in sync.
fn commit_tune_slots(
    w: &crate::MainWindow,
    state: &std::sync::Mutex<AppState>,
    proj: &std::path::Path,
    slots: Vec<String>,
) {
    let model: Vec<slint::SharedString> = slots
        .iter()
        .map(|s| slint::SharedString::from(s.as_str()))
        .collect();
    w.set_tune_active_colors(slint::ModelRc::from(model.as_slice()));
    w.set_tune_color_count(model.len() as i32);
    refresh_resolved_colors(w);
    if !has_active_border(state) {
        // The slot list still moved (the pane must stay coherent); only the
        // desktop apply is refused, and now it says so.
        w.set_border_save_error(slint::SharedString::from(TUNE_NEEDS_ACTIVE_BORDER));
        return;
    }
    let size = {
        let st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.cfg().border_size
    };
    let params = tune_params_from_window(w, size);
    match apply_live_draft(state, proj, &params) {
        Ok(()) => w.set_border_save_error(String::new().into()),
        Err(msg) => w.set_border_save_error(slint::SharedString::from(msg.as_str())),
    }
}

/// Append one gradient slot. `None` when the list is already at the Hyprland
/// maximum (8), so callers can simply do nothing.
fn slots_with_added(slots: &[String]) -> Option<Vec<String>> {
    if slots.len() >= 8 {
        return None;
    }
    let mut next = slots.to_vec();
    next.push(BORDER_SLOT_CYCLE[next.len() % BORDER_SLOT_CYCLE.len()].to_string());
    Some(next)
}

/// Drop the last gradient slot. `None` at the Hyprland minimum (2).
fn slots_with_removed(slots: &[String]) -> Option<Vec<String>> {
    if slots.len() <= 2 {
        return None;
    }
    let mut next = slots.to_vec();
    next.pop();
    Some(next)
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
/// overwrite so the Gallery follows without restart. `invalidate` names the
/// themes whose background changed on disk (the overwrite set): their cards
/// are blanked AFTER the carry-over merge so the thumb scheduler re-bakes
/// them — empty for paths that change no artwork (rename/delete/refresh).
fn sync_save_gallery_ui(
    w: &crate::MainWindow,
    gallery_tm: &std::sync::Arc<std::sync::Mutex<crate::theme_manager::ThemeManager>>,
    refresh_mosaic_page: &std::sync::Arc<dyn Fn(bool) + Send + Sync>,
    refresh_slice_ring: &std::sync::Arc<dyn Fn() + Send + Sync>,
    invalidate: &std::collections::HashSet<String>,
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
        // Overwritten artwork would otherwise keep its stale bake (the
        // merge above preserves old images, and the scheduler skips painted
        // rows): blank the affected rows so they re-resolve and re-bake.
        crate::shell::gallery::model::invalidate_card_bakes(model, invalidate);
    } else {
        w.set_gallery_cards(slint::ModelRc::new(slint::VecModel::from(new_rows)));
    }
    // Late active arrival after a fallback startup: snap focus onto it
    // BEFORE the ring refresh settles delta-base + focus-pos (zero drift).
    reaffirm_gallery_focus_on_active(w);
    refresh_mosaic_page(false);
    refresh_slice_ring();
}

/// Post-apply focus-restore bumps (U6 extension): delays in ms at which Rust
/// re-pings the gallery keyboard after a custom-palette apply.
///
/// WHY: the provider re-assert worker (noctalia.rs) can issue
/// `color-scheme-set` + `templates-apply` ~2/4/6 s after the apply when the
/// live scheme did not already hold, and each late desktop mutation drops
/// Slint's internal focus again — AFTER the transition-end reseed already
/// ran. Exactly three pings, last at 6.5 s (inside the worker's 8 s cap);
/// each ping only bumps a nonce the shell watches with guarded handlers,
/// so a ping can never steal focus from the panel, a drawer, or another
/// screen. A future edit must keep this a fixed handful, never a watcher.
pub(crate) const FOCUS_RESTORE_BUMP_DELAYS_MS: &[u64] = &[2500, 4500, 6500];

/// True when applying `theme_name` may trigger the provider's LATE palette
/// re-assert (and thus late desktop mutations that drop keyboard focus).
///
/// Mirrors the provider arm condition without touching the engine layer
/// (which stays UI-free): the theme's `source.txt` names a palette — the
/// second token is non-empty, custom or community, both restore as custom
/// when the file is there — AND its `palette.json` exists. Missing or
/// unreadable files and palette-less schemes (builtin without a file,
/// wallpaper) arm nothing, so those applies schedule no extra restores.
pub(crate) fn apply_arms_palette_reassert(themes_root: &std::path::Path, theme_name: &str) -> bool {
    let provider_dir = themes_root.join(theme_name).join("providers").join("noctalia-v5");
    let raw = match std::fs::read_to_string(provider_dir.join("source.txt")) {
        Ok(raw) => raw,
        Err(_) => return false,
    };
    let parts: Vec<&str> = raw.trim().splitn(2, ' ').collect();
    let name = parts.get(1).copied().unwrap_or("");
    if name.is_empty() {
        return false;
    }
    provider_dir.join("palette.json").is_file()
}

/// Schedule the bounded focus-restore bumps after an apply. Custom-palette
/// applies (see [`apply_arms_palette_reassert`]) get one nonce bump per
/// entry in [`FOCUS_RESTORE_BUMP_DELAYS_MS`] on the UI thread's existing
/// single-shot timer machinery — no watcher thread, no polling, at most
/// three timers that each fire once and die. Anything else schedules
/// nothing (no behaviour change for those applies).
pub(crate) fn schedule_post_apply_focus_restores(
    weak: &slint::Weak<crate::MainWindow>,
    theme_name: &str,
    themes_root: &std::path::Path,
) {
    if !apply_arms_palette_reassert(themes_root, theme_name) {
        return;
    }
    for delay_ms in FOCUS_RESTORE_BUMP_DELAYS_MS {
        let target = weak.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(*delay_ms), move || {
            if let Some(w) = target.upgrade() {
                w.set_focus_restore_nonce(w.get_focus_restore_nonce() + 1);
            }
        });
    }
}

/// Debounce guard: overlapping reasserts (timed fallbacks + event-driven)
/// must not stack multiple unset->set cycles — each cycle is a visible
/// fullscreen drop, and five stacked retries meant five visible minimizes.
static LAST_REASSERT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
/// The single pending reload re-consult: Some(deadline) = a delayed
/// re-consult is already armed to fire at that instant. One per refused
/// decision — a second Debounced refusal while Some arms nothing (the
/// pending re-consult re-checks the LIVE state, which already includes the
/// later re-float); the timer clears it on fire so a later burst can arm
/// again. Impure-shell bookkeeping, never a decision input.
static PENDING_RECONSULT_AT: std::sync::Mutex<Option<std::time::Instant>> =
    std::sync::Mutex::new(None);
/// Theme cycle generation: exactly one invisible unset->set per theme apply.
/// The first path to fire (event-driven configreloaded OR 4.8s fallback)
/// wins; the other becomes a no-op for that generation.
pub(crate) static THEME_GEN: std::sync::Mutex<u64> = std::sync::Mutex::new(0);
pub(crate) static THEME_CYCLE_FIRED_GEN: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);
pub(crate) static THEME_TRANSITIONING_FLAG: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Captured originals of the masked options, keyed by `MaskSpec::key`. One map
/// replaced four loose globals (W1): the masked set is a table now, so the store
/// follows it, and `animations` is deliberately NOT in that set.
static THEME_MASK_ORIGINALS: std::sync::Mutex<std::collections::BTreeMap<&'static str, String>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Serialises every path that WRITES a mask to the compositor: the transition's
/// apply, its finale restore, a reload-driven reapply and the startup repair.
///
/// Without it, a reapply whose guard passed while the originals still existed
/// could land AFTER the finale had restored and cleared them: the compositor
/// would stay masked with the in-memory store and the marker both gone, and the
/// next apply would then capture the masked values as if they were the
/// originals — the permanent damage this unit exists to prevent. The critical
/// section is one Lua eval through the composer (GLOBAL_CONTROLLER), and no
/// controller guard ever reaches for this lock, so it cannot deadlock.
/// Lock order is always serial -> store -> controller, never the reverse.
/// The same direction holds for the outer guards that reach the composer
/// since the command verbs moved behind it: `THEME_INTERLUDE` and the
/// `AppState` store are acquired BEFORE the controller and no controller
/// guard ever takes either one, so the graph stays acyclic.
static MASK_WRITE_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
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

// ── W5: advance a step on the compositor's signal, not the clock ──────────
//
// The transition is a cascade of timers (T=560, T=1500, T=1900, T=2470). A timer
// is a GUESS about when the compositor will be ready, and the guess depends on
// the machine: on a slower one the timer fires early, HVE does not end up
// immersive, and the shell bar stays above the window. The sentinel's own
// recording measured `configreloaded` bursts spanning 2 s to 12 s, so no fixed
// duration can be right.
//
// The fix keeps every timer as the FALLBACK and adds the compositor's event as
// the primary trigger. Whichever arrives first wins, exactly once. That shape is
// deliberate: a signal that never arrives degrades to today's behaviour and can
// never hang the desktop.

/// Which transition step a signal may advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransitionStep {
    /// The fullscreen cycle: advanced by the compositor's `fullscreen` event.
    /// Kept though it never fires in a normal transition (HVE is already
    /// immersive, so the compositor announces nothing) — see W5b below.
    Fullscreen,
    /// The finale: advanced when the reload BURST settles (`configreloaded`).
    /// This is the one that actually matches reality.
    Restore,
}

/// The step the transition is waiting on. `None` = nothing is waiting.
pub(crate) static THEME_WAITING_STEP: std::sync::Mutex<Option<TransitionStep>> =
    std::sync::Mutex::new(None);

/// Should an arriving signal advance the waiting step? Pure, so the whole race
/// story ("signal or timer first") reduces to one testable decision.
pub(crate) fn signal_advances_step(
    waiting: Option<TransitionStep>,
    signal: TransitionStep,
    signal_gen: u64,
    live_gen: u64,
) -> bool {
    waiting == Some(signal) && signal_gen == live_gen
}

/// Claim a waiting step so a signal and its fallback cannot both run it.
/// True for exactly one caller: the first to clear the wait.
pub(crate) fn claim_waiting_step(step: TransitionStep) -> bool {
    let mut waiting = THEME_WAITING_STEP.lock().unwrap();
    if *waiting == Some(step) {
        *waiting = None;
        true
    } else {
        false
    }
}

// ── W5b: wait for the RELOAD to settle, not for a fixed clock ─────────────
//
// The live test killed the `fullscreen` signal: during a normal transition HVE
// is ALREADY immersive, so the compositor has no state change to announce and
// the event never fires. The real waste, straight from the owner's logs:
//
//     masked          05.376
//     reload landed   06.287   <- the reload is DONE here
//     finale (clock)  08.377   <- the clock waited ~2s longer for nothing
//
// and the reload does not arrive once: it arrives in a BURST (the sentinel
// recorded 5-6 `configreloaded` inside one second, then a late one 15s later).
// No fixed duration can know when the burst ended.
//
// So the finale waits for the burst to SETTLE: every `configreloaded` restarts
// a short timer, and only when the timer survives its whole window does the
// reload count as finished. A late reload restarts it too — which is exactly
// the case a fixed clock got wrong.

/// Env-tunable so the owner can calibrate on a slow machine without a rebuild.
pub(crate) fn reload_settle_window() -> std::time::Duration {
    let ms = std::env::var("HVE_RELOAD_SETTLE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(450);
    std::time::Duration::from_millis(ms)
}

/// The whole enemy is the clock guessing. `true` when the settle timer is the
/// last event we saw for this transition (i.e. it was never restarted since).
pub(crate) fn reload_restarts_settle(
    transitioning: bool,
    waiting: Option<TransitionStep>,
) -> bool {
    transitioning && waiting == Some(TransitionStep::Restore)
}

// ── The armed settle plan ────────────────────────────────────────────────
// One transition at a time, so one shared slot is enough. It holds what the
// settle timer needs when it finally survives its window: the generation it was
// armed for and the window it must run the finale on.
static THEME_SETTLE_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Incremented on every reload that restarts the settle window.
static THEME_SETTLE_BUMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Called from the `configreloaded` line (IPC thread) while the finale waits.
/// Restarts the settle countdown: the reload burst is still going.
pub(crate) fn theme_reload_signal() {
    if !reload_restarts_settle(
        is_theme_transitioning_flag(),
        *THEME_WAITING_STEP.lock().unwrap(),
    ) {
        return;
    }
    let gen = *THEME_GEN.lock().unwrap();
    THEME_SETTLE_GEN.store(gen, std::sync::atomic::Ordering::Relaxed);
    // Bump a counter the armed timer watches: if it changed, the timer knows a
    // newer reload arrived and it must NOT run the finale yet.
    THEME_SETTLE_BUMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    tracing::debug!("[theme] reload arrived — settle window restarted (gen {})", gen);
}

/// Arm the settle wait for a transition. Returns the bump value at arm time, so
/// the timer can tell "nothing new happened" from "the burst is still going".
pub(crate) fn arm_reload_settle(gen: u64) -> u64 {
    THEME_SETTLE_GEN.store(gen, std::sync::atomic::Ordering::Relaxed);
    THEME_SETTLE_BUMP.load(std::sync::atomic::Ordering::Relaxed)
}

/// Did the reload burst settle since `armed_bump`? True = no newer reload
/// arrived, so the reload is finished and the finale may run.
pub(crate) fn reload_settled_since(armed_bump: u64, gen: u64) -> bool {
    THEME_SETTLE_BUMP.load(std::sync::atomic::Ordering::Relaxed) == armed_bump
        && THEME_SETTLE_GEN.load(std::sync::atomic::Ordering::Relaxed) == gen
}

/// Entry point for the IPC listener: a `fullscreen` event arrived from the
/// compositor. Advance the waiting step if it is the one we wait for and the
/// generation is live. Returns whether the signal was consumed.
pub(crate) fn theme_fullscreen_signal() {
    if !is_theme_transitioning_flag() {
        return;
    }
    let live_gen = *THEME_GEN.lock().unwrap();
    let waiting = *THEME_WAITING_STEP.lock().unwrap();
    if !signal_advances_step(waiting, TransitionStep::Fullscreen, live_gen, live_gen) {
        return;
    }
    if !claim_waiting_step(TransitionStep::Fullscreen) {
        return; // the fallback timer got there first
    }
    tracing::info!(
        "[theme] finale advanced by the compositor's fullscreen signal (gen {})",
        live_gen
    );
    // The finale runs on the UI thread: Slint timers inside it are thread-local.
    if let Err(e) = slint::invoke_from_event_loop(move || {
        // Re-check on arrival: the hop may land after a newer apply superseded
        // this transition. Running then would drive a fresh transition's finale
        // from a stale signal.
        if *THEME_GEN.lock().unwrap() != live_gen {
            tracing::debug!(
                "[theme] stale fullscreen signal dropped on arrival (gen {} is not live)",
                live_gen
            );
            return;
        }
        if let Some(w) = THEME_FINALE_WINDOW
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|w| w.upgrade())
        {
            theme_finale_run(w, live_gen);
        }
    }) {
        // Never drop the failure silently: this is the path that keeps the
        // transition alive when the compositor answers fast.
        tracing::warn!("[theme] fullscreen signal UI-thread hop failed: {}", e);
    }
}

/// The window the finale needs, so the IPC-thread signal can reach the UI. Set
/// when the transition starts and cleared when it ends.
pub(crate) static THEME_FINALE_WINDOW: std::sync::Mutex<Option<slint::Weak<crate::MainWindow>>> =
    std::sync::Mutex::new(None);

/// How long to wait BEFORE staging the empty desktop. Default is 0: stage
/// IMMEDIATELY, which is the feel the owner picked live ("ha quedado perfecto").
/// The 560ms house convention (letting the 420ms exit fade complete first) is
/// available as a value if that feel is ever wanted again.
///
/// Env-tunable so the feel can be calibrated without a rebuild.
pub(crate) fn interlude_stage_ms() -> u64 {
    std::env::var("HVE_INTERLUDE_STAGE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}

/// How long the EMPTY DESKTOP is shown before the finale returns the view.
///
/// This is the part the owner liked: the new colours and wallpaper get to be
/// seen, clean, with nothing on top. The old fixed clock gave it ~940ms by
/// accident (1500ms minus the 560ms stage). W5c keeps that display time, but
/// starts counting it from the STAGE, not from a fixed instant — so a slow
/// machine delays the stage and the display is still honoured.
///
/// Env-tunable so the owner can calibrate the feel without a rebuild.
pub(crate) fn interlude_display_ms() -> u64 {
    std::env::var("HVE_INTERLUDE_DISPLAY_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(940)
}

/// The generation whose interlude display currently owns the finale timing.
/// `0` = nobody. Guards the 1500ms safety net so it cannot fire over a live
/// interlude (the net exists only for a transition with NO reload at all).
static INTERLUDE_DISPLAY_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Is the interlude display already driving this generation's timing?
pub(crate) fn interlude_display_owned(gen: u64) -> bool {
    INTERLUDE_DISPLAY_GEN.load(std::sync::atomic::Ordering::Relaxed) == gen
}

/// Stage the empty desktop, HOLD it for the display time, then run the finale.
///
/// The reload has already settled (that is the real, guess-free part). From
/// here the timing is deliberately generous: the owner wants to SEE the theme.
pub(crate) fn begin_interlude_display(win: slint::Weak<crate::MainWindow>, gen: u64) {
    INTERLUDE_DISPLAY_GEN.store(gen, std::sync::atomic::Ordering::Relaxed);
    let weak = win;
    // Stage after the exit fade completes (420ms ease-in, 560ms in the house
    // convention) so the user watches the shell dissolve before the cut. Tunable
    // (HVE_INTERLUDE_STAGE_MS): 0 stages immediately.
    slint::Timer::single_shot(std::time::Duration::from_millis(interlude_stage_ms()), move || {
        if *THEME_GEN.lock().unwrap() != gen {
            return; // superseded
        }
        let target = {
            let st = THEME_INTERLUDE.lock().unwrap();
            if !st.should_move() {
                None
            } else {
                st.orig().map(|o| find_empty_workspace(o))
            }
        };
        if let Some(target) = target {
            if hypr_switch_view(&target) {
                THEME_INTERLUDE.lock().unwrap().mark_moved();
                tracing::info!(
                    "[theme] interlude: view staged on empty ws {} (gen {})",
                    target, gen
                );
            } else {
                tracing::warn!(
                    "[theme] interlude staging failed — stay-fullscreen fallback (gen {})",
                    gen
                );
            }
        }
        // HOLD: the display time the owner wants, counted from the stage.
        let weak_hold = weak.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(interlude_display_ms()), move || {
            if *THEME_GEN.lock().unwrap() != gen {
                return; // superseded
            }
            if let Some(w) = weak_hold.upgrade() {
                if claim_waiting_step(TransitionStep::Restore) {
                    tracing::info!(
                        "[theme] finale advanced after the interlude display (gen {})",
                        gen
                    );
                    theme_finale_run(w, gen);
                }
            }
        });
    });
}

/// Poll for the reload burst to settle, re-arming with NO cap.
///
/// The first shape re-armed only once, so a burst longer than two windows fell
/// back to the fixed clock — exactly the guessing this unit removes, and exactly
/// the case the sentinel recorded on the slow machine (bursts of 2s-12s). This
/// keeps re-checking until the burst is genuinely over, however long it takes,
/// then hands off to the interlude. Bounded only by the 3500ms watchdog upstream.
fn arm_settle_poll(win: slint::Weak<crate::MainWindow>, gen: u64, armed_bump: u64) {
    slint::Timer::single_shot(reload_settle_window(), move || {
        if *THEME_GEN.lock().unwrap() != gen {
            return; // superseded by a newer apply
        }
        if !reload_settled_since(armed_bump, gen) {
            // A newer reload arrived: the burst is still going. Re-arm with the
            // fresh bump and wait again — no cap, no fallback to guessing.
            tracing::debug!("[theme] reload burst still going (gen {}) — polling again", gen);
            let fresh = arm_reload_settle(gen);
            arm_settle_poll(win, gen, fresh);
            return;
        }
        tracing::info!("[theme] reload settled (gen {}) — staging the interlude", gen);
        begin_interlude_display(win, gen);
    });
}

/// The finale body: the part of the transition that re-enters fullscreen,
/// returns the view and restores the masks. Extracted to a named function so the
/// settled reload and the safety timer can both run it — exactly once.
pub(crate) fn theme_finale_run(win: crate::MainWindow, gen: u64) {
    if THEME_INTERLUDE.lock().unwrap().staged() {
        // View is on the stage: SKIP the focusing cycle. A focusing cycle
        // focuses HVE by title first, and focusing a window on another workspace
        // drags the VIEW back with it — the user would see floating HVE plus
        // their windows mid-cycle, 400ms before the intended return. Fullscreen
        // is re-ensured with the window-targeted dispatcher (no focus) at return.
        tracing::info!("[theme] finale: view staged — focusing cycle skipped");
    } else {
        // Stay-fullscreen fallback (the view never left): one masked unset→set
        // cycle while opacity is 0 forces a real fullscreen transition, so the
        // bar reliably hides.
        reassert_gallery_fullscreen();
    }
    // Fade-in after the cycle settled (150ms unset→set).
    let weak_re = win.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(400), move || {
        if let Some(w) = weak_re.upgrade() {
            w.set_theme_transitioning(false);
            tracing::info!("[theme] finale fade-in (gen {})", gen);
        }
        THEME_TRANSITIONING_FLAG.store(false, std::sync::atomic::Ordering::Relaxed);
        // Return the view FIRST — masks are still ON, so the flip is an instant
        // cut, and the entrance fade then plays over the real desktop.
        if let Some(orig) = THEME_INTERLUDE.lock().unwrap().take_restore() {
            // Reload chains may have re-floated HVE — re-set fullscreen with the
            // window-targeted dispatcher BEFORE the flip (no focus step, so the
            // view is never dragged back; idempotent when already fullscreen).
            let fs_ok = reassert_hve_fullscreen_targeted();
            let back_ok = hypr_switch_view(&orig);
            tracing::info!(
                "[theme] interlude: view returned to {} ok={} (targeted fullscreen {})",
                orig, back_ok, fs_ok
            );
        }
        // Masks + DND release AFTER the entrance fade completes (520ms ease-out):
        // animations must stay OFF through the flip, and toasts stay swallowed
        // during the entrance.
        slint::Timer::single_shot(std::time::Duration::from_millis(570), move || {
            restore_theme_masks();
            theme_dnd_release();
            tracing::info!("[theme] masks restored, DND released after entrance fade");
        });
    });
}

fn hypr_getoption_int(option: &str, fallback: &str) -> String {
    // Raw live-option read behind `Composer` (capability-routing Phase 3):
    // the option NAME is passed through verbatim here — its normalisation
    // stays in this caller, which also owns the parse below.
    let answer = crate::composer::global_controller()
        .and_then(|ctrl| ctrl.composer().read_option(option));
    option_int_from(answer.as_deref(), fallback)
}

/// The parse half of `hypr_getoption_int`, over the raw `getoption -j`
/// JSON text the composer returns (`None` on any failure). Byte-identical
/// to the pre-move parse: `int` first, then a trimmed numeric/boolean
/// `str`, otherwise the caller's fallback.
fn option_int_from(text: Option<&str>, fallback: &str) -> String {
    if let Some(t) = text {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
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

// ── Theme transition masks (W1) ─────────────────────────────────────────
// `hyprctl keyword` is REJECTED by Hyprland's non-legacy (Lua) parser: it
// prints "keyword can't work with non-legacy parsers. Use eval." and exits 0.
// The old `hypr_set` discarded that output, so the masks never reached the
// compositor (measured: 0/443 samples with blur=0 across 10 theme applies).
// Runtime options are set through Lua now, via `hyprctl eval 'hl.config(...)'`.

/// How a masked option is written as a Lua literal. `Flag` is the only kind the
/// current mask set needs (a boolean `getoption`); the type is kept as the
/// explicit extension point for the mask table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MaskKind {
    /// `true`/`false` option.
    Flag,
}

pub(crate) struct MaskSpec {
    /// Short id used in logs and as the originals-store key.
    pub(crate) key: &'static str,
    /// `hyprctl getoption` name (colon form), used to capture the original.
    pub(crate) option: &'static str,
    /// Dotted path understood by `hl.config` in the Lua parser.
    pub(crate) lua_path: &'static str,
    pub(crate) kind: MaskKind,
}

/// The options masked while a theme transition runs.
///
/// W3 settled this set at one entry. `animations`, `border` and `shadow` were
/// all masked by the original design, but that code never ran (the masks were
/// dead until W1), so none of them had ever been observed doing anything. The
/// A/B was then run live: with `border` and `shadow` out the owner saw no
/// difference in the reveal, and the probe measured `border` at its real values
/// (2-4) throughout instead of 0. `animations` stays out for a product reason:
/// the ANIMATED reveal is the effect the owner wants, so masking it would
/// degrade it. The only documented complaint — the blurred desktop — is what
/// remains masked.
pub(crate) const THEME_MASKS: &[MaskSpec] = &[MaskSpec {
    key: "blur",
    option: "decoration:blur:enabled",
    lua_path: "decoration.blur.enabled",
    kind: MaskKind::Flag,
}];

/// The value written while masking.
fn masked_literal(kind: MaskKind) -> &'static str {
    match kind {
        MaskKind::Flag => "false",
    }
}

/// Fallback when `getoption` cannot answer: the compositor's own defaults.
fn default_literal(kind: MaskKind) -> &'static str {
    match kind {
        MaskKind::Flag => "1",
    }
}

/// Normalise a captured original into a Lua literal for its kind. `None` means
/// the value is unusable and the caller must NOT write a partial payload.
pub(crate) fn lua_literal(kind: MaskKind, raw: &str) -> Option<String> {
    let t = raw.trim();
    match kind {
        MaskKind::Flag => match t {
            "1" | "true" => Some("true".to_string()),
            "0" | "false" => Some("false".to_string()),
            _ => None,
        },
    }
}

/// Nested Lua table under construction.
enum LuaNode {
    Leaf(String),
    Table(std::collections::BTreeMap<String, LuaNode>),
}

fn lua_render(node: &LuaNode) -> String {
    match node {
        LuaNode::Leaf(v) => v.clone(),
        LuaNode::Table(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{} = {}", k, lua_render(v)))
                .collect();
            format!("{{ {} }}", inner.join(", "))
        }
    }
}

/// A Lua identifier, so a dotted path can never inject arbitrary code.
fn is_lua_ident(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A literal the builder may emit: a boolean or an integer. Anything else (an
/// empty string, `[EMPTY]`) is refused rather than rendered into invalid Lua.
fn is_lua_literal(literal: &str) -> bool {
    matches!(literal, "true" | "false") || literal.parse::<i64>().is_ok()
}

/// Build ONE merged `hl.config({...})` chunk for the given (dotted path, Lua
/// literal) pairs. Pure and deterministic (keys sorted), so the exact string is
/// assertable in tests. `None` on an empty set or a self-contradicting path.
pub(crate) fn lua_config_payload(entries: &[(String, String)]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let mut root: std::collections::BTreeMap<String, LuaNode> = std::collections::BTreeMap::new();
    for (path, literal) in entries {
        let segments: Vec<&str> = path.split('.').collect();
        if segments.iter().any(|s| !is_lua_ident(s)) {
            return None;
        }
        if !is_lua_literal(literal) {
            return None;
        }
        let mut cursor = &mut root;
        for segment in &segments[..segments.len() - 1] {
            let node = cursor
                .entry((*segment).to_string())
                .or_insert_with(|| LuaNode::Table(std::collections::BTreeMap::new()));
            match node {
                LuaNode::Table(next) => cursor = next,
                LuaNode::Leaf(_) => return None, // path is both leaf and table
            }
        }
        let last = segments[segments.len() - 1].to_string();
        if cursor.contains_key(&last) {
            return None;
        }
        cursor.insert(last, LuaNode::Leaf(literal.clone()));
    }
    Some(format!("hl.config({})", lua_render(&LuaNode::Table(root))))
}

/// Build the payload for the whole mask set: the masked values, or the captured
/// originals. `None` when restoring and any original is missing or unusable.
pub(crate) fn theme_mask_payload(
    masked: bool,
    originals: &std::collections::BTreeMap<&'static str, String>,
) -> Option<String> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for spec in THEME_MASKS {
        let literal = if masked {
            masked_literal(spec.kind).to_string()
        } else {
            lua_literal(spec.kind, originals.get(spec.key)?)?
        };
        entries.push((spec.lua_path.to_string(), literal));
    }
    lua_config_payload(&entries)
}

/// Apply the mask payload through an injected runner, returning one message per
/// failure. Split from the I/O so the failure path is testable without spawning
/// processes — the whole bug was a failure nobody ever saw.
pub(crate) fn write_mask_payload(
    masked: bool,
    originals: &std::collections::BTreeMap<&'static str, String>,
    runner: impl Fn(&str) -> Result<(), String>,
) -> Vec<String> {
    let Some(payload) = theme_mask_payload(masked, originals) else {
        return vec![format!(
            "no usable mask payload (masked={masked}): a captured original is missing or unparseable"
        )];
    };
    match runner(&payload) {
        Ok(()) => Vec::new(),
        Err(e) => vec![e],
    }
}

/// Run one `hyprctl eval` Lua chunk. See `composer::hyprland::eval_outcome`
/// for why the exit status is not enough — this is the check whose absence
/// hid the bug for months.
///
/// Thin controller over the composer seam (capability-routing Phase 3): the
/// judgement, the argv and the error strings live behind the trait. The
/// detached fallback keeps the OLD spawn for the paths that run BEFORE
/// `composer::init_global` (the startup mask repair, an early IPC line) —
/// there the controller is `None` and a no-op would strand the desktop
/// masked. Signature unchanged: this function is passed as a value into
/// `write_mask_payload` / `restore_theme_masks_with`.
fn hypr_eval(lua: &str) -> Result<(), String> {
    match crate::composer::global_controller() {
        Some(ctrl) => ctrl.composer().eval_lua(lua),
        None => crate::composer::eval_lua_detached(lua),
    }
}

/// Geometry persist idle window (ms): how long the drag must stay quiet before
/// the ONE durable write lands. The hot apply is already on the desktop; this
/// only decides when the expensive file path (`geometry.sh` + `assemble.sh`,
/// ~175 ms measured) is allowed to run. `Timer::start` restarts a running
/// timer, so every tick pushes the deadline out and a whole drag writes once.
const GEOMETRY_PERSIST_IDLE_MS: u64 = 400;

/// (Re)arm the single deferred geometry persist. The coalescer holds the
/// latest geometry; this drains it exactly once after the burst goes quiet.
/// The drain reads the config (already updated by the hot path), so the saved
/// value is the drag's final one whatever order the timer and ticks land in.
fn arm_geometry_persist(
    timer: &slint::Timer,
    pending: &std::rc::Rc<std::cell::RefCell<callbacks::GeometryPersistCoalescer>>,
    state: &Arc<std::sync::Mutex<AppState>>,
) {
    let pending = pending.clone();
    let state = state.clone();
    timer.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_millis(GEOMETRY_PERSIST_IDLE_MS),
        move || drain_geometry_persist(&pending, &state),
    );
}

/// Run the coalesced geometry persist ONCE if anything is owed.
///
/// Shared by the idle timer and the shutdown flush, so both paths have the
/// same meaning: consume the owe, then run the durable path. Nothing owed is
/// a no-op (a stale timer fire must never rewrite the fragment). The drain
/// reads the config — already updated by the hot path — so the value saved is
/// the drag's final one whatever order the timer and the ticks land in.
fn drain_geometry_persist(
    pending: &std::rc::Rc<std::cell::RefCell<callbacks::GeometryPersistCoalescer>>,
    state: &Arc<std::sync::Mutex<AppState>>,
) {
    if pending.borrow_mut().take().is_none() {
        return;
    }
    let result = {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.apply_geometry()
    };
    if let Err(e) = result {
        tracing::error!("[HVE] Geometry persist error: {}", e);
    }
}

/// Flush any owed geometry persist RIGHT NOW, on the way out.
///
/// `arm_geometry_persist` is a trailing-edge coalescer: it only writes once the
/// drag goes quiet. If the process exits inside `GEOMETRY_PERSIST_IDLE_MS`
/// (window close, IPC quit, tray quit), that timer never fires and the whole
/// drag is lost — the persisted config and the fragment stay at the previous
/// value and the next `hyprctl reload` reverts what the user just saw. This is
/// the closing half of "deferred, never dropped": on shutdown the idle timer is
/// stopped (it can never fire again) and the same drain runs synchronously.
/// Safe with nothing owed — it is then a pure no-op.
fn flush_geometry_persist(
    timer: &slint::Timer,
    pending: &std::rc::Rc<std::cell::RefCell<callbacks::GeometryPersistCoalescer>>,
    state: &Arc<std::sync::Mutex<AppState>>,
) {
    timer.stop();
    drain_geometry_persist(pending, state);
}

/// Capture the current value of every masked option.
fn capture_mask_originals() -> std::collections::BTreeMap<&'static str, String> {
    let mut map = std::collections::BTreeMap::new();
    for spec in THEME_MASKS {
        let value = hypr_getoption_int(spec.option, default_literal(spec.kind));
        map.insert(spec.key, value);
    }
    map
}

fn log_mask_failures(failures: &[String]) {
    for failure in failures {
        tracing::warn!("[theme] mask write FAILED: {}", failure);
    }
}

// ── Crash safety: the mask marker ─────────────────────────────────────
// A crash between masking and restoring would leave the desktop without its
// blur, and an in-memory store dies with the process — worse, the
// NEXT apply would then capture the masked values as if they were the originals
// and make the damage permanent. Same protocol as the engine's colour-authority
// yield marker (src/providers/skwd_policy.rs): written BEFORE the mask lands (a
// marker without a mask is a harmless no-op for recovery; a mask without a
// marker would be unrecoverable) and cleared only after a successful restore.

/// Crash marker location: HVE cache dir + documented name.
pub(crate) fn mask_marker_path() -> std::path::PathBuf {
    crate::config::hve_cache_dir().join("theme-masks.json")
}

/// Persist the marker atomically (temp + sync + rename), like the mirror
/// protocol in `skwd_policy`: a torn marker must never be observed, and a
/// marker we cannot persist must be reported to the caller.
fn write_mask_marker(
    originals: &std::collections::BTreeMap<&'static str, String>,
) -> Result<(), String> {
    let text = serde_json::to_string(originals).map_err(|e| format!("serialise marker: {e}"))?;
    let path = mask_marker_path();
    let tmp = path.with_extension("tmp");
    let attempt = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if let Err(e) = attempt {
        // Never leave a stray temp behind: the mirror protocol sweeps orphans,
        // this is the small version of it.
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("write marker {}: {e}", path.display()));
    }
    Ok(())
}

fn clear_mask_marker() {
    if let Err(e) = std::fs::remove_file(mask_marker_path()) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("[theme] could not clear the mask marker: {}", e);
        }
    }
}

/// Parse a mask marker. Pure, so the protocol is testable without touching the
/// cache dir. Unknown keys are ignored; an empty result means nothing to do.
pub(crate) fn marker_from_json(
    text: &str,
) -> Option<std::collections::BTreeMap<&'static str, String>> {
    let parsed: std::collections::BTreeMap<String, String> = serde_json::from_str(text).ok()?;
    let mut out = std::collections::BTreeMap::new();
    for spec in THEME_MASKS {
        if let Some(value) = parsed.get(spec.key) {
            out.insert(spec.key, value.clone());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn read_mask_marker() -> Option<std::collections::BTreeMap<&'static str, String>> {
    marker_from_json(&std::fs::read_to_string(mask_marker_path()).ok()?)
}

/// FIRST WRITER WINS, AND FAIL CLOSED. A second theme apply during the first
/// transition must NOT re-capture: the compositor already holds the masked
/// values, so a re-capture would record them as the originals and every later
/// restore would write the masked values back — leaving the desktop without
/// blur, border and shadow for good. The interlude view state uses this same
/// guard.
///
/// The originals are adopted only if `persist` could record them: a mask whose
/// crash marker we could not write is a mask we refuse to place, because a mask
/// without a marker is unrecoverable (the colour-authority mirror refuses in
/// the same situation). Refusing degrades to "no masking this transition",
/// which is cosmetic; masking an unprotected desktop is not.
///
/// Returns the originals this apply must mask with, and whether this call did
/// the capturing.
pub(crate) fn mask_plan(
    store: &mut std::collections::BTreeMap<&'static str, String>,
    persist: impl FnOnce(&std::collections::BTreeMap<&'static str, String>) -> Result<(), String>,
    captured: impl FnOnce() -> std::collections::BTreeMap<&'static str, String>,
) -> Result<(std::collections::BTreeMap<&'static str, String>, bool), String> {
    if !store.is_empty() {
        return Ok((store.clone(), false));
    }
    let fresh = captured();
    persist(&fresh)?;
    *store = fresh;
    Ok((store.clone(), true))
}

/// Which originals an apply must adopt. A surviving marker IS the truth: the
/// live values may be the ones a previous transition left masked, and capturing
/// those would bake the masked values in as "originals" and lose the real ones
/// for good — the marker is the only remaining copy.
pub(crate) fn choose_originals(
    marker: Option<std::collections::BTreeMap<&'static str, String>>,
    live: impl FnOnce() -> std::collections::BTreeMap<&'static str, String>,
) -> std::collections::BTreeMap<&'static str, String> {
    marker.unwrap_or_else(live)
}

/// Capture (first writer only) and persist the marker BEFORE the mask lands.
fn begin_mask() -> Result<std::collections::BTreeMap<&'static str, String>, String> {
    let mut store = THEME_MASK_ORIGINALS.lock().unwrap();
    let (originals, _first) = mask_plan(&mut store, write_mask_marker, || {
        choose_originals(read_mask_marker(), capture_mask_originals)
    })?;
    Ok(originals)
}

/// Startup repair: replay a mask a previous process died between masking and
/// restoring. Never panics and never aborts startup.
pub(crate) fn startup_recover_crashed_masks() {
    let _serial = MASK_WRITE_SERIAL.lock().unwrap();
    let Some(originals) = read_mask_marker() else {
        tracing::info!("[theme] mask startup repair: nothing to repair");
        return;
    };
    let failures = write_mask_payload(false, &originals, hypr_eval);
    log_mask_failures(&failures);
    if failures.is_empty() {
        clear_mask_marker();
        tracing::warn!(
            "[theme] mask startup repair: a crashed transition had left the compositor masked; \
             restored {:?}",
            originals
        );
    }
}

/// Restore through an injected runner. The originals are cleared ONLY on
/// success, so a failed restore can be retried — the transition watchdog calls
/// this path a second time.
fn restore_theme_masks_with(runner: impl Fn(&str) -> Result<(), String>) -> Vec<String> {
    let originals = THEME_MASK_ORIGINALS.lock().unwrap().clone();
    if originals.is_empty() {
        return Vec::new();
    }
    let failures = write_mask_payload(false, &originals, runner);
    if failures.is_empty() {
        THEME_MASK_ORIGINALS.lock().unwrap().clear();
    }
    failures
}

pub(crate) fn restore_theme_masks() {
    let _serial = MASK_WRITE_SERIAL.lock().unwrap();
    if THEME_MASK_ORIGINALS.lock().unwrap().is_empty() {
        // Nothing was masked in this process. Do NOT clear a surviving marker:
        // it may be the last copy of the real originals after a crashed
        // transition, and the next apply or startup heals from it.
        return;
    }
    let failures = restore_theme_masks_with(hypr_eval);
    log_mask_failures(&failures);
    if failures.is_empty() {
        clear_mask_marker();
        tracing::info!("[theme] masks restored after the transition");
    }
}

/// A late `configreloaded` can arrive AFTER the finale cleared the store. With
/// no originals there is nothing that could ever restore the mask, so writing
/// it would strand the desktop masked. Refuse.
pub(crate) fn should_reapply(transitioning: bool, have_originals: bool) -> bool {
    transitioning && have_originals
}

/// Re-apply the transition masks after a config reload. Every `hyprctl reload`
/// resets runtime keyword overrides to the config values (verified live), which
/// un-masks the transition mid-flight. Called on EVERY configreloaded line
/// during a theme transition — bypasses the 3s throttle (cheap, idempotent).
pub(crate) fn reapply_theme_masks() {
    let _serial = MASK_WRITE_SERIAL.lock().unwrap();
    let originals = THEME_MASK_ORIGINALS.lock().unwrap().clone();
    if !should_reapply(is_theme_transitioning_flag(), !originals.is_empty()) {
        return;
    }
    let failures = write_mask_payload(true, &originals, hypr_eval);
    log_mask_failures(&failures);
    if failures.is_empty() {
        tracing::info!("[theme] masks re-applied after reload wipe");
    }
}

fn reassert_debounce_ok() -> bool {
    let mut last = LAST_REASSERT.lock().unwrap();
    let now = std::time::Instant::now();
    if last.is_some_and(|t| now.duration_since(t) < std::time::Duration::from_millis(REASSERT_DEBOUNCE_MS)) {
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
    // The shared unset->set mechanics (same dispatch the reload restore
    // uses); the gallery expansion stays THIS caller's gate, re-checked at
    // step 2 so a gallery close mid-cycle cancels the set.
    run_fullscreen_cycle(|| crate::shell::Shell::is_gallery_expanded());
}

/// Timed safety net after a theme apply: REMOVED — the single unset->set
/// cycle now runs inside the transparent fade window (event-driven path in
/// hypr_ipc, or the 4.8s fallback here), so timed retries would only add
/// visible flashes after fade-in.

// ── Fullscreen survives a config reload ────────────────────────────────
// Every `hyprctl reload` resets the runtime keyword overrides AND
// re-floats HVE, dropping its immersive fullscreen. The reload path must
// put it back: one pure decision + a thin impure shell, debounced with the
// shared reassert window, and never while a theme transition is in flight
// (the finale owns that cycle). See
// odd/tasks/fullscreen-survives-config-reload.md.

/// Shared debounce window for visible fullscreen cycles (gallery reassert
/// AND reload restore): at most one visible unset->set cycle per window.
pub(crate) const REASSERT_DEBOUNCE_MS: u64 = 1500;

/// Why the reload restore declined. Logged at debug; each variant is its
/// own unit test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReloadFsSkip {
    /// A theme transition is in flight — the finale owns the cycle.
    ThemeTransitionInFlight,
    /// HVE is not supposed to be showing (ShowStateMachine not Visible).
    HveHidden,
    /// HVE is not inside the immersive presentation (no gallery session,
    /// or the settings panel floats — the window is the live border
    /// preview there, fullscreen would destroy it).
    NotImmersive,
    /// HVE is already immersive fullscreen — nothing to restore.
    AlreadyFullscreen,
    /// A fullscreen cycle ran inside the shared debounce window.
    Debounced,
}

/// The pure decision for the reload restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReloadFsAction {
    /// Run the unset->set fullscreen cycle now.
    Cycle,
    /// Do nothing, for a recorded reason.
    Nothing(ReloadFsSkip),
}

/// Pure decision (no I/O): given the state a config reload leaves, decide
/// whether to restore immersive fullscreen. Every branch is headless-tested.
///
/// - `transition_in_flight`: THEME_TRANSITIONING_FLAG — the finale owns the
///   cycle inside a theme transition, so the restore must refuse.
/// - `hve_supposed_visible`: ShowStateMachine == Visible (Hidden/Entering
///   never restore — the settle owns fullscreen during a show).
/// - `immersive_expected`: the app believes HVE should HOLD immersive
///   fullscreen right now — gallery session active and the settings panel
///   NOT floating (the floating panel is a deliberate windowed
///   presentation; the window IS the live border preview there).
/// - `hve_fullscreen`: `hyprctl clients -j` snapshot of the HVE client.
/// - `since_last_cycle`: time since LAST_REASSERT (None = never cycled).
pub(crate) fn reload_fs_action(
    transition_in_flight: bool,
    hve_supposed_visible: bool,
    immersive_expected: bool,
    hve_fullscreen: bool,
    since_last_cycle: Option<std::time::Duration>,
) -> ReloadFsAction {
    if transition_in_flight {
        return ReloadFsAction::Nothing(ReloadFsSkip::ThemeTransitionInFlight);
    }
    if !hve_supposed_visible {
        return ReloadFsAction::Nothing(ReloadFsSkip::HveHidden);
    }
    if !immersive_expected {
        return ReloadFsAction::Nothing(ReloadFsSkip::NotImmersive);
    }
    if hve_fullscreen {
        return ReloadFsAction::Nothing(ReloadFsSkip::AlreadyFullscreen);
    }
    if since_last_cycle.is_some_and(|d| d < std::time::Duration::from_millis(REASSERT_DEBOUNCE_MS)) {
        return ReloadFsAction::Nothing(ReloadFsSkip::Debounced);
    }
    ReloadFsAction::Cycle
}

/// Decision wiring: run the cycle exactly when the pure decision says so.
/// Split from the entry so the reload path is headless-testable (tests
/// inject a recording runner). Returns the action that was consulted.
pub(crate) fn apply_reload_fs_action(
    action: ReloadFsAction,
    run_cycle: impl FnOnce(),
) -> ReloadFsAction {
    if matches!(action, ReloadFsAction::Cycle) {
        run_cycle();
    }
    action
}

/// The reload restore's impure runner: claim the shared debounce, then run
/// the shared unset->set cycle. The claim is atomic at fire time — two
/// parallel reload lines cannot both win the 1.5s window (the 4-actor race
/// the unconditional event-path cycle caused).
fn run_reload_cycle() {
    if reassert_debounce_ok() {
        run_fullscreen_cycle(reload_cycle_step2_ok);
    } else {
        tracing::debug!(
            "[reassert] reload restore lost the debounce claim (parallel cycle)"
        );
    }
}

/// Step-2 gate of the reload restore cycle: between the unset and the set
/// (150ms) HVE may have been hidden, a transition may have started, or the
/// user may have left the immersive presentation (closed the gallery /
/// opened the floating panel) — either way the set must not land (the
/// unset alone is the state the reload already left, so cancelling is
/// harmless).
fn reload_cycle_step2_ok() -> bool {
    !is_theme_transitioning_flag() && hve_supposed_visible() && immersive_expected()
}

/// Whether HVE is supposed to be showing: reuse the ShowStateMachine as the
/// single source of truth — Visible only. Hidden = parked in the
/// scratchpad (tray); Entering = a show in flight whose settle owns
/// fullscreen. No new visibility flag.
fn hve_supposed_visible() -> bool {
    crate::composer::global_controller()
        .is_some_and(|c| c.show_state() == crate::show_state::ShowState::Visible)
}

/// Whether the app believes HVE should HOLD immersive fullscreen right now:
/// the immersive Gallery session is active AND the settings panel is not
/// floating. The panel float is a deliberate windowed presentation (the
/// window IS the live border preview — shell/mod.rs schedule_float), so a
/// fullscreen force there would destroy what the user is tuning. Same gate
/// `run_settle` uses before its fullscreen dispatch.
fn immersive_expected() -> bool {
    crate::composer::global_controller().is_some_and(|c| c.gallery_session_active())
        && !crate::shell::Shell::is_panel_floating_global()
}

/// Elapsed since the last fullscreen cycle (LAST_REASSERT), if any.
fn last_reassert_elapsed() -> Option<std::time::Duration> {
    LAST_REASSERT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .map(|t| t.elapsed())
}

/// Shared unset->set fullscreen cycle (the reassert mechanics). Step 1
/// drops fullscreen (fires a real state change — a bare `set` on a window
/// Hyprland already believes is fullscreen is a compositor NO-OP, see the
/// root-cause note on `reassert_gallery_fullscreen`); step 2 re-enters
/// fullscreen after the compositor settles, gated on `step2_gate()`
/// re-checked at fire time so a hide/close/transition mid-cycle cancels
/// the set.
fn run_fullscreen_cycle(step2_gate: impl Fn() -> bool + 'static) {
    if let Some(mut ctrl) = crate::composer::global_controller() {
        // Step 1 of the cycle: drop fullscreen (fires a real state change).
        let _ = ctrl.composer().set_fullscreen(false);
        if !ctrl.gallery_session_active() {
            let _ = ctrl.enter_gallery_session();
        }
    }
    // Step 2: re-enter fullscreen after the compositor settles the unset.
    slint::Timer::single_shot(std::time::Duration::from_millis(150), move || {
        if !step2_gate() {
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

/// Reload-path entry (configreloaded line, UI thread): gather the live
/// state, consult the pure decision, act. Runs on the UI thread only —
/// `slint::Timer` is thread-local, so a step-2 timer created on the IPC
/// listener thread would never fire (hypr_ipc hops via
/// invoke_from_event_loop).
pub(crate) fn reassert_fullscreen_after_reload() {
    let action = reload_restore_action();
    match action {
        ReloadFsAction::Cycle => {
            apply_reload_fs_action(action, run_reload_cycle);
        }
        ReloadFsAction::Nothing(ReloadFsSkip::Debounced) => {
            // The last reload of a burst can refuse inside the shared
            // debounce window: reload A cycled (stamped LAST_REASSERT),
            // reload B re-floated HVE 1.4s later and is refused. If B is
            // the LAST line, nothing else would ever restore the
            // fullscreen — HVE would stay windowed below the bar. Arm the
            // single delayed re-consult: once the window expires it re-runs
            // this same decision against the live state. Bounded: one
            // pending per refused decision, the tick never re-arms.
            tracing::debug!(
                "[reassert] reload restore debounced — arming the single delayed re-consult"
            );
            arm_reload_reconsult(reload_reconsult_tick);
        }
        ReloadFsAction::Nothing(skip) => {
            tracing::debug!("[reassert] reload restore skipped: {:?}", skip);
        }
    }
}

/// Gather the live state and consult the pure decision — shared by the
/// configreloaded-line entry and the delayed re-consult. The re-consult
/// must re-gather (not reuse the stale refusal inputs): that is the
/// verifier's "re-run the decision once against the live state".
fn reload_restore_action() -> ReloadFsAction {
    reload_fs_action(
        is_theme_transitioning_flag(),
        hve_supposed_visible(),
        immersive_expected(),
        crate::composer::hyprland::query_hve_seen().fullscreen,
        last_reassert_elapsed(),
    )
}

/// The single delayed re-consult, fired once the debounce window expired:
/// re-run the FULL decision against the live state. Cycle → the shared
/// unset→set cycle (which re-claims the debounce atomically at fire time);
/// any refusal — including a fresh Debounced if another cycle ran in the
/// meantime — does nothing. NEVER re-arms: one delayed re-consult per
/// refused decision, never a self-perpetuating timer.
fn reload_reconsult_tick() {
    let action = reload_restore_action();
    match action {
        ReloadFsAction::Cycle => {
            apply_reload_fs_action(action, run_reload_cycle);
        }
        ReloadFsAction::Nothing(skip) => {
            tracing::debug!("[reassert] reload re-consult declined: {:?}", skip);
        }
    }
}

/// Pure timing for the single delayed re-consult: how long until the
/// shared debounce window expires, measured from the last cycle. A
/// Debounced refusal guarantees elapsed < window, so the re-consult fires
/// exactly when the window expires, never inside it (a timer can only be
/// late). Defensive: never cycled (None) or elapsed ≥ window → after one
/// full window (the refusal cannot have happened in those states, but the
/// delay stays bounded and never underflows).
fn reload_reconsult_delay(since_last_cycle: Option<std::time::Duration>) -> std::time::Duration {
    let window = std::time::Duration::from_millis(REASSERT_DEBOUNCE_MS);
    since_last_cycle
        .map(|elapsed| window.saturating_sub(elapsed))
        .unwrap_or(window)
}

/// Arm the single delayed re-consult after the debounce window (the
/// last-reload-of-a-burst fix). ONE single-shot `slint::Timer` on the UI
/// thread (thread-local timers — the caller already hopped); a second
/// Debounced refusal while one is pending arms nothing (the pending one
/// re-checks the live state, which already includes the later re-float —
/// single owner, no stacked timers/cycles). Returns whether THIS call
/// armed it.
fn arm_reload_reconsult(tick: impl FnOnce() + 'static) -> bool {
    let mut pending = PENDING_RECONSULT_AT.lock().unwrap_or_else(|e| e.into_inner());
    if pending.is_some() {
        tracing::debug!("[reassert] reload re-consult already pending — no second timer");
        return false;
    }
    let delay = reload_reconsult_delay(last_reassert_elapsed());
    *pending = Some(std::time::Instant::now() + delay);
    slint::Timer::single_shot(delay, move || {
        *PENDING_RECONSULT_AT.lock().unwrap_or_else(|e| e.into_inner()) = None;
        tick();
    });
    true
}

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

/// The active workspace name behind `Composer` (capability-routing Phase 3):
/// the raw `activeworkspace -j` call moved into the composer, which keeps the
/// same JSON path (`name`). Falls back to "2" exactly like the raw call did
/// when the compositor did not answer.
fn get_active_workspace_name() -> String {
    active_workspace_name(
        crate::composer::global_controller().and_then(|ctrl| ctrl.composer().active_workspace()),
    )
}

/// The contract of `get_active_workspace_name` over the composer's answer:
/// no answer → the same `"2"` the raw query fell back to.
fn active_workspace_name(answer: Option<String>) -> String {
    answer.unwrap_or_else(|| "2".to_string())
}

fn find_empty_workspace(exclude: &str) -> String {
    // Find a normal workspace with no windows, not special, not the current one.
    // Falls back to "10" (Hyprland creates it on demand).
    let answer = crate::composer::global_controller()
        .and_then(|ctrl| ctrl.composer().query_json(crate::composer::CompositorQuery::Workspaces));
    empty_workspace_from(answer.as_deref(), exclude)
}

/// The contract of `find_empty_workspace` over an optional raw
/// `workspaces -j` answer (`None` when the composer did not answer):
/// same selection order and same `"10"` / `"11"` fallbacks as before the
/// call moved behind `Composer`.
fn empty_workspace_from(workspaces_json: Option<&str>, exclude: &str) -> String {
    let out = workspaces_json.and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok());
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
    // Validation stays HERE (the unsafe-name rejection is this function's
    // contract); the spawn itself is the composer's dispatch capability.
    match crate::composer::global_controller() {
        Some(ctrl) => ctrl.composer().dispatch_script(&script),
        None => crate::composer::dispatch_script_detached(&script),
    }
}

/// Re-set fullscreen on HVE via the WINDOW-TARGETED v5 dispatcher (reuses
/// composer's verified builder). Crucially it carries NO focus step: the
/// focus-by-title inside `reassert_gallery_fullscreen` targets HVE on
/// orig_ws and focusing a window on another workspace drags the VIEW back
/// with it — the early ventanita+windows flash. Idempotent when HVE is
/// already fullscreen (no-op), a real transition when a reload re-floated it.
fn reassert_hve_fullscreen_targeted() -> bool {
    let script = crate::composer::hyprland::v5_set_fullscreen(true);
    match crate::composer::global_controller() {
        Some(ctrl) => ctrl.composer().dispatch_script(&script),
        None => crate::composer::dispatch_script_detached(&script),
    }
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
            // Single-instance handoff: another instance owns the lock, so this
            // process must create NO window and must NOT steal the lock. It
            // asks the running instance to raise its window over the existing
            // IPC socket (idempotent `show`) and exits with the report's code.
            // `handoff_show_running` is bounded internally, so the launcher
            // always terminates; when the instance cannot be reached it exits
            // non-zero with both ways out.
            let report = ipc::handoff_show_running();
            if report.succeeded {
                println!("{}", report.message);
            } else {
                eprintln!("{}", report.message);
            }
            std::process::exit(report.exit_code);
        } else {
            tracing::error!("Failed to acquire exclusive lock: {}", e);
        }
        std::process::exit(1);
    }
    let lock = Arc::new(std::sync::Mutex::new(Some(lock_file)));

    // ── Crash repair for the colour-authority yield (W2) ──────────────
    // If a previous HVE died between the yield flip and the restore, the
    // engine's colour scheme would stay off until a manual fix. Replay the
    // repair once, now (only the owning instance runs it — the handoff
    // path above exits first): restore the previous value when a marker
    // says we left it off and log the outcome either way. A failure never
    // aborts startup.
    crate::providers::skwd_policy::startup_recover_crashed_yield();
    // A crash between masking and restoring would leave the desktop without
    // blur, border and shadow (and the next apply would make it permanent).
    // Replay it before anything else touches the compositor.
    startup_recover_crashed_masks();

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

    // ── Borders panel i18n (B4) ────────────────────────────────────────
    // The section's ~50 labels live in the exported `BordersText` global so
    // the tune pane reads them wherever it is instantiated; this fills that
    // global from the same embedded map the shell strings above use.
    panel_i18n::apply_borders(&window, &tr);
    // The shared chrome — the left nav rail and the panel header hint — is not
    // Borders-specific, so it lives in its own global and gets its own pass.
    panel_i18n::apply_panel_chrome(&window, &tr);

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
    crate::providers::register_default_providers(&mut theme_manager, &engine);
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
        // Upgrade window (capability-routing R2): a theme applied before
        // the colour-authority descriptor existed declares none. Rebuild it
        // once from that theme's own saved state — a quiet no-op when the
        // descriptor is already there or when the state to derive it from
        // is missing. Never fails startup: every error is logged.
        theme_manager.restore_missing_colour_authority();
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
            crate::providers::register_default_providers(&mut gtm, &engine);
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
    // Late-bound SharedState for the gallery apply path: the card-click
    // wiring below runs BEFORE `state` (AppState) is created, so the
    // closure cannot capture it directly. Filled right after creation; the
    // handler refreshes the in-memory preset state from disk after apply.
    let gallery_state_slot: std::sync::Arc<
        std::sync::Mutex<Option<crate::app_state::SharedState>>,
    > = std::sync::Arc::new(std::sync::Mutex::new(None));
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
        let mut sources: Vec<(usize, String, Option<thumbs::PreviewSpec>)> = Vec::new();
        for i in 0..model.row_count() {
            if let Some(row) = model.row_data(i) {
                if row.thumb.size().width > 0 {
                    continue; // already marshaled — no duplicate work
                }
                let src = thumbs::plan_preview_source(&themes_root.join(row.name.as_str()));
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
            let gallery_state_c = gallery_state_slot.clone();
            let proj_c = proj.clone();
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
                        // Disk write + late-bound SharedState refresh (the
                        // gallery_state_slot is filled once AppState exists).
                        let mut cfg = crate::config::Config::load();
                        cfg.last_applied_theme = name.clone();
                        let _ = cfg.save();
                        // Refresh the in-memory copy from the disk the
                        // provider apply just wrote: without this AppState.cfg
                        // still carries the previous manual preset files, so
                        // the Borders panel would re-seed its marker from the
                        // manual value and any later in-memory save would
                        // revert the disk. Lock discipline: the slot guard is
                        // dropped before the reload, so only the state lock is
                        // held across its brief load/save — never two locks at
                        // once, and no engine calls while held (same policy as
                        // every other SharedState callback).
                        let shared = gallery_state_c
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .clone();
                        if let Some(ref shared) = shared {
                            shared
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .reload_config_after_theme(&name);
                        }
                        // The theme carried a border: populate the Borders
                        // tune pane through the same shared sync a manual
                        // border click uses, so the pane shows the theme's
                        // chips/angle/glow/geometry instead of the previous
                        // manual values. Lock discipline: the state lock is
                        // held only for the short clone below — the sync's
                        // file I/O runs unlocked, and the marker re-seed
                        // reads the window only.
                        let (applied_border, geometry) = shared
                            .as_ref()
                            .map(|s| {
                                let st = s.lock().unwrap_or_else(|e| e.into_inner());
                                (
                                    st.cfg().active_border_file.clone(),
                                    crate::callbacks::BorderGeometry::from_config(st.cfg()),
                                )
                            })
                            .unwrap_or_default();
                        if let Some(w) = win.upgrade() {
                            crate::sync_border_tune_pane(&w, &proj_c, &applied_border, geometry);
                            crate::presets::reseed_active_border_index(&w, &applied_border);
                        }
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
                        // A restored custom palette may be re-asserted LATE
                        // (~2 s after apply): schedule the bounded focus
                        // restores covering that window. Non-custom applies
                        // schedule nothing.
                        schedule_post_apply_focus_restores(&win, &name, &gallery_themes_root);
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
                        // W5: arm the signal wait AT THE START (T=0), not inside
                        // the timer. The compositor's `fullscreen>>1` can arrive
                        // at any moment from here on, and the whole point is that
                        // a FAST compositor may signal before the 1500ms fallback
                        // ever fires. Arming at the timer (the previous shape)
                        // made every early signal unlistenable — the fix was
                        // wired but inert.
                        *THEME_WAITING_STEP.lock().unwrap() = Some(TransitionStep::Fullscreen);
                        *THEME_FINALE_WINDOW.lock().unwrap() = Some(win.clone());
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
                        // Mask blur + border + shadow for the whole transition,
                        // so the reveal has no blurred desktop and no window
                        // frame. `animations` is deliberately NOT masked: the
                        // ANIMATED reveal is the effect the owner wants.
                        // Reloads wipe runtime options — hypr_ipc re-applies them
                        // on every configreloaded line.
                        {
                            let _serial = MASK_WRITE_SERIAL.lock().unwrap();
                            match begin_mask() {
                                Err(e) => tracing::warn!(
                                    "[theme] masking SKIPPED (no crash marker): {}",
                                    e
                                ),
                                Ok(originals) => {
                                    let failures = write_mask_payload(true, &originals, hypr_eval);
                                    log_mask_failures(&failures);
                                    tracing::info!(
                                        "[theme] masked {:?} ({} write failure(s))",
                                        originals,
                                        failures.len()
                                    );
                                }
                            }
                        }
                        if let Some(w) = win.upgrade() {
                            w.set_theme_transitioning(true);
                        }
                        let weak_back = win.clone();
                        let fallback_gen = *THEME_GEN.lock().unwrap();
                        // T+560ms: the interlude stage now lives inside
                        // `begin_interlude_display`, which runs it after the
                        // reload settles AND holds it for the display time. The
                        // standalone stage that used to sit here would double the
                        // staging and race its own hold, so it is gone.
                        // W5c: the ORDER the owner wants back. The finale must
                        // NOT ride the reload's timing — it must give the empty
                        // desktop its time on screen first. The old fixed clock
                        // did that by accident (1500ms happened to leave ~940ms
                        // of showing); W5b removed the accident and the empty
                        // desktop became a flicker that exposed the real
                        // workspace. So: detect the reload for real (no guessing)
                        // -> stage the empty desktop -> HOLD it for the display
                        // time -> finale.
                        let armed_bump = arm_reload_settle(fallback_gen);
                        *THEME_WAITING_STEP.lock().unwrap() = Some(TransitionStep::Restore);
                        // W5c + review fix: the wait must survive an ARBITRARILY
                        // long burst. The first shape re-armed only ONCE and then
                        // gave up to the fixed clock, so a burst longer than two
                        // windows (~900ms) — the 2s-12s bursts recorded on the
                        // slow laptop — fell back to the very guessing this unit
                        // removes. The poll keeps re-arming while the burst is
                        // alive, with NO cap. The 1500ms net is no longer its
                        // escape hatch; it is only for a transition with no
                        // reload at all.
                        arm_settle_poll(weak_back.clone(), fallback_gen, armed_bump);
                        // Safety net: a transition that never sees a reload still
                        // finishes. Unchanged from the pre-W5 behaviour.
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
                            // The net fires only when the interlude never took
                            // ownership of this generation. With the unbounded
                            // settle poll, that is now the "no reload arrived at
                            // all" case (the poll would otherwise have staged and
                            // held it). If the interlude owns the timing, this is
                            // a no-op.
                            if !interlude_display_owned(fallback_gen) {
                                if let Some(w) = weak_back.upgrade() {
                                    let claimed_restore = claim_waiting_step(TransitionStep::Restore);
                                    let claimed_fs = claim_waiting_step(TransitionStep::Fullscreen);
                                    if claimed_restore || claimed_fs {
                                        tracing::info!(
                                            "[theme] finale advanced by the safety timer (gen {})",
                                            fallback_gen
                                        );
                                        theme_finale_run(w, fallback_gen);
                                    }
                                }
                            }
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
        // NOTE: panel_section_selected is wired further down, after `state` exists, because D4
        // draft cleanup needs the engine to restore the active border when leaving Borders.
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
        // NOTE: on_panel_back is wired further down, after `state` exists, because
        // D4 draft teardown needs the engine to restore the active border on close.
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

    // ── Seed the Borders tune pane from the loaded theme ──
    // This runs once, strictly AFTER `refresh_visual_state` above: the sync
    // bakes concrete `slint::Color`s from `token-*` (`refresh_resolved_colors`),
    // so seeding before the theme push would freeze the default accent in the
    // pane's chips and previews. Without this seed the ONLY writers of the
    // pane are the apply paths, so a fresh launch showed Slint defaults (no
    // chips, glow off, 2/32/5/5) while the desktop rendered the loaded theme.
    // An empty active border deliberately clears the pane, so there is nothing
    // to seed — skip it (same reason `sync_border_tune_pane` guards on empty).
    if !cfg.active_border_file.is_empty() {
        let geometry = crate::callbacks::BorderGeometry::from_config(&cfg);
        crate::seed_borders_pane_from_persisted(
            &window,
            &proj,
            &cfg.active_border_file,
            geometry,
        );
    }

    // ── Central view-model: Config + Engine + ThemeManager behind one lock ──
    // The engine now lives inside AppState; the providers registered above
    // keep their own engine clones, so the outer Arc can be dropped.
    let state = Arc::new(std::sync::Mutex::new(AppState::new(
        cfg,
        (*engine).clone(),
        theme_manager,
    )));
    // Publish SharedState to the late-bound gallery slot so the card-click
    // handler (wired before this point) can refresh in-memory preset state
    // from disk after a gallery apply.
    *gallery_state_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
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

    // ── D4: Section-leave draft cleanup ──
    // Wire this AFTER state is created so we can access the engine for re-apply.
    // All section changes (rail clicks + keyboard) flow through this single callback.
    {
        let shell_c = shell.clone();
        let state_c = state.clone();
        let proj_c = proj.clone();
        let weak = window.as_weak();
        window.on_panel_section_selected(move |section| {
            tracing::debug!("[panel][mouse] section-selected section={} (rail click)", section);
            if crate::shell::Shell::is_mutating(&shell_c) {
                tracing::debug!("[panel] section-selected ignored — mutating");
                return;
            }
            let idx = section.max(0) as usize;
            let target = crate::shell::nav::PanelSection::from_index(idx).unwrap_or(crate::shell::nav::PanelSection::Save);
            let (is_closed, is_open, prev_section) = crate::shell::Shell::with_nav(&shell_c, |n| {
                let state = n.panel_state();
                let is_closed = state == crate::shell::nav::PanelState::Closed;
                let is_open = matches!(state, crate::shell::nav::PanelState::Open(_));
                let prev = n.panel_section();
                (is_closed, is_open, prev)
            });
            tracing::debug!("[panel] section-selected target={:?} is_closed={} is_open={}", target, is_closed, is_open);
            if is_closed {
                let _ = crate::shell::Shell::enter_panel(&shell_c, target);
            } else if is_open {
                // D4: leaving Borders drops the hidden draft and restores the
                // active border (same teardown as closing the panel).
                if prev_section == Some(crate::shell::nav::PanelSection::Borders)
                    && target != crate::shell::nav::PanelSection::Borders
                {
                    tracing::debug!("[panel] leaving Borders section — cleaning up draft");
                    teardown_border_draft(&state_c, &proj_c, &weak);
                }
                crate::shell::Shell::set_panel_section(&shell_c, target);
            }
            // Entering Borders — by opening the panel onto it or by switching
            // section — re-seeds the marker from what is actually persisted.
            // The index is otherwise written only at startup and on apply, so
            // a preset list that changed after that seed (or was not ready
            // when it ran) left the board unmarked while a border was loaded.
            // The tune pane is seeded here too: it is the only other place
            // (besides startup) where the pane can be shown without an apply,
            // and without this the pane kept the previous/empty values while
            // the desktop rendered the loaded theme.
            if target == crate::shell::nav::PanelSection::Borders {
                if let Some(w) = weak.upgrade() {
                    // Re-selecting the open Borders section must not re-seed
                    // (see `borders_entry_should_seed`): a live draft would be
                    // wiped from the pane. The marker re-seed is equally safe
                    // to skip there — the board is already correct.
                    if borders_entry_should_seed(is_closed, prev_section) {
                        let (active_file, geometry) = {
                            let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                            (
                                st.cfg().active_border_file.clone(),
                                crate::callbacks::BorderGeometry::from_config(st.cfg()),
                            )
                        };
                        crate::seed_borders_pane_from_persisted(&w, &proj_c, &active_file, geometry);
                    }
                }
            }
        });
    }

    // ── Esc / back press: close the panel (D4 draft teardown on close) ──
    // Wired here, after `state` exists, so closing the panel from the Borders
    // section restores the active border instead of leaving the draft live.
    {
        let shell_c = shell.clone();
        let state_c = state.clone();
        let proj_c = proj.clone();
        let win = window.as_weak();
        window.on_panel_back(move || {
            tracing::debug!("[panel][kbd] back pressed (Esc)");
            let is_mutating = crate::shell::Shell::is_mutating(&shell_c);
            let (is_open, on_borders) = crate::shell::Shell::with_nav(&shell_c, |n| {
                (
                    matches!(n.panel_state(), crate::shell::nav::PanelState::Open(_)),
                    n.panel_section() == Some(crate::shell::nav::PanelSection::Borders),
                )
            });
            if is_mutating || is_open {
                if is_open && on_borders {
                    tracing::debug!("[panel] closing panel from Borders — cleaning up draft");
                    teardown_border_draft(&state_c, &proj_c, &win);
                }
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
                    let mut sources: Vec<(usize, String, Option<thumbs::PreviewSpec>)> = Vec::new();
                    for i in 0..model.row_count() {
                        if let Some(row) = model.row_data(i) {
                            if row.thumb.size().width > 0 {
                                continue;
                            }
                            let src = thumbs::plan_preview_source(&gallery_themes_root2.join(row.name.as_str()));
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
            let proj_c = proj.clone();
            let gallery_themes_root_c = gallery_themes_root.clone();
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
                // The theme carried a border: populate the Borders tune pane
                // through the same shared sync a manual border click uses
                // (same chips/angle/glow/geometry), so entering Borders
                // afterwards needs no restart. Short lock for the clone
                // only; the sync's file I/O runs unlocked.
                let (applied_border, geometry) = {
                    let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    (
                        st.cfg().active_border_file.clone(),
                        crate::callbacks::BorderGeometry::from_config(st.cfg()),
                    )
                };
                if let Some(w) = weak.upgrade() {
                    crate::sync_border_tune_pane(&w, &proj_c, &applied_border, geometry);
                    crate::presets::reseed_active_border_index(&w, &applied_border);
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
                    // Same late re-assert window as a gallery-click apply: a
                    // restored custom palette can drop focus seconds later,
                    // after the panel-close reseed already ran.
                    schedule_post_apply_focus_restores(&weak, &name, &gallery_themes_root_c);
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
                // Rename changes no artwork: empty invalidation set keeps
                // every bake (no flicker for unrelated cards).
                sync_save_gallery_ui(&w, &gallery_tm_c, &refresh_mosaic_page_c, &refresh_slice_ring_c, &std::collections::HashSet::new());
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
                sync_save_gallery_ui(&w, &gallery_tm_c, &refresh_mosaic_page_c, &refresh_slice_ring_c, &std::collections::HashSet::new());
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
                sync_save_gallery_ui(&w, &gallery_tm_c, &refresh_mosaic_page_c, &refresh_slice_ring_c, &std::collections::HashSet::new());
            });
        }
        {
            let state_c = state.clone();
            let gallery_tm_c = gallery_tm.clone();
            let refresh_mosaic_page_c = refresh_mosaic_page.clone();
            let refresh_slice_ring_c = refresh_slice_ring.clone();
            let gallery_themes_root_c = gallery_themes_root.clone();
            let stage_dims_c = stage_dims.clone();
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
                // The overwrite changed this theme's artwork on disk:
                // invalidate its card AFTER the carry-over merge (stale bake
                // would otherwise win) and re-run the existing thumb
                // scheduler so the blank row re-resolves and re-bakes.
                // Unconditional for the affected theme only — a content-keyed
                // warm cache makes an unchanged source cheap, and unrelated
                // cards keep their bakes (no mass invalidation, no flicker).
                let mut invalidated = std::collections::HashSet::new();
                invalidated.insert(name_str.clone());
                sync_save_gallery_ui(&w, &gallery_tm_c, &refresh_mosaic_page_c, &refresh_slice_ring_c, &invalidated);
                schedule_thumbs(&weak, &gallery_themes_root_c, &stage_dims_c, refresh_mosaic_page_c.clone());
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
        // Preset rename/delete dialogs (Borders + Motion) — same shared dialog
        // component as the theme Save section, with the section's own strings.
        window.set_border_rename_title(tr.tr_shared(
            "borders.rename_dialog.title",
            "Rename border preset",
        ));
        window.set_border_delete_title(tr.tr_shared(
            "borders.delete_dialog.title",
            "Delete border preset",
        ));
        window.set_border_delete_msg(tr.tr_shared(
            "borders.delete_dialog.message",
            "Are you sure you want to delete",
        ));
        window.set_border_cancel_text(tr.tr_shared("themes.cancel", "Cancel"));
        window.set_animation_rename_title(tr.tr_shared(
            "animations.rename_dialog.title",
            "Rename animation preset",
        ));
        window.set_animation_rename_placeholder(tr.tr_shared(
            "animations.rename_dialog.placeholder",
            "Animation preset name…",
        ));
        window.set_animation_delete_title(tr.tr_shared(
            "animations.delete_dialog.title",
            "Delete animation preset",
        ));
        window.set_animation_delete_msg(tr.tr_shared(
            "animations.delete_dialog.message",
            "Are you sure you want to delete",
        ));
        window.set_animation_cancel_text(tr.tr_shared("themes.cancel", "Cancel"));
    }

    // ── Panel Borders tune: INSTANT hot geometry + ONE deferred persist ──
    // The tick must be felt at once, and the value must still survive a reload.
    // Those are two different writes:
    //   • HOT — only the options that changed, as one `hl.config({...})` chunk
    //     through `hyprctl eval` (~5 ms measured live). It reaches the running
    //     compositor immediately but does NOT survive `hyprctl reload`.
    //   • DURABLE — the fragment + `assemble.sh` path (~175 ms measured) writes
    //     what `dofile` reads back on reload. It is coalesced behind
    //     `arm_geometry_persist`, so a drag writes ONCE instead of per 80 ms
    //     tick. The config value written by the hot path is what gets saved.
    //
    // The coalescer and its timer are declared OUTSIDE the wiring block so the
    // shutdown flush below can still drain an owed persist after the event loop
    // returns (B5): a quit inside the 400 ms idle window must not drop the drag.
    let geometry_persist_pending = std::rc::Rc::new(std::cell::RefCell::new(
        callbacks::GeometryPersistCoalescer::default(),
    ));
    let geometry_persist_timer = std::rc::Rc::new(slint::Timer::default());
    {
        let state_c = state.clone();
        let weak = window.as_weak();
        let guard_permits = guard_permits;
        let guard_block_message = guard_block_message.clone();
        let persist_pending = geometry_persist_pending.clone();
        let persist_timer = geometry_persist_timer.clone();
        window.on_panel_apply_geometry(move |size, radius, gap_in, gap_out| {
            tracing::debug!("[borders][mouse|kbd] apply-geometry size={} radius={} gap_in={} gap_out={}", size, radius, gap_in, gap_out);
            if !guard_permits {
                tracing::warn!("[guard] geometry apply refused (migration guard)");
                if let Some(w) = weak.upgrade() {
                    w.set_shell_status_text(guard_block_message.clone());
                }
                return;
            }
            let next = callbacks::BorderGeometry { size, radius, gap_in, gap_out };
            {
                let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                let prev = callbacks::BorderGeometry {
                    size: st.cfg().border_size,
                    radius: st.cfg().border_radius,
                    gap_in: st.cfg().gaps_in,
                    gap_out: st.cfg().gaps_out,
                };
                if next == prev {
                    return;
                }
                // Hot: push ONLY what changed, right now.
                if let Some(chunk) = callbacks::geometry_lua_chunk(&prev, &next) {
                    if let Err(e) = hypr_eval(&chunk) {
                        tracing::error!("[HVE] Hot geometry apply failed: {}", e);
                    }
                }
                st.cfg_mut().border_size = size;
                st.cfg_mut().border_radius = radius;
                st.cfg_mut().gaps_in = gap_in;
                st.cfg_mut().gaps_out = gap_out;
                // B5: the config must not lag 400 ms behind the desktop. A
                // crash inside the idle window would otherwise lose the whole
                // drag from disk. This is a small JSON write (~ms), so the hot
                // path keeps its millisecond budget; only the fragment/assemble
                // half stays coalesced. A config write failure is not fatal
                // here (best-effort hardening) — the drain below retries it.
                let _ = st.cfg().save();
            }
            // Durable: one write once the drag goes quiet (restart = trailing).
            persist_pending.borrow_mut().request(next);
            arm_geometry_persist(&persist_timer, &persist_pending, &state_c);
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
        let proj_c = proj.clone();
        window.on_panel_apply_border(move |idx, file| {
            tracing::debug!("[borders][mouse|kbd] apply-border idx={} file={} (click/card)", idx, file);
            if !guard_permits {
                tracing::warn!("[guard] border apply refused (migration guard)");
                if let Some(w) = weak.upgrade() {
                    w.set_shell_status_text(guard_block_message.clone());
                }
                return;
            }
            use crate::callbacks::BorderGeometry;
            let file_str = file.to_string();
            let (is_deact, result) = {
                let mut st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                let is_deact = st.cfg().active_border_file == file_str;
                let new = if is_deact { String::new() } else { file_str.clone() };
                st.cfg_mut().active_border_file = new.clone();
                let _ = st.cfg().save();
                let arg = if is_deact { "none" } else { &new };
                let res = st.engine().apply_border(arg);
                // D3: a card carries border IDENTITY only. Geometry is
                // persisted state the loaded theme wrote (and Hyprland
                // renders), so a click preserves it — the retired lookup
                // table used to overwrite it with 2/32/5/5 on every pick.
                (is_deact, res)
            };
            if let Err(e) = result {
                tracing::error!("[HVE] Border error: {}", e);
            }
            // Read the geometry AFTER the identity toggle: deactivation and
            // activation both leave it untouched, so the pane shows what the
            // desktop actually has.
            let geometry = {
                let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                BorderGeometry::from_config(st.cfg())
            };
            if let Some(w) = weak.upgrade() {
                w.set_active_border_index(if is_deact { -1 } else { idx });
                // Tune pane population lives in the single shared sync (used
                // by the manual path and both theme-apply paths alike).
                let file = if is_deact { String::new() } else { file_str.clone() };
                crate::sync_border_tune_pane(&w, &proj_c, &file, geometry);
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
        let proj_c = proj.clone();
        window.on_panel_save_border_preset(move |name| {
            let name_str = name.to_string();
            tracing::debug!("[borders][preset] save name={}", name_str);
            let result = if let Some(w) = weak.upgrade() {
                // Task 2.4: build params from tune properties → generate_border_lua_full
                let size = {
                    let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    st.cfg().border_size
                };
                use crate::preset_store::PresetStore;
                let params = tune_params_from_window(&w, size);
                let content = PresetStore::generate_border_lua_full(&name_str, &params);
                let store = PresetStore::new("borders");
                let r = store.save(&name_str, &content);
                if r.is_ok() {
                    let store2 = PresetStore::new("borders");
                    let names: Vec<_> = store2.list()
                        .into_iter()
                        .map(|n| slint::SharedString::from(n.as_str()))
                        .collect();
                    let tags: Vec<_> = names.iter().map(|_| slint::SharedString::from("CUSTOM")).collect();
                    w.set_user_border_preset_names(slint::ModelRc::from(names.as_slice()));
                    w.set_user_border_preset_tags(slint::ModelRc::from(tags.as_slice()));
                    w.set_border_save_error(String::new().into());
                    w.set_border_preset_name(String::new().into());
                    // The working state is now the saved preset (D4).
                    w.set_tune_dirty(false);
                    // D4: remove hidden draft after save + re-apply saved border
                    PresetStore::remove_draft(&proj_c);
                    {
                        let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                        if let Err(e) = st.engine().apply_border(&name_str) {
                            tracing::error!("[borders][preset] re-apply after save failed: {}", e);
                            drop(st);
                            if let Some(w) = weak.upgrade() {
                                w.set_border_save_error(slint::SharedString::from(
                                    &format!("Border apply failed: {}", e),
                                ));
                            }
                        }
                    }
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
    // ── Border tune-changed: live draft (D4) ──
    {
        let state_c = state.clone();
        let weak = window.as_weak();
        let proj_c = proj.clone();
        window.on_panel_tune_changed(move |colors, angle, inactive, glow, anims, rule| {
            tracing::debug!("[borders][tune] tune-changed angle={} rule={}", angle, rule);
            if !has_active_border(&state_c) {
                tracing::debug!(
                    "[borders][tune] refused: no active border preset (edit would have no target)"
                );
                if let Some(w) = weak.upgrade() {
                    w.set_border_save_error(slint::SharedString::from(TUNE_NEEDS_ACTIVE_BORDER));
                }
                return;
            }
            use crate::preset_store::PresetStore;
            let size = {
                let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                st.cfg().border_size
            };
            // colors is now a [string] model; join for decode_colors
            let colors_joined: String = {
                let mut parts = Vec::new();
                for i in 0..colors.row_count() {
                    if let Some(s) = colors.row_data(i) {
                        parts.push(s.to_string());
                    }
                }
                parts.join(",")
            };
            let params = crate::border_preset::BorderParams {
                active_colors: PresetStore::decode_colors(&colors_joined),
                angle,
                inactive: PresetStore::decode_colors(&inactive.to_string()).into_iter().next()
                    .unwrap_or(crate::border_preset::BorderColor::Token(crate::border_preset::PaletteToken::SurfaceLowest)),
                border_size: Some(size),
                glow: PresetStore::decode_glow(&glow.to_string()),
                rule_enabled: rule,
                animations: PresetStore::decode_animations(&anims.to_string()),
                curves: Vec::new(),
            };
            let content = PresetStore::generate_border_lua_full("draft", &params);
            match PresetStore::write_draft(&content, &proj_c) {
                Ok(_fragment) => {
                    // Assemble via the engine's existing script runner.
                    let st = state_c.lock().unwrap_or_else(|e| e.into_inner());
                    if let Err(e) = st.engine().run_script("assemble.sh", &[]) {
                        tracing::error!("[borders][tune] assemble after draft write failed: {}", e);
                        drop(st);
                        // Robustness fix (U3 carry-over): remove the draft fragment
                        // so a later successful assemble cannot silently pick up
                        // stale state from this failed attempt.
                        PresetStore::remove_draft(&proj_c);
                        if let Some(w) = weak.upgrade() {
                            w.set_border_save_error(slint::SharedString::from(
                                &format!("Live preview failed: {}", e),
                            ));
                        }
                    } else if let Some(w) = weak.upgrade() {
                        // Clear any previous error on successful apply
                        w.set_border_save_error(String::new().into());
                        // Re-resolve the colour channels. `tune-colors-resolved`
                        // is what the pane's chips and the picker's `seed` read,
                        // and it was only rebuilt on load/add/remove — so after
                        // committing a CUSTOM colour the chip kept showing the old
                        // one and reopening the picker re-seeded that stale colour
                        // instead of the value just stored.
                        refresh_resolved_colors(&w);
                    }
                }
                Err(e) => {
                    tracing::error!("[borders][tune] draft write failed: {}", e);
                    if let Some(w) = weak.upgrade() {
                        w.set_border_save_error(slint::SharedString::from(
                            &format!("Draft write failed: {}", e),
                        ));
                    }
                }
            }
        });
    }
    // ── Border slot add/remove (U3.3) ──
    // Hyprland supports 2..8 gradient colors; the pane clamps and so do we, and
    // every change refreshes the live draft so the desktop follows immediately.
    {
        let state_c = state.clone();
        let weak = window.as_weak();
        let proj_c = proj.clone();
        window.on_panel_add_color_slot(move || {
            let Some(w) = weak.upgrade() else { return };
            let Some(slots) = slots_with_added(&read_tune_slots(&w)) else {
                return;
            };
            commit_tune_slots(&w, &state_c, &proj_c, slots);
        });
    }
    {
        let state_c = state.clone();
        let weak = window.as_weak();
        let proj_c = proj.clone();
        window.on_panel_remove_color_slot(move || {
            let Some(w) = weak.upgrade() else { return };
            let Some(slots) = slots_with_removed(&read_tune_slots(&w)) else {
                return;
            };
            commit_tune_slots(&w, &state_c, &proj_c, slots);
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
    // The binding is an owned ColorWatcher guard: dropping it at exit (below)
    // kills AND reaps the child — a raw Child drop would orphan the watcher
    // (odd/hide-idempotency-and-singleton-watcher W1).
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

    // ── Initial visibility according to the mode ──
    if tray_mode {
        // Lazy load: we don't call show() until the user presses SUPER+H.
        // The first toggle via IPC will create the xdg_toplevel with a debounce
        // of 400ms so the Wayland roundtrip is not sabotaged
        // by an accidental repeated key press.
        //
        // This fully avoids the set_minimized() problem on Wayland
        // (there is no programmatic un-minimize) and is 100% WM-agnostic.
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

        // On Wayland/Hyprland, show() does not guarantee automatic focus.
        // We force focus via Composer to avoid the initial double-click.
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

    // ── Flush a geometry drag a quit interrupted (B5) ──
    // The durable persist is coalesced behind a 400 ms idle timer; quitting
    // inside that window (window close, IPC quit, tray quit) would drop the
    // whole drag and the next `hyprctl reload` would revert the value the user
    // just saw. The idle timer can never fire again, so stop it and run the
    // same drain synchronously before the process leaves.
    flush_geometry_persist(
        &geometry_persist_timer,
        &geometry_persist_pending,
        &state,
    );

    // Clean up IPC socket after the event loop stops
    ipc::cleanup();

    // Real teardown of the color watcher: the guard's Drop kills + reaps the
    // child script. (A raw `std::process::Child` drop would NOT kill it — the
    // orphaned watchers that predate the guard are out of scope here.)
    drop(_color_watcher);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── W1: the transition masks must actually reach the compositor ──
    // `hyprctl keyword` is rejected by Hyprland's non-legacy (Lua) parser, so
    // the old `hypr_set` silently did nothing: measured 0/443 samples with
    // blur=0 across 10 theme applies. These pin the payload built for
    // `hyprctl eval 'hl.config(...)'` and the honest failure path.

    #[test]
    fn theme_masks_are_exactly_the_decided_set() {
        // W3 settled this: only the blurred desktop is masked. `animations` is
        // out for a product reason (the owner wants the animated reveal), and
        // `border`/`shadow` were dropped by the live A/B.
        for spec in THEME_MASKS {
            assert!(
                !spec.option.contains("animations") && !spec.lua_path.contains("animations"),
                "animations must NOT be masked, found option {:?}",
                spec.option
            );
        }
        let keys: Vec<&str> = THEME_MASKS.iter().map(|s| s.key).collect();
        assert_eq!(keys, vec!["blur"]);
    }

    // ── W5: a step advances on the compositor's signal, or its fallback ──

    #[test]
    fn a_signal_advances_only_its_own_waiting_step() {
        // Happy path: the fullscreen signal arrives while that step waits.
        assert!(signal_advances_step(
            Some(TransitionStep::Fullscreen),
            TransitionStep::Fullscreen,
            7,
            7
        ));
        // Nothing waiting: a stale signal must never drive a step.
        assert!(!signal_advances_step(None, TransitionStep::Fullscreen, 7, 7));
        // A different step than the waiting one is refused.
        assert!(!signal_advances_step(
            Some(TransitionStep::Fullscreen),
            TransitionStep::Fullscreen,
            6,
            7
        ));
    }

    #[test]
    fn a_signal_from_a_superseded_generation_is_ignored() {
        // A rapid second apply bumps the generation: the first apply's late
        // signal must not advance the new transition's step.
        assert!(!signal_advances_step(
            Some(TransitionStep::Fullscreen),
            TransitionStep::Fullscreen,
            6,
            7
        ));
        assert!(signal_advances_step(
            Some(TransitionStep::Fullscreen),
            TransitionStep::Fullscreen,
            7,
            7
        ));
    }

    #[test]
    fn signal_and_fallback_cannot_both_run_the_same_step() {
        // The claim is the race breaker: whoever gets it runs the step; the
        // loser (the fallback timer, or a duplicate signal) is refused.
        *THEME_WAITING_STEP.lock().unwrap() = Some(TransitionStep::Fullscreen);
        assert!(claim_waiting_step(TransitionStep::Fullscreen), "first claim wins");
        assert!(
            !claim_waiting_step(TransitionStep::Fullscreen),
            "the same step cannot be claimed twice"
        );
        // Nothing was waiting after the claim: a later signal is a no-op.
        assert!(!claim_waiting_step(TransitionStep::Fullscreen));
        *THEME_WAITING_STEP.lock().unwrap() = None;
    }

    #[test]
    fn a_signal_arriving_before_the_fallback_claims_the_step() {
        // THE REGRESSION THIS PINS: the wait must be armed at T=0, so a signal
        // that arrives BEFORE the fallback timer can claim the step. The first
        // implementation armed it inside the timer, which made every early
        // signal unlistenable (the fix was wired but inert).
        *THEME_WAITING_STEP.lock().unwrap() = Some(TransitionStep::Fullscreen);
        // Early signal: it finds a step waiting and claims it.
        assert!(
            claim_waiting_step(TransitionStep::Fullscreen),
            "a signal before the timer must be able to claim the step"
        );
        // The fallback timer then finds nothing waiting and is a no-op: the
        // finale cannot run twice.
        assert!(
            !claim_waiting_step(TransitionStep::Fullscreen),
            "the fallback must not re-run a step the signal already took"
        );
        *THEME_WAITING_STEP.lock().unwrap() = None;
    }

    // ── W5b: the finale waits for the reload burst to settle ──

    #[test]
    // These read/write the SHARED settle atomics, so they must not run in
    // parallel with each other (the whole suite runs multi-threaded).
    #[serial_test::serial]
    fn only_a_waiting_transition_restarts_the_settle_window() {
        // A reload during the restore wait: the burst is still going.
        assert!(reload_restarts_settle(
            true,
            Some(TransitionStep::Restore)
        ));
        // No transition: a reload must never arm anything.
        assert!(!reload_restarts_settle(false, Some(TransitionStep::Restore)));
        // Waiting on a different step: this reload is not ours.
        assert!(!reload_restarts_settle(
            true,
            Some(TransitionStep::Fullscreen)
        ));
        assert!(!reload_restarts_settle(true, None));
    }

    #[test]
    #[serial_test::serial]
    fn the_settle_window_fires_only_when_nothing_new_arrived() {
        // Armed at bump 5, gen 3; nothing new since -> the reload is done.
        THEME_SETTLE_BUMP.store(5, std::sync::atomic::Ordering::Relaxed);
        THEME_SETTLE_GEN.store(3, std::sync::atomic::Ordering::Relaxed);
        assert!(reload_settled_since(5, 3));
        // A newer reload arrived (bump moved) -> NOT settled, re-arm.
        THEME_SETTLE_BUMP.store(6, std::sync::atomic::Ordering::Relaxed);
        assert!(!reload_settled_since(5, 3));
        // A newer generation owns the slot -> not ours.
        THEME_SETTLE_BUMP.store(5, std::sync::atomic::Ordering::Relaxed);
        assert!(!reload_settled_since(5, 4));
    }

    #[test]
    #[serial_test::serial]
    fn the_live_reload_burst_shape_is_handled() {
        // The sentinel recorded 5-6 reloads inside one second. Each must restart
        // the window: only the LAST one, followed by silence, may fire.
        THEME_SETTLE_GEN.store(7, std::sync::atomic::Ordering::Relaxed);
        THEME_SETTLE_BUMP.store(0, std::sync::atomic::Ordering::Relaxed);
        let armed = arm_reload_settle(7);
        for _ in 0..5 {
            THEME_SETTLE_BUMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        assert!(
            !reload_settled_since(armed, 7),
            "five reloads in one second must NOT look settled"
        );
        // Silence afterwards: the last arm sees a stable bump -> settled.
        let rearmed = arm_reload_settle(7);
        assert!(reload_settled_since(rearmed, 7));
    }

    #[test]
    fn lua_literal_normalises_captured_originals() {
        assert_eq!(lua_literal(MaskKind::Flag, "1").as_deref(), Some("true"));
        assert_eq!(lua_literal(MaskKind::Flag, "true").as_deref(), Some("true"));
        assert_eq!(lua_literal(MaskKind::Flag, "0").as_deref(), Some("false"));
        assert_eq!(lua_literal(MaskKind::Flag, "false").as_deref(), Some("false"));
        assert_eq!(lua_literal(MaskKind::Flag, "[EMPTY]"), None);
    }

    #[test]
    fn lua_payload_pins_the_exact_masked_set() {
        let payload = theme_mask_payload(true, &std::collections::BTreeMap::new())
            .expect("the masked set must build");
        assert_eq!(
            payload,
            "hl.config({ decoration = { blur = { enabled = false } } })"
        );
    }

    #[test]
    fn lua_payload_restores_the_captured_originals() {
        let mut originals = std::collections::BTreeMap::new();
        originals.insert("blur", "1".to_string());
        // Unknown keys in the store are ignored: only the mask set is written.
        originals.insert("border", "2".to_string());
        originals.insert("shadow", "true".to_string());
        let payload = theme_mask_payload(false, &originals).expect("restore must build");
        assert_eq!(payload, "hl.config({ decoration = { blur = { enabled = true } } })");
    }

    #[test]
    fn a_missing_original_refuses_the_restore_payload() {
        // Never write a half-restore: the masked option has no captured original.
        let originals = std::collections::BTreeMap::new();
        assert_eq!(theme_mask_payload(false, &originals), None);
    }

    #[test]
    fn lua_payload_refuses_a_path_that_is_both_leaf_and_table() {
        let entries = vec![
            ("a.b".to_string(), "1".to_string()),
            ("a.b.c".to_string(), "2".to_string()),
        ];
        assert_eq!(lua_config_payload(&entries), None);
    }

    #[test]
    fn a_rejected_eval_is_reported_not_swallowed() {
        let mut originals = std::collections::BTreeMap::new();
        originals.insert("blur", "1".to_string());
        originals.insert("border", "2".to_string());
        originals.insert("shadow", "1".to_string());
        let failures = write_mask_payload(true, &originals, |_lua| {
            Err("keyword can't work with non-legacy parsers".to_string())
        });
        assert_eq!(failures.len(), 1, "one payload, one reported failure");
        assert!(
            failures[0].contains("non-legacy"),
            "the reason must survive, got {:?}",
            failures[0]
        );
    }

    #[test]
    fn a_successful_eval_reports_no_failure() {
        let mut originals = std::collections::BTreeMap::new();
        originals.insert("blur", "1".to_string());
        originals.insert("border", "2".to_string());
        originals.insert("shadow", "1".to_string());
        let failures = write_mask_payload(true, &originals, |_lua| Ok(()));
        assert!(failures.is_empty(), "got {:?}", failures);
    }

    #[test]
    fn a_failed_restore_keeps_the_originals_for_a_retry() {
        // The transition watchdog calls the restore path a second time, so a
        // failed restore must NOT discard what it would need to retry with.
        let mut originals = std::collections::BTreeMap::new();
        originals.insert("blur", "1".to_string());
        originals.insert("border", "2".to_string());
        originals.insert("shadow", "1".to_string());
        *THEME_MASK_ORIGINALS.lock().unwrap() = originals;

        let failures = restore_theme_masks_with(|_lua| Err("boom".to_string()));
        assert_eq!(failures.len(), 1, "the failure must be reported");
        assert_eq!(
            THEME_MASK_ORIGINALS.lock().unwrap().len(),
            3,
            "a failed restore must keep the originals so the watchdog can retry"
        );

        // A successful retry does clear the store.
        let failures = restore_theme_masks_with(|_lua| Ok(()));
        assert!(failures.is_empty(), "got {:?}", failures);
        assert!(
            THEME_MASK_ORIGINALS.lock().unwrap().is_empty(),
            "a successful restore must clear the store"
        );
    }

    #[test]
    fn a_second_apply_does_not_recapture_the_masked_values() {
        // The critical case: the first apply captures the TRUE originals; a
        // second apply during the transition must NOT capture the values the
        // first one just wrote, or every later restore would write the MASKED
        // values back and break the desktop for good.
        let mut store = std::collections::BTreeMap::new();
        let originals = || {
            let mut m = std::collections::BTreeMap::new();
            m.insert("blur", "1".to_string());
            m
        };
        let (plan, first) = mask_plan(&mut store, |_| Ok(()), originals).expect("marker persisted");
        assert!(first, "the first apply captures");
        assert_eq!(plan.get("blur").map(String::as_str), Some("1"));

        let already_masked = || {
            let mut m = std::collections::BTreeMap::new();
            m.insert("blur", "false".to_string());
            m
        };
        let (plan, first) =
            mask_plan(&mut store, |_| Ok(()), already_masked).expect("already captured");
        assert!(!first, "a newer apply must NOT capture");
        assert_eq!(
            plan.get("blur").map(String::as_str),
            Some("1"),
            "the true original must survive the second apply"
        );
        assert_eq!(store.get("blur").map(String::as_str), Some("1"));
    }

    #[test]
    fn a_mask_is_refused_when_its_crash_marker_cannot_be_written() {
        // Documented invariant: a mask without a marker is unrecoverable, so a
        // marker we cannot persist means we must NOT mask (cosmetic degrade)
        // rather than mask an unprotected desktop. The colour-authority mirror
        // refuses in the same situation (skwd_policy.rs propagates the marker
        // error and the yield never happens).
        let mut store = std::collections::BTreeMap::new();
        let captured = || {
            let mut m = std::collections::BTreeMap::new();
            m.insert("blur", "1".to_string());
            m
        };
        let outcome = mask_plan(&mut store, |_| Err("disk full".to_string()), captured);
        assert!(outcome.is_err(), "the marker failure must propagate");
        assert!(outcome.unwrap_err().contains("disk full"));
        assert!(
            store.is_empty(),
            "nothing may be adopted when the crash marker could not be written"
        );
    }

    #[test]
    fn a_surviving_marker_beats_capturing_the_live_values() {
        // A crashed transition can leave the compositor masked with the store
        // gone but the marker alive. Capturing the live values then would read
        // the MASKED ones and bake them in as "originals". The marker wins.
        let marker = || {
            let mut m = std::collections::BTreeMap::new();
            m.insert("blur", "1".to_string());
            m
        };
        let live = || {
            let mut m = std::collections::BTreeMap::new();
            m.insert("blur", "false".to_string());
            m
        };
        let chosen = choose_originals(Some(marker()), live);
        assert_eq!(
            chosen.get("blur").map(String::as_str),
            Some("1"),
            "the surviving marker must win over the masked live values"
        );

        let chosen = choose_originals(None, live);
        assert_eq!(chosen.get("blur").map(String::as_str), Some("false"));
    }

    #[test]
    fn the_apply_block_masks_only_in_the_ok_arm_of_begin_mask() {
        // House-style driving pin (same idea as the write_marker body pin in
        // providers/skwd_policy.rs): the pure `mask_plan` test above would still
        // pass if the apply block regressed to masking after a refused
        // begin_mask, so pin the wiring itself.
        let src = std::fs::read_to_string("src/main.rs").expect("src/main.rs must exist");
        let anchor = src
            .find("match begin_mask()")
            .expect("the apply block must match on begin_mask()");
        let after = &src[anchor..];
        let err_arm = after.find("Err(e) =>").expect("an Err arm");
        let ok_arm = after.find("Ok(originals) =>").expect("an Ok arm");
        let write = after
            .find("write_mask_payload(true")
            .expect("the mask write must exist");
        assert!(err_arm < ok_arm, "Err must be handled before Ok");
        assert!(
            write > ok_arm,
            "the mask write must live in the Ok arm: a refused begin_mask must never mask"
        );
    }


    #[test]
    fn a_late_reapply_refuses_when_no_originals_are_left() {
        assert!(should_reapply(true, true));
        assert!(
            !should_reapply(true, false),
            "no originals -> nothing could ever restore it, so do not mask"
        );
        assert!(!should_reapply(false, true), "not transitioning -> do not mask");
    }

    #[test]
    fn the_builder_refuses_anything_it_cannot_render_as_valid_lua() {
        fn build(entries: &[(&str, &str)]) -> Option<String> {
            let v: Vec<(String, String)> = entries
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect();
            lua_config_payload(&v)
        }
        assert!(build(&[("a.b", "1")]).is_some());
        assert!(build(&[]).is_none(), "empty set");
        assert!(build(&[("a.b.", "1")]).is_none(), "trailing dot");
        assert!(build(&[(".a", "1")]).is_none(), "leading dot");
        assert!(build(&[("a b", "1")]).is_none(), "not an identifier");
        assert!(build(&[("a", "")]).is_none(), "empty literal");
        assert!(build(&[("a", "1abc")]).is_none(), "not a literal");
        assert!(build(&[("a", "-3")]).is_some(), "negative integers are fine");
    }

    #[test]
    fn one_unusable_original_refuses_the_whole_restore() {
        let mut originals = std::collections::BTreeMap::new();
        originals.insert("blur", "[EMPTY]".to_string());
        assert_eq!(theme_mask_payload(false, &originals), None);
        let failures = write_mask_payload(false, &originals, |_lua| Ok(()));
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("no usable mask payload"));
    }

    #[test]
    fn the_marker_survives_a_round_trip_and_ignores_unknown_keys() {
        let text = r#"{"blur":"1","border":"2","shadow":"true","future_option":"9"}"#;
        let map = marker_from_json(text).expect("a usable marker");
        assert_eq!(map.get("blur").map(String::as_str), Some("1"));
        assert_eq!(map.len(), 1, "only masked keys are kept; unknown/legacy keys are ignored");
        assert!(marker_from_json("not json").is_none());
        assert!(marker_from_json(r#"{"unrelated":"1"}"#).is_none());
    }

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

    // ── Border gradient slot add/remove (U3.3) ──
    // The pane's +/- buttons were declared through all four Slint layers but
    // had no Rust handler, so they were dead. These pin the clamping rules.
    #[test]
    fn border_slot_add_clamps_at_the_hyprland_maximum() {
        let seven: Vec<String> = (0..7).map(|i| format!("p:s{}", i)).collect();
        let eight = slots_with_added(&seven).expect("7 -> 8 is allowed");
        assert_eq!(eight.len(), 8);
        assert!(slots_with_added(&eight).is_none(), "8 is the maximum");
    }

    #[test]
    fn border_slot_add_appends_a_distinct_default() {
        let two = vec!["p:primary".to_string(), "p:secondary".to_string()];
        let three = slots_with_added(&two).expect("2 -> 3 is allowed");
        assert_eq!(three.len(), 3);
        assert_eq!(three[..2], two[..], "existing slots are preserved");
        assert!(three[2].starts_with("p:"), "new slot is a palette token");
        assert_ne!(three[2], three[1], "new slot differs from the previous one");
    }

    #[test]
    fn border_slot_remove_clamps_at_the_hyprland_minimum() {
        let three: Vec<String> = (0..3).map(|i| format!("p:s{}", i)).collect();
        let two = slots_with_removed(&three).expect("3 -> 2 is allowed");
        assert_eq!(two, three[..2].to_vec(), "the LAST slot is the one removed");
        assert!(slots_with_removed(&two).is_none(), "2 is the minimum");
    }

    #[test]
    fn border_slot_add_then_remove_round_trips() {
        let start: Vec<String> = (0..4).map(|i| format!("p:s{}", i)).collect();
        let grown = slots_with_added(&start).expect("4 -> 5 is allowed");
        let back = slots_with_removed(&grown).expect("5 -> 4 is allowed");
        assert_eq!(back, start);
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

    // ── Entry seed must not run on a re-select of the active section ─────
    // Clicking the Borders rail item while Borders is already open re-emits
    // `section-selected` (PanelMenu always fires on click). Re-seeding there
    // would clear the pane's dirty marker and show persisted values while the
    // user's live draft fragment is still assembled — a silent draft wipe. Only
    // a real entry (panel opening onto Borders, or switching from another
    // section) seeds.
    #[test]
    fn borders_entry_seed_skips_a_reselect_of_the_active_section() {
        use crate::shell::nav::PanelSection;
        assert!(
            super::borders_entry_should_seed(true, Some(PanelSection::Borders)),
            "opening the panel onto Borders is an entry, even from a previous Borders"
        );
        assert!(
            super::borders_entry_should_seed(false, Some(PanelSection::Save)),
            "switching to Borders from another section is an entry"
        );
        assert!(
            super::borders_entry_should_seed(false, None),
            "no previous section means the panel is being entered"
        );
        assert!(
            !super::borders_entry_should_seed(false, Some(PanelSection::Borders)),
            "re-clicking the active Borders section must not wipe the live draft"
        );
    }

    // ── B5/B7: the deferred persist must not be dropped on shutdown ──────
    // `arm_geometry_persist` only writes when its 400 ms idle timer fires. A
    // quit inside that window (window close, IPC, tray quit) would drop the
    // whole drag: the config and the fragment keep the previous value and the
    // next `hyprctl reload` reverts what the user just saw. The flush closes
    // that window by running the same drain on the way out, before the process
    // leaves.
    //
    // Two different things are protected below and they must not be confused:
    //   • `flush_on_shutdown_writes_the_last_geometry` pins the DRAIN's
    //     semantics (the last value wins, nothing owed is a no-op).
    //   • `shutdown_flushes_the_deferred_geometry_before_teardown` pins the
    //     WIRING: that `main` actually calls the flush on the shutdown path.
    // B5 shipped a test that called the drain directly and called it a day;
    // that is why B7 replaces the missing half, not the drain coverage.

    /// Temp project whose `geometry.sh` records its four arguments instead of
    /// writing a fragment and firing a reload. No Hyprland, no `hyprctl`.
    fn temp_geometry_engine() -> (crate::engine::Engine, tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let scripts = dir.path().join("assets").join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        let record = dir.path().join("geometry-record.txt");
        let script = format!(
            "#!/bin/bash\necho \"$1 $2 $3 $4\" > \"{}\"\nexit 0\n",
            record.display()
        );
        let path = scripts.join("geometry.sh");
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&path, perm).unwrap();
        }
        let engine = crate::engine::Engine::new(dir.path());
        (engine, dir, record)
    }

    /// Drain semantics: the shutdown flush must persist the drag even when the
    /// idle timer never fired, and a flush with nothing owed must be a no-op.
    /// This exercises `flush_geometry_persist` directly, so it proves the
    /// DRAIN is correct — not that `main` runs it. The wiring is pinned by
    /// `shutdown_flushes_the_deferred_geometry_before_teardown` below (B7).
    #[test]
    fn flush_on_shutdown_writes_the_last_geometry() {
        let _env = crate::test_utils::TempEnv::new();
        let (engine, proj, record) = temp_geometry_engine();

        let mut cfg = crate::config::Config::default();
        cfg.border_size = 2;
        cfg.border_radius = 32;
        cfg.gaps_in = 5;
        cfg.gaps_out = 5;
        let tm = crate::theme_manager::ThemeManager::new(proj.path());
        let state = Arc::new(std::sync::Mutex::new(AppState::new(cfg, engine, tm)));

        // The hot path already moved the in-memory config and staged the
        // durable write; the 400 ms timer has NOT fired (the process exits now).
        {
            let mut st = state.lock().unwrap();
            st.cfg_mut().border_size = 5;
            st.cfg_mut().border_radius = 44;
            st.cfg_mut().gaps_in = 8;
            st.cfg_mut().gaps_out = 10;
        }
        let pending = std::rc::Rc::new(std::cell::RefCell::new(
            crate::callbacks::GeometryPersistCoalescer::default(),
        ));
        pending.borrow_mut().request(crate::callbacks::BorderGeometry {
            size: 5,
            radius: 44,
            gap_in: 8,
            gap_out: 10,
        });
        let timer = slint::Timer::default();

        // Shutdown: no timer fire, just the flush.
        flush_geometry_persist(&timer, &pending, &state);

        // The durable path ran once, with the drag's LAST value.
        assert_eq!(
            std::fs::read_to_string(&record).unwrap().trim(),
            "5 44 8 10",
            "the shutdown flush must run geometry.sh with the last value"
        );
        // The persisted config carries the last value too.
        let on_disk = crate::config::Config::load();
        assert_eq!(on_disk.border_size, 5);
        assert_eq!(on_disk.border_radius, 44);
        assert_eq!(on_disk.gaps_in, 8);
        assert_eq!(on_disk.gaps_out, 10);
        assert!(!pending.borrow().has_pending(), "the flush consumed the owe");

        // A second flush owes nothing and must not run the durable path again.
        std::fs::remove_file(&record).unwrap();
        flush_geometry_persist(&timer, &pending, &state);
        assert!(!record.exists(), "a flush with nothing owed must not write");
    }

    // ── B7: the shutdown flush must be WIRED, not just available ──────────
    // The test above calls `flush_geometry_persist` directly, so deleting the
    // production call in `main` leaves it green. That is a false safety net:
    // it cannot catch the regression it exists to catch. This test parses
    // `src/main.rs` with a real Rust AST (`syn`) and asserts the call sits at
    // the top level of `main`, AFTER `run_event_loop_until_quit()` and BEFORE
    // `ipc::cleanup()` (teardown). It catches the three plausible regressions:
    // the call removed, reordered before the loop, or nested into a branch
    // that may never run.
    //
    // Why this shape and not an end-to-end binary run: spawning
    // `env!("CARGO_BIN_EXE_hve")` needs a live Wayland/XRandR session and a
    // real compositor to reach `run_event_loop_until_quit`, and the suite must
    // never touch the keeper's desktop (B6). The AST check is the strongest
    // hermetic option that is still a real test of the wiring; the live binary
    // run complementary to it is the keeper's own manual test.

    /// Parse `src/main.rs` and return `main`'s body block.
    fn main_fn_body() -> syn::Block {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs"))
            .expect("read src/main.rs");
        let file: syn::File = syn::parse_file(&src).expect("parse src/main.rs");
        for item in file.items {
            if let syn::Item::Fn(func) = item {
                if func.sig.ident == "main" {
                    return *func.block;
                }
            }
        }
        panic!("main() not found in src/main.rs");
    }

    /// The last path segment of a plain function-call statement, if the
    /// statement is exactly `some::path(...);`, with any trailing `?` peeled
    /// off (`slint::run_event_loop_until_quit()?` is a `Try`, not a bare call).
    /// Anything that is not a plain call (a `let`, a macro, a nested block)
    /// yields `None`.
    fn stmt_call_name(stmt: &syn::Stmt) -> Option<String> {
        fn peel(expr: &syn::Expr) -> Option<&syn::ExprCall> {
            match expr {
                syn::Expr::Call(call) => Some(call),
                syn::Expr::Try(try_expr) => peel(&try_expr.expr),
                _ => None,
            }
        }
        let syn::Stmt::Expr(expr, _) = stmt else {
            return None;
        };
        let call = peel(expr)?;
        let syn::Expr::Path(path) = &*call.func else {
            return None;
        };
        path.path
            .segments
            .last()
            .map(|seg| seg.ident.to_string())
    }

    /// Index of the first top-level statement that is a plain call to `callee`.
    fn call_index(stmts: &[syn::Stmt], callee: &str) -> Option<usize> {
        stmts
            .iter()
            .position(|stmt| stmt_call_name(stmt).as_deref() == Some(callee))
    }

    /// True when `expr` contains a call whose last path segment is `name`,
    /// at any depth. Used to prove a statement does NOT smuggle the flush into
    /// a branch or loop.
    fn expr_mentions_call(expr: &syn::Expr, name: &str) -> bool {
        use syn::visit::Visit;
        struct Finder<'a> {
            name: &'a str,
            found: bool,
        }
        impl<'a, 'ast> Visit<'ast> for Finder<'a> {
            fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
                if let syn::Expr::Path(path) = &*call.func {
                    if path
                        .path
                        .segments
                        .last()
                        .is_some_and(|seg| seg.ident == self.name)
                    {
                        self.found = true;
                    }
                }
                syn::visit::visit_expr_call(self, call);
            }
        }
        let mut finder = Finder { name, found: false };
        finder.visit_expr(expr);
        finder.found
    }

    /// True when a statement contains a call to `name` ANYWHERE (the statement
    /// itself, or nested inside blocks, branches, loops or closures).
    fn stmt_mentions_call(stmt: &syn::Stmt, name: &str) -> bool {
        match stmt {
            syn::Stmt::Expr(expr, _) => expr_mentions_call(expr, name),
            syn::Stmt::Local(local) => local
                .init
                .as_ref()
                .is_some_and(|init| expr_mentions_call(&init.expr, name)),
            _ => false,
        }
    }

    #[test]
    fn shutdown_flushes_the_deferred_geometry_before_teardown() {
        const FLUSH: &str = "flush_geometry_persist";
        let body = main_fn_body();

        // 1. main calls the flush exactly once, as a plain top-level call.
        let flush_calls: Vec<&syn::Stmt> = body
            .stmts
            .iter()
            .filter(|stmt| stmt_mentions_call(stmt, FLUSH))
            .collect();
        assert_eq!(
            flush_calls.len(),
            1,
            "main must call flush_geometry_persist exactly once"
        );
        let flush = call_index(&body.stmts, FLUSH)
            .expect("the single flush mention must be a plain top-level call, \
                     never nested in a branch or loop");

        // 2. It runs after the event loop returns and before teardown.
        let quit = call_index(&body.stmts, "run_event_loop_until_quit")
            .expect("main must call run_event_loop_until_quit");
        let cleanup = call_index(&body.stmts, "cleanup")
            .expect("main must call ipc::cleanup during teardown");
        assert!(
            flush > quit,
            "the geometry flush must run after run_event_loop_until_quit returns \
             (flush at statement {flush}, quit at {quit})"
        );
        assert!(
            flush < cleanup,
            "the geometry flush must run before ipc::cleanup (flush at statement \
             {flush}, cleanup at {cleanup})"
        );
    }

    /// B4 — the Borders panel i18n must be applied by the REAL startup path.
    ///
    /// `borders_panel_labels_follow_the_language` proves `panel_i18n::apply_borders`
    /// translates the whole panel, but a test that only calls the function
    /// itself would keep passing if `main()` stopped calling it — the exact
    /// false safety net B7 removed for the geometry flush. So this pins the
    /// call site: `main` calls it exactly once, as a plain top-level statement,
    /// before the event loop starts.
    #[test]
    fn main_applies_the_borders_i18n_before_the_event_loop() {
        const APPLY: &str = "apply_borders";
        let body = main_fn_body();

        let calls: Vec<&syn::Stmt> = body
            .stmts
            .iter()
            .filter(|stmt| stmt_mentions_call(stmt, APPLY))
            .collect();
        assert_eq!(
            calls.len(),
            1,
            "main must call panel_i18n::apply_borders exactly once"
        );
        let apply = call_index(&body.stmts, APPLY)
            .expect("the single apply_borders mention must be a plain top-level call, \
                     never nested in a branch or loop");

        // The panel is built from this global before the window is shown, so
        // the first frame is already translated.
        let quit = call_index(&body.stmts, "run_event_loop_until_quit");
        if let Some(quit) = quit {
            assert!(
                apply < quit,
                "the Borders i18n must be applied before the event loop \
                 (apply at statement {apply}, quit at {quit})"
            );
        }
    }

    /// Panel chrome (nav rail + header hint) — the shared `PanelText` global
    /// must be applied by the REAL startup path, the B7 lesson: a test that only
    /// calls `apply_panel_chrome` itself keeps passing if `main()` stops calling
    /// it. So this pins the call site the same way the Borders pass is pinned.
    #[test]
    fn main_applies_the_panel_chrome_i18n_before_the_event_loop() {
        const APPLY: &str = "apply_panel_chrome";
        let body = main_fn_body();

        let calls: Vec<&syn::Stmt> = body
            .stmts
            .iter()
            .filter(|stmt| stmt_mentions_call(stmt, APPLY))
            .collect();
        assert_eq!(
            calls.len(),
            1,
            "main must call panel_i18n::apply_panel_chrome exactly once"
        );
        let apply = call_index(&body.stmts, APPLY)
            .expect("the single apply_panel_chrome mention must be a plain top-level call, \
                     never nested in a branch or loop");

        let quit = call_index(&body.stmts, "run_event_loop_until_quit");
        if let Some(quit) = quit {
            assert!(
                apply < quit,
                "the panel chrome i18n must be applied before the event loop \
                 (apply at statement {apply}, quit at {quit})"
            );
        }
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

    // ── W2: startup crash repair for the colour-authority yield ───────
    // The keeper's design requires that a crash between the yield flip and
    // the restore is repaired on the NEXT start: main() must invoke the
    // repair once at startup, after logging is up. Comment lines are
    // stripped so a commented-out call cannot satisfy this.
    #[test]
    fn startup_repairs_a_crashed_colour_authority_yield() {
        let src = std::fs::read_to_string("src/main.rs").expect("src/main.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let repair = ["startup_recover_crashed_yield", "()"].concat();
        assert!(
            code.contains(&repair),
            "main() must run the startup crash repair for the yield"
        );
        let call_at = code.find(&repair).expect("call must exist");
        let lock_at = code
            .find("Arc::new(std::sync::Mutex::new(Some(lock_file)))")
            .expect("the single-instance lock must still exist");
        assert!(
            call_at > lock_at,
            "the repair must run only after the owning instance holds the lock"
        );
    }

    // ── W3: immersive fullscreen survives a config reload ──────────────
    // Every `hyprctl reload` resets the runtime keyword overrides AND
    // re-floats HVE, dropping its immersive fullscreen. The reload path
    // must restore it: pure decision (reload_fs_action) + thin impure
    // shell, debounced with the shared reassert window, and never while a
    // theme transition is in flight (the finale owns that cycle).

    /// (a) reload + immersive + visible + not fullscreen + no transition +
    /// no recent cycle → run the unset→set cycle.
    #[test]
    fn reload_fs_restore_cycles_when_reload_took_fullscreen_away() {
        use std::time::Duration;
        assert_eq!(
            reload_fs_action(false, true, true, false, None),
            ReloadFsAction::Cycle,
            "a reload wiped fullscreen while HVE is in the immersive presentation: restore it"
        );
        // Never cycled (None) is not "inside the debounce window".
        assert_eq!(
            reload_fs_action(false, true, true, false, Some(Duration::from_millis(REASSERT_DEBOUNCE_MS))),
            ReloadFsAction::Cycle,
            "a cycle exactly at the debounce boundary is allowed"
        );
    }

    /// (b) Each blocking reason alone → do nothing.
    #[test]
    fn reload_fs_restore_does_nothing_for_each_blocking_reason() {
        use std::time::Duration;
        // A theme transition is in flight — the finale owns the cycle.
        assert_eq!(
            reload_fs_action(true, true, true, false, None),
            ReloadFsAction::Nothing(ReloadFsSkip::ThemeTransitionInFlight)
        );
        // HVE parked/hidden — never fire while hidden.
        assert_eq!(
            reload_fs_action(false, false, true, false, None),
            ReloadFsAction::Nothing(ReloadFsSkip::HveHidden)
        );
        // HVE is not inside the immersive presentation (collapsed Home, or
        // the settings panel floats — the window is the live border preview
        // there, fullscreen would destroy it). The brief did not settle
        // this gate; the app's own presentation code (shell/mod.rs float
        // path, run_settle) demands it.
        assert_eq!(
            reload_fs_action(false, true, false, false, None),
            ReloadFsAction::Nothing(ReloadFsSkip::NotImmersive)
        );
        // Already immersive fullscreen — nothing to restore.
        assert_eq!(
            reload_fs_action(false, true, true, true, None),
            ReloadFsAction::Nothing(ReloadFsSkip::AlreadyFullscreen)
        );
        // A cycle ran inside the debounce window — one visible cycle max
        // per 1.5s (the shared reassert contract).
        assert_eq!(
            reload_fs_action(false, true, true, false, Some(Duration::from_millis(REASSERT_DEBOUNCE_MS - 1))),
            ReloadFsAction::Nothing(ReloadFsSkip::Debounced)
        );
    }

    /// (c) The reload path must run the cycle EXACTLY when the pure
    /// decision says Cycle. A runner invoked on any refusal (or not
    /// invoked on Cycle) fails this test.
    #[test]
    fn reload_restore_runs_the_cycle_exactly_when_the_decision_says_cycle() {
        use std::time::Duration;

        let mut cycles = 0;
        let action = apply_reload_fs_action(
            reload_fs_action(false, true, true, false, None),
            || cycles += 1,
        );
        assert_eq!(action, ReloadFsAction::Cycle);
        assert_eq!(cycles, 1, "Cycle must run the shared cycle runner");

        let mut cycles = 0;
        let hidden = apply_reload_fs_action(
            reload_fs_action(false, false, true, false, None),
            || cycles += 1,
        );
        assert!(matches!(hidden, ReloadFsAction::Nothing(ReloadFsSkip::HveHidden)));
        assert_eq!(cycles, 0, "firing while HVE is hidden is the parked-window bug");

        let mut cycles = 0;
        let not_immersive = apply_reload_fs_action(
            reload_fs_action(false, true, false, false, None),
            || cycles += 1,
        );
        assert!(matches!(
            not_immersive,
            ReloadFsAction::Nothing(ReloadFsSkip::NotImmersive)
        ));
        assert_eq!(cycles, 0, "the floating panel / collapsed Home must never be fullscreened");

        let mut cycles = 0;
        let transitioning = apply_reload_fs_action(
            reload_fs_action(true, true, true, false, None),
            || cycles += 1,
        );
        assert!(matches!(
            transitioning,
            ReloadFsAction::Nothing(ReloadFsSkip::ThemeTransitionInFlight)
        ));
        assert_eq!(cycles, 0, "the finale owns the cycle inside a theme transition");

        let mut cycles = 0;
        let fullscreen = apply_reload_fs_action(
            reload_fs_action(false, true, true, true, None),
            || cycles += 1,
        );
        assert!(matches!(fullscreen, ReloadFsAction::Nothing(ReloadFsSkip::AlreadyFullscreen)));
        assert_eq!(cycles, 0, "already fullscreen needs no cycle");

        let mut cycles = 0;
        let debounced = apply_reload_fs_action(
            reload_fs_action(false, true, true, false, Some(Duration::from_millis(100))),
            || cycles += 1,
        );
        assert!(matches!(debounced, ReloadFsAction::Nothing(ReloadFsSkip::Debounced)));
        assert_eq!(cycles, 0, "a cycle inside the debounce window must not stack");
    }

    /// (d) The existing transition finale ownership is unchanged: no cycle
    /// can be fired while a theme transition is in flight, whatever else
    /// the state says.
    #[test]
    fn transition_finale_ownership_unchanged_no_reload_cycle_in_flight() {
        let mut cycles = 0;
        let action = apply_reload_fs_action(
            reload_fs_action(true, true, true, false, None),
            || cycles += 1,
        );
        assert!(matches!(
            action,
            ReloadFsAction::Nothing(ReloadFsSkip::ThemeTransitionInFlight)
        ));
        assert_eq!(cycles, 0, "no reload restore may fire while the finale owns the cycle");
        // The gallery finale keeps its generation single-fire claim: the
        // transition branch of reassert_gallery_fullscreen must still be
        // present and consult the generation (comment-stripped source pin).
        let src = std::fs::read_to_string("src/main.rs").expect("src/main.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let claim = "THEME_CYCLE_FIRED_GEN";
        assert!(code.contains(claim), "the finale single-fire claim must survive");
        assert!(
            code.contains("fired.is_some_and(|g| g == gen)"),
            "the finale single-fire check must survive"
        );
    }

    /// The reload restore reuses the gallery reassert's cycle mechanics:
    /// with HVE Visible and fullscreen dropped by the reload, the shared
    /// cycle dispatches set_fullscreen(false) first and set_fullscreen(true)
    /// after the settle gap — nothing else.
    #[test]
    fn reload_restore_shared_cycle_dispatches_unset_then_set_on_the_composer() {
        i_slint_backend_testing::init_no_event_loop();
        let win = crate::MainWindow::new().unwrap();
        win.show().unwrap();

        let (fake, calls) = composer::tests::FakeComposer::new();
        let mut ctrl = composer::Controller::new(Box::new(fake));
        // "HVE is supposed to be showing": the ShowStateMachine must say
        // Visible (Hidden/Entering never restore).
        assert!(ctrl.begin_show_machine());
        assert!(ctrl.confirm_focus_machine());
        assert_eq!(ctrl.show_state(), crate::show_state::ShowState::Visible);
        composer::init_global(ctrl);

        // Prime the shared debounce to the past so the claim is granted.
        *LAST_REASSERT.lock().unwrap() =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(10));

        let action = apply_reload_fs_action(
            reload_fs_action(false, true, true, false, None),
            run_reload_cycle,
        );
        assert_eq!(action, ReloadFsAction::Cycle);

        {
            let recorded = calls.lock().unwrap().clone();
            assert!(
                recorded.iter().any(|c| c == "set_fullscreen(false)"),
                "step 1 must drop fullscreen to force a real transition, got: {:?}",
                recorded
            );
        }

        // Advance the mock clock past the 150ms settle gap: step 2 must
        // re-enter fullscreen (gate passes: still visible, no transition).
        for _ in 0..20 {
            i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
        }
        {
            let recorded = calls.lock().unwrap().clone();
            assert!(
                recorded.iter().any(|c| c == "set_fullscreen(true)"),
                "step 2 must re-enter fullscreen once the compositor settles, got: {:?}",
                recorded
            );
        }
        let _ = &win; // platform guard
    }

    /// (c) The reload path wiring: the configreloaded handler in
    /// hypr_ipc.rs must consult the fullscreen restore on the line path.
    /// Comment-stripped read so a commented-out call cannot satisfy it
    /// (house precedent: startup_repairs_a_crashed_colour_authority_yield).
    #[test]
    fn configreloaded_path_consults_the_fullscreen_restore() {
        let src = std::fs::read_to_string("src/hypr_ipc.rs").expect("src/hypr_ipc.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            code.contains("reassert_fullscreen_after_reload"),
            "the configreloaded line path must consult the fullscreen restore"
        );
        // The restore must run on EVERY line, before the 3s throttle: it is
        // wired through the line-level handler, not the throttled callback.
        assert!(
            code.contains("handle_configreloaded_line"),
            "the restore must live in the line-level (always-run) handler"
        );
    }

    // ── W3 correction (cross-model FAIL, re-verified): a Debounced refusal
    // must not be terminal. Reload A drops fullscreen → the restore cycles
    // and stamps LAST_REASSERT. Reload B re-floats HVE 1.4s later (burst) →
    // `Debounced` → refused. If B is the LAST reload of the burst, HVE
    // stays windowed below the bar FOREVER. Fix: one delayed re-consult
    // (single-shot timer) after the debounce window re-runs the FULL
    // decision against the live state; bounded — the tick never re-arms,
    // and one pending re-consult per refused decision (no second timer).

    /// BLOCKING CASE: a reload inside the debounce window that is the last
    /// of the burst must still end with HVE immersive — the delayed
    /// re-consult runs the cycle once the window expires.
    #[test]
    fn debounced_last_reload_of_a_burst_still_ends_immersive_via_delayed_reconsult() {
        i_slint_backend_testing::init_no_event_loop();
        let win = crate::MainWindow::new().unwrap();
        win.show().unwrap();

        // Reload A already cycled: the shared debounce stamp is 1.4s old,
        // so a reload B arriving NOW is refused (inside the 1.5s window).
        *LAST_REASSERT.lock().unwrap() =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(1400));
        *PENDING_RECONSULT_AT.lock().unwrap() = None;

        // The re-consult tick (the real one re-gathers live state; here the
        // decision inputs are injected headlessly): once the window has
        // expired, HVE is STILL supposed to be immersive and STILL
        // re-floated (last reload of the burst) → Cycle → cycle runs.
        let cycles = std::sync::Arc::new(std::sync::Mutex::new(0u32));
        {
            let cycles = std::sync::Arc::clone(&cycles);
            let armed = arm_reload_reconsult(move || {
                let action = reload_fs_action(
                    false, // no transition in flight
                    true,  // HVE still Visible
                    true,  // still immersive (gallery session, panel docked)
                    false, // STILL windowed: reload B re-floated it
                    Some(std::time::Duration::from_millis(REASSERT_DEBOUNCE_MS + 100)), // window expired
                );
                assert_eq!(
                    action,
                    ReloadFsAction::Cycle,
                    "once the debounce window expired, the decision must cycle"
                );
                apply_reload_fs_action(action, move || *cycles.lock().unwrap() += 1);
            });
            assert!(
                armed,
                "a Debounced refusal must arm the single delayed re-consult"
            );
        }

        // Nothing runs before the window expires…
        assert_eq!(*cycles.lock().unwrap(), 0, "no cycle before the window expires");

        // …and once the mock clock passes the remaining window (1.4s old
        // stamp → ~100ms left), the single-shot fires exactly once and the
        // cycle runs: the last reload of the burst ends immersive.
        for _ in 0..20 {
            i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
        }
        assert_eq!(
            *cycles.lock().unwrap(),
            1,
            "the delayed re-consult must run the cycle once the window expires — \
             the last reload of a burst must not leave HVE stuck windowed below the bar"
        );
        let _ = &win; // platform guard
    }

    /// The re-consult finds HVE ALREADY fullscreen → do nothing (there is
    /// nothing to restore; the decision says so).
    #[test]
    fn delayed_reconsult_finds_hve_fullscreen_and_does_nothing() {
        i_slint_backend_testing::init_no_event_loop();
        let win = crate::MainWindow::new().unwrap();
        win.show().unwrap();

        *LAST_REASSERT.lock().unwrap() =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(1400));
        *PENDING_RECONSULT_AT.lock().unwrap() = None;

        let cycles = std::sync::Arc::new(std::sync::Mutex::new(0u32));
        {
            let cycles = std::sync::Arc::clone(&cycles);
            let armed = arm_reload_reconsult(move || {
                let action = reload_fs_action(
                    false,
                    true,
                    true,
                    true, // HVE already immersive fullscreen by re-consult time
                    Some(std::time::Duration::from_millis(REASSERT_DEBOUNCE_MS + 100)),
                );
                assert_eq!(
                    action,
                    ReloadFsAction::Nothing(ReloadFsSkip::AlreadyFullscreen),
                    "a re-consult that finds HVE fullscreen must refuse"
                );
                apply_reload_fs_action(action, move || *cycles.lock().unwrap() += 1);
            });
            assert!(armed);
        }

        for _ in 0..20 {
            i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
        }
        assert_eq!(
            *cycles.lock().unwrap(),
            0,
            "already fullscreen needs no cycle — the re-consult must do nothing"
        );
        let _ = &win;
    }

    /// The re-consult finds HVE parked/hidden → do nothing (never fire
    /// while hidden — a re-consult is no exception).
    #[test]
    fn delayed_reconsult_finds_hve_hidden_and_does_nothing() {
        i_slint_backend_testing::init_no_event_loop();
        let win = crate::MainWindow::new().unwrap();
        win.show().unwrap();

        *LAST_REASSERT.lock().unwrap() =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(1400));
        *PENDING_RECONSULT_AT.lock().unwrap() = None;

        let cycles = std::sync::Arc::new(std::sync::Mutex::new(0u32));
        {
            let cycles = std::sync::Arc::clone(&cycles);
            let armed = arm_reload_reconsult(move || {
                let action = reload_fs_action(
                    false,
                    false, // HVE parked in the scratchpad by re-consult time
                    true,
                    false,
                    Some(std::time::Duration::from_millis(REASSERT_DEBOUNCE_MS + 100)),
                );
                assert_eq!(
                    action,
                    ReloadFsAction::Nothing(ReloadFsSkip::HveHidden),
                    "a re-consult that finds HVE hidden must refuse"
                );
                apply_reload_fs_action(action, move || *cycles.lock().unwrap() += 1);
            });
            assert!(armed);
        }

        for _ in 0..20 {
            i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
        }
        assert_eq!(
            *cycles.lock().unwrap(),
            0,
            "firing the cycle while HVE is parked/hidden is the parked-window bug — \
             a re-consult must not reintroduce it"
        );
        let _ = &win;
    }

    /// The retry does not stack: a second Debounced refusal while one
    /// re-consult is already pending does NOT create a second timer — the
    /// pending one re-checks the live state, which already includes the
    /// later re-float (single owner, debounced, no stacked cycles).
    #[test]
    fn second_debounced_refusal_does_not_create_a_second_timer() {
        i_slint_backend_testing::init_no_event_loop();
        let win = crate::MainWindow::new().unwrap();
        win.show().unwrap();

        *LAST_REASSERT.lock().unwrap() =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(1400));
        *PENDING_RECONSULT_AT.lock().unwrap() = None;

        let ticks = std::sync::Arc::new(std::sync::Mutex::new(0u32));
        {
            let ticks_first = std::sync::Arc::clone(&ticks);
            let first = arm_reload_reconsult(move || *ticks_first.lock().unwrap() += 1);
            assert!(first, "the first refusal arms the re-consult");

            let ticks_second = std::sync::Arc::clone(&ticks);
            let second = arm_reload_reconsult(move || *ticks_second.lock().unwrap() += 1);
            assert!(
                !second,
                "a second refusal while one re-consult is pending must NOT create a second timer"
            );
        }

        // Advance well past the window: the tick must have run exactly ONCE.
        for _ in 0..100 {
            i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(16));
        }
        assert_eq!(
            *ticks.lock().unwrap(),
            1,
            "one delayed re-consult per refused decision — two refusals in one window \
             fire exactly one re-consult"
        );
        // The pending marker is cleared once the timer fires: a later burst
        // can arm again (bounded, not wedged).
        assert!(
            PENDING_RECONSULT_AT.lock().unwrap().is_none(),
            "the pending marker must clear once the re-consult fires"
        );
        let _ = &win;
    }

    /// Wiring + boundedness pins (comment-stripped, house precedent): the
    /// entry arms the single re-consult on a Debounced refusal; the tick
    /// itself never re-arms (one delayed re-consult per refused decision,
    /// never a self-perpetuating timer); the arm uses exactly one
    /// single-shot slint timer.
    #[test]
    fn reload_reconsult_wiring_and_boundedness_pins() {
        let src = std::fs::read_to_string("src/main.rs").expect("src/main.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            code.contains("arm_reload_reconsult"),
            "the Debounced refusal must arm the delayed re-consult"
        );

        let tick_start = code.find("fn reload_reconsult_tick").expect("re-consult tick must exist");
        let tick_end = code[tick_start..].find("\n}").expect("tick body must close") + tick_start;
        let tick_body = &code[tick_start..tick_end];
        assert!(
            !tick_body.contains("arm_reload_reconsult"),
            "the re-consult tick must never re-arm — bounded, no retry loop"
        );
        assert!(
            !tick_body.contains("Timer::"),
            "the tick must not schedule any timer — one re-consult per refused decision"
        );

        let arm_start = code.find("fn arm_reload_reconsult").expect("arm must exist");
        let arm_end = code[arm_start..].find("\n}").expect("arm body must close") + arm_start;
        let arm_body = &code[arm_start..arm_end];
        assert_eq!(
            arm_body.matches("Timer::single_shot").count(),
            1,
            "exactly ONE single-shot timer per refused decision"
        );
        assert!(
            code.contains("PENDING_RECONSULT_AT"),
            "the one-pending-re-consult guard must exist (no stacked timers)"
        );
    }

    // ── Moved query helpers: fallback behaviour pinned (capability-routing
    //    Phase 3). The raw `hyprctl` calls left `main.rs`; these pin that
    //    the observable contract — JSON path, selection order and fallback
    //    values — is byte-identical to the pre-move code. ─────────────

    /// `get_active_workspace_name`: a composer that cannot answer yields
    /// the same `"2"` the raw `activeworkspace -j` failure path produced.
    #[test]
    fn active_workspace_name_keeps_the_two_fallback() {
        assert_eq!(active_workspace_name(None), "2", "no answer must fall back to \"2\"");
        assert_eq!(
            active_workspace_name(Some("7".to_string())),
            "7",
            "a parsed workspace name passes through untouched"
        );
    }

    /// `find_empty_workspace`: same selection order (first NORMAL workspace
    /// with zero windows, skipping `special:*`, the excluded workspace and
    /// empty names) and same `"10"` / `"11"` fallbacks for absent or
    /// unparsable output.
    #[test]
    fn empty_workspace_from_keeps_selection_order_and_fallbacks() {
        let payload = r#"[
            {"name":"special:minimized","windows":0},
            {"name":"5","windows":2},
            {"name":"6","windows":0},
            {"name":"7","windows":0}
        ]"#;
        assert_eq!(
            empty_workspace_from(Some(payload), "6"),
            "7",
            "special is skipped, busy workspaces are skipped, the excluded one too"
        );
        assert_eq!(
            empty_workspace_from(Some(payload), "7"),
            "6",
            "selection keeps the original ordering: first eligible workspace wins"
        );
        assert_eq!(
            empty_workspace_from(Some(payload), "3"),
            "6",
            "without an exclusion the first eligible workspace wins"
        );
        assert_eq!(empty_workspace_from(None, "3"), "10", "absent output falls back to \"10\"");
        assert_eq!(
            empty_workspace_from(None, "10"),
            "11",
            "excluded \"10\" falls back to \"11\""
        );
        assert_eq!(empty_workspace_from(Some("not json"), "3"), "10", "garbage output falls back");
        assert_eq!(
            empty_workspace_from(Some(r#"[{"windows":0}]"#), "3"),
            "10",
            "an empty workspace NAME must never be selected"
        );
    }

    /// `hypr_getoption_int`: same parse order (`int` first, then a numeric
    /// or boolean `str`, trimmed) and the caller's fallback otherwise.
    #[test]
    fn option_int_from_keeps_the_pre_move_parse() {
        assert_eq!(option_int_from(None, "1"), "1", "no answer must yield the fallback");
        assert_eq!(option_int_from(Some("not json"), "1"), "1", "garbage must yield the fallback");
        assert_eq!(option_int_from(Some(r#"[3]"#), "1"), "1", "a non-object answer yields the fallback");
        assert_eq!(option_int_from(Some(r#"{"int":-4}"#), "1"), "-4", "int wins");
        assert_eq!(
            option_int_from(Some(r#"{"int":3,"str":"true"}"#), "1"),
            "3",
            "`int` is read before `str`"
        );
        assert_eq!(option_int_from(Some(r#"{"str":"12"}"#), "1"), "12", "numeric str passes");
        assert_eq!(option_int_from(Some(r#"{"str":" 12 "}"#), "1"), "12", "str is trimmed");
        assert_eq!(
            option_int_from(Some(r#"{"str":"true"}"#), "1"),
            "true",
            "boolean str passes"
        );
        assert_eq!(
            option_int_from(Some(r#"{"str":"false"}"#), "1"),
            "false",
            "boolean str passes"
        );
        assert_eq!(
            option_int_from(Some(r#"{"str":"[EMPTY]"}"#), "1"),
            "1",
            "a non-numeric str yields the fallback"
        );
    }

    // ── Command verbs moved behind Composer (capability-routing Phase 3) ─
    // House-style wiring pins (comment-stripped, same precedent as the
    // begin_mask arm pin): the raw `hyprctl` spawns leave the core file,
    // while the seam shape (`global_controller()` with a detached fallback
    // for the pre-`init_global` paths) and the exact `Fn(&str) ->
    // Result<(), String>` runner signature must stay.

    /// Body of one `fn <sig>`: from the signature to its matching closing
    /// brace (brace counting — every pinned body keeps its braces balanced,
    /// including the `{:?}` format placeholders).
    fn fn_body(src: &str, sig: &str) -> String {
        let start = src.find(sig).unwrap_or_else(|| panic!("{sig} must exist"));
        let open = src[start..].find('{').expect("the function must have a body") + start;
        let mut depth = 0usize;
        for (i, ch) in src[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return src[start..open + i + 1].to_string();
                    }
                }
                _ => {}
            }
        }
        panic!("{sig}: body must close");
    }

    /// Comment-stripped production body of `src/main.rs` for wiring pins.
    fn main_rs_code() -> String {
        let src = std::fs::read_to_string("src/main.rs").expect("src/main.rs must exist");
        src.lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn hypr_eval_reaches_the_composer_and_never_spawns_itself() {
        let code = main_rs_code();
        let body = fn_body(&code, "fn hypr_eval(lua: &str) -> Result<(), String>");
        assert!(
            body.contains("global_controller()"),
            "hypr_eval must ask the registered composer, got: {body}"
        );
        assert!(
            body.contains("eval_lua"),
            "hypr_eval must use the eval capability verb, got: {body}"
        );
        assert!(
            body.contains("eval_lua_detached"),
            "a pre-init_global caller (startup mask repair, early IPC line) must keep the OLD spawn via the detached fallback — never a silent no-op, got: {body}"
        );
        assert!(
            !body.contains("Command::new"),
            "hypr_eval must not spawn hyprctl itself, got: {body}"
        );
    }

    #[test]
    fn switch_view_and_targeted_reassert_route_through_the_composer() {
        let code = main_rs_code();
        for sig in [
            "fn hypr_switch_view(ws: &str) -> bool",
            "fn reassert_hve_fullscreen_targeted() -> bool",
        ] {
            let body = fn_body(&code, sig);
            assert!(
                body.contains("dispatch_script"),
                "{sig} must run its script through the composer's dispatch verb, got: {body}"
            );
            assert!(
                body.contains("dispatch_script_detached"),
                "{sig} must fall back to the OLD spawn before init_global — never a silent no-op, got: {body}"
            );
            assert!(
                !body.contains("Command::new"),
                "{sig} must not spawn hyprctl itself, got: {body}"
            );
        }
    }

    /// The load-bearing seam: `hypr_eval` is passed as a function VALUE into
    /// `write_mask_payload` / `restore_theme_masks_with` (both take
    /// `impl Fn(&str) -> Result<(), String>`), and the tests drive those with
    /// injected runners. The exact signature cannot change.
    #[test]
    fn hypr_eval_keeps_the_runner_signature_the_mask_seam_depends_on() {
        let code = main_rs_code();
        assert!(
            code.contains("fn hypr_eval(lua: &str) -> Result<(), String>"),
            "the Fn(&str) -> Result<(), String> runner signature is load-bearing"
        );
        for seam in [
            "write_mask_payload(false, &originals, hypr_eval)",
            "write_mask_payload(true, &originals, hypr_eval)",
            "restore_theme_masks_with(hypr_eval)",
        ] {
            assert!(code.contains(seam), "the seam must keep passing hypr_eval: {seam}");
        }
    }

}
