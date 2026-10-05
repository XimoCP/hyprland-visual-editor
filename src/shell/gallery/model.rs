// HVE — Gallery model (theme-gallery PR1 Foundation + PR4 Polish).
//
// Data layer for the theme gallery: ThemeCard + ThemeGalleryModel + LRU200
// thumbnail cache + reduced-motion + MIT footer. Engine sealed — reads
// ThemeManager::list() → ThemeInfo only.
//
// MIT credit: visual language translated from skwd-wall (MIT, © liixini).

use crate::shell::gallery::views::GalleryStyle;
use crate::theme_manager::ThemeInfo;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

/// Fallback accent color when a theme has no palette (spec R6).
pub const FALLBACK_ACCENT: &str = "#4fc3f7";

/// MIT credit footer (spec R7): persistent footer attribution to skwd-wall.
pub const MIT_FOOTER: &str =
    "Visual language translated from skwd-wall (MIT, \u{00A9} liixini) https://github.com/liixini/skwd-wall";
/// Link to skwd-wall MIT repo.
pub const MIT_FOOTER_LINK: &str = "https://github.com/liixini/skwd-wall";

/// Thumbnail cache capacity R8: bounded LRU 200 items.
pub const THUMBNAIL_CACHE_CAPACITY: usize = 200;

/// Whether the system prefers reduced motion (S23). Checks env var
/// `HVE_REDUCED_MOTION=1` or `PREFERS_REDUCED_MOTION`. Pure helper for
/// tests and the Chrome reduced-motion path.
pub fn prefers_reduced_motion() -> bool {
    std::env::var("HVE_REDUCED_MOTION").map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false)
        || std::env::var("PREFERS_REDUCED_MOTION").map(|v| v == "1").unwrap_or(false)
}

/// Duration 0 when reduced-motion is active, otherwise original duration.
/// S23: all animations disabled (duration 0) when prefers-reduced-motion.
pub fn effective_duration(original_ms: u64, reduced: bool) -> u64 {
    if reduced { 0 } else { original_ms }
}

/// Complete-look color scheme carried by each card (spec R1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryColors {
    pub primary: String,
    pub secondary: String,
    pub tertiary: String,
    pub accent: String,
    pub surface: String,
}

impl Default for GalleryColors {
    fn default() -> Self {
        Self {
            primary: FALLBACK_ACCENT.to_string(),
            secondary: "#ffb4ab".to_string(),
            tertiary: "#8bceff".to_string(),
            accent: FALLBACK_ACCENT.to_string(),
            surface: "#1d100e".to_string(),
        }
    }
}

/// Border config carried by each card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorderConfig {
    pub size: i32,
    pub radius: i32,
    pub color: String,
}

impl Default for BorderConfig {
    fn default() -> Self {
        Self {
            size: 2,
            radius: 32,
            color: FALLBACK_ACCENT.to_string(),
        }
    }
}

/// Background for a card: wallpaper file or solid color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Background {
    // Kept per design §1 (Background = Wallpaper(path) | Solid(color));
    // providers currently emit solid colors only.
    #[allow(dead_code)]
    Wallpaper(PathBuf),
    Solid(String),
}

impl Default for Background {
    fn default() -> Self {
        Self::Solid("#1d100e".to_string())
    }
}

/// One complete-look card in the gallery (spec R1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeCard {
    pub name: String,
    pub colors: GalleryColors,
    pub border: BorderConfig,
    pub background: Background,
    pub shader: Option<String>,
    pub saved_at: String,
    pub is_active: bool,
    pub providers: Vec<String>,
    pub thumb_path: Option<PathBuf>,
}

impl ThemeCard {
    /// Map a ThemeInfo (from ThemeManager::list) to a ThemeCard with defaults.
    /// Real palette/border/background/shader enrichment lands in PR2+ when
    /// provider state is read; PR1 keeps sealed-engine mapping pure.
    pub fn from_info(info: &ThemeInfo) -> Self {
        Self {
            name: info.name.clone(),
            colors: GalleryColors::default(),
            border: BorderConfig::default(),
            background: Background::default(),
            shader: None,
            saved_at: info.saved_at.clone(),
            is_active: info.is_active,
            providers: info.providers.clone(),
            thumb_path: None,
        }
    }

    /// Map a slice of ThemeInfo to ThemeCards (preserves order).
    pub fn from_infos(infos: &[ThemeInfo]) -> Vec<Self> {
        infos.iter().map(Self::from_info).collect()
    }
}

/// Gallery data model: owned theme cards + focused index + style.
///
/// Pure Rust, no Slint types; Engine sealed — only ThemeInfo is read.
/// Focus movement is clamped, no wrap (spec S13).
#[derive(Debug, Clone)]
pub struct ThemeGalleryModel {
    themes: Vec<ThemeCard>,
    focused_index: usize,
    // Style is owned by the UI layer (GalleryRoot current-style mirror);
    // kept here per design §1 and exercised via with_style in tests.
    #[cfg_attr(not(test), allow(dead_code))]
    style: GalleryStyle,
}

impl ThemeGalleryModel {
    /// Create a model from cards, focus at 0, style Slice.
    pub fn new(themes: Vec<ThemeCard>) -> Self {
        Self {
            themes,
            focused_index: 0,
            style: GalleryStyle::default(),
        }
    }

    /// Create with explicit style.
    /// Intentional design §1 API; style currently mirrors through the UI.
    #[allow(dead_code)]
    pub fn with_style(themes: Vec<ThemeCard>, style: GalleryStyle) -> Self {
        Self {
            themes,
            focused_index: 0,
            style,
        }
    }

    /// Map ThemeInfos directly to a model (focus 0, Slice).
    pub fn from_infos(infos: &[ThemeInfo]) -> Self {
        Self::new(ThemeCard::from_infos(infos))
    }

    /// Number of cards.
    pub fn len(&self) -> usize {
        self.themes.len()
    }

    // ── Headless-verified accessors: the UI mirrors focus/style via its own
    //    properties, so these design §1 accessors are exercised by tests. ──
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.themes.is_empty()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn focused_index(&self) -> usize {
        self.focused_index
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn focused_card(&self) -> Option<&ThemeCard> {
        self.themes.get(self.focused_index)
    }

    pub fn themes(&self) -> &[ThemeCard] {
        &self.themes
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn style(&self) -> GalleryStyle {
        self.style
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_style(&mut self, style: GalleryStyle) {
        self.style = style;
    }

    /// Move focus by delta, clamped to 0..len-1, no wrap (S13).
    /// Returns new focused index.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn move_focus(&mut self, delta: isize) -> usize {
        if self.themes.is_empty() {
            self.focused_index = 0;
            return 0;
        }
        let max = self.themes.len() as isize - 1;
        let next = self.focused_index as isize + delta;
        self.focused_index = next.clamp(0, max) as usize;
        self.focused_index
    }

    /// Set focused index directly, clamped.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_focused_index(&mut self, idx: usize) -> usize {
        if self.themes.is_empty() {
            self.focused_index = 0;
            return 0;
        }
        self.focused_index = idx.min(self.themes.len() - 1);
        self.focused_index
    }

    /// Refresh model from fresh ThemeInfos (e.g. after delete/rename).
    /// Preserves focus clamped.
    pub fn refresh(&mut self, infos: &[ThemeInfo]) {
        let new_themes = ThemeCard::from_infos(infos);
        let was_empty = self.themes.is_empty();
        self.themes = new_themes;
        if was_empty {
            self.focused_index = 0;
        } else if self.focused_index >= self.themes.len().max(1) {
            self.focused_index = self.themes.len().saturating_sub(1);
        }
    }
}

// ── Thumbnail LRU cache R8 (bounded 200) ──────────────────────────────

/// Simple LRU cache for thumbnails (R8). Bounded at
/// `THUMBNAIL_CACHE_CAPACITY` 200 entries. `get` promotes to most-recent,
/// `insert` evicts least-recent when full.
pub struct ThumbnailCache {
    capacity: usize,
    map: HashMap<String, PathBuf>,
    order: VecDeque<String>,
}

impl ThumbnailCache {
    pub fn new() -> Self {
        Self::with_capacity(THUMBNAIL_CACHE_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    // LRU inspection API (R8): exercised by tests; the runtime path only
    // inserts/evicts through GallerySlot.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// Insert or update; promotes to most-recent. Evicts LRU when over capacity.
    pub fn insert(&mut self, key: String, path: PathBuf) {
        if self.map.contains_key(&key) {
            self.order.retain(|k| k != &key);
        } else if self.map.len() >= self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, path);
    }

    /// Get path and promote to most-recent.
    /// Intentional LRU API (R8); runtime path inserts via prewarm only.
    #[allow(dead_code)]
    pub fn get(&mut self, key: &str) -> Option<PathBuf> {
        if self.map.contains_key(key) {
            self.order.retain(|k| k != key);
            self.order.push_back(key.to_string());
            self.map.get(key).cloned()
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

impl Default for ThumbnailCache {
    fn default() -> Self {
        Self::new()
    }
}

// ── Bake carry-over (S5 flicker fix) ────────────────────────────────────
// Gallery rebuilds (theme applied, list refresh) construct fresh cards
// with EMPTY images; the async thumb/slat pipeline refills them later, so
// the Slice wall blanks to bare borders and pops back "all at once" when
// the bakes land. Baked images are deterministic per theme, so they are
// carried over BY NAME: rebuilt cards reuse the previous model's thumb,
// hero and slat bakes; schedule_thumbs' already-marshaled guard then does
// zero work and the wall never flickers. Non-image fields (accent, flags)
// always come from the FRESH row.

pub fn carry_over_bakes(old_rows: &[crate::GalleryCardData], fresh: Vec<crate::GalleryCardData>) -> Vec<crate::GalleryCardData> {
    let mut out = fresh;
    for card in out.iter_mut() {
        if let Some(prev) = old_rows.iter().find(|o| o.name == card.name) {
            card.thumb = prev.thumb.clone();
            card.hero = prev.hero.clone();
            card.slat_image = prev.slat_image.clone();
            card.slat_expanded_image = prev.slat_expanded_image.clone();
        }
    }
    out
}

/// In-place diff of a persistent `VecModel<GalleryCardData>` to `new_rows` (R3.1/R3.2).
///
/// - Diff by `name` (theme identity).
/// - Removes stale rows by descending index (delegate safety).
/// - Updates ONLY changed rows via `set_row_data`; image fields are preserved
///   from the old row unless the old image is empty and the new row carries one
///   (absorbs `carry_over_bakes` semantics).
/// - Pushes appended rows; untouched delegates are never rewritten.
/// - Model identity (`&VecModel` pointer) is unchanged — caller keeps the same ModelRc.
pub fn sync_cards(model: &slint::VecModel<crate::GalleryCardData>, new_rows: Vec<crate::GalleryCardData>) {
    use slint::Model as _;

    // ── 1) Remove stale names (descending for delegate stability) ──
    let mut to_remove: Vec<usize> = Vec::new();
    for i in 0..model.row_count() {
        if let Some(old) = model.row_data(i) {
            if !new_rows.iter().any(|n| n.name == old.name) {
                to_remove.push(i);
            }
        }
    }
    to_remove.sort_unstable_by(|a, b| b.cmp(a));
    for idx in to_remove {
        model.remove(idx);
    }

    // ── 2) Update / push in new_rows order ──
    let new_len = new_rows.len();
    for (i, new_row) in new_rows.into_iter().enumerate() {
        if i < model.row_count() {
            if let Some(old) = model.row_data(i) {
                if old.name == new_row.name {
                    // Merge images: keep old if non-empty, else keep new (absorb)
                    let mut merged = new_row.clone();
                    if old.thumb.size().width > 0 {
                        merged.thumb = old.thumb.clone();
                    }
                    if old.hero.size().width > 0 {
                        merged.hero = old.hero.clone();
                    }
                    if old.slat_image.size().width > 0 {
                        merged.slat_image = old.slat_image.clone();
                    }
                    if old.slat_expanded_image.size().width > 0 {
                        merged.slat_expanded_image = old.slat_expanded_image.clone();
                    }
                    // Diff: only rewrite if something actually changed
                    let providers_equal = merged.providers.row_count() == old.providers.row_count();
                    let needs = merged.name != old.name
                        || merged.saved_at != old.saved_at
                        || merged.is_active != old.is_active
                        || merged.accent != old.accent
                        || merged.primary != old.primary
                        || merged.secondary != old.secondary
                        || merged.tertiary != old.tertiary
                        || merged.surface != old.surface
                        || merged.border_size != old.border_size
                        || merged.border_radius != old.border_radius
                        || merged.border_color != old.border_color
                        || merged.shader != old.shader
                        || merged.thumb_path != old.thumb_path
                        || !providers_equal
                        || merged.thumb.size() != old.thumb.size()
                        || merged.hero.size() != old.hero.size()
                        || merged.slat_image.size() != old.slat_image.size()
                        || merged.slat_expanded_image.size() != old.slat_expanded_image.size();
                    if needs {
                        model.set_row_data(i, merged);
                    }
                    continue;
                } else {
                    // Order mismatch (should not happen for append/remove-only after phase 1,
                    // but handle by overwriting the slot with the expected name).
                    model.set_row_data(i, new_row);
                    continue;
                }
            }
        } else {
            // Append remaining new rows
            model.push(new_row);
        }
    }

    // ── 3) Trim any excess tail (defensive) ──
    while model.row_count() > new_len {
        model.remove(model.row_count() - 1);
    }
}

/// Blank the baked images of the named cards so they re-bake (U7 preview
/// refresh). `sync_cards` deliberately keeps old bakes (S5 flicker fix),
/// and the thumb scheduler skips rows that already carry images — so after
/// an overwrite or a refresh store the card would show stale pixels forever
/// with no invalidation path. Call AFTER `sync_cards`: the merge must run first so
/// fresh non-image fields land, then only the affected rows drop their
/// four image fields and the existing scheduler re-resolves + re-bakes
/// them. Untouched rows keep their bakes (no mass invalidation, no
/// flicker). Invalidation is unconditional for the affected theme — a
/// content-keyed warm cache makes an unchanged source cheap, which is
/// simpler and always correct versus comparing before/after.
pub fn invalidate_card_bakes(
    model: &slint::VecModel<crate::GalleryCardData>,
    names: &std::collections::HashSet<String>,
) {
    use slint::Model as _;
    if names.is_empty() {
        return;
    }
    for i in 0..model.row_count() {
        if let Some(mut row) = model.row_data(i) {
            if names.contains(row.name.as_str()) {
                row.thumb = slint::Image::default();
                row.hero = slint::Image::default();
                row.slat_image = slint::Image::default();
                row.slat_expanded_image = slint::Image::default();
                model.set_row_data(i, row);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme_manager::ThemeInfo;

    /// GalleryCardData fixture with defaults and one baked image slot set.
    fn card(name: &str, baked: bool) -> crate::GalleryCardData {
        let img = if baked {
            slint::Image::from_rgba8(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(4, 4))
        } else {
            slint::Image::default()
        };
        crate::GalleryCardData {
            name: name.to_string().into(),
            saved_at: "".into(),
            is_active: false,
            providers: slint::ModelRc::new(slint::VecModel::from(Vec::<slint::SharedString>::new())),
            accent: slint::Color::from_rgb_u8(1, 2, 3),
            primary: slint::Color::from_rgb_u8(0, 0, 0),
            secondary: slint::Color::from_rgb_u8(0, 0, 0),
            tertiary: slint::Color::from_rgb_u8(0, 0, 0),
            surface: slint::Color::from_rgb_u8(0, 0, 0),
            border_size: 0,
            border_radius: 0,
            border_color: slint::Color::from_rgb_u8(0, 0, 0),
            shader: "".into(),
            thumb_path: "".into(),
            thumb: img.clone(),
            hero: img.clone(),
            slat_image: img.clone(),
            slat_expanded_image: img,
        }
    }

    fn has_bakes(c: &crate::GalleryCardData) -> bool {
        c.thumb.size().width > 0 && c.slat_image.size().width > 0 && c.slat_expanded_image.size().width > 0
    }

    // ── S5 RED: bake carry-over across gallery rebuilds ────────────

    #[test]
    fn carry_over_bakes_preserves_images_by_name() {
        let old = vec![card("Alpha", true), card("Beta", true)];
        let fresh = vec![card("Alpha", false), card("Beta", false)];
        let out = carry_over_bakes(&old, fresh);
        assert!(out.iter().all(has_bakes), "matched cards must keep their bakes");
        assert_eq!(out[0].name, "Alpha");
        assert_eq!(out[1].name, "Beta");
    }

    #[test]
    fn carry_over_bakes_new_theme_starts_blank_and_order_preserved() {
        let old = vec![card("Alpha", true)];
        let fresh = vec![card("Zulu", false), card("Alpha", false)];
        let out = carry_over_bakes(&old, fresh);
        assert_eq!(out.len(), 2, "count preserved");
        assert_eq!(out[0].name, "Zulu", "fresh order preserved");
        assert!(!has_bakes(&out[0]), "unmatched new theme starts blank (async bake fills it)");
        assert!(has_bakes(&out[1]), "existing theme keeps its bakes");
    }

    #[test]
    fn carry_over_bakes_fresh_fields_win_for_non_image_data() {
        let mut old = card("Alpha", true);
        old.is_active = false;
        old.accent = slint::Color::from_rgb_u8(9, 9, 9);
        let mut fresh = card("Alpha", false);
        fresh.is_active = true; // theme was just applied — fresh flag must survive
        fresh.accent = slint::Color::from_rgb_u8(1, 2, 3);
        let out = carry_over_bakes(&[old], vec![fresh]);
        assert!(out[0].is_active, "non-image fields come from the FRESH row");
        assert_eq!(out[0].accent, slint::Color::from_rgb_u8(1, 2, 3));
        assert!(has_bakes(&out[0]), "…while bakes come from the old row");
    }

    fn info(name: &str, active: bool, providers: &[&str], saved_at: &str) -> ThemeInfo {
        ThemeInfo {
            name: name.to_string(),
            saved_at: saved_at.to_string(),
            is_active: active,
            providers: providers.iter().map(|s| s.to_string()).collect(),
        }
    }

    // ── 1.2 RED: mapping empty/active/providers ────────────────────

    #[test]
    fn gallery_maps_empty() {
        let infos: Vec<ThemeInfo> = vec![];
        let cards = ThemeCard::from_infos(&infos);
        assert!(cards.is_empty(), "empty ThemeManager.list() must map to empty cards");
    }

    #[test]
    fn gallery_active_flag() {
        let infos = vec![
            info("Nord", true, &["hve-presets"], "2026-01-01T00:00:00.000Z"),
            info("Cyber", false, &["hve-presets"], "2026-01-02T00:00:00.000Z"),
        ];
        let cards = ThemeCard::from_infos(&infos);
        assert_eq!(cards.len(), 2);
        assert!(cards[0].is_active, "first card active flag must be true");
        assert!(!cards[1].is_active, "second card active flag must be false");
        assert_eq!(cards[0].name, "Nord");
        assert_eq!(cards[1].name, "Cyber");
    }

    #[test]
    fn gallery_maps_providers_and_timestamp() {
        let infos = vec![info(
            "Ocean",
            false,
            &["hve-presets", "noctalia"],
            "2026-03-15T12:34:56.000Z",
        )];
        let cards = ThemeCard::from_infos(&infos);
        assert_eq!(cards[0].providers, vec!["hve-presets", "noctalia"]);
        assert_eq!(cards[0].saved_at, "2026-03-15T12:34:56.000Z");
    }

    #[test]
    fn gallery_card_defaults_match_tokens() {
        let i = info("Test", false, &[], "");
        let c = ThemeCard::from_info(&i);
        assert_eq!(c.colors.accent, FALLBACK_ACCENT, "accent defaults to fallback #4fc3f7");
        assert_eq!(c.colors.primary, FALLBACK_ACCENT);
        assert_eq!(c.border.size, 2, "border size default 2");
        assert_eq!(c.border.radius, 32, "border radius default 32");
        assert_eq!(c.border.color, FALLBACK_ACCENT);
        assert!(c.shader.is_none(), "shader defaults to None");
        assert!(c.thumb_path.is_none());
        assert_eq!(c.background, Background::Solid("#1d100e".to_string()));
    }

    #[test]
    fn gallery_from_info_preserves_name() {
        let i = info("My Theme", false, &[], "2026-08-22T00:00:00.000Z");
        let c = ThemeCard::from_info(&i);
        assert_eq!(c.name, "My Theme");
        assert_eq!(c.saved_at, "2026-08-22T00:00:00.000Z");
    }

    // ── 1.4 ThemeGalleryModel + 1.5 focus clamp S13 ────────────────

    #[test]
    fn gallery_focus_clamped_initial_zero() {
        let model = ThemeGalleryModel::new(vec![
            ThemeCard::from_info(&info("A", false, &[], "")),
            ThemeCard::from_info(&info("B", false, &[], "")),
        ]);
        assert_eq!(model.focused_index(), 0);
        assert_eq!(model.len(), 2);
    }

    #[test]
    fn gallery_focus_clamped_move_right() {
        let mut model = ThemeGalleryModel::new(vec![
            ThemeCard::from_info(&info("A", false, &[], "")),
            ThemeCard::from_info(&info("B", false, &[], "")),
            ThemeCard::from_info(&info("C", false, &[], "")),
        ]);
        assert_eq!(model.move_focus(1), 1);
        assert_eq!(model.move_focus(1), 2);
        assert_eq!(model.move_focus(1), 2, "clamped at last, no wrap S13");
    }

    #[test]
    fn clamp_bounds_left_and_right_no_wrap() {
        let mut model = ThemeGalleryModel::new(vec![
            ThemeCard::from_info(&info("A", false, &[], "")),
            ThemeCard::from_info(&info("B", false, &[], "")),
        ]);
        // At 0, moving left stays 0
        assert_eq!(model.move_focus(-1), 0, "clamped at first, no wrap");
        assert_eq!(model.move_focus(-5), 0);
        // Move to last
        assert_eq!(model.move_focus(5), 1, "clamped at last, no wrap");
        assert_eq!(model.move_focus(1), 1);
    }

    #[test]
    fn gallery_model_empty_focus_is_zero() {
        let mut model = ThemeGalleryModel::new(vec![]);
        assert_eq!(model.focused_index(), 0);
        assert!(model.focused_card().is_none());
        assert_eq!(model.move_focus(1), 0);
        assert_eq!(model.move_focus(-1), 0);
        assert!(model.is_empty());
    }

    #[test]
    fn gallery_model_style_switch() {
        let mut model = ThemeGalleryModel::new(vec![ThemeCard::from_info(&info("A", false, &[], ""))]);
        assert_eq!(model.style(), crate::shell::gallery::views::GalleryStyle::Slice);
        model.set_style(crate::shell::gallery::views::GalleryStyle::Hexagon);
        assert_eq!(model.style(), crate::shell::gallery::views::GalleryStyle::Hexagon);
        model.set_style(crate::shell::gallery::views::GalleryStyle::Mosaic);
        assert_eq!(model.style(), crate::shell::gallery::views::GalleryStyle::Mosaic);
    }

    #[test]
    fn gallery_model_from_infos_sets_focus_zero() {
        let infos = vec![
            info("One", false, &[], "2026-01-01T00:00:00.000Z"),
            info("Two", true, &[], "2026-01-02T00:00:00.000Z"),
        ];
        let model = ThemeGalleryModel::from_infos(&infos);
        assert_eq!(model.len(), 2);
        assert_eq!(model.focused_index(), 0);
        assert_eq!(model.themes()[0].name, "One");
        assert!(model.themes()[1].is_active);
    }

    // ── T3.1 RED: sync_cards in-place by name ─────────────────────
    #[test]
    fn sync_cards_updates_rows_in_place() {
        use slint::{Model, VecModel};
        // Persistent model with baked images
        let mut a = card("Alpha", true);
        a.is_active = false;
        a.accent = slint::Color::from_rgb_u8(10, 10, 10);
        let mut b = card("Beta", true);
        b.is_active = false;
        b.accent = slint::Color::from_rgb_u8(20, 20, 20);
        // Beta second scenario: old empty, new carries baked (absorb carry_over_bakes)
        // Actually for this test, start with Beta baked, but we will also verify the absorb case
        // via a second model below — keep this one simple: same set, one row changes.
        let model = VecModel::from(vec![a.clone(), b.clone()]);
        let ptr_before = &model as *const _ as usize;

        // Fresh rows (simulating to_gallery_cards): empty images, Alpha flips active + accent
        let mut fresh_a = card("Alpha", false);
        fresh_a.is_active = true;
        fresh_a.accent = slint::Color::from_rgb_u8(99, 99, 99);
        let mut fresh_b = card("Beta", false); // unchanged non-image fields
        fresh_b.accent = slint::Color::from_rgb_u8(20, 20, 20);

        crate::shell::gallery::model::sync_cards(&model, vec![fresh_a, fresh_b]);

        let ptr_after = &model as *const _ as usize;
        assert_eq!(ptr_before, ptr_after, "model identity must not change");
        assert_eq!(model.row_count(), 2);
        let out_a = model.row_data(0).unwrap();
        let out_b = model.row_data(1).unwrap();
        // Alpha: non-image from new, images preserved from old (old baked)
        assert!(out_a.is_active, "Alpha is_active must come from new row");
        assert_eq!(out_a.accent, slint::Color::from_rgb_u8(99, 99, 99));
        assert!(has_bakes(&out_a), "Alpha images preserved from old (old non-empty wins)");
        // Beta: untouched — still baked, still inactive, accent unchanged
        assert!(!out_b.is_active);
        assert_eq!(out_b.accent, slint::Color::from_rgb_u8(20, 20, 20));
        assert!(has_bakes(&out_b), "Beta untouched row keeps its bakes");

        // Absorb case: old empty + new carries → new bake kept
        let empty_old = card("Gamma", false);
        let model2 = VecModel::from(vec![empty_old]);
        let mut fresh_gamma = card("Gamma", true); // new carries baked
        fresh_gamma.is_active = true;
        crate::shell::gallery::model::sync_cards(&model2, vec![fresh_gamma]);
        let out_g = model2.row_data(0).unwrap();
        assert!(has_bakes(&out_g), "when old empty and new carries, absorb new bake");
        assert!(out_g.is_active);
    }

    // ── T3.2 RED: incremental push/remove ─────────────────────────
    #[test]
    fn sync_cards_pushes_and_removes_incrementally() {
        use slint::{Model, VecModel};
        // Push: 2 → 3 (append)
        let model = VecModel::from(vec![card("A", true), card("B", true)]);
        let fresh_a = card("A", false);
        let fresh_b = card("B", false);
        let mut fresh_c = card("C", false);
        fresh_c.is_active = true;
        crate::shell::gallery::model::sync_cards(&model, vec![fresh_a.clone(), fresh_b.clone(), fresh_c.clone()]);
        assert_eq!(model.row_count(), 3, "append must push");
        assert_eq!(model.row_data(0).unwrap().name, "A");
        assert_eq!(model.row_data(1).unwrap().name, "B");
        assert_eq!(model.row_data(2).unwrap().name, "C");
        // Surviving rows kept their bakes (old non-empty preserved)
        assert!(has_bakes(&model.row_data(0).unwrap()));
        assert!(has_bakes(&model.row_data(1).unwrap()));
        assert!(!has_bakes(&model.row_data(2).unwrap()), "new appended theme starts blank");

        // Remove: 3 → 2 (middle removed, descending safety)
        let model2 = VecModel::from(vec![card("A", true), card("B", true), card("C", true)]);
        // Keep A and C, drop B
        let fresh_a2 = card("A", false);
        let fresh_c2 = card("C", false);
        crate::shell::gallery::model::sync_cards(&model2, vec![fresh_a2, fresh_c2]);
        assert_eq!(model2.row_count(), 2, "removal must shrink");
        assert_eq!(model2.row_data(0).unwrap().name, "A");
        assert_eq!(model2.row_data(1).unwrap().name, "C");
        // Surviving rows preserved delegates: images still baked from old
        assert!(has_bakes(&model2.row_data(0).unwrap()));
        assert!(has_bakes(&model2.row_data(1).unwrap()));
        // Also test removal at end: 2 → 1
        let model3 = VecModel::from(vec![card("A", true), card("B", true)]);
        crate::shell::gallery::model::sync_cards(&model3, vec![card("A", false)]);
        assert_eq!(model3.row_count(), 1);
        assert_eq!(model3.row_data(0).unwrap().name, "A");
    }

    // ── U7 RED: overwrite invalidation (stale preview refresh) ──────
    // `sync_cards` deliberately keeps old bakes (S5 flicker fix above), so
    // after an overwrite the card shows stale pixels forever: nothing
    // blanks it and `schedule_thumbs` skips rows that already carry images.
    // The refresh path must blank ONLY the overwritten theme's four image
    // fields AFTER the merge, so the existing scheduler re-resolves and
    // re-bakes it (a content-keyed warm cache makes an unchanged source
    // cheap — unconditional invalidation of the affected theme is the
    // simple, always-correct choice over before/after comparison).
    #[test]
    fn overwrite_invalidation_blanks_only_the_overwritten_card() {
        use slint::{Model, VecModel};
        use std::collections::HashSet;
        // Overwrite-style refresh: both cards still carry their stale bakes.
        let model = VecModel::from(vec![card("Alpha", true), card("Beta", true)]);
        let names: HashSet<String> = ["Alpha".to_string()].into_iter().collect();
        crate::shell::gallery::model::invalidate_card_bakes(&model, &names);
        // The overwritten card drops all four baked images: blank rows are
        // exactly what the thumb scheduler picks up for re-bake.
        let alpha = model.row_data(0).unwrap();
        assert_eq!(alpha.thumb.size().width, 0, "overwritten thumb must be blank for re-bake");
        assert_eq!(alpha.hero.size().width, 0, "overwritten hero must be blank for re-bake");
        assert_eq!(alpha.slat_image.size().width, 0, "overwritten slat must be blank for re-bake");
        assert_eq!(
            alpha.slat_expanded_image.size().width, 0,
            "overwritten expanded slat must be blank for re-bake"
        );
        // The untouched card keeps its bakes: no mass invalidation, no
        // visible flicker for unrelated cards.
        let beta = model.row_data(1).unwrap();
        assert!(has_bakes(&beta), "untouched card must keep its bakes");
    }

    // ── U7 control: empty invalidation set keeps every bake ──────────
    // Delete/rename/refresh-only paths pass an empty set: no row may drop
    // its images (the wall must not flicker on unrelated refreshes).
    #[test]
    fn invalidation_with_empty_set_keeps_all_bakes() {
        use slint::{Model, VecModel};
        use std::collections::HashSet;
        let model = VecModel::from(vec![card("Alpha", true), card("Beta", true)]);
        crate::shell::gallery::model::invalidate_card_bakes(&model, &HashSet::new());
        assert!(has_bakes(&model.row_data(0).unwrap()), "empty set must not blank Alpha");
        assert!(has_bakes(&model.row_data(1).unwrap()), "empty set must not blank Beta");
    }
}
