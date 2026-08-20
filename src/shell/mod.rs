//! HVE 2 shell: state-driven navigation and slot mounting.
//!
//! The shell is the HVE 2 "trunk": a single window that mutates instead of
//! stacking. `NavState` (in `nav`) is the pure Rust-owned navigation state
//! machine (nav-shell spec R1, R2). Slot mounting, window sizing and the
//! Slint chrome land in later deliveries on top of it.
//!
//! MIT credit: visual language, geometry and tokens are translated from
//! skwd-wall (MIT, © liixini, https://github.com/liixini/skwd-wall). No GPL
//! code from hyprmod is used.

// `NavState`, `SlotRegistry` and `SizePolicy` are not reachable from `main`
// until the Shell root is wired (task 4.3); deliveries 2-4 consume these
// types. Remove this allow once `main.rs` builds `Shell` (it keeps `cargo
// check` warning-free meanwhile).
#[allow(dead_code)]
pub mod nav;
#[allow(dead_code)]
pub mod size;
#[allow(dead_code)]
pub mod slots;
#[cfg(test)]
mod ui_tests;
