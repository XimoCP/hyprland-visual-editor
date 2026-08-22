// HVE 2 — Gallery views (theme-gallery module 2, PR3).
//
// Stub for PR1: the view trait and three presentation styles land in PR2/PR3.
// This file exists so `gallery::views` is importable and PR1's Scaffold task
// (`cargo test gallery -- --list`) discovers the gallery module.
//
// MIT credit: visual language translated from skwd-wall (MIT, © liixini).
// No GPL code from hyprmod is used.

/// Presentation styles 1:1 with skwd-wall (spec R2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GalleryStyle {
    #[default]
    Slice,
    Hexagon,
    Mosaic,
}
