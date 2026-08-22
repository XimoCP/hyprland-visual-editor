// HVE 2 — Gallery model (theme-gallery PR1 Foundation).
//
// Data layer for the theme gallery: ThemeCard + ThemeGalleryModel.
// Engine is sealed — reads ThemeManager::list() → ThemeInfo only.
//
// MIT credit: visual language translated from skwd-wall (MIT, © liixini).

use crate::theme_manager::ThemeInfo;
use std::path::PathBuf;

/// Fallback accent color when a theme has no palette (spec R6).
pub const FALLBACK_ACCENT: &str = "#4fc3f7";

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
}
