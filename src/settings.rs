use crate::config::{hve_cache_dir, hve_format, hve_settings_path};
use std::path::PathBuf;

/// Ensure the settings file exists with a default float rule and empty keybinds section.
/// Called once at startup — does NOT overwrite an existing file.
pub fn ensure_settings_file() {
    let path = hve_settings_path();
    if path.exists() {
        return;
    }

    let format = hve_format();
    let wr_marker_start = if format == "lua" {
        "-- >>> HVE WINDOW RULES <<<"
    } else {
        "# >>> HVE WINDOW RULES <<<"
    };
    let wr_marker_end = if format == "lua" {
        "-- >>> HVE WINDOW RULES END <<<"
    } else {
        "# >>> HVE WINDOW RULES END <<<"
    };
    let kb_marker_start = if format == "lua" {
        "-- >>> HVE KEYBINDS <<<"
    } else {
        "# >>> HVE KEYBINDS <<<"
    };
    let kb_marker_end = if format == "lua" {
        "-- >>> HVE KEYBINDS END <<<"
    } else {
        "# >>> HVE KEYBINDS END <<<"
    };
    let as_marker_start = if format == "lua" {
        "-- >>> HVE AUTOSTART <<<"
    } else {
        "# >>> HVE AUTOSTART <<<"
    };
    let as_marker_end = if format == "lua" {
        "-- >>> HVE AUTOSTART END <<<"
    } else {
        "# >>> HVE AUTOSTART END <<<"
    };
    let _ = std::fs::create_dir_all(hve_cache_dir());

    let content = if format == "lua" {
        format!(
            r#"{wr_marker_start}
hl.window_rule({{
  name  = "hve-floating",
  match = {{ title = "^Hyprland Visual Editor$" }},
  float = true,
  size  = {{ "95%", "95%" }},
  move  = {{ "center", "center" }},
}})
{wr_marker_end}
{kb_marker_start}
{kb_marker_end}
{as_marker_start}
{as_marker_end}
"#,
        )
    } else {
        format!(
            r#"{wr_marker_start}
windowrulev2 = float, title:^(Hyprland Visual Editor)$
windowrulev2 = center, title:^(Hyprland Visual Editor)$
windowrulev2 = size 95% 95%, title:^(Hyprland Visual Editor)$
{wr_marker_end}
{kb_marker_start}
{kb_marker_end}
{as_marker_start}
{as_marker_end}
"#,
        )
    };

    match std::fs::write(&path, content) {
        Ok(_) => tracing::info!("[settings] Created at {}", path.display()),
        Err(e) => tracing::error!("[settings] Failed to create: {}", e),
    }
}

/// Toggle HVE window rules between float (default) and tile.
///
/// Operates on hve-settings.lua/.conf — a dedicated file separate from
/// the overlay (which is managed by assemble.sh). This file is loaded AFTER
/// the user's windowrules.lua so its rule wins.
///
/// When `tiling` is false (default): `float = true` → window floats
/// When `tiling` is true:           `tile  = true` → window tiles
///
/// Returns true when the file was rewritten (and `hyprctl reload` fired).
/// Unchanged content is a strict no-op: every reload re-floats HVE and
/// kills a pending fullscreen dispatch, so the startup path — which calls
/// this on EVERY launch — must not reload when the rules already match.
pub(crate) fn set_tiling_window_rules(tiling: bool) -> bool {
    let path = hve_settings_path();
    if !path.exists() {
        tracing::warn!("[windowrules] File not found — creating default");
        ensure_settings_file();
    }

    let format = hve_format();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[windowrules] Failed to read: {}", e);
            return false;
        }
    };

    let marker_start = if format == "lua" {
        "-- >>> HVE WINDOW RULES <<<"
    } else {
        "# >>> HVE WINDOW RULES <<<"
    };
    let marker_end = if format == "lua" {
        "-- >>> HVE WINDOW RULES END <<<"
    } else {
        "# >>> HVE WINDOW RULES END <<<"
    };

    // Build the replacement block
    let rules_block: String = window_rules_block(tiling, format, marker_start, marker_end);

    // Steady state: same block already on disk → skip the rewrite AND the
    // reload. A reload re-floats HVE and drops any pending fullscreen.
    if !marker_block_needs_update(&content, marker_start, marker_end, &rules_block) {
        tracing::info!(
            "[windowrules] {} mode already applied — skip rewrite+reload (tile={})",
            if tiling { "TILING" } else { "FLOATING" },
            tiling,
        );
        return false;
    }

    // Replace content between markers
    let new_content = if content.contains(marker_start) {
        let mut result = String::new();
        let mut in_block = false;
        let mut replaced = false;
        for line in content.lines() {
            if line.trim() == marker_start {
                in_block = true;
                if !replaced {
                    result.push_str(&rules_block);
                    result.push('\n');
                    replaced = true;
                }
                continue;
            }
            if line.trim() == marker_end {
                in_block = false;
                continue;
            }
            if !in_block {
                result.push_str(line);
                result.push('\n');
            }
        }
        result
    } else {
        // No markers yet → append the block
        let mut result = content;
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(&rules_block);
        result.push('\n');
        result
    };

    match std::fs::write(&path, new_content) {
        Ok(_) => {
            tracing::info!(
                "[windowrules] {} mode applied (tile={})",
                if tiling { "TILING" } else { "FLOATING" },
                tiling,
            );
            if let Err(e) = std::process::Command::new("hyprctl")
                .arg("reload")
                .output()
                .map(|_| ())
            {
                tracing::warn!("[hve] hyprctl reload failed: {}", e);
            }
            true
        }
        Err(e) => {
            tracing::error!("[windowrules] Failed to write: {}", e);
            false
        }
    }
}

/// Pure: desired window-rules block for (`tiling`, `format`).
/// Extracted verbatim from `set_tiling_window_rules` so the compare-before-
/// write guard and tests share one source of truth.
fn window_rules_block(tiling: bool, format: &str, marker_start: &str, marker_end: &str) -> String {
    if tiling {
        // Tiling ON → tile rule (overrides float from user's windowrules.lua)
        if format == "lua" {
            format!(
                r#"{marker_start}
hl.window_rule({{
  name  = "hve-floating",
  match = {{ title = "^Hyprland Visual Editor$" }},
  tile  = true,
}})
{marker_end}"#,
            )
        } else {
            format!(
                r#"{marker_start}
windowrulev2 = tile, title:^(Hyprland Visual Editor)$
{marker_end}"#,
            )
        }
    } else {
        // Tiling OFF → float rule (default)
        if format == "lua" {
            format!(
                r#"{marker_start}
hl.window_rule({{
  name  = "hve-floating",
  match = {{ title = "^Hyprland Visual Editor$" }},
  float = true,
  size  = {{ "95%", "95%" }},
  move  = {{ "center", "center" }},
}})
{marker_end}"#,
            )
        } else {
            format!(
                r#"{marker_start}
windowrulev2 = float, title:^(Hyprland Visual Editor)$
windowrulev2 = center, title:^(Hyprland Visual Editor)$
windowrulev2 = size 95% 95%, title:^(Hyprland Visual Editor)$
{marker_end}"#,
            )
        }
    }
}

/// Pure: whether the block between `marker_start`/`marker_end` in `content`
/// differs from `desired`. Returns true when the markers are absent (append
/// path). Whitespace-normalized so semantically identical blocks skip the
/// rewrite — and with it the `hyprctl reload` that would drop fullscreen.
pub(crate) fn marker_block_needs_update(
    content: &str,
    marker_start: &str,
    marker_end: &str,
    desired: &str,
) -> bool {
    extract_marker_block(content, marker_start, marker_end).is_none_or(|current| {
        current.trim() != desired.trim()
    })
}

/// Extract the full block including both marker lines, if both markers
/// are present. The result compares directly against a desired block from
/// the `*_block` constructors (whitespace-normalized by the caller).
fn extract_marker_block(content: &str, marker_start: &str, marker_end: &str) -> Option<String> {
    let mut in_block = false;
    let mut found_start = false;
    let mut block = String::new();
    for line in content.lines() {
        if line.trim() == marker_start {
            in_block = true;
            found_start = true;
            block.push_str(line);
            block.push('\n');
            continue;
        }
        if line.trim() == marker_end {
            if found_start {
                block.push_str(line);
                block.push('\n');
                return Some(block);
            }
            in_block = false;
            continue;
        }
        if in_block {
            block.push_str(line);
            block.push('\n');
        }
    }
    None
}

/// Write or remove the essential HVE keybind (SUPER+H) between
/// `>>> HVE KEYBINDS <<<` markers in the hve-settings file. Supports both
/// Lua and conf formats. The 4 extra binds (ALT+Q/N/B/S) were removed per
/// user request (2026-09-07) — only SUPER+H (toggle-tray) is vital.
/// When `enabled` is true: writes the single toggle-tray bind.
/// When `enabled` is false: removes the markers and their content entirely.
/// Calls `hyprctl reload` after a successful write.
pub(crate) fn set_keybinds(enabled: bool) {
    let path = hve_settings_path();
    if !path.exists() {
        tracing::warn!("[keybinds] File not found — creating default");
        ensure_settings_file();
    }

    let format = hve_format();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[keybinds] Failed to read: {}", e);
            return;
        }
    };

    let marker_start = if format == "lua" {
        "-- >>> HVE KEYBINDS <<<"
    } else {
        "# >>> HVE KEYBINDS <<<"
    };
    let marker_end = if format == "lua" {
        "-- >>> HVE KEYBINDS END <<<"
    } else {
        "# >>> HVE KEYBINDS END <<<"
    };

    // Build the replacement block when enabled — only SUPER+H is vital
    let keybinds_block: String = if enabled {
        if format == "lua" {
            format!(
                r#"{marker_start}
hl.bind("SUPER + H", hl.dsp.exec_cmd("hve-ipc toggle-tray"))
{marker_end}"#,
            )
        } else {
            format!(
                r#"{marker_start}
bind = SUPER, H, exec, hve-ipc toggle-tray
{marker_end}"#,
            )
        }
    } else {
        String::new()
    };

    // Steady state: identical block already on disk → skip the rewrite
    // AND the reload (same fullscreen rationale as the window rules;
    // startup re-applies keybinds on every launch).
    let markers_present = content.contains(marker_start);
    if !enabled && !markers_present {
        return; // already clean — true no-op
    }
    if enabled
        && markers_present
        && !marker_block_needs_update(&content, marker_start, marker_end, &keybinds_block)
    {
        tracing::info!("[keybinds] already applied — skip rewrite+reload");
        return;
    }

    // Replace or remove content between markers
    let new_content = if content.contains(marker_start) {
        let mut result = String::new();
        let mut in_block = false;
        for line in content.lines() {
            if line.trim() == marker_start {
                in_block = true;
                if enabled {
                    result.push_str(&keybinds_block);
                    result.push('\n');
                }
                continue;
            }
            if line.trim() == marker_end {
                in_block = false;
                continue;
            }
            if !in_block {
                result.push_str(line);
                result.push('\n');
            }
        }
        result
    } else if enabled {
        // No markers yet → append the block
        let mut result = content;
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(&keybinds_block);
        result.push('\n');
        result
    } else {
        // Already no markers and disabled → nothing to do
        content
    };

    match std::fs::write(&path, new_content) {
        Ok(_) => {
            tracing::info!(
                "[keybinds] {}",
                if enabled { "WRITTEN" } else { "REMOVED" }
            );
            if let Err(e) = std::process::Command::new("hyprctl")
                .arg("reload")
                .output()
                .map(|_| ())
            {
                tracing::warn!("[hve] hyprctl reload failed: {}", e);
            }
        }
        Err(e) => {
            tracing::error!("[keybinds] Failed to write: {}", e);
        }
    }
}

/// Toggle HVE autostart in hve-settings.lua.
///
/// When enabled, writes `hl.on("hyprland.start", ...)` with `hl.exec_cmd("hve --tray")`
/// between markers. When disabled, removes the block entirely (markers stay in the
/// template for next enable).
///
/// This avoids touching the user's exec.lua — everything lives in our managed file,
/// which is dofile'd from hyprland.lua. Clean uninstall: delete hve-settings.lua
/// and remove the dofile line from hyprland.lua.
pub(crate) fn set_autostart(enabled: bool) {
    let path = hve_settings_path();
    if !path.exists() {
        tracing::warn!("[autostart] hve-settings not found — creating default");
        ensure_settings_file();
    }

    let format = hve_format();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[autostart] Failed to read hve-settings: {}", e);
            return;
        }
    };

    let marker_start = if format == "lua" {
        "-- >>> HVE AUTOSTART <<<"
    } else {
        "# >>> HVE AUTOSTART <<<"
    };
    let marker_end = if format == "lua" {
        "-- >>> HVE AUTOSTART END <<<"
    } else {
        "# >>> HVE AUTOSTART END <<<"
    };

    let exe = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("hve"))
        .display()
        .to_string();

    let autostart_block: String = if enabled {
        if format == "lua" {
            format!(
                r#"{marker_start}
hl.on("hyprland.start", function()
    hl.exec_cmd("{} --tray")
end)
{marker_end}"#,
                exe
            )
        } else {
            format!(
                r#"{marker_start}
exec-once = {} --tray
{marker_end}"#,
                exe
            )
        }
    } else {
        String::new()
    };

    // Same steady-state guard as keybinds/window rules: no rewrite means
    // no `hyprctl reload` means no dropped fullscreen at startup.
    let markers_present = content.contains(marker_start);
    if !enabled && !markers_present {
        return; // already clean — true no-op
    }
    if enabled
        && markers_present
        && !marker_block_needs_update(&content, marker_start, marker_end, &autostart_block)
    {
        tracing::info!("[autostart] already applied — skip rewrite+reload");
        return;
    }

    // Replace or remove content between markers
    let new_content = if content.contains(marker_start) {
        let mut result = String::new();
        let mut in_block = false;
        for line in content.lines() {
            if line.trim() == marker_start {
                in_block = true;
                if enabled {
                    result.push_str(&autostart_block);
                    result.push('\n');
                }
                continue;
            }
            if line.trim() == marker_end {
                in_block = false;
                continue;
            }
            if !in_block {
                result.push_str(line);
                result.push('\n');
            }
        }
        result
    } else if enabled {
        // No markers yet → append the block
        let mut result = content;
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(&autostart_block);
        result.push('\n');
        result
    } else {
        // Disabled with no markers → nothing to do
        content
    };

    match std::fs::write(&path, new_content) {
        Ok(_) => {
            tracing::info!(
                "[autostart] {} in hve-settings.lua",
                if enabled { "Enabled" } else { "Disabled" }
            );
            if let Err(e) = std::process::Command::new("hyprctl")
                .arg("reload")
                .output()
                .map(|_| ())
            {
                tracing::warn!("[hve] hyprctl reload failed: {}", e);
            }
        }
        Err(e) => tracing::error!("[autostart] Failed to write hve-settings: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempEnv;

    // ── set_keybinds ─────────────────────────────────────────────────

    #[test]
    fn test_set_keybinds_writes_and_removes() {
        let _env = TempEnv::new();

        // Ensure settings file exists (created in the current hve_format, lua or conf)
        ensure_settings_file();
        assert!(hve_settings_path().exists(), "settings file should exist");

        // Enable keybinds
        set_keybinds(true);
        let content = std::fs::read_to_string(hve_settings_path()).unwrap();
        assert!(
            content.contains(">>> HVE KEYBINDS <<<"),
            "should have keybinds start marker"
        );
        assert!(
            content.contains(">>> HVE KEYBINDS END <<<"),
            "should have keybinds end marker"
        );
        assert!(content.contains("hve-ipc"), "should have hve-ipc commands");
        assert!(
            content.contains("toggle-tray"),
            "should have toggle-tray bind"
        );
        // Only SUPER+H is kept — the 4 extra ALT binds were removed (2026-09-07)
        assert!(
            !content.contains("pause-restart"),
            "should NOT have pause-restart after slim-down"
        );
        assert!(!content.contains("next-anim"), "should NOT have next-anim");
        assert!(!content.contains("next-border"), "should NOT have next-border");
        assert!(!content.contains("next-shader"), "should NOT have next-shader");

        // Disable keybinds
        set_keybinds(false);
        let content = std::fs::read_to_string(hve_settings_path()).unwrap();
        assert!(
            !content.contains(">>> HVE KEYBINDS <<<"),
            "should NOT have keybinds start marker"
        );
        assert!(
            !content.contains(">>> HVE KEYBINDS END <<<"),
            "should NOT have keybinds end marker"
        );
        assert!(
            !content.contains("hve-ipc"),
            "should NOT have hve-ipc commands"
        );
    }

    #[test]
    fn test_set_keybinds_noop_when_disabled_and_markers_absent() {        let _env = TempEnv::new();

        // Create settings file manually WITHOUT keybinds markers
        let path = hve_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let no_kb_content = "# >>> HVE WINDOW RULES <<<\nwindowrulev2 = float, title:^(Hyprland Visual Editor)$\n# >>> HVE WINDOW RULES END <<<\n";
        std::fs::write(&path, no_kb_content).unwrap();

        let before = std::fs::read_to_string(&path).unwrap();
        assert!(
            !before.contains("HVE KEYBINDS"),
            "test file should not have keybinds markers"
        );

        // Disable when no markers exist → should be no-op
        set_keybinds(false);
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(before, after, "disabling when markers absent should be no-op");
    }

    // ── Startup fullscreen race: skip rewrite+reload when rules unchanged ──
    // Every `hyprctl reload` re-floats HVE and kills a pending fullscreen
    // dispatch. The startup path calls set_tiling_window_rules on EVERY
    // launch, so an unchanged file must not be rewritten nor reloaded.

    fn file_mtime(path: &std::path::Path) -> std::time::SystemTime {
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    fn assert_floating_content(content: &str) {
        if super::super::config::hve_format() == "lua" {
            assert!(content.contains("float = true"), "lua floating rule present");
        } else {
            assert!(
                content.contains("windowrulev2 = float"),
                "conf floating rule present"
            );
        }
        assert!(!content.contains("tile"), "no tiling rule in floating mode");
    }

    fn assert_tiling_content(content: &str) {
        assert!(content.contains("tile"), "tiling rule present, got:\n{content}");
    }

    #[test]
    fn tiling_rules_skip_rewrite_when_floating_already_applied() {
        let _env = TempEnv::new();
        ensure_settings_file(); // default file is floating
        set_tiling_window_rules(false); // canonicalize floating state
        let path = hve_settings_path();
        let before = std::fs::read_to_string(&path).unwrap();
        assert_floating_content(&before);
        let before_mtime = file_mtime(&path);

        set_tiling_window_rules(false);

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(before, after, "content must be untouched");
        assert_eq!(
            before_mtime,
            file_mtime(&path),
            "same-mode apply must NOT rewrite the file (rewrite fires hyprctl reload, which kills pending fullscreen)"
        );
    }

    #[test]
    fn tiling_rules_skip_rewrite_when_tiling_already_applied() {
        let _env = TempEnv::new();
        ensure_settings_file();
        set_tiling_window_rules(true);
        let path = hve_settings_path();
        let before = std::fs::read_to_string(&path).unwrap();
        assert_tiling_content(&before);
        let before_mtime = file_mtime(&path);

        set_tiling_window_rules(true);

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(before, after, "content must be untouched");
        assert_eq!(
            before_mtime,
            file_mtime(&path),
            "same-mode apply must NOT rewrite the file (rewrite fires hyprctl reload, which kills pending fullscreen)"
        );
    }

    #[test]
    fn tiling_rules_rewrite_when_mode_changes() {
        let _env = TempEnv::new();
        ensure_settings_file();
        set_tiling_window_rules(false);
        let path = hve_settings_path();
        assert_floating_content(&std::fs::read_to_string(&path).unwrap());

        set_tiling_window_rules(true);
        assert_tiling_content(&std::fs::read_to_string(&path).unwrap());

        set_tiling_window_rules(false);
        assert_floating_content(&std::fs::read_to_string(&path).unwrap());
    }

    #[test]
    fn marker_block_needs_update_detects_changes() {
        // Pure seam: only the block between markers decides rewrite vs skip.
        let start = "# >>> HVE WINDOW RULES <<<";
        let end = "# >>> HVE WINDOW RULES END <<<";
        let desired = format!("{start}\nwindowrulev2 = tile, title:^(HVE)$\n{end}");
        let same = format!("header\n{desired}\nfooter\n");
        assert!(
            !super::marker_block_needs_update(&same, start, end, &desired),
            "identical block must not need an update"
        );
        let different = format!("header\n{start}\nwindowrulev2 = float, title:^(HVE)$\n{end}\nfooter\n");
        assert!(
            super::marker_block_needs_update(&different, start, end, &desired),
            "different block must need an update"
        );
        assert!(
            super::marker_block_needs_update("no markers here\n", start, end, &desired),
            "missing markers must need an update"
        );
    }

    #[test]
    fn keybinds_skip_rewrite_when_already_enabled() {
        // Startup calls set_keybinds(true) on every launch — same guard as
        // the window rules: no rewrite means no extra hyprctl reload.
        let _env = TempEnv::new();
        ensure_settings_file();
        set_keybinds(true);
        let path = hve_settings_path();
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(before.contains("hve-ipc"), "precondition: keybinds written");
        let before_mtime = file_mtime(&path);

        set_keybinds(true);

        assert_eq!(before, std::fs::read_to_string(&path).unwrap());
        assert_eq!(
            before_mtime,
            file_mtime(&path),
            "re-enabling identical keybinds must NOT rewrite the file"
        );
    }

    #[test]
    fn autostart_skip_rewrite_when_state_matches() {
        // Same guard for autostart: repeated applies with the same state
        // must not rewrite nor reload.
        let _env = TempEnv::new();
        ensure_settings_file();
        set_autostart(false);
        let path = hve_settings_path();
        let before = std::fs::read_to_string(&path).unwrap();
        let before_mtime = file_mtime(&path);

        set_autostart(false);

        assert_eq!(before, std::fs::read_to_string(&path).unwrap());
        assert_eq!(
            before_mtime,
            file_mtime(&path),
            "re-disabling autostart must NOT rewrite the file"
        );
    }
}