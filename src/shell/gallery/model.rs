// HVE 2 — Gallery model (theme-gallery PR1 Foundation + PR4 Polish).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme_manager::ThemeInfo;

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
}
