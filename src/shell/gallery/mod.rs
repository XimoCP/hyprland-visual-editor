// HVE 2 — Gallery module (theme-gallery, PR1 Foundation).
//
// Re-exports the gallery data layer (ThemeCard, ThemeGalleryModel) and the
// view style enum. Engine is sealed — this module only reads ThemeManager.
//
// MIT credit: visual language translated from skwd-wall (MIT, © liixini).

pub mod model;
pub mod slot;
pub mod views;

pub use model::{Background, BorderConfig, GalleryColors, ThemeCard, ThemeGalleryModel};
pub use slot::GallerySlot;
pub use views::GalleryStyle;
