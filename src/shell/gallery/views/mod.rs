// HVE 2 — Gallery views (theme-gallery module 2, PR2 Slice).
//
// PR1 scaffold + PR2 Slice carousel. Hex/Mosaic land in PR3.
//
// The Rust view engines below own focus/flip/video/geometry logic that is
// verified headlessly by unit tests; at runtime presentation is rendered by
// the Slint delegates (ui/gallery/*), so parts of these engines are
// intentionally dead in the binary while remaining test-covered.
//
// MIT credit: visual language translated from skwd-wall (MIT, © liixini).
// No GPL code from hyprmod is used.

#[allow(dead_code)]
pub mod hexagon;
#[allow(dead_code)]
pub mod mosaic;
#[allow(dead_code)]
pub mod slice;

pub use hexagon::HexagonView;
pub use mosaic::MosaicView;
pub use slice::{GalleryKey, SliceView};

/// Chrome style-switch cross-fade duration 200ms S6 (3.8).
pub const CHROME_SWITCH_DURATION_MS: u64 = 200;

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
/// Contract implemented by all three view engines and exercised through
/// them in tests; the runtime dispatches keys via Shell/GallerySlot.
#[allow(dead_code)]
pub trait GalleryView: Send + Sync {
    fn style(&self) -> GalleryStyle;
    fn handle_key(&mut self, key: GalleryKey) -> bool;
}
