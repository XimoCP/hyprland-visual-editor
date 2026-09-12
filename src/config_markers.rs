//! Single source of truth for the Lua settings marker constants HVE emits and
//! scans for, plus the HVE-managed block markers injected into `hyprland.lua`.
//!
//! The shell scripts mirror these exact literals across their own process
//! boundary; this module owns the Rust side.

/// Start/end markers for the window-rules section of the HVE settings file.
pub(crate) const LUA_WINDOW_RULES_START: &str = "-- >>> HVE WINDOW RULES <<<";
pub(crate) const LUA_WINDOW_RULES_END: &str = "-- >>> HVE WINDOW RULES END <<<";

/// Start/end markers for the keybinds section of the HVE settings file.
pub(crate) const LUA_KEYBINDS_START: &str = "-- >>> HVE KEYBINDS <<<";
pub(crate) const LUA_KEYBINDS_END: &str = "-- >>> HVE KEYBINDS END <<<";

/// Start/end markers for the autostart section of the HVE settings file.
pub(crate) const LUA_AUTOSTART_START: &str = "-- >>> HVE AUTOSTART <<<";
pub(crate) const LUA_AUTOSTART_END: &str = "-- >>> HVE AUTOSTART END <<<";

/// Start/end markers for the HVE-managed block injected into `hyprland.lua`.
/// `HVE_BLOCK_START` is the marker the migration guard requires before it will
/// treat a valid `hyprland.lua` as ready for mutation.
///
/// `HVE_BLOCK_END` is mirrored verbatim by `assets/scripts/init.sh` (which
/// removes the whole block on disable); the Rust side only needs the start
/// marker, so keep the end constant for contract parity.
pub(crate) const HVE_BLOCK_START: &str = "-- >>> HYPRLAND VISUAL EDITOR START <<<";
#[allow(dead_code)]
pub(crate) const HVE_BLOCK_END: &str = "-- >>> HYPRLAND VISUAL EDITOR END <<<";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_settings_markers_exact_strings() {
        assert_eq!(LUA_WINDOW_RULES_START, "-- >>> HVE WINDOW RULES <<<");
        assert_eq!(LUA_WINDOW_RULES_END, "-- >>> HVE WINDOW RULES END <<<");
        assert_eq!(LUA_KEYBINDS_START, "-- >>> HVE KEYBINDS <<<");
        assert_eq!(LUA_KEYBINDS_END, "-- >>> HVE KEYBINDS END <<<");
        assert_eq!(LUA_AUTOSTART_START, "-- >>> HVE AUTOSTART <<<");
        assert_eq!(LUA_AUTOSTART_END, "-- >>> HVE AUTOSTART END <<<");
    }

    #[test]
    fn hve_block_markers_exact_strings() {
        assert_eq!(
            HVE_BLOCK_START,
            "-- >>> HYPRLAND VISUAL EDITOR START <<<"
        );
        assert_eq!(HVE_BLOCK_END, "-- >>> HYPRLAND VISUAL EDITOR END <<<");
    }

    #[test]
    fn every_marker_is_a_lua_comment() {
        let markers = [
            LUA_WINDOW_RULES_START,
            LUA_WINDOW_RULES_END,
            LUA_KEYBINDS_START,
            LUA_KEYBINDS_END,
            LUA_AUTOSTART_START,
            LUA_AUTOSTART_END,
            HVE_BLOCK_START,
            HVE_BLOCK_END,
        ];

        for marker in markers {
            assert!(
                marker.starts_with("-- >>> "),
                "marker must be a Lua comment: {marker}"
            );
            assert!(
                marker.ends_with(" <<<"),
                "marker must close with <<<: {marker}"
            );
            assert!(
                !marker.starts_with('#'),
                "Lua-only contract forbids conf markers: {marker}"
            );
        }
    }
}
