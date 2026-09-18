//! Shared file-editing helpers.
//!
//! `edit_between_markers` and `extract_block` lived here until the
//! `hyprland-settings` provider was removed from the product (task A7). That
//! provider was their only production consumer; the remaining uses were the
//! unit tests, so both functions and their tests were deleted to keep
//! `cargo check` warning-free.
