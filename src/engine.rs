use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone)]
pub struct Engine {
    scripts_dir: PathBuf,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ColorScheme {
    pub primary: String,
    pub secondary: String,
    pub tertiary: String,
    pub surface: String,
    pub surface_lowest: String,
    pub accent: String,
}

/// Errors that can occur during engine operations.
#[derive(Debug)]
pub enum EngineError {
    /// The requested script file does not exist on disk.
    ScriptNotFound {
        path: PathBuf,
        script_name: String,
    },
    /// The script path is not valid UTF-8 (e.g. from a non-UTF-8 project dir)
    /// and cannot be passed to `bash`.
    InvalidScriptPath {
        path: PathBuf,
        script_name: String,
    },
    /// The script process could not be spawned (I/O error).
    ProcessSpawnFailed {
        script_name: String,
        source: std::io::Error,
    },
    /// The script ran but exited with a non-zero status.
    ExecutionFailed {
        script_name: String,
        exit_code: Option<i32>,
        stderr: String,
    },
    /// Failed to parse script output (e.g. JSON).
    ParseFailed {
        details: String,
        source: Option<serde_json::Error>,
    },
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::ScriptNotFound { path, script_name } => {
                write!(
                    f,
                    "Script '{}' not found at {}",
                    script_name,
                    path.display()
                )
            }
            EngineError::InvalidScriptPath { path, script_name } => {
                write!(
                    f,
                    "Script '{}' has a non-UTF-8 path: {}",
                    script_name,
                    path.display()
                )
            }
            EngineError::ProcessSpawnFailed { script_name, source } => {
                write!(f, "Failed to execute '{}': {}", script_name, source)
            }
            EngineError::ExecutionFailed {
                script_name,
                exit_code,
                stderr,
            } => match exit_code {
                Some(code) => {
                    write!(
                        f,
                        "Script '{}' exited with code {}: {}",
                        script_name, code, stderr
                    )
                }
                None => {
                    write!(
                        f,
                        "Script '{}' terminated by signal: {}",
                        script_name, stderr
                    )
                }
            },
            EngineError::ParseFailed { details, .. } => {
                write!(f, "{}", details)
            }
        }
    }
}

impl Error for EngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            EngineError::ProcessSpawnFailed { source, .. } => Some(source),
            EngineError::ParseFailed {
                source: Some(s), ..
            } => Some(s),
            _ => None,
        }
    }
}

impl Engine {
    pub fn new(project_dir: &std::path::Path) -> Self {
        let scripts_dir = project_dir.join("assets").join("scripts");
        Self { scripts_dir }
    }

    /// The project's `assets/` directory (the parent of the scripts dir).
    /// Read-only: callers that need the shipped preset directories ask here
    /// instead of re-deriving them from the project root.
    pub fn assets_dir(&self) -> PathBuf {
        self.scripts_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.scripts_dir.clone())
    }

    pub fn run_script(&self, script_name: &str, args: &[&str]) -> Result<String, EngineError> {
        let script_path = self.scripts_dir.join(script_name);
        if !script_path.exists() {
            return Err(EngineError::ScriptNotFound {
                path: script_path,
                script_name: script_name.to_string(),
            });
        }

        let script_str = script_path.to_str().ok_or_else(|| EngineError::InvalidScriptPath {
            path: script_path.clone(),
            script_name: script_name.to_string(),
        })?;

        let output = Command::new("bash")
            .arg(script_str)
            .args(args)
            .output()
            .map_err(|e| EngineError::ProcessSpawnFailed {
                script_name: script_name.to_string(),
                source: e,
            })?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(EngineError::ExecutionFailed {
                script_name: script_name.to_string(),
                exit_code: output.status.code(),
                stderr: stderr.to_string(),
            })
        }
    }

    pub fn init_enable(&self) -> Result<String, EngineError> {
        self.run_script("init.sh", &["enable"])
    }

    pub fn init_disable(&self) -> Result<String, EngineError> {
        self.run_script("init.sh", &["disable"])
    }

    pub fn apply_animation(&self, file: &str) -> Result<String, EngineError> {
        self.run_script("apply_animation.sh", &[file])
    }

    pub fn apply_border(&self, file: &str) -> Result<String, EngineError> {
        self.run_script("border.sh", &[file])
    }

    pub fn apply_shader(&self, file: &str) -> Result<String, EngineError> {
        self.run_script("shader.sh", &[file])
    }

    /// Apply window geometry: border thickness (`size`), corner radius
    /// (`radius`, in px), and inner/outer gaps. All four go to `geometry.sh`,
    /// which writes `general.border_size`, `general.gaps_in`, `general.gaps_out`
    /// and `decoration.rounding`.
    pub fn apply_geometry(
        &self,
        size: i32,
        radius: i32,
        gaps_in: i32,
        gaps_out: i32,
    ) -> Result<String, EngineError> {
        self.run_script(
            "geometry.sh",
            &[
                &size.to_string(),
                &radius.to_string(),
                &gaps_in.to_string(),
                &gaps_out.to_string(),
            ],
        )
    }

    /// Read current colors from the active color source (Noctalia/pywal/matugen/manual).
    /// Returns a `ColorScheme` with hex color strings (e.g. "#2ec436").
    pub fn get_colors(&self) -> Result<ColorScheme, EngineError> {
        let output = self.run_script("get_colors.sh", &[])?;
        serde_json::from_str(&output).map_err(|e| EngineError::ParseFailed {
            details: format!("Color JSON parse error: {}", e),
            source: Some(e),
        })
    }

    pub fn scan(&self, category: &str) -> Result<Vec<PresetInfo>, EngineError> {
        let output = self.run_script("scan.sh", &[category])?;
        let data: Vec<ScanEntry> =
            serde_json::from_str(&output).map_err(|e| EngineError::ParseFailed {
                details: format!("JSON parse error: {}", e),
                source: Some(e),
            })?;

        Ok(data.into_iter().map(scan_entry_to_preset_info).collect())
    }
}

#[derive(Debug, Clone)]
pub struct PresetInfo {
    /// i18n key path (e.g. "animations.presets.01_relampago.title")
    pub i18n_title: String,
    /// i18n key path (e.g. "animations.presets.01_relampago.desc")
    pub i18n_desc: String,
    /// Raw fallback title from preset metadata
    pub raw_title: String,
    /// Raw fallback description from preset metadata
    pub raw_desc: String,
    pub file: String,
    pub tag: String,
}

/// An entry produced by the scan script JSON output.
#[derive(serde::Deserialize, Debug)]
pub(crate) struct ScanEntry {
    #[serde(rename = "file")]
    file: String,
    #[serde(rename = "title")]
    title: Option<String>,
    #[serde(rename = "desc")]
    desc: Option<String>,
    #[serde(rename = "rawTitle")]
    raw_title: Option<String>,
    #[serde(rename = "rawDesc")]
    raw_desc: Option<String>,
    #[serde(rename = "tag")]
    tag: Option<String>,
}

/// Convert a single [`ScanEntry`] into a [`PresetInfo`], applying the
/// i18n fallback rules:
///
/// * `i18n_title` / `i18n_desc` — empty string if the source field is `None`
/// * `raw_title` — the source `raw_title`, or the `file` name as fallback
/// * `raw_desc` — empty string if the source field is `None`
/// * `tag` — `"USER"` if the source field is `None`
pub(crate) fn scan_entry_to_preset_info(e: ScanEntry) -> PresetInfo {
    PresetInfo {
        i18n_title: e.title.unwrap_or_default(),
        i18n_desc: e.desc.unwrap_or_default(),
        raw_title: e.raw_title.unwrap_or_else(|| e.file.clone()),
        raw_desc: e.raw_desc.unwrap_or_default(),
        file: e.file,
        tag: e.tag.unwrap_or_else(|| "USER".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── EngineError::Display ─────────────────────────────────────────

    #[test]
    fn test_engine_error_display_script_not_found() {
        let err = EngineError::ScriptNotFound {
            path: PathBuf::from("/tmp/foo.sh"),
            script_name: "foo.sh".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("foo.sh"), "msg should name the script: {msg}");
        assert!(msg.contains("/tmp/foo.sh"), "msg should show the path: {msg}");
    }

    #[test]
    fn test_engine_error_display_invalid_script_path() {
        let err = EngineError::InvalidScriptPath {
            path: PathBuf::from("/tmp/foo.sh"),
            script_name: "foo.sh".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("foo.sh"), "msg should name the script: {msg}");
        assert!(msg.contains("non-UTF-8"), "msg should explain the problem: {msg}");
    }

    #[test]
    fn test_engine_error_source_invalid_script_path_is_none() {
        let err = EngineError::InvalidScriptPath {
            path: PathBuf::from("x"),
            script_name: "x".into(),
        };
        assert!(err.source().is_none());
    }

    #[test]
    fn test_engine_error_display_process_spawn_failed() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err = EngineError::ProcessSpawnFailed {
            script_name: "run.sh".into(),
            source: io_err,
        };
        let msg = err.to_string();
        assert!(msg.contains("run.sh"), "msg should name the script: {msg}");
        assert!(msg.contains("no such file"), "msg should include the io error: {msg}");
    }

    #[test]
    fn test_engine_error_display_execution_failed_with_code() {
        let err = EngineError::ExecutionFailed {
            script_name: "test.sh".into(),
            exit_code: Some(42),
            stderr: "permission denied".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("test.sh"));
        assert!(msg.contains("42"));
        assert!(msg.contains("permission denied"));
    }

    #[test]
    fn test_engine_error_display_execution_failed_no_code() {
        let err = EngineError::ExecutionFailed {
            script_name: "kill.sh".into(),
            exit_code: None,
            stderr: "terminated by signal".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("terminated by signal"));
        assert!(msg.contains("signal"));
    }

    #[test]
    fn test_engine_error_display_parse_failed() {
        let err = EngineError::ParseFailed {
            details: "invalid token at line 1".into(),
            source: None,
        };
        assert_eq!(err.to_string(), "invalid token at line 1");
    }

    // ── EngineError: std::error::Error ───────────────────────────────

    #[test]
    fn test_engine_error_source_process_spawn_failed() {
        let inner = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
        let err = EngineError::ProcessSpawnFailed {
            script_name: "x.sh".into(),
            source: inner,
        };
        let src = err.source();
        assert!(src.is_some(), "ProcessSpawnFailed should report a source");
        assert!(
            src.unwrap().is::<std::io::Error>(),
            "source should be std::io::Error"
        );
    }

    #[test]
    fn test_engine_error_source_parse_failed_with_inner() {
        let inner = serde_json::from_str::<ColorScheme>("{invalid}").unwrap_err();
        let err = EngineError::ParseFailed {
            details: "nope".into(),
            source: Some(inner),
        };
        let src = err.source();
        assert!(src.is_some(), "ParseFailed with source should report one");
    }

    #[test]
    fn test_engine_error_source_script_not_found_is_none() {
        let err = EngineError::ScriptNotFound {
            path: PathBuf::from("x"),
            script_name: "x".into(),
        };
        assert!(err.source().is_none());
    }

    #[test]
    fn test_engine_error_source_execution_failed_is_none() {
        let err = EngineError::ExecutionFailed {
            script_name: "x".into(),
            exit_code: Some(1),
            stderr: String::new(),
        };
        assert!(err.source().is_none());
    }

    #[test]
    fn test_engine_error_source_parse_failed_no_source_is_none() {
        let err = EngineError::ParseFailed {
            details: "bad".into(),
            source: None,
        };
        assert!(err.source().is_none());
    }

    // ── ColorScheme deserialisation ───────────────────────────────────

    #[test]
    fn test_color_scheme_deserialise_valid() {
        // Use r##"..."## to avoid collision with "# inside JSON values
        let json = r##"{
            "primary": "#ff0000",
            "secondary": "#00ff00",
            "tertiary": "#0000ff",
            "surface": "#ffffff",
            "surface_lowest": "#eeeeee",
            "accent": "#ff8800"
        }"##;
        let scheme: ColorScheme = serde_json::from_str(json).expect("valid JSON should parse");
        assert_eq!(scheme.primary, "#ff0000");
        assert_eq!(scheme.secondary, "#00ff00");
        assert_eq!(scheme.tertiary, "#0000ff");
        assert_eq!(scheme.surface, "#ffffff");
        assert_eq!(scheme.surface_lowest, "#eeeeee");
        assert_eq!(scheme.accent, "#ff8800");
    }

    #[test]
    fn test_color_scheme_deserialise_invalid_returns_err() {
        let result: Result<ColorScheme, _> = serde_json::from_str("not json at all");
        assert!(result.is_err(), "invalid JSON should fail to deserialise");
    }

    #[test]
    fn test_color_scheme_deserialise_missing_field_fails() {
        let json = r##"{ "primary": "#fff" }"##;
        let result: Result<ColorScheme, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing fields should cause parse error");
    }

    // ── Engine::new() ─────────────────────────────────────────────────

    #[test]
    fn test_engine_new_sets_scripts_dir() {
        let proj = PathBuf::from("/some/project");
        let engine = Engine::new(&proj);
        let expected = PathBuf::from("/some/project/assets/scripts");
        assert_eq!(engine.scripts_dir, expected);
    }

    // ── run_script: non-UTF-8 path handling ─────────────────────────

    #[cfg(unix)]
    #[test]
    fn test_run_script_non_utf8_project_dir_returns_error() {
        use std::os::unix::ffi::OsStrExt;

        let base = tempfile::tempdir().unwrap();
        // A project directory whose name contains a non-UTF-8 byte. The script
        // file exists on disk, but its path cannot be converted to a UTF-8 str
        // to pass to `bash` — run_script must return an error, not panic.
        let weird = std::ffi::OsStr::from_bytes(b"non-\xff-utf8");
        let proj = base.path().join(weird);
        let scripts_dir = proj.join("assets").join("scripts");
        std::fs::create_dir_all(&scripts_dir).unwrap();
        std::fs::write(scripts_dir.join("scan.sh"), "#!/bin/bash\necho hi\n").unwrap();

        let engine = Engine::new(&proj);
        let result = engine.run_script("scan.sh", &[]);
        assert!(
            matches!(result, Err(EngineError::InvalidScriptPath { .. })),
            "non-UTF-8 script path should yield InvalidScriptPath, got {:?}",
            result.map(|_| ())
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_run_script_non_utf8_missing_script_reports_not_found() {
        use std::os::unix::ffi::OsStrExt;

        let base = tempfile::tempdir().unwrap();
        let weird = std::ffi::OsStr::from_bytes(b"non-\xff-utf8");
        let proj = base.path().join(weird);
        std::fs::create_dir_all(proj.join("assets").join("scripts")).unwrap();

        // Missing script inside a non-UTF-8 dir: the existence check fires
        // first and must report ScriptNotFound, not panic on to_str().
        let engine = Engine::new(&proj);
        let result = engine.run_script("missing.sh", &[]);
        assert!(
            matches!(result, Err(EngineError::ScriptNotFound { .. })),
            "missing script should yield ScriptNotFound, got {:?}",
            result.map(|_| ())
        );
    }

    // ── ScanEntry deserialisation ─────────────────────────────────────

    #[test]
    fn test_scan_entry_deserialise_all_fields() {
        let json = r##"{
            "file": "wobbly.ron",
            "title": "Wobbly Animations",
            "desc": "Smooth wobbly animation effects",
            "rawTitle": "Wobbly",
            "rawDesc": "Wobbly animations desc",
            "tag": "SYSTEM"
        }"##;
        let entry: ScanEntry = serde_json::from_str(json).expect("valid JSON should parse");
        assert_eq!(entry.file, "wobbly.ron");
        assert_eq!(entry.title.as_deref(), Some("Wobbly Animations"));
        assert_eq!(entry.desc.as_deref(), Some("Smooth wobbly animation effects"));
        assert_eq!(entry.raw_title.as_deref(), Some("Wobbly"));
        assert_eq!(entry.raw_desc.as_deref(), Some("Wobbly animations desc"));
        assert_eq!(entry.tag.as_deref(), Some("SYSTEM"));
    }

    #[test]
    fn test_scan_entry_deserialise_minimal_only_file() {
        let json = r##"{ "file": "my-preset.ron" }"##;
        let entry: ScanEntry = serde_json::from_str(json).expect("file-only JSON should parse");
        assert_eq!(entry.file, "my-preset.ron");
        assert!(entry.title.is_none());
        assert!(entry.desc.is_none());
        assert!(entry.raw_title.is_none());
        assert!(entry.raw_desc.is_none());
        assert!(entry.tag.is_none());
    }

    #[test]
    fn test_scan_entry_deserialise_some_optionals_present() {
        let json = r##"{
            "file": "transp.ron",
            "title": "Transparency",
            "tag": "SYSTEM"
        }"##;
        let entry: ScanEntry = serde_json::from_str(json).expect("partial JSON should parse");
        assert_eq!(entry.file, "transp.ron");
        assert_eq!(entry.title.as_deref(), Some("Transparency"));
        assert!(entry.desc.is_none());
        assert!(entry.raw_title.is_none());
        assert!(entry.raw_desc.is_none());
        assert_eq!(entry.tag.as_deref(), Some("SYSTEM"));
    }

    #[test]
    fn test_scan_entry_deserialise_empty_vec() {
        let json = "[]";
        let entries: Vec<ScanEntry> =
            serde_json::from_str(json).expect("empty array should parse");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_scan_entry_deserialise_invalid_json_returns_err() {
        let result: Result<ScanEntry, _> = serde_json::from_str("not json at all");
        assert!(result.is_err(), "invalid JSON should fail to deserialise");
    }

    #[test]
    fn test_scan_entry_deserialise_missing_required_file_field_fails() {
        let json = r##"{ "title": "foo" }"##;
        let result: Result<ScanEntry, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "missing required 'file' field should cause a parse error"
        );
    }

    #[test]
    fn test_scan_entry_deserialise_multiple_entries() {
        let json = r##"[
            { "file": "a.ron", "title": "A" },
            { "file": "b.ron", "title": "B" }
        ]"##;
        let entries: Vec<ScanEntry> =
            serde_json::from_str(json).expect("valid array should parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].file, "a.ron");
        assert_eq!(entries[1].file, "b.ron");
    }

    // ── scan_entry_to_preset_info transformation ──────────────────────

    #[test]
    fn test_transform_all_fields() {
        let entry = ScanEntry {
            file: "wobbly.ron".into(),
            title: Some("Wobbly Animations".into()),
            desc: Some("Smooth wobbly effects".into()),
            raw_title: Some("Wobbly".into()),
            raw_desc: Some("Wobbly desc".into()),
            tag: Some("SYSTEM".into()),
        };
        let info = scan_entry_to_preset_info(entry);
        assert_eq!(info.i18n_title, "Wobbly Animations");
        assert_eq!(info.i18n_desc, "Smooth wobbly effects");
        assert_eq!(info.raw_title, "Wobbly");
        assert_eq!(info.raw_desc, "Wobbly desc");
        assert_eq!(info.file, "wobbly.ron");
        assert_eq!(info.tag, "SYSTEM");
    }

    #[test]
    fn test_transform_no_i18n_fields_defaults_to_empty() {
        let entry = ScanEntry {
            file: "minimal.ron".into(),
            title: None,
            desc: None,
            raw_title: None,
            raw_desc: None,
            tag: None,
        };
        let info = scan_entry_to_preset_info(entry);
        assert_eq!(info.i18n_title, "");
        assert_eq!(info.i18n_desc, "");
        assert_eq!(info.raw_title, "minimal.ron");
        assert_eq!(info.raw_desc, "");
        assert_eq!(info.file, "minimal.ron");
        assert_eq!(info.tag, "USER");
    }

    #[test]
    fn test_transform_raw_title_fallback_to_file() {
        let entry = ScanEntry {
            file: "custom.ron".into(),
            title: Some("Custom".into()),
            desc: None,
            raw_title: None,
            raw_desc: None,
            tag: Some("CUSTOM".into()),
        };
        let info = scan_entry_to_preset_info(entry);
        assert_eq!(info.raw_title, "custom.ron");
    }

    #[test]
    fn test_transform_raw_title_uses_source_when_present() {
        let entry = ScanEntry {
            file: "custom.ron".into(),
            title: None,
            desc: None,
            raw_title: Some("My Custom".into()),
            raw_desc: None,
            tag: None,
        };
        let info = scan_entry_to_preset_info(entry);
        assert_eq!(info.raw_title, "My Custom");
    }

    #[test]
    fn test_transform_tag_defaults_to_user() {
        let entry = ScanEntry {
            file: "user-preset.ron".into(),
            title: Some("My Preset".into()),
            desc: Some("A user preset".into()),
            raw_title: None,
            raw_desc: None,
            tag: None,
        };
        let info = scan_entry_to_preset_info(entry);
        assert_eq!(info.tag, "USER");
    }

    #[test]
    fn test_transform_tag_preserved_when_set() {
        let entry = ScanEntry {
            file: "system-preset.ron".into(),
            title: None,
            desc: None,
            raw_title: None,
            raw_desc: None,
            tag: Some("SYSTEM".into()),
        };
        let info = scan_entry_to_preset_info(entry);
        assert_eq!(info.tag, "SYSTEM");
    }

    #[test]
    fn test_transform_only_file_minimal_entry() {
        let entry = ScanEntry {
            file: "bare.ron".into(),
            title: None,
            desc: None,
            raw_title: None,
            raw_desc: None,
            tag: None,
        };
        let info = scan_entry_to_preset_info(entry);
        assert_eq!(info.i18n_title, "");
        assert_eq!(info.i18n_desc, "");
        assert_eq!(info.raw_title, "bare.ron");
        assert_eq!(info.raw_desc, "");
        assert_eq!(info.file, "bare.ron");
        assert_eq!(info.tag, "USER");
    }
}
