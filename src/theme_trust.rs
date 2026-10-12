//! What a theme package carries that can run code when it is applied
//! (`odd/tasks/theme-package-trust.md`, W1 — detect only: nothing calls this
//! yet, the gate is W2).
//!
//! A theme folder is UNTRUSTED input. Two things inside it execute when the
//! theme is applied:
//!
//! - A provider `settings.toml` (the shipped one is `noctalia-v5`) may carry
//!   a `[hooks]` section whose values are shell commands. The apply copies
//!   that file VERBATIM over the live one and then reloads, so a packaged
//!   hook runs on apply.
//! - A `*.lua` preset under `presets/<category>/` is installed into the user
//!   preset store and `cat`ed unvalidated into the assembled `overlay.lua`
//!   that Hyprland `dofile`s.
//!
//! [`inspect_theme_package`] reports both so the warning (W2) can word itself
//! from facts: the hook KEYS with their COMMAND TEXT, and the preset names.
//!
//! Honest limit, stated here and in the warning itself: this report feeds a
//! CONSENT GATE, not a sandbox. It says what a package carries; it cannot
//! make an accepted theme harmless — if the keeper confirms, the theme's code
//! runs as him.
//!
//! Contract of this module: PURE and READ-ONLY (no write, no directory is
//! ever created) and TOTAL — a missing, unreadable, malformed, empty or
//! non-UTF8 file never panics: it only narrows the report. Reading is
//! line-based like every other `settings.toml` reader in the repo (there is
//! no TOML crate).

use std::path::{Path, PathBuf};

/// One shell hook a provider's `settings.toml` declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellHook {
    /// The provider directory that carries the file (e.g. `noctalia-v5`).
    pub provider: String,
    /// The `[hooks]` key (`started`, `colors_changed`, `shutdown`, …).
    pub key: String,
    /// The command text exactly as the file holds it, quotes stripped.
    pub command: String,
}

/// One `*.lua` preset the package carries under `presets/<category>/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaPreset {
    /// The preset category directory (`animations`, `borders`, `shaders`).
    pub category: String,
    /// The file name (`Space.lua`).
    pub file_name: String,
}

impl LuaPreset {
    /// The theme-relative location of the preset (`presets/animations/x.lua`).
    pub fn theme_relative_path(&self) -> PathBuf {
        Path::new(crate::theme_presets::PRESETS_DIR)
            .join(&self.category)
            .join(&self.file_name)
    }
}

/// What a theme package carries that can execute when it is applied.
///
/// Shaped so the warning (W2) can be worded directly from it: the hook keys
/// and command texts under [`shell_hooks`](Self::shell_hooks), the preset
/// names and paths under [`lua_presets`](Self::lua_presets).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ThemeTrustReport {
    /// Every `[hooks]` command of every `providers/*/settings.toml`, in
    /// provider-directory order (sorted) and file order inside a file.
    pub shell_hooks: Vec<ShellHook>,
    /// Every `*.lua` preset under `presets/<category>/`, in
    /// [`crate::theme_presets::CATEGORIES`] order and file-name order
    /// inside a category (deterministic, so a warning reads the same twice).
    pub lua_presets: Vec<LuaPreset>,
}

impl ThemeTrustReport {
    /// Whether the package carries neither shell hooks nor Lua presets: such
    /// a theme applies exactly as it did before this detector existed.
    pub fn is_empty(&self) -> bool {
        self.shell_hooks.is_empty() && self.lua_presets.is_empty()
    }
}

/// Read `theme_dir` and report what the package carries. READ-ONLY: nothing
/// is written and no directory is created, for any input — a theme path that
/// does not exist simply yields an empty report.
pub fn inspect_theme_package(theme_dir: &Path) -> ThemeTrustReport {
    ThemeTrustReport {
        shell_hooks: shell_hooks(theme_dir),
        lua_presets: lua_presets(theme_dir),
    }
}

/// The `[hooks]` commands of every `providers/<provider>/settings.toml`.
/// A missing file is the normal shape (only `noctalia-v5` writes one); an
/// unreadable or non-UTF8 one is warned about and skipped.
fn shell_hooks(theme_dir: &Path) -> Vec<ShellHook> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(theme_dir.join("providers")) else {
        return out;
    };
    let mut providers: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    providers.sort();
    for provider_dir in providers {
        let provider = provider_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if provider.is_empty() {
            continue;
        }
        let settings = provider_dir.join("settings.toml");
        match std::fs::read_to_string(&settings) {
            Ok(text) => out.extend(hooks_in_settings(&text, &provider)),
            // No settings.toml is the normal shape, not a defect.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(
                "[theme-trust] cannot read {} (skipped): {}",
                settings.display(),
                e
            ),
        }
    }
    out
}

/// The hooks one `settings.toml` text declares, in file order.
fn hooks_in_settings(text: &str, provider: &str) -> Vec<ShellHook> {
    let mut out = Vec::new();
    let mut in_hooks = false;
    for line in text.split('\n') {
        if let Some(section) = section_name(line) {
            in_hooks = section == "hooks" || section.starts_with("hooks.");
            continue;
        }
        if !in_hooks {
            continue;
        }
        if let Some((key, command)) = hook_command(line) {
            out.push(ShellHook {
                provider: provider.to_string(),
                key: key.to_string(),
                command: command.to_string(),
            });
        }
    }
    out
}

/// The section name a `settings.toml` header line declares, `None` for every
/// other line. House convention (same as `providers/noctalia.rs`): in a
/// machine-written file a `#` always starts a comment, so the code is the
/// line up to the first `#`.
fn section_name(line: &str) -> Option<&str> {
    let code = line.split('#').next().unwrap_or("").trim();
    Some(code.strip_prefix('[')?.strip_suffix(']')?.trim())
}

/// One `key = value` line of a `[hooks]` section as `(key, command)`, `None`
/// when the line carries no command: a comment, a blank line, no `=`, an
/// empty key, an empty value, or an ARRAY value — `[hooks]` also holds data
/// (`community_ids = [ … ]`) that Noctalia never executes, and reporting it
/// as a command would make the warning lie.
fn hook_command(line: &str) -> Option<(&str, &str)> {
    let code = line.split('#').next().unwrap_or("").trim_end();
    if code.trim().is_empty() {
        return None;
    }
    // `code` is a prefix of `line`, so the `=` index is valid in both.
    let eq_at = code.find('=')?;
    let mut key = code[..eq_at].trim();
    if key.is_empty() {
        return None;
    }
    // A quoted key (`"started" = …`) is legal TOML; report the bare name.
    if key.len() >= 2
        && ((key.starts_with('"') && key.ends_with('"'))
            || (key.starts_with('\'') && key.ends_with('\'')))
    {
        key = &key[1..key.len() - 1];
        if key.is_empty() {
            return None;
        }
    }
    let after = &line[eq_at + 1..];
    let value_at = eq_at + 1 + (after.len() - after.trim_start().len());
    let rest = &line[value_at..];
    let command = match rest.chars().next()? {
        quote @ ('"' | '\'') => {
            // An unterminated quote (garbage input): report the rest of the
            // line rather than dropping a real hook — this is a detector.
            let end = rest[1..]
                .find(quote)
                .map(|at| at + 1)
                .unwrap_or(rest.len());
            &rest[1..end]
        }
        '[' => return None, // an array is data, never a command
        _ => {
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '#')
                .unwrap_or(rest.len());
            if end == 0 {
                return None;
            }
            &rest[..end]
        }
    };
    if command.trim().is_empty() {
        return None;
    }
    Some((key, command))
}

/// Every `*.lua` file a theme carries under `presets/<category>/`, in
/// [`crate::theme_presets::CATEGORIES`] order — exactly the directories
/// `install_theme_presets` reads on apply, so the report matches what can
/// reach the user preset store and, from there, `overlay.lua`.
fn lua_presets(theme_dir: &Path) -> Vec<LuaPreset> {
    let mut out = Vec::new();
    for category in crate::theme_presets::CATEGORIES {
        let dir = theme_dir
            .join(crate::theme_presets::PRESETS_DIR)
            .join(category);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .filter(|name| is_lua_file(name))
            .collect();
        names.sort();
        out.extend(names.into_iter().map(|file_name| LuaPreset {
            category: category.to_string(),
            file_name,
        }));
    }
    out
}

/// Whether a file name marks a Lua preset (`*.lua`, any case — the store
/// matches extensions case-insensitively too).
fn is_lua_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lua"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// The shipped shape: a theme's `noctalia-v5` `settings.toml`.
    fn write_settings(theme: &Path, text: &str) {
        let dir = theme.join("providers").join("noctalia-v5");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("settings.toml"), text).unwrap();
    }

    fn write_preset(theme: &Path, category: &str, name: &str) {
        let dir = theme
            .join(crate::theme_presets::PRESETS_DIR)
            .join(category);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), "-- preset body\n").unwrap();
    }

    /// Every file under `root`, relative and sorted — a read-only fingerprint.
    fn tree(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(path.strip_prefix(root).unwrap().display().to_string());
                }
            }
        }
        out.sort();
        out
    }

    /// W1: the report carries each hook key with its command text VERBATIM
    /// (the warning words itself from both), names the provider file it came
    /// from, and does NOT mistake data for commands: `[hooks]` also holds
    /// `community_ids = [ … ]`, which Noctalia never executes, and keys of
    /// other sections (`config_version`, `[wallpaper] path`) are not hooks.
    #[test]
    fn reports_each_hook_key_and_its_command_text_verbatim() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            "config_version = \"1.2\"\n\
             \n\
             [hooks]\n\
             started = \"/home/other/.local/share/sddm/themes/SilentDinamic/sddm-sync.sh\"\n\
             colors_changed = \"notify-send palette changed\"\n\
             community_ids = [ \"abc\" ]\n\
             shutdown = \"sddm-watch.sh\" # the keeper's watchdog\n\
             \n\
             [wallpaper]\n\
             path = \"/home/other/wall.png\"\n",
        );

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.shell_hooks,
            vec![
                ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "started".into(),
                    command: "/home/other/.local/share/sddm/themes/SilentDinamic/sddm-sync.sh"
                        .into(),
                },
                ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "colors_changed".into(),
                    command: "notify-send palette changed".into(),
                },
                ShellHook {
                    provider: "noctalia-v5".into(),
                    key: "shutdown".into(),
                    command: "sddm-watch.sh".into(),
                },
            ],
            "each [hooks] key must come back with its command text verbatim, in file order; \
             an array value or another section's key is not a hook, got: {:?}",
            report.shell_hooks
        );
        assert!(
            !report.is_empty(),
            "a theme carrying hooks must not read as empty"
        );
        assert!(report.lua_presets.is_empty(), "this fixture carries no presets");
    }

    /// Every `providers/<provider>/settings.toml` is walked, not only the
    /// shipped `noctalia-v5`, and each hook names the provider it came from.
    #[test]
    fn walks_every_provider_settings_file() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(&theme, "[hooks]\nstarted = \"boot.sh\"\n");
        let other = theme.join("providers").join("some-other-tool");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("settings.toml"), "[hooks]\nshutdown = \"bye.sh\"\n").unwrap();

        let report = inspect_theme_package(&theme);

        let pairs: Vec<(String, String)> = report
            .shell_hooks
            .iter()
            .map(|h| (h.provider.clone(), h.key.clone()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("noctalia-v5".to_string(), "started".to_string()),
                ("some-other-tool".to_string(), "shutdown".to_string()),
            ],
            "provider dirs are walked in sorted order, got: {:?}",
            report.shell_hooks
        );
    }

    /// A `settings.toml` with no `[hooks]` at all carries no hook.
    #[test]
    fn a_settings_toml_without_a_hooks_section_reports_no_hooks() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(
            &theme,
            "[wallpaper]\npath = \"/home/other/wall.png\"\n\n[shell]\nenabled = true\n",
        );

        let report = inspect_theme_package(&theme);

        assert!(report.shell_hooks.is_empty(), "got: {:?}", report.shell_hooks);
        assert!(
            report.is_empty(),
            "a theme with neither hooks nor Lua presets must read as empty"
        );
    }

    /// The SHIPPED shape: package-time sanitizing drops the machine-only hook
    /// lines and leaves the header with an EMPTY body — no hook, no warning.
    #[test]
    fn an_empty_hooks_section_reports_no_hooks() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(&theme, "[hooks]\n\n[idle]\nenabled = true\n");

        let report = inspect_theme_package(&theme);

        assert!(report.shell_hooks.is_empty(), "got: {:?}", report.shell_hooks);
        assert!(report.is_empty());
    }

    /// W1: the report names the Lua presets the package carries (names AND
    /// theme-relative paths for the warning), only from the category
    /// directories `install_theme_presets` reads on apply, and only `*.lua`
    /// — a stray text file, a `.frag` shader and a preset outside the
    /// installable categories never reach `overlay.lua`, so they are not
    /// Lua presets this gate must name.
    #[test]
    fn reports_the_lua_presets_the_package_carries() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_preset(&theme, "animations", "travel.lua");
        write_preset(&theme, "borders", "Space.lua");
        write_preset(&theme, "animations", "notes.txt");
        write_preset(&theme, "borders", "scan.frag");
        write_preset(&theme, "stray-category", "evil.lua");

        let report = inspect_theme_package(&theme);

        assert_eq!(
            report.lua_presets,
            vec![
                LuaPreset {
                    category: "animations".into(),
                    file_name: "travel.lua".into(),
                },
                LuaPreset {
                    category: "borders".into(),
                    file_name: "Space.lua".into(),
                },
            ],
            "every installable *.lua must be named in CATEGORIES order, got: {:?}",
            report.lua_presets
        );
        assert_eq!(
            report.lua_presets[0].theme_relative_path(),
            Path::new("presets/animations/travel.lua"),
            "the warning must be able to show where the preset lives"
        );
        assert!(report.shell_hooks.is_empty());
        assert!(!report.is_empty(), "a theme carrying presets must not read as empty");
    }

    /// Acceptance 2: a theme carrying neither applies exactly as before —
    /// the detector says so with an empty report.
    #[test]
    fn a_theme_with_neither_hooks_nor_lua_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        let provider = theme.join("providers").join("hve-presets");
        fs::create_dir_all(&provider).unwrap();
        fs::write(provider.join("state.json"), "{\"active_anim_file\":\"x.lua\"}").unwrap();

        let report = inspect_theme_package(&theme);

        assert_eq!(report, ThemeTrustReport::default());
        assert!(report.is_empty());
    }

    /// READ-ONLY and TOTAL: inspecting a theme changes nothing on disk, and
    /// a theme path that does not exist yields an empty report without being
    /// created (a detector never writes).
    #[test]
    fn inspection_is_read_only_and_never_creates_anything() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        write_settings(&theme, "[hooks]\nstarted = \"x.sh\"\n");
        write_preset(&theme, "animations", "a.lua");
        let before = tree(&theme);

        let _ = inspect_theme_package(&theme);
        assert_eq!(tree(&theme), before, "inspection must not touch the tree");

        let missing = tmp.path().join("NeverExisted");
        assert!(inspect_theme_package(&missing).is_empty());
        assert!(
            !missing.exists(),
            "the detector must never create the theme directory it is given"
        );
    }

    /// TOTAL: a settings.toml that is missing, empty or garbage never
    /// panics, and the parser reports what it CAN read — even from the
    /// garbage, an unterminated quote after a real `[hooks]` header still
    /// surfaces the command text instead of hiding the hook.
    #[test]
    fn a_missing_empty_or_garbage_settings_toml_never_panics() {
        let tmp = tempfile::tempdir().unwrap();

        // Missing: the provider dir exists, the file does not.
        let missing = tmp.path().join("Missing");
        fs::create_dir_all(missing.join("providers").join("noctalia-v5")).unwrap();

        // Empty.
        let empty = tmp.path().join("Empty");
        write_settings(&empty, "");

        // Garbage.
        let garbage = tmp.path().join("Garbage");
        write_settings(
            &garbage,
            "@@@ not toml at all\n\
             no equals sign here\n\
             [hooks\n\
             started = \"lost under the broken header\"\n\
             [hooks]\n\
             colors_changed = \"still a command\n\
             = \"no key\"\n\
             nothing_here\n\
             ",
        );

        assert!(inspect_theme_package(&missing).shell_hooks.is_empty());
        assert!(inspect_theme_package(&empty).shell_hooks.is_empty());
        let report = inspect_theme_package(&garbage);
        assert_eq!(
            report.shell_hooks,
            vec![ShellHook {
                provider: "noctalia-v5".into(),
                key: "colors_changed".into(),
                command: "still a command".into(),
            }],
            "what can be read is reported, what cannot is skipped: {:?}",
            report.shell_hooks
        );
    }

    /// TOTAL: a non-UTF8 settings.toml cannot be read as text at all — the
    /// report just narrows to nothing instead of panicking.
    #[test]
    fn a_non_utf8_settings_toml_never_panics() {
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        let dir = theme.join("providers").join("noctalia-v5");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("settings.toml"), b"\xff\xfe[hooks]\nstarted = \"\xff\xfe\"\n")
            .unwrap();

        let report = inspect_theme_package(&theme);

        assert!(report.shell_hooks.is_empty(), "got: {:?}", report.shell_hooks);
        assert!(report.is_empty());
    }

    /// TOTAL: a preset file name that is not valid UTF-8 is skipped, exactly
    /// like `install_theme_presets` skips it (it can never be installed, so
    /// it can never run).
    #[cfg(unix)]
    #[test]
    fn a_preset_file_name_that_is_not_utf8_is_skipped() {
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::tempdir().unwrap();
        let theme = tmp.path().join("Theme");
        let dir = theme.join(crate::theme_presets::PRESETS_DIR).join("animations");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(std::ffi::OsStr::from_bytes(b"\xff.lua")), b"-- opaque\n").unwrap();

        let report = inspect_theme_package(&theme);

        assert!(report.lua_presets.is_empty(), "got: {:?}", report.lua_presets);
        assert!(report.is_empty());
    }
}
