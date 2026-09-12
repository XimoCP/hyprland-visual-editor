//! Pure migration guard over (`hyprland.lua` content, `hyprland.conf`
//! existence). Decision matrix per spec R6/R7.
//!
//! The predicate is intentionally filesystem-free: callers read the files and
//! pass content/existence in, which keeps every matrix row trivially testable.

// Not yet consumed by production: the mutation gates that call this land in PR 3
// of the drop-conf-support chain. This allows the self-contained contract + its
// tests to compile green until then.
#![allow(dead_code)]

use crate::config_markers::HVE_BLOCK_START;

/// Outcome of evaluating the migration guard for a Hyprland config state.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ConfigGuardDecision {
    /// Valid Lua with the HVE block present: mutating paths may proceed.
    LuaReady,
    /// Valid Lua but no HVE block: prompt the user to run `init.sh enable`.
    PromptToEnable,
    /// Conf-only system: refuse HVE mutations and show the migration message.
    ConfOnly,
    /// No Lua and no conf: fresh install, nothing to migrate.
    NothingToDo,
}

/// Evaluate the migration guard from `hyprland.lua` content and
/// `hyprland.conf` existence.
///
/// A present-and-valid `hyprland.lua` (containing `hl.` or `require(`) is
/// always treated as a Lua user and never guarded, even when a stray
/// `hyprland.conf` also exists.
pub(crate) fn evaluate_config_guard(
    lua: Option<&str>,
    conf_exists: bool,
) -> ConfigGuardDecision {
    if let Some(content) = lua {
        if is_valid_lua(content) {
            return if content.contains(HVE_BLOCK_START) {
                ConfigGuardDecision::LuaReady
            } else {
                ConfigGuardDecision::PromptToEnable
            };
        }
    }

    if conf_exists {
        ConfigGuardDecision::ConfOnly
    } else {
        ConfigGuardDecision::NothingToDo
    }
}

/// A file counts as Lua when it references the Hyprland Lua API or requires
/// another module. Anything else is an invalid stub for guard purposes.
fn is_valid_lua(content: &str) -> bool {
    content.contains("hl.") || content.contains("require(")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_markers::HVE_BLOCK_START;

    fn valid_lua_with_hve_marker() -> String {
        format!("{HVE_BLOCK_START}\nhl.config({{}})\n")
    }

    #[test]
    fn valid_lua_with_hve_marker_is_lua_ready() {
        assert_eq!(
            evaluate_config_guard(Some(&valid_lua_with_hve_marker()), true),
            ConfigGuardDecision::LuaReady
        );
    }

    #[test]
    fn valid_lua_without_hve_marker_prompts_to_enable() {
        assert_eq!(
            evaluate_config_guard(Some("require(\"user.config\")\n"), false),
            ConfigGuardDecision::PromptToEnable
        );
    }

    #[test]
    fn absent_lua_with_conf_is_conf_only() {
        assert_eq!(
            evaluate_config_guard(None, true),
            ConfigGuardDecision::ConfOnly
        );
    }

    #[test]
    fn absent_lua_without_conf_is_nothing_to_do() {
        assert_eq!(
            evaluate_config_guard(None, false),
            ConfigGuardDecision::NothingToDo
        );
    }

    #[test]
    fn invalid_lua_with_conf_is_conf_only() {
        assert_eq!(
            evaluate_config_guard(Some("general { gaps_in = 5 }\n"), true),
            ConfigGuardDecision::ConfOnly
        );
    }

    #[test]
    fn invalid_lua_without_conf_is_nothing_to_do() {
        assert_eq!(
            evaluate_config_guard(Some("general { gaps_in = 5 }\n"), false),
            ConfigGuardDecision::NothingToDo
        );
    }

    #[test]
    fn valid_lua_user_is_never_guarded_even_with_conf() {
        // R6: a valid Lua user must not be treated as conf-only when a stray
        // hyprland.conf also exists.
        assert_eq!(
            evaluate_config_guard(Some("hl.env(\"HVE\", \"1\")\n"), true),
            ConfigGuardDecision::PromptToEnable
        );
    }

    #[test]
    fn hve_marker_only_matters_for_valid_lua() {
        assert_eq!(
            evaluate_config_guard(Some(&format!("{HVE_BLOCK_START}\nnot lua\n")), true),
            ConfigGuardDecision::ConfOnly
        );
    }
}
