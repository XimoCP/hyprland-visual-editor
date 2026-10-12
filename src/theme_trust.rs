//! What a theme package carries that can run code when it is applied
//! (`odd/tasks/theme-package-trust.md`, W1 — detect, W2 — the consent
//! gate below): [`inspect_theme_package`] reads a package, and
//! [`TrustGate`] decides whether an apply attempt proceeds, holds for
//! the warning, or is blocked behind it.
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

// ── W2: the consent gate ─────────────────────────────────────────────
//
// Detection alone is inert: every APPLY path must consult it and stop
// when the report is non-empty. The gate is the pure decision layer
// between the detector and the window — it holds an attempt, remembers
// it as pending, and only lets it through once the keeper has confirmed
// the warning (one-shot) or when the package carries nothing risky.
//
// It deliberately holds NO UI state and does NO I/O: the caller
// inspects the package itself, so the report the warning shows and the
// report consulted on the confirmed re-entry are both read fresh from
// disk at that moment. Contract: a theme that carries neither hooks nor
// Lua presets never reaches `Hold` — it applies exactly as it did
// before this module existed.

/// Which apply path an attempt came from (W2 gates both entry points).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOrigin {
    /// `on_gallery_card_clicked` — the gallery card / mosaic tile.
    Gallery,
    /// `on_panel_apply_saved_theme` — the panel Save list.
    Panel,
}

/// An attempt the gate is holding until the keeper decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingApply {
    /// The theme name the attempt resolved to.
    pub name: String,
    /// The index the original callback received, to re-invoke it on confirm.
    pub index: i32,
    /// Which entry point must be re-invoked to apply after confirmation.
    pub origin: ApplyOrigin,
}

/// What the caller must do with one apply attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    /// Apply exactly as before: nothing risky, or a confirmed theme.
    Proceed,
    /// Do NOT apply: the warning is now showing for [`TrustGate::pending`].
    Hold,
    /// Do NOT apply: the warning is already open (the gate is modal —
    /// nothing is applied behind it until the keeper decides).
    Blocked,
}

/// One-shot consent state shared by both apply paths.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TrustGate {
    /// The attempt the warning is currently asking about.
    pending: Option<PendingApply>,
    /// The name the keeper confirmed, consumed by the next matching
    /// [`TrustGate::request`] and cleared by [`TrustGate::finish_confirm`].
    confirmed: Option<String>,
}

impl TrustGate {
    /// A closed gate (no pending attempt, no confirmation).
    pub const fn new() -> Self {
        Self {
            pending: None,
            confirmed: None,
        }
    }

    /// Whether the warning is (or must be) on screen.
    pub fn is_open(&self) -> bool {
        self.pending.is_some()
    }

    /// The attempt being held, for diagnostics.
    pub fn pending(&self) -> Option<&PendingApply> {
        self.pending.as_ref()
    }

    /// The name confirmed but not yet re-entered, for diagnostics.
    pub fn confirmed(&self) -> Option<&str> {
        self.confirmed.as_deref()
    }

    /// Consult the gate for ONE apply attempt.
    ///
    /// Order matters and is pinned by test: the modal check wins (nothing
    /// applies behind an open warning), then the one-shot confirmation,
    /// then the report — an empty report is the pre-W2 behaviour and is
    /// never held.
    pub fn request(
        &mut self,
        name: &str,
        index: i32,
        origin: ApplyOrigin,
        report: &ThemeTrustReport,
    ) -> GateDecision {
        if self.pending.is_some() {
            return GateDecision::Blocked;
        }
        if self.confirmed.as_deref() == Some(name) {
            self.confirmed = None;
            return GateDecision::Proceed;
        }
        if report.is_empty() {
            return GateDecision::Proceed;
        }
        self.pending = Some(PendingApply {
            name: name.to_string(),
            index,
            origin,
        });
        GateDecision::Hold
    }

    /// The keeper pressed Continue: take the pending attempt and arm the
    /// one-shot confirmation for its name. The caller re-invokes the SAME
    /// entry point, whose `request` then returns [`GateDecision::Proceed`].
    pub fn begin_confirm(&mut self) -> Option<PendingApply> {
        let pending = self.pending.take()?;
        self.confirmed = Some(pending.name.clone());
        Some(pending)
    }

    /// Close the one-shot confirmation after the re-invocation returns. A
    /// re-entry that never reached `request` (the mutating guard, a
    /// vanished window) must NOT leave stale consent behind: the next
    /// attempt on that theme warns again.
    pub fn finish_confirm(&mut self) {
        self.confirmed = None;
    }

    /// The keeper pressed Cancel (or Escape): forget the pending attempt
    /// and change nothing. Returns whether there was one.
    pub fn cancel(&mut self) -> bool {
        self.pending.take().is_some()
    }
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

    // ── W2: the consent gate decisions ────────────────────────────────

    /// A package that carries a shell hook (the CRITICAL shape).
    fn risky_theme(root: &Path) -> PathBuf {
        let theme = root.join("Risky");
        write_settings(&theme, "[hooks]\nstarted = \"sddm-watch.sh\"\n");
        theme
    }

    /// A package that carries neither hooks nor Lua presets.
    fn clean_theme(root: &Path) -> PathBuf {
        let theme = root.join("Plain");
        let provider = theme.join("providers").join("hve-presets");
        fs::create_dir_all(&provider).unwrap();
        fs::write(provider.join("state.json"), "{}").unwrap();
        theme
    }

    /// W2 acceptance 3 (decision half): a theme carrying hooks is NOT
    /// applied — the gate holds it and shows the warning — and it applies
    /// ONLY after the keeper confirms, on the same entry point that asked.
    #[test]
    fn gate_holds_a_risky_theme_and_applies_it_only_after_confirmation() {
        let tmp = tempfile::tempdir().unwrap();
        let report = inspect_theme_package(&risky_theme(tmp.path()));
        let mut gate = TrustGate::new();

        assert_eq!(
            gate.request("Risky", 3, ApplyOrigin::Gallery, &report),
            GateDecision::Hold,
            "a theme carrying a shell hook must be held, not applied"
        );
        assert!(gate.is_open(), "the warning must be open for the held attempt");
        assert_eq!(
            gate.pending(),
            Some(&PendingApply {
                name: "Risky".into(),
                index: 3,
                origin: ApplyOrigin::Gallery,
            }),
            "the gate must remember what to apply and which path asked"
        );
        // A second attempt while the warning is open changes nothing.
        assert_eq!(
            gate.request("Risky", 3, ApplyOrigin::Gallery, &report),
            GateDecision::Blocked
        );

        // Confirm: the pending attempt comes back so the caller can
        // re-invoke the SAME entry point, armed for exactly that name.
        let pending = gate.begin_confirm().expect("a held attempt to confirm");
        assert_eq!(pending.name, "Risky");
        assert_eq!(pending.origin, ApplyOrigin::Gallery);
        assert_eq!(
            gate.request("Risky", 3, ApplyOrigin::Gallery, &report),
            GateDecision::Proceed,
            "the confirmed re-entry must apply exactly as before"
        );
        gate.finish_confirm();
        assert!(!gate.is_open(), "confirming must close the warning");
        assert_eq!(gate.confirmed(), None, "the confirmation is one-shot");

        // Consent is not remembered forever: applying that theme again
        // warns again.
        assert_eq!(
            gate.request("Risky", 3, ApplyOrigin::Gallery, &report),
            GateDecision::Hold,
            "a second apply of the same risky theme must warn again"
        );
    }

    /// W2 acceptance 3 (declining): Cancel forgets the attempt, applies
    /// nothing, and the next attempt asks again.
    #[test]
    fn gate_cancel_changes_nothing_and_the_next_attempt_warns_again() {
        let tmp = tempfile::tempdir().unwrap();
        let report = inspect_theme_package(&risky_theme(tmp.path()));
        let mut gate = TrustGate::new();

        assert_eq!(
            gate.request("Risky", 0, ApplyOrigin::Panel, &report),
            GateDecision::Hold
        );
        assert!(gate.cancel(), "declining a held attempt reports it");
        assert!(!gate.is_open(), "Cancel must close the warning");
        assert_eq!(gate.pending(), None, "nothing stays pending after Cancel");
        assert_eq!(
            gate.confirmed(),
            None,
            "Cancel must never arm a confirmation"
        );
        assert!(
            !gate.cancel(),
            "cancel with nothing pending is a no-op (idempotent)"
        );
        assert_eq!(
            gate.request("Risky", 0, ApplyOrigin::Panel, &report),
            GateDecision::Hold,
            "after declining, applying the same theme must warn again"
        );
    }

    /// W2 acceptance 2: a theme carrying neither hooks nor Lua presets
    /// applies exactly as it did before the gate existed — no hold, no
    /// warning, no pending state.
    #[test]
    fn gate_lets_a_theme_with_neither_hooks_nor_lua_apply_without_a_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let report = inspect_theme_package(&clean_theme(tmp.path()));
        assert!(report.is_empty(), "precondition: the fixture carries nothing");
        let mut gate = TrustGate::new();

        assert_eq!(
            gate.request("Plain", 1, ApplyOrigin::Gallery, &report),
            GateDecision::Proceed,
            "the pre-W2 behaviour must be untouched for a clean theme"
        );
        assert!(!gate.is_open());
        assert_eq!(gate.pending(), None);
        assert_eq!(
            gate.request("Plain", 1, ApplyOrigin::Panel, &report),
            GateDecision::Proceed,
            "the second path must be gated by the same rule"
        );
    }

    /// The warning is MODAL: while it is open no other apply attempt gets
    /// through — not even a clean theme — and the pending attempt stays
    /// the one the keeper was asked about.
    #[test]
    fn gate_is_modal_while_the_warning_is_open() {
        let tmp = tempfile::tempdir().unwrap();
        let risky = inspect_theme_package(&risky_theme(tmp.path()));
        let clean = inspect_theme_package(&clean_theme(tmp.path()));
        let mut gate = TrustGate::new();

        assert_eq!(
            gate.request("Risky", 0, ApplyOrigin::Gallery, &risky),
            GateDecision::Hold
        );
        assert_eq!(
            gate.request("Plain", 1, ApplyOrigin::Gallery, &clean),
            GateDecision::Blocked,
            "nothing may apply behind the open warning"
        );
        assert_eq!(
            gate.request("Other", 2, ApplyOrigin::Panel, &risky),
            GateDecision::Blocked
        );
        assert_eq!(
            gate.pending().map(|p| p.name.as_str()),
            Some("Risky"),
            "the modal keeps the attempt it was opened for"
        );
    }

    /// A confirmation that never reached `request` (the mutating guard
    /// swallowed the re-entry, the window went away) must not leave stale
    /// consent: the next attempt on that theme warns again.
    #[test]
    fn a_confirmation_that_never_reaches_the_request_expires() {
        let tmp = tempfile::tempdir().unwrap();
        let report = inspect_theme_package(&risky_theme(tmp.path()));
        let mut gate = TrustGate::new();

        assert_eq!(
            gate.request("Risky", 5, ApplyOrigin::Gallery, &report),
            GateDecision::Hold
        );
        let pending = gate.begin_confirm().expect("held attempt");
        assert_eq!(pending.index, 5);
        // The re-invoke never happened: the caller closes the round trip.
        gate.finish_confirm();
        assert!(!gate.is_open());
        assert_eq!(
            gate.request("Risky", 5, ApplyOrigin::Gallery, &report),
            GateDecision::Hold,
            "an unspent confirmation must not silently apply the theme"
        );
    }
}
