//! OpenRGB provider: the keeper's RGB lighting travels with a theme.
//!
//! Saving a theme captures the live lighting state as an unchanged OpenRGB
//! profile document inside the theme directory; applying a theme loads it
//! back. Both directions talk to a RUNNING OpenRGB instance — the keeper's
//! `--server` start — because a client invocation that has to acquire the
//! devices itself is slow and fights the running instance for them.
//!
//! The provider is OPTIONAL by construction: it is registered only when the
//! machine has both the `openrgb` binary and its SDK server answering
//! ([`detected`]). On every other machine nothing is registered, no theme
//! records this id, and no other feature changes. That is what makes a
//! published provider invisible instead of intrusive.
//!
//! A lighting failure NEVER costs the theme: [`ThemeProvider::save`] and
//! [`ThemeProvider::apply`] run inside the theme transaction, where a returned
//! error aborts the whole thing — losing presets and colours because the fan
//! controller hiccuped is collateral damage. Every failure here is logged and
//! swallowed, and the rest of the theme proceeds.

use crate::theme_manager::{ProviderCapabilities, ThemeFact, ThemeProvider};
use std::fs;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Stable provider id (it is recorded in every theme that captured lighting).
pub const PROVIDER_ID: &str = "openrgb";

/// The profile file inside `{theme_dir}/providers/openrgb/`.
pub const PROFILE_FILE: &str = "profile.json";

/// OpenRGB's own default SDK server port, used when its config does not say.
pub const DEFAULT_SERVER_PORT: u16 = 6742;

/// The fixed name of the transient profile a capture writes into OpenRGB's own
/// profiles directory before the bytes move into the theme. A CONSTANT on
/// purpose: a per-run name would leave one ghost entry in the running
/// instance's profile list per save.
const CAPTURE_PROFILE: &str = "hve-capture";

/// The fixed profile name an apply stages the theme's bytes under. OpenRGB
/// resolves a profile NAME, and an absolute path is a silent no-op (measured
/// 2026-11-08), so the bytes are staged here and loaded by this name.
const APPLY_PROFILE: &str = "hve-theme";

/// How long one `openrgb` invocation may run before it is killed.
const COMMAND_DEADLINE: Duration = Duration::from_secs(5);

/// How long the liveness probe waits for the SDK server.
const PROBE_TIMEOUT: Duration = Duration::from_millis(300);

/// Distinct colours the info panel shows for a captured theme.
const SWATCH_LIMIT: usize = 6;

/// OpenRGB's config directory (`~/.config/OpenRGB`), or `None` when the
/// session declares no config home at all.
///
/// Deliberately NOT a `/tmp` fallback: the capture reads profile bytes from
/// here and copies them into the theme, so guessing a world-writable directory
/// would let another local user plant the record. Without a config dir the
/// provider simply does not capture.
fn openrgb_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("OpenRGB"))
}

/// The OpenRGB profiles directory, the one place the running server writes a
/// saved profile to.
fn profiles_dir() -> Option<PathBuf> {
    openrgb_config_dir().map(|dir| dir.join("profiles"))
}

/// OpenRGB's own config file, which records the port its SDK server listens on.
fn config_file() -> Option<PathBuf> {
    openrgb_config_dir().map(|dir| dir.join("OpenRGB.json"))
}

/// The SDK server port OpenRGB records in its config, falling back to
/// [`DEFAULT_SERVER_PORT`] for a missing file, unparsable JSON or an absurd
/// value (a port under 1024 is never a server this provider may probe).
fn server_port_from(raw: &str) -> u16 {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| {
            v.get("Server")?
                .get("default_port")?
                .as_u64()
                .and_then(|p| u16::try_from(p).ok())
        })
        .filter(|p| *p >= 1024)
        .unwrap_or(DEFAULT_SERVER_PORT)
}

/// The port this machine's OpenRGB records.
fn server_port() -> u16 {
    config_file()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|raw| server_port_from(&raw))
        .unwrap_or(DEFAULT_SERVER_PORT)
}

/// Whether something accepts a TCP connection on `port` of the loopback
/// interface — i.e. the SDK server is up and will answer a command.
fn server_answers(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), PROBE_TIMEOUT).is_ok()
}

/// Whether `bin` runs and answers `--version`. Side-effect free: a version
/// print never touches a device.
fn binary_answers(bin: &str, deadline: Duration) -> bool {
    run_binary(bin, &["--version"], deadline).is_some_and(|s| s.success())
}

/// Whether this machine can drive OpenRGB right now: the binary answers AND
/// its SDK server accepts a connection. Both, because every command this
/// provider runs needs the running instance.
///
/// Memoized for the process lifetime: the registration router asks more than
/// once (the app builds two managers), and the answer cannot change meaning for
/// a session — a client spawned by a registration probe is not a signal worth
/// paying a spawn-and-connect on every call.
pub fn detected() -> bool {
    static DETECTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DETECTED.get_or_init(|| {
        let port = server_port();
        binary_answers("openrgb", COMMAND_DEADLINE) && server_answers(port)
    })
}

/// Run `bin` with `args` under a deadline, returning its exit status.
///
/// Output is discarded ([`Stdio::null`]) so a chatty child can never deadlock
/// on a full pipe, and a child that outlives `deadline` is killed instead of
/// awaited — the theme transaction runs on the UI path.
fn run_binary(bin: &str, args: &[&str], deadline: Duration) -> Option<ExitStatus> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + deadline;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `0xRRGGBB` (OpenRGB's own colour encoding) as the `#rrggbb` string the
/// gallery renders.
fn hex_colour(value: u64) -> String {
    format!("#{:06x}", value & 0x00ff_ffff)
}

pub struct OpenRgbProvider {
    /// The `openrgb` executable to run (a bare name in production, an absolute
    /// path to a stub in tests).
    bin: String,
    /// Where the running instance writes a saved profile; `None` when the
    /// session declares no config home, in which case nothing is captured.
    profiles_dir: Option<PathBuf>,
    /// Deadline for one invocation.
    deadline: Duration,
}

impl OpenRgbProvider {
    pub fn new() -> Self {
        Self {
            bin: "openrgb".to_string(),
            profiles_dir: profiles_dir(),
            deadline: COMMAND_DEADLINE,
        }
    }

    /// The provider with injected endpoints. Tests use it to reach a stub
    /// binary and a temporary profiles directory, so no test touches the
    /// keeper's real lighting.
    #[cfg(test)]
    pub(crate) fn with_paths(bin: &str, profiles_dir: PathBuf, deadline: Duration) -> Self {
        Self {
            bin: bin.to_string(),
            profiles_dir: Some(profiles_dir),
            deadline,
        }
    }

    /// The provider directory inside a theme.
    fn provider_dir(theme_dir: &Path) -> PathBuf {
        theme_dir.join("providers").join(PROVIDER_ID)
    }

    /// Run `openrgb` with `args` and report the status; `None` when it could
    /// not be spawned, died, or outlived the deadline.
    fn run(&self, args: &[&str]) -> Option<ExitStatus> {
        run_binary(&self.bin, args, self.deadline)
    }

    /// Ask the running instance for a profile and move its bytes into the
    /// theme. Never returns `Err`: every failure is logged and the theme
    /// proceeds (S5).
    fn capture(&self, theme_dir: &Path, captured: &Path) -> Result<(), String> {
        let Some(status) = self.run(&["--save-profile", CAPTURE_PROFILE]) else {
            tracing::warn!(
                "[themes] openrgb: capture skipped — the command did not answer within {}s",
                self.deadline.as_secs_f32()
            );
            return Ok(());
        };
        if !status.success() {
            tracing::warn!(
                "[themes] openrgb: capture skipped — save-profile exited with {:?}",
                status.code()
            );
            return Ok(());
        }

        // A theme record is an untrusted file by the same rule any theme
        // record is: bounded, regular-file-guarded read.
        let raw = match crate::theme_media::read_theme_record(captured) {
            Ok(raw) => raw,
            Err(e) => {
                tracing::warn!("[themes] openrgb: capture skipped — {e}");
                return Ok(());
            }
        };

        let provider_dir = Self::provider_dir(theme_dir);
        if let Err(e) = fs::create_dir_all(&provider_dir) {
            tracing::warn!("[themes] openrgb: capture skipped — cannot create dir: {e}");
            return Ok(());
        }

        // Publish the bytes atomically: a half-written profile must never look
        // like a captured one.
        let target = provider_dir.join(PROFILE_FILE);
        let temp = provider_dir.join(format!(".{PROFILE_FILE}.tmp"));
        match fs::write(&temp, raw.as_bytes()).and_then(|()| fs::rename(&temp, &target)) {
            Ok(()) => tracing::info!("[themes] openrgb: lighting captured into the theme"),
            Err(e) => {
                let _ = fs::remove_file(&temp);
                tracing::warn!("[themes] openrgb: capture skipped — cannot write profile: {e}");
            }
        }
        Ok(())
    }

    /// Copy the theme's record into `staged` and load it by its fixed name.
    /// Never returns `Err`: every failure is logged and the theme proceeds.
    fn stage_and_load(&self, profile: &Path, staged: &Path) -> Result<(), String> {
        let raw = match crate::theme_media::read_theme_record(profile) {
            Ok(raw) => raw,
            Err(e) => {
                tracing::warn!("[themes] openrgb: lighting apply skipped — {e}");
                return Ok(());
            }
        };
        if let Err(e) = fs::write(staged, raw.as_bytes()) {
            tracing::warn!("[themes] openrgb: lighting apply skipped — cannot stage profile: {e}");
            return Ok(());
        }

        match self.run(&["--profile", APPLY_PROFILE]) {
            Some(status) if status.success() => {
                tracing::info!("[themes] openrgb: lighting applied from the theme");
            }
            Some(status) => tracing::warn!(
                "[themes] openrgb: lighting apply exited with {:?}",
                status.code()
            ),
            None => tracing::warn!(
                "[themes] openrgb: lighting apply skipped — no answer within {}s",
                self.deadline.as_secs_f32()
            ),
        }
        Ok(())
    }
}

impl Default for OpenRgbProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeProvider for OpenRgbProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn display_name_key(&self) -> &str {
        "themes.provider.openrgb"
    }

    fn icon(&self) -> &str {
        "rgb"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::empty()
    }

    /// Capture the live lighting into `{theme_dir}/providers/openrgb/profile.json`.
    ///
    /// The running instance writes the profile into ITS OWN profiles
    /// directory (an absolute name is sanitized into a flat file there), so
    /// the capture reads those bytes back and moves them into the theme. Every
    /// failure here is logged and swallowed (S5): the theme must save even
    /// when the lighting cannot be captured.
    fn save(&self, theme_dir: &Path) -> Result<(), String> {
        let Some(profiles_dir) = self.profiles_dir.clone() else {
            tracing::warn!(
                "[themes] openrgb: capture skipped — this session declares no config home"
            );
            return Ok(());
        };
        let captured = profiles_dir.join(format!("{CAPTURE_PROFILE}.json"));

        let outcome = self.capture(theme_dir, &captured);

        // HVE's own scratch never survives a capture ATTEMPT, whatever the
        // outcome: the profile list stays free of leftovers even when the
        // command failed after writing the file, or when the copy was skipped.
        let _ = fs::remove_file(&captured);
        outcome
    }

    /// Load the theme's own profile into the running instance.
    ///
    /// By NAME, after staging the theme's bytes into OpenRGB's own profiles
    /// directory: `openrgb --profile <absolute path>` prints "loaded
    /// successfully" and changes NOTHING (measured 2026-11-08), while a name
    /// is what the running instance actually resolves. The staging file uses a
    /// fixed name and is removed after the attempt, so the keeper's own
    /// profile list is never confused with HVE's scratch and no per-theme file
    /// accumulates. A theme without a record applies exactly as before (S4).
    fn apply(&self, theme_dir: &Path) -> Result<(), String> {
        let profile = Self::provider_dir(theme_dir).join(PROFILE_FILE);
        if !profile.is_file() {
            tracing::debug!("[themes] openrgb: theme holds no lighting record; nothing to apply");
            return Ok(());
        }
        let Some(profiles_dir) = self.profiles_dir.clone() else {
            tracing::warn!(
                "[themes] openrgb: lighting apply skipped — this session declares no config home"
            );
            return Ok(());
        };

        let staged = profiles_dir.join(format!("{APPLY_PROFILE}.json"));
        let outcome = self.stage_and_load(&profile, &staged);
        let _ = fs::remove_file(&staged);
        outcome
    }

    /// What the info panel shows for a theme that captured lighting: how many
    /// devices answered, plus the profile's own colours as swatches.
    fn theme_facts(&self, provider_dir: &Path) -> Vec<ThemeFact> {
        let Ok(raw) = crate::theme_media::read_theme_record(&provider_dir.join(PROFILE_FILE)) else {
            return Vec::new();
        };
        let Ok(profile) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return Vec::new();
        };
        let Some(controllers) = profile.get("controllers").and_then(|c| c.as_array()) else {
            return Vec::new();
        };

        let mut swatches: Vec<String> = Vec::new();
        for colour in controllers
            .iter()
            .filter_map(|c| c.get("colors").and_then(|colours| colours.as_array()))
            .flatten()
            .filter_map(|colour| colour.as_u64())
            .map(hex_colour)
        {
            if swatches.len() >= SWATCH_LIMIT {
                break;
            }
            if !swatches.contains(&colour) {
                swatches.push(colour);
            }
        }

        vec![ThemeFact {
            label: "RGB devices".to_string(),
            value: controllers.len().to_string(),
            swatches,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A stub `openrgb` in `dir`, with a shell body appended after the shebang.
    fn stub(dir: &Path, body: &str) -> String {
        let path = dir.join("openrgb");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write stub");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod stub");
        path.to_string_lossy().to_string()
    }

    fn theme_dir(dir: &Path) -> PathBuf {
        let theme = dir.join("Night");
        fs::create_dir_all(theme.join("providers")).expect("theme dir");
        theme
    }

    const PROFILE_ONE_DEVICE: &str = r#"{"controllers":[{"colors":[16711680,16711680,65280]}],"profile_name":"hve-capture"}"#;

    // ── identity ─────────────────────────────────────────────────────

    #[test]
    fn id_is_openrgb() {
        assert_eq!(OpenRgbProvider::new().id(), "openrgb");
    }

    #[test]
    fn identity_keys_are_declared() {
        let p = OpenRgbProvider::new();
        assert!(!p.display_name_key().is_empty());
        assert!(!p.icon().is_empty());
    }

    // ── capture (save) ───────────────────────────────────────────────

    #[test]
    fn save_moves_the_captured_profile_into_the_theme() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let profiles = tmp.path().join("profiles");
        fs::create_dir_all(&profiles).expect("profiles");
        let bin = stub(
            tmp.path(),
            &format!(
                "if [ \"$1\" = \"--save-profile\" ]; then\n  printf '%s' '{}' > \"{}/$2.json\"\nfi\nexit 0",
                PROFILE_ONE_DEVICE,
                profiles.display()
            ),
        );
        let theme = theme_dir(tmp.path());
        let p = OpenRgbProvider::with_paths(&bin, profiles.clone(), COMMAND_DEADLINE);

        p.save(&theme).expect("save never fails the theme");

        let stored = theme
            .join("providers")
            .join(PROVIDER_ID)
            .join(PROFILE_FILE);
        assert_eq!(fs::read_to_string(stored).expect("profile written"), PROFILE_ONE_DEVICE);
    }

    #[test]
    fn save_removes_the_transient_profile_it_asked_for() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let profiles = tmp.path().join("profiles");
        fs::create_dir_all(&profiles).expect("profiles");
        let bin = stub(
            tmp.path(),
            &format!(
                "printf '%s' '{}' > \"{}/$2.json\"; exit 0",
                PROFILE_ONE_DEVICE,
                profiles.display()
            ),
        );
        let theme = theme_dir(tmp.path());
        let p = OpenRgbProvider::with_paths(&bin, profiles.clone(), COMMAND_DEADLINE);

        p.save(&theme).expect("save");

        assert!(!profiles.join(format!("{CAPTURE_PROFILE}.json")).exists());
    }

    #[test]
    fn save_overwrites_a_previous_capture() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let profiles = tmp.path().join("profiles");
        fs::create_dir_all(&profiles).expect("profiles");
        let bin = stub(
            tmp.path(),
            &format!(
                "printf '%s' \"$(cat \"{}/next\")\" > \"{}/$2.json\"; exit 0",
                tmp.path().display(),
                profiles.display()
            ),
        );
        let theme = theme_dir(tmp.path());
        let p = OpenRgbProvider::with_paths(&bin, profiles, COMMAND_DEADLINE);

        fs::write(tmp.path().join("next"), PROFILE_ONE_DEVICE).expect("first state");
        p.save(&theme).expect("first save");

        let second = r#"{"controllers":[{"colors":[255]}],"profile_name":"hve-capture"}"#;
        fs::write(tmp.path().join("next"), second).expect("second state");
        p.save(&theme).expect("second save");

        let stored = theme
            .join("providers")
            .join(PROVIDER_ID)
            .join(PROFILE_FILE);
        assert_eq!(fs::read_to_string(stored).expect("profile"), second);
    }

    #[test]
    fn save_never_fails_the_theme_when_the_binary_is_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let theme = theme_dir(tmp.path());
        let p = OpenRgbProvider::with_paths(
            "/nonexistent/openrgb",
            tmp.path().join("profiles"),
            COMMAND_DEADLINE,
        );

        assert!(p.save(&theme).is_ok());
        assert!(!theme.join("providers").join(PROVIDER_ID).exists());
    }

    #[test]
    fn save_never_fails_the_theme_when_the_command_fails() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bin = stub(tmp.path(), "exit 7");
        let theme = theme_dir(tmp.path());
        let p = OpenRgbProvider::with_paths(&bin, tmp.path().join("profiles"), COMMAND_DEADLINE);

        assert!(p.save(&theme).is_ok());
    }

    #[test]
    fn save_never_fails_the_theme_when_the_captured_file_is_absent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bin = stub(tmp.path(), "exit 0");
        let theme = theme_dir(tmp.path());
        let profiles = tmp.path().join("profiles");
        let p = OpenRgbProvider::with_paths(&bin, profiles, COMMAND_DEADLINE);

        assert!(p.save(&theme).is_ok());
        assert!(!theme.join("providers").join(PROVIDER_ID).exists());
    }

    #[test]
    fn save_removes_the_transient_profile_even_when_the_command_fails() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let profiles = tmp.path().join("profiles");
        fs::create_dir_all(&profiles).expect("profiles");
        // Writes the profile, THEN fails: the leftover must still be removed,
        // which is what makes the documented cleanup unconditional.
        let bin = stub(
            tmp.path(),
            &format!(
                "printf '%s' '{}' > \"{}/$2.json\"; exit 1",
                PROFILE_ONE_DEVICE,
                profiles.display()
            ),
        );
        let theme = theme_dir(tmp.path());
        let p = OpenRgbProvider::with_paths(&bin, profiles.clone(), COMMAND_DEADLINE);

        assert!(p.save(&theme).is_ok());
        assert!(
            !profiles.join(format!("{CAPTURE_PROFILE}.json")).exists(),
            "a failed capture must not leave HVE scratch behind"
        );
    }

    #[test]
    fn a_hung_command_is_killed_at_the_deadline() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pid_file = tmp.path().join("child.pid");
        // Short sleep: long enough to outlive the deadline, short enough that a
        // leftover grandchild cannot linger in the test environment.
        let bin = stub(
            tmp.path(),
            &format!("echo $$ > {}; sleep 2", pid_file.display()),
        );
        let theme = theme_dir(tmp.path());
        let p = OpenRgbProvider::with_paths(
            &bin,
            tmp.path().join("profiles"),
            Duration::from_millis(120),
        );

        let started = Instant::now();
        assert!(p.save(&theme).is_ok());
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "a hung openrgb must not hold the theme transaction: {:?}",
            started.elapsed()
        );

        let pid = fs::read_to_string(&pid_file).expect("stub pid");
        let pid = pid.trim();
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "the child must be killed and reaped, not merely abandoned (pid {pid})"
        );
    }

    // ── apply ────────────────────────────────────────────────────────

    #[test]
    fn apply_stages_the_record_and_loads_it_by_name() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let profiles = tmp.path().join("profiles");
        fs::create_dir_all(&profiles).expect("profiles");
        let log = tmp.path().join("argv.log");
        let seen = tmp.path().join("staged.json");
        // Records the arguments AND the bytes OpenRGB was asked to load, so the
        // test proves the theme's own record reached the staged name.
        let bin = stub(
            tmp.path(),
            &format!(
                "printf '%s\\n' \"$@\" > {}\ncp \"{}/{}.json\" {} 2>/dev/null\nexit 0",
                log.display(),
                profiles.display(),
                APPLY_PROFILE,
                seen.display()
            ),
        );
        let theme = theme_dir(tmp.path());
        let provider_dir = theme.join("providers").join(PROVIDER_ID);
        fs::create_dir_all(&provider_dir).expect("provider dir");
        fs::write(provider_dir.join(PROFILE_FILE), PROFILE_ONE_DEVICE).expect("profile");
        let p = OpenRgbProvider::with_paths(&bin, profiles.clone(), COMMAND_DEADLINE);

        p.apply(&theme).expect("apply");

        let argv = fs::read_to_string(&log).expect("argv log");
        let mut lines = argv.lines();
        assert_eq!(lines.next(), Some("--profile"));
        assert_eq!(
            lines.next(),
            Some(APPLY_PROFILE),
            "OpenRGB resolves a NAME; an absolute path is a silent no-op"
        );
        assert_eq!(
            fs::read_to_string(&seen).expect("staged bytes"),
            PROFILE_ONE_DEVICE,
            "the staged profile must be the theme's own record"
        );
        assert!(
            !profiles.join(format!("{APPLY_PROFILE}.json")).exists(),
            "the staging file must not survive the apply"
        );
    }

    #[test]
    fn apply_is_a_quiet_noop_without_a_profile() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = tmp.path().join("argv.log");
        let bin = stub(
            tmp.path(),
            &format!("printf '%s\\n' \"$@\" > {}", log.display()),
        );
        let theme = theme_dir(tmp.path());
        let p = OpenRgbProvider::with_paths(&bin, tmp.path().join("profiles"), COMMAND_DEADLINE);

        assert!(p.apply(&theme).is_ok());
        assert!(!log.exists(), "no command may reach openrgb without a record");
    }

    #[test]
    fn apply_never_fails_the_theme_when_openrgb_fails() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bin = stub(tmp.path(), "exit 3");
        let theme = theme_dir(tmp.path());
        let provider_dir = theme.join("providers").join(PROVIDER_ID);
        fs::create_dir_all(&provider_dir).expect("provider dir");
        fs::write(provider_dir.join(PROFILE_FILE), PROFILE_ONE_DEVICE).expect("profile");
        let p = OpenRgbProvider::with_paths(&bin, tmp.path().join("profiles"), COMMAND_DEADLINE);

        assert!(p.apply(&theme).is_ok());
    }

    // ── detection ────────────────────────────────────────────────────

    #[test]
    fn the_recorded_port_is_used_when_present() {
        assert_eq!(
            server_port_from(r#"{"Server":{"default_port":7000}}"#),
            7000
        );
    }

    #[test]
    fn the_default_port_is_used_for_anything_unusable() {
        for raw in ["", "not json", r#"{"Server":{}}"#, r#"{"Server":{"default_port":80}}"#] {
            assert_eq!(server_port_from(raw), DEFAULT_SERVER_PORT, "raw: {raw}");
        }
    }

    #[test]
    fn a_listening_server_is_detected_and_a_closed_port_is_not() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        assert!(server_answers(port), "a live listener must answer");

        // A port nobody listens on: bound only to learn a free number, then
        // released WITHOUT ever connecting to it (a previous connection left in
        // the backlog keeps answering after the listener is dropped).
        let mut closed = None;
        for _ in 0..5 {
            let free = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let candidate = free.local_addr().expect("addr").port();
            drop(free);
            if !server_answers(candidate) {
                closed = Some(candidate);
                break;
            }
        }
        let closed = closed.expect("a released port must not answer");
        assert!(!server_answers(closed));
    }

    #[test]
    fn a_binary_that_answers_version_is_detected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bin = stub(tmp.path(), "exit 0");
        assert!(binary_answers(&bin, COMMAND_DEADLINE));

        let failing = stub(tmp.path(), "exit 1");
        assert!(!binary_answers(&failing, COMMAND_DEADLINE));
        assert!(!binary_answers("/nonexistent/openrgb", COMMAND_DEADLINE));
    }

    // ── panel facts ──────────────────────────────────────────────────

    #[test]
    fn facts_report_the_device_count_and_distinct_swatches() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join(PROFILE_FILE),
            r#"{"controllers":[{"colors":[16711680,16711680,65280]}]}"#,
        )
        .expect("profile");
        let p = OpenRgbProvider::new();

        let facts = p.theme_facts(tmp.path());

        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].value, "1");
        assert_eq!(facts[0].swatches, vec!["#ff0000", "#00ff00"]);
    }

    #[test]
    fn facts_are_empty_without_a_profile() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(OpenRgbProvider::new().theme_facts(tmp.path()).is_empty());
    }

    #[test]
    fn swatches_stop_at_the_limit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let colours: Vec<String> = (1..=10u64).map(|v| v.to_string()).collect();
        fs::write(
            tmp.path().join(PROFILE_FILE),
            format!(r#"{{"controllers":[{{"colors":[{}]}}]}}"#, colours.join(",")),
        )
        .expect("profile");

        let facts = OpenRgbProvider::new().theme_facts(tmp.path());
        assert_eq!(facts[0].swatches.len(), SWATCH_LIMIT);
    }

    /// The provider through the REAL theme transaction: the manager records
    /// the id in `meta.json`, the theme carries the bytes, and an apply hands
    /// the stored profile to OpenRGB. This is the seam the app uses.
    #[test]
    fn the_theme_manager_records_the_provider_and_restores_the_lighting() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let profiles = tmp.path().join("profiles");
        fs::create_dir_all(&profiles).expect("profiles");
        let log = tmp.path().join("argv.log");
        let bin = stub(
            tmp.path(),
            &format!(
                "if [ \"$1\" = \"--save-profile\" ]; then\n  printf '%s' '{}' > \"{}/$2.json\"\nelse\n  printf '%s\\n' \"$@\" >> {}\nfi\nexit 0",
                PROFILE_ONE_DEVICE,
                profiles.display(),
                log.display()
            ),
        );
        let mut manager = crate::theme_manager::ThemeManager::new(tmp.path());
        manager.register_provider(Box::new(OpenRgbProvider::with_paths(
            &bin,
            profiles,
            COMMAND_DEADLINE,
        )));

        let ids = manager.provider_ids();
        manager.save("Night", &ids).expect("theme save");

        let theme_dir = tmp.path().join("hve").join("themes").join("Night");
        let meta = fs::read_to_string(theme_dir.join("meta.json")).expect("meta");
        assert!(
            meta.contains(PROVIDER_ID),
            "the saved theme must record the provider: {meta}"
        );
        assert!(theme_dir
            .join("providers")
            .join(PROVIDER_ID)
            .join(PROFILE_FILE)
            .is_file());

        manager.apply("Night", || Ok(())).expect("theme apply");

        let argv = fs::read_to_string(&log).expect("apply reached openrgb");
        assert!(argv.contains("--profile"), "apply must load the profile: {argv}");
        assert!(
            argv.contains(APPLY_PROFILE),
            "apply must load by NAME, never by absolute path: {argv}"
        );
    }

    /// The shipped reference set is FROZEN: a sample must never carry a
    /// lighting record. The gallery hides a theme whose recorded providers are
    /// not all registered, so a sample repackaged from the keeper's folder
    /// would vanish for every user who does not run OpenRGB.
    #[test]
    fn the_shipped_themes_carry_no_lighting() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("themes");
        let offenders = lighting_records_under(&root);
        assert!(
            offenders.is_empty(),
            "the shipped themes must stay without a lighting record (see \
             assets/themes/README.md); offenders: {offenders:?}"
        );
    }

    /// The guard above must actually bite: a fixture that DOES hold the record
    /// is reported, so a silent pass cannot come from a scanner that reads
    /// nothing.
    #[test]
    fn the_shipped_theme_guard_detects_a_lighting_record() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let clean = tmp.path().join("Clean");
        fs::create_dir_all(clean.join("providers").join("hve-presets")).expect("clean theme");
        fs::write(
            clean.join("meta.json"),
            r#"{"providers":["noctalia-v5","hve-presets"]}"#,
        )
        .expect("clean meta");

        let dirty = tmp.path().join("Dirty");
        fs::create_dir_all(dirty.join("providers").join(PROVIDER_ID)).expect("dirty theme");
        fs::write(
            dirty.join("meta.json"),
            format!(r#"{{"providers":["noctalia-v5","{PROVIDER_ID}"]}}"#),
        )
        .expect("dirty meta");

        assert_eq!(lighting_records_under(tmp.path()), vec!["Dirty".to_string()]);
    }

    /// Theme folders under `root` that carry a lighting record, by name.
    fn lighting_records_under(root: &Path) -> Vec<String> {
        let Ok(entries) = fs::read_dir(root) else {
            return Vec::new();
        };
        let mut offenders: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter(|entry| {
                let dir = entry.path();
                let meta = fs::read_to_string(dir.join("meta.json")).unwrap_or_default();
                meta.contains(PROVIDER_ID) || dir.join("providers").join(PROVIDER_ID).exists()
            })
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        offenders.sort();
        offenders
    }

    /// Manual, machine-only check — NEVER part of the suite (`--ignored`).
    /// It drives the real running OpenRGB instance: capture the live lighting
    /// into a temporary theme and load it straight back. Both directions are
    /// non-destructive (the applied state is the one just captured), but the
    /// proof only means anything on a machine that actually owns the devices:
    ///
    /// `cargo test -- --ignored live_openrgb_round_trip`
    ///
    /// The barrier that matters: applying a captured theme must CHANGE the
    /// lighting. The earlier version of this test applied the state that was
    /// already live, so a no-op passed as a success — and that is exactly how
    /// `--profile <absolute path>` (a silent no-op in OpenRGB 1.1) went
    /// unnoticed. This version changes the lights first, restores them through
    /// the theme, and compares the result.
    #[test]
    #[ignore = "needs a running OpenRGB SDK server and real devices"]
    fn live_openrgb_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let provider = OpenRgbProvider::new();
        assert!(detected(), "a running OpenRGB SDK server must be detected");

        // 1. Capture the keeper's current lighting.
        let saved = theme_dir(tmp.path());
        provider.save(&saved).expect("capture");
        let saved_profile = saved.join("providers").join(PROVIDER_ID).join(PROFILE_FILE);
        assert!(saved_profile.is_file(), "the theme must hold the captured profile");
        let original = colours_of(&saved_profile);

        let facts = provider.theme_facts(&saved.join("providers").join(PROVIDER_ID));
        assert_eq!(facts.len(), 1);
        assert!(
            facts[0].value.parse::<usize>().unwrap_or(0) > 0,
            "at least one device must answer, got {:?}",
            facts[0].value
        );

        // 2. Move the lights somewhere else, through the real CLI.
        let changed = run_binary(
            "openrgb",
            &["--mode", "static", "--color", "FFFFFF"],
            COMMAND_DEADLINE,
        )
        .is_some();
        assert!(changed, "the white probe must at least run");

        let after_change = theme_dir(tmp.path().join("changed").as_path());
        provider.save(&after_change).expect("capture after the change");
        let moved = colours_of(
            &after_change.join("providers").join(PROVIDER_ID).join(PROFILE_FILE),
        );
        if moved == original {
            eprintln!(
                "live check skipped: no device accepted the static-white probe, so a \
                 restore cannot be distinguished from a no-op on this hardware"
            );
            return;
        }

        // 3. Restore through the theme, and prove the live state came back.
        provider.apply(&saved).expect("apply");
        let restored = theme_dir(tmp.path().join("restored").as_path());
        provider.save(&restored).expect("capture after the apply");
        let back = colours_of(
            &restored.join("providers").join(PROVIDER_ID).join(PROFILE_FILE),
        );

        assert_eq!(
            back, original,
            "applying the theme must put the captured lighting back"
        );
    }

    /// The colour list of every controller in a stored profile.
    fn colours_of(profile: &Path) -> Vec<Vec<u64>> {
        let raw = fs::read_to_string(profile).expect("profile readable");
        let parsed = serde_json::from_str::<serde_json::Value>(&raw).expect("profile json");
        parsed
            .get("controllers")
            .and_then(|c| c.as_array())
            .map(|controllers| {
                controllers
                    .iter()
                    .map(|controller| {
                        controller
                            .get("colors")
                            .and_then(|colours| colours.as_array())
                            .map(|colours| {
                                colours.iter().filter_map(|c| c.as_u64()).collect::<Vec<u64>>()
                            })
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
