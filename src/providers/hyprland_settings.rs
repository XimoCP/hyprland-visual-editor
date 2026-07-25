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

pub struct HyprlandSettingsProvider {
    format: String,
}

impl HyprlandSettingsProvider {
    pub fn new() -> Self {
        Self {
            format: crate::hve_format().to_string(),
        }
    }

    fn markers(&self) -> (&'static str, &'static str, &'static str, &'static str, &'static str, &'static str) {
        let (wr_start, wr_end, kb_start, kb_end, as_start, as_end) = if self.format == "lua" {
            ("-- >>> HVE WINDOW RULES <<<", "-- >>> HVE WINDOW RULES END <<<",
             "-- >>> HVE KEYBINDS <<<", "-- >>> HVE KEYBINDS END <<<",
             "-- >>> HVE AUTOSTART <<<", "-- >>> HVE AUTOSTART END <<<")
        } else {
            ("# >>> HVE WINDOW RULES <<<", "# >>> HVE WINDOW RULES END <<<",
             "# >>> HVE KEYBINDS <<<", "# >>> HVE KEYBINDS END <<<",
             "# >>> HVE AUTOSTART <<<", "# >>> HVE AUTOSTART END <<<")
        };
        (wr_start, wr_end, kb_start, kb_end, as_start, as_end)
    }

    fn settings_path(&self) -> std::path::PathBuf {
        crate::hve_settings_path()
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

        let (wr_start, wr_end, kb_start, kb_end, as_start, as_end) = self.markers();

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
            crate::ensure_settings_file();
        }

        let state_path = theme_dir.join("providers").join(self.id()).join("state.json");
        let raw = fs::read_to_string(&state_path)
            .map_err(|e| format!("Cannot read state.json: {}", e))?;
        let state: HyprlandSettingsState = serde_json::from_str(&raw)
            .map_err(|e| format!("Parse error: {}", e))?;

        let (wr_start, wr_end, kb_start, kb_end, as_start, as_end) = self.markers();

        // Apply window rules
        if let Some(content) = &state.window_rules {
            edit_between_markers(&path, wr_start, wr_end, content)
                .map_err(|e| format!("Cannot apply window rules: {}", e))?;
            tracing::debug!("[hyprland-settings] Applied window rules");
        }

        // Apply keybinds
        if let Some(content) = &state.keybinds {
            edit_between_markers(&path, kb_start, kb_end, content)
                .map_err(|e| format!("Cannot apply keybinds: {}", e))?;
            tracing::debug!("[hyprland-settings] Applied keybinds");
        }

        // Apply autostart
        if let Some(content) = &state.autostart {
            edit_between_markers(&path, as_start, as_end, content)
                .map_err(|e| format!("Cannot apply autostart: {}", e))?;
            tracing::debug!("[hyprland-settings] Applied autostart");
        }

        Ok(())
    }

}
