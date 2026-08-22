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
}
