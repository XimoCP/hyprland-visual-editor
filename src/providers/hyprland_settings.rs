use crate::config_markers::{
    LUA_AUTOSTART_END, LUA_AUTOSTART_START, LUA_KEYBINDS_END, LUA_KEYBINDS_START,
    LUA_WINDOW_RULES_END, LUA_WINDOW_RULES_START,
};
use crate::theme_manager::{ProviderCapabilities, ThemeProvider};
use crate::utils::{edit_between_markers, extract_block};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyprlandSettingsState {
    pub keybinds: Option<String>,
    pub autostart: Option<String>,
    pub window_rules: Option<String>,
}

pub struct HyprlandSettingsProvider;

impl HyprlandSettingsProvider {
    pub fn new() -> Self {
        Self
    }

    /// Remove a marker block (markers AND content) from `path` when present.
    /// Returns `Ok(true)` if a block was removed, `Ok(false)` if the markers
    /// were absent. Used to restore the "disabled" state: a block saved as
    /// `None` (markers absent at save time) must delete the currently active
    /// block on apply, otherwise the disabled state is silently lost.
    fn remove_between_markers(&self, path: &Path, start_marker: &str, end_marker: &str) -> Result<bool, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;

        let (Some(start_idx), Some(end_idx)) =
            (content.find(start_marker), content.find(end_marker))
        else {
            return Ok(false);
        };
        if end_idx < start_idx {
            return Ok(false);
        }

        let before = &content[..start_idx];
        let after = &content[end_idx + end_marker.len()..];
        fs::write(path, format!("{}{}", before, after))
            .map_err(|e| format!("Cannot write {}: {}", path.display(), e))?;
        Ok(true)
    }

    fn settings_path(&self) -> std::path::PathBuf {
        crate::config::hve_settings_path()
    }
}

impl Default for HyprlandSettingsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeProvider for HyprlandSettingsProvider {
    fn id(&self) -> &str {
        "hyprland-settings"
    }

    fn display_name_key(&self) -> &str {
        "themes.provider.hyprland-settings"
    }

    fn icon(&self) -> &str {
        "⌘"
    }

    fn shell(&self) -> &str {
        "hyprland"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::KEYBINDS
            | ProviderCapabilities::AUTOSTART
            | ProviderCapabilities::WINDOW_RULES
    }

    fn save(&self, theme_dir: &Path) -> Result<(), String> {
        let path = self.settings_path();
        if !path.exists() {
            tracing::info!("[hyprland-settings] No settings file found — skipping save");
            return Ok(());
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Cannot read settings file: {}", e))?;

        let (wr_start, wr_end, kb_start, kb_end, as_start, as_end) = (
            LUA_WINDOW_RULES_START,
            LUA_WINDOW_RULES_END,
            LUA_KEYBINDS_START,
            LUA_KEYBINDS_END,
            LUA_AUTOSTART_START,
            LUA_AUTOSTART_END,
        );

        let state = HyprlandSettingsState {
            window_rules: extract_block(&content, wr_start, wr_end),
            keybinds: extract_block(&content, kb_start, kb_end),
            autostart: extract_block(&content, as_start, as_end),
        };

        let provider_dir = theme_dir.join("providers").join(self.id());
        fs::create_dir_all(&provider_dir)
            .map_err(|e| format!("Cannot create provider dir: {}", e))?;

        let json = serde_json::to_string_pretty(&state)
            .map_err(|e| format!("Serialize error: {}", e))?;
        fs::write(provider_dir.join("state.json"), &json)
            .map_err(|e| format!("Cannot write state.json: {}", e))?;

        tracing::info!("[hyprland-settings] Saved keybinds={}, autostart={}, window_rules={}",
            state.keybinds.is_some(), state.autostart.is_some(), state.window_rules.is_some());

        Ok(())
    }

    fn apply(&self, theme_dir: &Path) -> Result<(), String> {
        let path = self.settings_path();

        // Ensure settings file exists before applying
        if !path.exists() {
            crate::settings::ensure_settings_file();
        }

        let state_path = theme_dir.join("providers").join(self.id()).join("state.json");
        let raw = fs::read_to_string(&state_path)
            .map_err(|e| format!("Cannot read state.json: {}", e))?;
        let state: HyprlandSettingsState = serde_json::from_str(&raw)
            .map_err(|e| format!("Parse error: {}", e))?;

        let (wr_start, wr_end, kb_start, kb_end, as_start, as_end) = (
            LUA_WINDOW_RULES_START,
            LUA_WINDOW_RULES_END,
            LUA_KEYBINDS_START,
            LUA_KEYBINDS_END,
            LUA_AUTOSTART_START,
            LUA_AUTOSTART_END,
        );

        // Apply window rules — `None` means the block was disabled when the
        // theme was saved, so the currently active block must be REMOVED.
        match &state.window_rules {
            Some(content) => {
                edit_between_markers(&path, wr_start, wr_end, content)
                    .map_err(|e| format!("Cannot apply window rules: {}", e))?;
                tracing::debug!("[hyprland-settings] Applied window rules");
            }
            None => {
                if self.remove_between_markers(&path, wr_start, wr_end)
                    .map_err(|e| format!("Cannot remove window rules: {}", e))?
                {
                    tracing::debug!("[hyprland-settings] Removed window rules block");
                }
            }
        }

        // Apply keybinds
        match &state.keybinds {
            Some(content) => {
                edit_between_markers(&path, kb_start, kb_end, content)
                    .map_err(|e| format!("Cannot apply keybinds: {}", e))?;
                tracing::debug!("[hyprland-settings] Applied keybinds");
            }
            None => {
                if self.remove_between_markers(&path, kb_start, kb_end)
                    .map_err(|e| format!("Cannot remove keybinds: {}", e))?
                {
                    tracing::debug!("[hyprland-settings] Removed keybinds block");
                }
            }
        }

        // Apply autostart
        match &state.autostart {
            Some(content) => {
                edit_between_markers(&path, as_start, as_end, content)
                    .map_err(|e| format!("Cannot apply autostart: {}", e))?;
                tracing::debug!("[hyprland-settings] Applied autostart");
            }
            None => {
                if self.remove_between_markers(&path, as_start, as_end)
                    .map_err(|e| format!("Cannot remove autostart: {}", e))?
                {
                    tracing::debug!("[hyprland-settings] Removed autostart block");
                }
            }
        }

        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempEnv;
    use tempfile::TempDir;

    fn write_settings(content: &str) {
        let path = crate::config::hve_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
    }

    fn read_settings() -> String {
        std::fs::read_to_string(crate::config::hve_settings_path()).unwrap()
    }

    fn provider_dir(theme: &std::path::Path) -> std::path::PathBuf {
        theme.join("providers").join("hyprland-settings")
    }

    fn write_state(theme: &std::path::Path, json: &str) {
        let pdir = provider_dir(theme);
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("state.json"), json).unwrap();
    }

    /// Apply with `keybinds: null` (saved while disabled) must REMOVE the
    /// currently active keybinds block instead of leaving it behind.
    #[test]
    fn test_apply_removes_block_when_state_none() {
        let _env = TempEnv::new();
        write_settings(
            "-- >>> HVE WINDOW RULES <<<\n\
             -- HVE 2 manages its own window state via the Composer trait\n\
             -- >>> HVE WINDOW RULES END <<<\n\
             -- >>> HVE KEYBINDS <<<\n\
             hl.bind(\"SUPER + H\", hl.dsp.exec_cmd(\"hve-ipc toggle-tray\"))\n\
             -- >>> HVE KEYBINDS END <<<\n",
        );

        let theme = TempDir::new().unwrap();
        write_state(
            theme.path(),
            r#"{
                "keybinds": null,
                "autostart": null,
                "window_rules": "-- HVE 2 manages its own window state via the Composer trait"
            }"#,
        );

        let provider = HyprlandSettingsProvider::new();
        provider.apply(theme.path()).expect("apply should succeed");

        let after = read_settings();
        assert!(
            !after.contains("HVE KEYBINDS"),
            "disabled keybinds block should be removed: {after}"
        );
        assert!(
            after.contains("HVE WINDOW RULES"),
            "enabled window rules should still be present: {after}"
        );
    }

    /// Full loop: save() while markers are absent records `None`; a later
    /// apply() on a file with active blocks removes them (keybinds + autostart)
    /// while re-writing the captured window rules.
    #[test]
    fn test_save_with_absent_markers_then_apply_removes_active_block() {
        let _env = TempEnv::new();
        // Settings file WITHOUT keybinds/autostart markers at save time.
        write_settings(
            "-- >>> HVE WINDOW RULES <<<\n\
             -- HVE 2 manages its own window state via the Composer trait\n\
             -- >>> HVE WINDOW RULES END <<<\n",
        );

        let theme = TempDir::new().unwrap();
        let provider = HyprlandSettingsProvider::new();
        provider.save(theme.path()).expect("save should succeed");

        let state_raw =
            std::fs::read_to_string(provider_dir(theme.path()).join("state.json")).unwrap();
        let state: HyprlandSettingsState = serde_json::from_str(&state_raw).unwrap();
        assert!(state.keybinds.is_none(), "keybinds should be None when markers absent at save");
        assert!(state.autostart.is_none(), "autostart should be None when markers absent at save");
        assert!(state.window_rules.is_some(), "window rules block should be captured");

        // Later the settings file gains active keybinds/autostart blocks
        // (e.g. enabled manually after the theme was saved).
        write_settings(
            "-- >>> HVE WINDOW RULES <<<\n\
             -- HVE 2 manages its own window state via the Composer trait\n\
             -- >>> HVE WINDOW RULES END <<<\n\
             -- >>> HVE KEYBINDS <<<\n\
             hl.bind(\"SUPER + H\", hl.dsp.exec_cmd(\"hve-ipc toggle-tray\"))\n\
             -- >>> HVE KEYBINDS END <<<\n\
             -- >>> HVE AUTOSTART <<<\n\
             hl.on(\"hyprland.start\", function()\n\
             hl.exec_cmd(\"hve --tray\")\n\
             end)\n\
             -- >>> HVE AUTOSTART END <<<\n",
        );

        provider.apply(theme.path()).expect("apply should succeed");

        let after = read_settings();
        assert!(
            !after.contains("HVE KEYBINDS"),
            "keybinds block should be removed on apply: {after}"
        );
        assert!(
            !after.contains("HVE AUTOSTART"),
            "autostart block should be removed on apply: {after}"
        );
        assert!(
            after.contains("HVE WINDOW RULES"),
            "captured window rules should still be present: {after}"
        );
    }

    /// `None` state against a file with no markers is a no-op (nothing to remove).
    #[test]
    fn test_apply_none_when_no_markers_is_noop() {
        let _env = TempEnv::new();
        write_settings(
            "some = user config\n\
             another = line\n",
        );

        let theme = TempDir::new().unwrap();
        write_state(
            theme.path(),
            r#"{
                "keybinds": null,
                "autostart": null,
                "window_rules": null
            }"#,
        );

        let provider = HyprlandSettingsProvider::new();
        provider.apply(theme.path()).expect("apply should succeed");

        let after = read_settings();
        assert_eq!(after, "some = user config\nanother = line\n", "file should be untouched");
    }
}
