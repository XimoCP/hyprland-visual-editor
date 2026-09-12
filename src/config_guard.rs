//! Pure migration guard over (`hyprland.lua` content, `hyprland.conf`
//! existence). Decision matrix per spec R6/R7.
//!
//! The predicate is intentionally filesystem-free: callers read the files and
//! pass content/existence in, which keeps every matrix row trivially testable.
//! [`evaluate_config_guard_at`] is the thin path adapter used at startup.

use crate::config_markers::HVE_BLOCK_START;
use std::path::{Path, PathBuf};

/// Outcome of evaluating the migration guard for a Hyprland config state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl ConfigGuardDecision {
    /// Whether HVE may write to the user's config: preset/theme applies,
    /// engine callbacks and the generated settings file.
    ///
    /// Only a fully migrated system (`LuaReady`) or a fresh install with
    /// nothing to migrate (`NothingToDo`) may mutate. `PromptToEnable` and
    /// `ConfOnly` must not, because their writes would be ignored.
    pub(crate) fn permits_mutation(self) -> bool {
        matches!(self, Self::LuaReady | Self::NothingToDo)
    }

    /// Whether `init.sh enable` may inject the HVE block.
    ///
    /// Enabling is the remediation for a valid-Lua system that lacks the HVE
    /// block (`PromptToEnable`); a conf-only system would ignore the injected
    /// Lua, so it is refused there too.
    pub(crate) fn permits_init_enable(self) -> bool {
        !matches!(self, Self::ConfOnly)
    }
}

/// Read the real `hyprland.lua` / `hyprland.conf` files under `hypr_dir` and
/// evaluate the migration guard (used at startup, before any settings write).
pub(crate) fn evaluate_config_guard_at(hypr_dir: &Path) -> ConfigGuardDecision {
    let lua = std::fs::read_to_string(hypr_dir.join("hyprland.lua")).ok();
    let conf_exists = hypr_dir.join("hyprland.conf").exists();
    evaluate_config_guard(lua.as_deref(), conf_exists)
}

/// Evaluate the guard at the standard `$XDG_CONFIG_HOME/hypr` (or
/// `~/.config/hypr`) location, falling back to `NothingToDo` when no home
/// directory can be resolved.
pub(crate) fn evaluate_config_guard_default() -> ConfigGuardDecision {
    let hypr_dir: Option<PathBuf> = dirs::config_dir()
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("hypr"));

    match hypr_dir {
        Some(dir) => evaluate_config_guard_at(&dir),
        None => ConfigGuardDecision::NothingToDo,
    }
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

    // ── Mutation policy (R7/R8): the gates wired into main.rs in PR 3 ──

    #[test]
    fn lua_ready_permits_mutations_and_init_enable() {
        let d = ConfigGuardDecision::LuaReady;
        assert!(d.permits_mutation(), "a migrated Lua system may mutate");
        assert!(d.permits_init_enable());
    }

    #[test]
    fn prompt_to_enable_refuses_mutation_but_permits_init_enable() {
        let d = ConfigGuardDecision::PromptToEnable;
        assert!(
            !d.permits_mutation(),
            "applies must be refused until the HVE block is enabled"
        );
        assert!(d.permits_init_enable(), "init enable is the remediation");
    }

    #[test]
    fn conf_only_refuses_mutation_and_init_enable() {
        let d = ConfigGuardDecision::ConfOnly;
        assert!(!d.permits_mutation());
        assert!(
            !d.permits_init_enable(),
            "injecting Lua into a conf-only system would be ignored"
        );
    }

    #[test]
    fn nothing_to_do_preserves_current_behavior() {
        let d = ConfigGuardDecision::NothingToDo;
        assert!(d.permits_mutation(), "a fresh install keeps writing settings");
        assert!(d.permits_init_enable());
    }

    // ── Path adapter: main.rs reads the real Hyprland files ───────────

    #[test]
    fn path_adapter_reads_lua_and_conf_from_disk() {
        let ready = tempfile::tempdir().unwrap();
        std::fs::write(
            ready.path().join("hyprland.lua"),
            format!("{HVE_BLOCK_START}\nhl.config({{}})\n"),
        )
        .unwrap();
        assert_eq!(
            evaluate_config_guard_at(ready.path()),
            ConfigGuardDecision::LuaReady
        );

        let conf_only = tempfile::tempdir().unwrap();
        std::fs::write(
            conf_only.path().join("hyprland.conf"),
            "general { gaps_in = 5 }\n",
        )
        .unwrap();
        assert_eq!(
            evaluate_config_guard_at(conf_only.path()),
            ConfigGuardDecision::ConfOnly
        );

        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            evaluate_config_guard_at(empty.path()),
            ConfigGuardDecision::NothingToDo
        );
    }
}
