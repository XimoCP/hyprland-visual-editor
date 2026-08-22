// HVE 2 — Gallery views (theme-gallery module 2, PR2 Slice).
//
// PR1 scaffold + PR2 Slice carousel. Hex/Mosaic land in PR3.
//
// MIT credit: visual language translated from skwd-wall (MIT, © liixini).
// No GPL code from hyprmod is used.

pub mod slice;

pub use slice::{GalleryKey, SliceView};

/// Presentation styles 1:1 with skwd-wall (spec R2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GalleryStyle {
    #[default]
    Slice,
    Hexagon,
    Mosaic,
}

/// Trait for gallery presentation styles (design §2, PR2 Slice).
///
/// Each view owns its focus/flip/video state and reports whether a key
/// was consumed (true = handler took ownership, false = bubble).
pub trait GalleryView: Send + Sync {
    fn style(&self) -> GalleryStyle;
    fn handle_key(&mut self, key: GalleryKey) -> bool;
}
