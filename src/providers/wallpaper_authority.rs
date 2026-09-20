//! Single owner of "who controls the wallpaper right now".
//!
//! Two signals, each used for what it actually knows:
//! - The COMPOSITOR's layer stack (`hyprctl -j layers`) says WHICH layer
//!   is on top ("lo que se ve encima") — ground truth for visibility.
//! - The DAEMON (`skwd-helm` / `skwd-wall-v2` `outputs --json`) says the
//!   TYPE of its own layers (`"static"`, `"video"`, `"we"`).
//!
//! Painter names are never trusted for a kind: the skwd suite paints BOTH
//! static images and animated wallpapers (verified live:
//! `skwd-paper-v2-*` namespaces, `skwd-wall-vk` video renderer processes),
//! so a topmost skwd layer is attributed through the daemon's per-output
//! `type`, matched by output name against connected daemon outputs. No
//! match (or no daemon) vetoes the decision. A dedicated video painter
//! (e.g. `mpvpaper`) is video by nature, no daemon needed. Anything else
//! is unknown: no deletion, legacy capture, warn with namespace + pid.
//!
//! KNOWN LIMITATION (round-3 verification): a skwd-suite layer is typed by
//! the daemon's `type`, and that field can lag — the daemon's `current` is
//! provably stale at times (it reported one image while the painted layer
//! and `last-applied.json` held another). `enrich_with_proc_names`
//! deliberately skips skwd namespaces, so a skwd-native VIDEO painted under
//! a `skwd-*` namespace whose `type` still reads "static" would be
//! classified Static. Unproven live; it is the one input where the old
//! name-guessing failure mode could reappear. To settle it, capture
//! `hyprctl -j layers` and `skwd-helm outputs --json` back to back within a
//! second of applying a skwd-native video and compare the `type` against
//! what is actually rendered.
//!
//! Theme save consults this module so a theme captures ONLY the currently
//! active background instead of both.
//!
//! A different wallpaper manager later is a one-place change: extend
//! [`VIDEO_PAINTERS`] / [`SKWD_SUITE_MARKERS`] (namespace / process-name
//! substrings) and keep the pure parse/decide/plan functions below
//! unchanged.
//!
//! Timing budget: the AUTHORITY queries in this module (1 × compositor +
//! 2 × daemon attempts, each bounded by ≤250 ms) block at most ~750 ms
//! per save even with every helper hung (previously ~12 s: 2 saves ×
//! 2 binaries × 3 s; ~1.5 s across the dual main + gallery save in that
//! pathological case). That bound covers ONLY this module's queries: the
//! same `save()` additionally performs two UNBOUNDED `noctalia_msg` calls
//! (`color-scheme-get`, `wallpaper-get` — `noctalia_runtime.rs` uses a
//! bare `.output()` with no timeout), so a full panel save can block
//! longer than the authority budget when the Noctalia IPC itself hangs.
//! A live query answers in single-digit ms; expiry falls back to the
//! lossless capture-everything path, so tight bounds cost precision,
//! never data.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// Bounded wait for one `hyprctl -j layers` call, so a hung compositor
/// query can never freeze the save path (see the module docs for the math).
pub const COMPOSITOR_TIMEOUT: Duration = Duration::from_millis(250);

/// Bounded wait per authority binary (was 3 s — far too long for the UI
/// thread). Each attempt expires into the honest legacy capture, never a
/// guess and never a hang.
pub const AUTHORITY_TIMEOUT: Duration = Duration::from_millis(250);

/// Client preference order: `skwd-helm` first, then `skwd-wall-v2`.
/// Mirrors the delegation order in `noctalia.rs::delegate_to_skwd_walld`.
pub const AUTHORITY_BINARIES: [&str; 2] = ["skwd-helm", "skwd-wall-v2"];

/// The kind of background currently on screen, as reported by the authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallpaperKind {
    Static,
    Video,
}

/// One output entry from the authority's JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityOutput {
    pub name: String,
    pub kind: WallpaperKind,
    pub connected: bool,
    pub current: String,
}

/// What theme save must capture for the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavePlan {
    /// Authority says static: write the static record only, drop any video manifest.
    StaticOnly,
    /// Authority says video: write the video manifest only, drop any static record.
    VideoOnly,
    /// Authority missing/unreachable/timed-out/unparseable: keep the legacy
    /// behaviour (capture what is available). Never guess a kind.
    Unknown,
}

/// Raw shape of `skwd-helm outputs --json` / `current --json`:
/// `{ "outputs": [ { "name", "current", "path", "type", "connected", ... } ] }`.
#[derive(Debug, Deserialize)]
struct AuthorityDocument {
    #[serde(default)]
    outputs: Option<Vec<AuthorityEntry>>,
}

#[derive(Debug, Deserialize)]
struct AuthorityEntry {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    current: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    /// Absent `connected` means connected: the daemon always reports it, and
    /// dropping an output we cannot classify would silently lose data.
    #[serde(default = "connected_default")]
    connected: bool,
}

fn connected_default() -> bool {
    true
}

fn parse_kind(raw: Option<&str>, output_name: &str) -> Result<WallpaperKind, String> {
    match raw {
        Some("static") => Ok(WallpaperKind::Static),
        // "video" and "we" (Wallpaper Engine scenes) are both animated: a
        // static-only capture would silently lose them.
        Some("video") | Some("we") => Ok(WallpaperKind::Video),
        other => Err(format!(
            "output '{}' has unknown or missing type: {:?} (expected \"static\", \"video\" or \"we\")",
            output_name, other
        )),
    }
}

/// Parse the authority's JSON into per-output entries.
///
/// Every entry must carry a known `type` (`"static"` | `"video"` | `"we"`);
/// anything else — malformed JSON, missing outputs, unknown type words —
/// is an error, never a guess.
pub fn parse_authority_outputs(json: &str) -> Result<Vec<AuthorityOutput>, String> {
    let doc: AuthorityDocument =
        serde_json::from_str(json).map_err(|e| format!("Cannot parse authority JSON: {}", e))?;
    let entries = doc
        .outputs
        .ok_or_else(|| "Authority JSON has no \"outputs\" array".to_string())?;
    entries
        .into_iter()
        .map(|e| {
            let name = e.name.clone().unwrap_or_default();
            let kind = parse_kind(e.kind.as_deref(), &name)?;
            let current = e
                .current
                .or(e.path)
                .unwrap_or_default();
            Ok(AuthorityOutput {
                name,
                kind,
                connected: e.connected,
                current,
            })
        })
        .collect()
}

/// Decide the single active background kind.
///
/// Multi-output rule: only CONNECTED outputs vote; if ANY connected output
/// reports video, the kind is video. A running video sits on the compositor
/// overlay above the static layer, so the visible background is the video —
/// a static-only capture would silently lose it. No connected outputs (or an
/// empty list) yields `None`, which maps to [`SavePlan::Unknown`].
pub fn decide_wallpaper_kind(outputs: &[AuthorityOutput]) -> Option<WallpaperKind> {
    let mut saw_static = false;
    for o in outputs {
        if !o.connected {
            continue;
        }
        match o.kind {
            WallpaperKind::Video => return Some(WallpaperKind::Video),
            WallpaperKind::Static => saw_static = true,
        }
    }
    if saw_static {
        Some(WallpaperKind::Static)
    } else {
        None
    }
}

/// Map a decided kind to what save must capture. `None` (no usable signal)
/// preserves the legacy capture-everything behaviour — never guess.
pub fn plan_from_kind(kind: Option<WallpaperKind>) -> SavePlan {
    match kind {
        Some(WallpaperKind::Static) => SavePlan::StaticOnly,
        Some(WallpaperKind::Video) => SavePlan::VideoOnly,
        None => SavePlan::Unknown,
    }
}

/// Painters that are video BY NATURE (dedicated video renderers): a
/// topmost opaque layer from one of these means a live video is visible,
/// no daemon signal needed.
///
/// Substring match, so versioned or per-output namespaces (e.g.
/// `mpvpaper-HDMI-A-1`) still classify. To support a new dedicated video
/// painter, add its layer namespace and/or process name here — that is
/// the whole change. Verified live on the keeper's machine: `mpvpaper` is
/// Noctalia's video plugin (video only). Anything unlisted classifies as
/// `None`: an UNKNOWN painter is never guessed, the caller falls back
/// honestly and logs the namespace and pid.
const VIDEO_PAINTERS: [&str; 1] = ["mpvpaper"];

/// Substring markers for layers owned by the skwd suite (`skwd-paper`,
/// `skwd-wall`, versioned `skwd-paper-v2-*` namespaces, the `skwd-wall-vk`
/// video renderer process). The suite paints BOTH static and animated
/// wallpapers, so ownership only routes the layer to daemon attribution
/// ([`decide_active_kind`]) — it never decides a kind by itself.
const SKWD_SUITE_MARKERS: [&str; 1] = ["skwd"];

/// Owner of one topmost background layer: a dedicated video painter, the
/// skwd suite (kind comes from the daemon), or unknown (never guessed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerOwner {
    Video,
    Skwd,
    Unknown,
}

/// Attribute one topmost background layer to its owner: the layer's
/// Wayland namespace first, the pid's process name (`/proc/<pid>/comm`)
/// as fallback for generic or versioned namespaces. Video is checked
/// first so a painter matching both tables classifies as video — a live
/// video on top is what the viewer sees.
fn layer_owner(namespace: &str, proc_name: Option<&str>) -> LayerOwner {
    for haystack in [Some(namespace), proc_name].into_iter().flatten() {
        if VIDEO_PAINTERS.iter().any(|p| haystack.contains(p)) {
            return LayerOwner::Video;
        }
    }
    for haystack in [Some(namespace), proc_name].into_iter().flatten() {
        if SKWD_SUITE_MARKERS.iter().any(|p| haystack.contains(p)) {
            return LayerOwner::Skwd;
        }
    }
    LayerOwner::Unknown
}

/// True for layers owned by the skwd suite (see [`SKWD_SUITE_MARKERS`]).
fn is_skwd_suite(namespace: &str) -> bool {
    SKWD_SUITE_MARKERS.iter().any(|p| namespace.contains(p))
}

/// Classify the topmost background layer by its painter: the layer's
/// Wayland namespace first, the pid's process name (`/proc/<pid>/comm`)
/// as fallback for generic or versioned namespaces.
///
/// Only dedicated video painters decide a kind here. Skwd-suite layers
/// return `None`: their kind comes from the daemon via
/// [`decide_active_kind`], never from the name.
pub fn classify_painter(namespace: &str, proc_name: Option<&str>) -> Option<WallpaperKind> {
    match layer_owner(namespace, proc_name) {
        LayerOwner::Video => Some(WallpaperKind::Video),
        LayerOwner::Skwd | LayerOwner::Unknown => None,
    }
}

/// One background layer entry from `hyprctl -j layers`. Only `alpha`,
/// `namespace` and `pid` matter here; the rest is ignored. A missing
/// `alpha` defaults to 0.0 (transparent): unconfirmed opacity is not
/// trusted, the entry is skipped rather than guessed.
#[derive(Debug, Deserialize)]
struct LayerEntry {
    #[serde(default)]
    alpha: f32,
    #[serde(default)]
    namespace: String,
    #[serde(default)]
    pid: u32,
}

/// One monitor from `hyprctl -j layers`: `{ "levels": { "0": [...] } }`.
/// Level `"0"` is the background level; within it, LATER entries draw ON
/// TOP. A missing `"0"` key means no background signal for that output.
#[derive(Debug, Deserialize)]
struct LayersOutput {
    #[serde(default)]
    levels: HashMap<String, Vec<LayerEntry>>,
}

/// What the compositor shows on top for one output. `kind` is `None` when
/// there is no opaque background layer or the topmost painter is unknown —
/// both map to [`SavePlan::Unknown`] (capture everything, never guess).
/// `top_namespace` / `top_pid` name the painter for the honest log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositorOutput {
    pub name: String,
    pub kind: Option<WallpaperKind>,
    pub top_namespace: String,
    pub top_pid: u32,
}

/// Parse the compositor's layer JSON into per-output topmost layers.
///
/// Pure and fixture-driven: classification uses the namespace only. Only
/// dedicated video painters yield a kind here; skwd-suite layers yield
/// `None` — their kind comes from the daemon via [`decide_active_kind`],
/// never from the name. The live query ([`query_active_wallpaper_kind`])
/// additionally resolves the pid's process name for topmost layers the
/// namespace alone cannot classify — `/proc` access must never leak into
/// the pure path, or tests would depend on the machine's live process
/// table.
pub fn parse_compositor_layers(json: &str) -> Result<Vec<CompositorOutput>, String> {
    let doc: HashMap<String, LayersOutput> =
        serde_json::from_str(json).map_err(|e| format!("Cannot parse layers JSON: {}", e))?;
    let mut outputs: Vec<CompositorOutput> = doc
        .into_iter()
        .map(|(name, out)| {
            let top = out
                .levels
                .get("0")
                .and_then(|entries| entries.iter().filter(|e| e.alpha > 0.0).last());
            match top {
                Some(entry) => CompositorOutput {
                    name,
                    kind: classify_painter(&entry.namespace, None),
                    top_namespace: entry.namespace.clone(),
                    top_pid: entry.pid,
                },
                None => CompositorOutput {
                    name,
                    kind: None,
                    top_namespace: String::new(),
                    top_pid: 0,
                },
            }
        })
        .collect();
    outputs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(outputs)
}

/// Attribute one topmost skwd-suite layer to a daemon output.
///
/// Exact match on output name against CONNECTED daemon outputs only: the
/// compositor's output name is the only identity both signals share (a
/// layer entry carries no path to match against). A disconnected match,
/// a name with no daemon entry, or no daemon data at all vetoes the whole
/// decision (`None` → [`SavePlan::Unknown`]) — never a name-based guess.
fn attribute_skwd_layer(output_name: &str, daemon: &[AuthorityOutput]) -> Option<WallpaperKind> {
    daemon
        .iter()
        .find(|o| o.connected && o.name == output_name)
        .map(|o| o.kind)
}

/// Decide the single active background kind from both parsed signals.
///
/// `compositor` is the `hyprctl -j layers` stack (see
/// [`parse_compositor_layers`]); `daemon` the `outputs --json` entries
/// (empty when the daemon is unavailable — which then vetoes every skwd
/// layer, since an unattributable skwd layer is never guessed). Per
/// output, topmost opaque layer:
/// - dedicated video painter (or a video kind the live query confirmed
///   via the pid's process name) → video, no daemon needed;
/// - skwd suite → the daemon's `type` for the same output name;
/// - anything else (unknown painter, no opaque layer) → veto (`None`).
///
/// Multi-output rule: any video wins; any veto sinks the whole decision.
/// No outputs at all yields `None`.
pub fn decide_active_kind_from_outputs(
    compositor: &[CompositorOutput],
    daemon: &[AuthorityOutput],
) -> Option<WallpaperKind> {
    if compositor.is_empty() {
        return None;
    }
    let mut saw_static = false;
    for o in compositor {
        if o.kind == Some(WallpaperKind::Video) {
            return Some(WallpaperKind::Video);
        }
        if o.top_namespace.is_empty() {
            return None;
        }
        match layer_owner(&o.top_namespace, None) {
            LayerOwner::Video => return Some(WallpaperKind::Video),
            LayerOwner::Skwd => match attribute_skwd_layer(&o.name, daemon) {
                Some(WallpaperKind::Video) => return Some(WallpaperKind::Video),
                Some(WallpaperKind::Static) => saw_static = true,
                None => return None,
            },
            LayerOwner::Unknown => return None,
        }
    }
    if saw_static {
        Some(WallpaperKind::Static)
    } else {
        None
    }
}

/// Decide the single active background kind from both JSON signals.
///
/// Pure and fixture-driven: `None` compositor JSON (compositor
/// unreachable), unparseable compositor JSON, and unattributable skwd
/// layers (daemon missing, unparseable, or silent for that output) all
/// yield `None` → [`SavePlan::Unknown`]. The daemon alone NEVER decides:
/// it only types skwd-suite layers the compositor already proved are on
/// top.
pub fn decide_active_kind(
    compositor_json: Option<&str>,
    daemon_json: Option<&str>,
) -> Option<WallpaperKind> {
    let compositor = parse_compositor_layers(compositor_json?).ok()?;
    let daemon: Vec<AuthorityOutput> = daemon_json
        .and_then(|json| parse_authority_outputs(json).ok())
        .unwrap_or_default();
    decide_active_kind_from_outputs(&compositor, &daemon)
}

/// Best-effort process name for a layer pid (`/proc/<pid>/comm`).
/// `None` on any failure — the caller treats it as "no fallback signal".
fn proc_name_for_pid(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{}/comm", pid))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Run one helper command with a bounded wait.
///
/// The JSON payloads here are a few KB (well under the pipe buffer), so
/// polling `try_wait` cannot deadlock on a full pipe.
fn run_bounded_command(binary: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = std::process::Command::new(binary)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run {}: {}", binary, e))?;
    let start = Instant::now();
    loop {
        match child
            .try_wait()
            .map_err(|e| format!("{} wait failed: {}", binary, e))?
        {
            Some(_) => {
                let out = child
                    .wait_with_output()
                    .map_err(|e| format!("{} output read failed: {}", binary, e))?;
                if out.status.success() {
                    return Ok(String::from_utf8_lossy(&out.stdout).to_string());
                }
                return Err(format!(
                    "{} failed: {}",
                    binary,
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{} timed out after {:?}", binary, timeout));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

/// Run one authority binary with a bounded wait.
fn run_authority_query(binary: &str, timeout: Duration) -> Result<String, String> {
    run_bounded_command(binary, &["outputs", "--json"], timeout)
}

/// Second chance for topmost layers the namespace alone cannot classify:
/// resolve the pid's process name (`/proc/<pid>/comm`) for a dedicated
/// video painter hiding behind a generic namespace. Skwd-suite namespaces
/// skip the lookup (their kind comes from the daemon regardless), and
/// still-unknown painters are logged with namespace and pid — never
/// guessed. `/proc` access lives only in this live helper, never in the
/// pure parse/decide path.
fn enrich_with_proc_names(outputs: &mut [CompositorOutput]) {
    for o in outputs.iter_mut() {
        if o.kind.is_none() && !o.top_namespace.is_empty() && !is_skwd_suite(&o.top_namespace) {
            if let Some(proc) = proc_name_for_pid(o.top_pid) {
                o.kind = classify_painter(&o.top_namespace, Some(&proc));
            }
            if o.kind.is_none() {
                tracing::warn!(
                    "[wallpaper] Unclassifiable topmost background layer \
                     on '{}': namespace='{}' pid={} — not guessing a kind",
                    o.name,
                    o.top_namespace,
                    o.top_pid,
                );
            }
        }
    }
}

/// Fetch the wallpaper daemon's raw `outputs --json` (secondary signal:
/// it only knows the TYPE of its own layers, never which one is on top).
///
/// Tries [`AUTHORITY_BINARIES`] in order (`skwd-helm` first, then
/// `skwd-wall-v2`) and returns the raw JSON from the first binary that
/// yields parseable output with at least one connected output. `None`
/// (with a debug log naming every reason) when the daemon is unavailable —
/// which vetoes every skwd layer downstream, never a guess. Each attempt
/// is bounded by [`AUTHORITY_TIMEOUT`]; never hangs past
/// `2 * AUTHORITY_TIMEOUT`.
fn query_daemon_json() -> Option<String> {
    let mut reasons = Vec::new();
    for binary in AUTHORITY_BINARIES {
        match run_authority_query(binary, AUTHORITY_TIMEOUT) {
            Ok(json) => match parse_authority_outputs(&json) {
                Ok(outputs) => {
                    if outputs.iter().any(|o| o.connected) {
                        return Some(json);
                    }
                    reasons.push(format!("{}: no connected outputs", binary));
                }
                Err(e) => reasons.push(format!("{}: {}", binary, e)),
            },
            Err(e) => reasons.push(e),
        }
    }
    tracing::debug!(
        "[wallpaper] Daemon type signal unavailable ({}); skwd layers stay unattributed",
        reasons.join("; "),
    );
    None
}

/// Log when the daemon's own view disagrees with the decided kind. The
/// compositor always wins — it shows what is actually on top.
fn log_daemon_disagreement(daemon_json: Option<&str>, kind: WallpaperKind) {
    let daemon_outputs: Vec<AuthorityOutput> = daemon_json
        .and_then(|json| parse_authority_outputs(json).ok())
        .unwrap_or_default();
    if let Some(daemon_kind) = decide_wallpaper_kind(&daemon_outputs) {
        if daemon_kind != kind {
            tracing::warn!(
                "[wallpaper] Compositor says {:?} but daemon says {:?} — \
                 compositor wins (it shows what is on top)",
                kind,
                daemon_kind,
            );
        }
    }
}

/// Ask which background is active right now: the COMPOSITOR says which
/// layer is on top, the daemon says the type of the skwd suite's layers
/// (see [`decide_active_kind`]).
///
/// Pure fast path first (no `/proc` touch): it decides whenever every
/// topmost layer is a dedicated video painter or an attributable skwd
/// layer. Only when an unknown painter vetoes that path are pid process
/// names resolved for a second chance before giving up.
///
/// If the compositor query fails, an `Err` naming the reason is returned
/// so the caller keeps the legacy capture-everything behaviour. The
/// daemon alone NEVER decides: it is blind to other painters (verified
/// live reporting "static" while a video covered the screen), so a
/// compositor outage backfilled from the daemon would resurrect the very
/// bug this module exists to kill. Never panics; this module's queries
/// never hang past `COMPOSITOR_TIMEOUT + 2 * AUTHORITY_TIMEOUT`
/// (≈750 ms per save).
pub fn query_active_wallpaper_kind() -> Result<WallpaperKind, String> {
    let compositor_json = run_bounded_command("hyprctl", &["-j", "layers"], COMPOSITOR_TIMEOUT)
        .map_err(|e| format!("compositor unavailable: {}", e))?;
    let daemon_json = query_daemon_json();
    if let Some(kind) = decide_active_kind(Some(&compositor_json), daemon_json.as_deref()) {
        log_daemon_disagreement(daemon_json.as_deref(), kind);
        return Ok(kind);
    }
    let mut outputs = parse_compositor_layers(&compositor_json)?;
    enrich_with_proc_names(&mut outputs);
    let daemon_outputs: Vec<AuthorityOutput> = daemon_json
        .as_deref()
        .and_then(|json| parse_authority_outputs(json).ok())
        .unwrap_or_default();
    let kind = decide_active_kind_from_outputs(&outputs, &daemon_outputs).ok_or_else(|| {
        "compositor layer stack has no attributable opaque background layer".to_string()
    })?;
    log_daemon_disagreement(daemon_json.as_deref(), kind);
    Ok(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn static_json() -> &'static str {
        r#"{"outputs": [
            {"name": "DP-3", "current": "/pic/joker1.png", "path": "/pic/joker1.png",
             "type": "static", "connected": true},
            {"name": "HDMI-A-1", "current": "/pic/joker1.png", "path": "/pic/joker1.png",
             "type": "static", "connected": true}
        ]}"#
    }

    fn video_json() -> &'static str {
        r#"{"outputs": [
            {"name": "DP-3", "current": "/vid/loop.mp4", "path": "/vid/loop.mp4",
             "type": "video", "connected": true}
        ]}"#
    }

    #[test]
    fn parses_static_outputs() {
        let outputs = parse_authority_outputs(static_json()).expect("static JSON must parse");
        assert_eq!(outputs.len(), 2);
        assert!(outputs.iter().all(|o| o.kind == WallpaperKind::Static));
        assert_eq!(outputs[0].current, "/pic/joker1.png");
    }

    #[test]
    fn parses_video_output() {
        let outputs = parse_authority_outputs(video_json()).expect("video JSON must parse");
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].kind, WallpaperKind::Video);
        assert_eq!(outputs[0].current, "/vid/loop.mp4");
    }

    #[test]
    fn parses_we_type_as_video() {
        // Wallpaper Engine scenes are animated: they must capture as video.
        let outputs = parse_authority_outputs(
            r#"{"outputs": [
                {"name": "DP-3", "current": "we://scene", "path": "we://scene",
                 "type": "we", "connected": true}
            ]}"#,
        )
        .expect("we JSON must parse");
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].kind, WallpaperKind::Video);
    }

    #[test]
    fn agreeing_static_outputs_decide_static() {
        let outputs = parse_authority_outputs(static_json()).unwrap();
        assert_eq!(decide_wallpaper_kind(&outputs), Some(WallpaperKind::Static));
    }

    #[test]
    fn agreeing_video_outputs_decide_video() {
        let outputs = parse_authority_outputs(video_json()).unwrap();
        assert_eq!(decide_wallpaper_kind(&outputs), Some(WallpaperKind::Video));
    }

    #[test]
    fn disagreeing_outputs_decide_video() {
        // Multi-output rule: any connected video wins (see decide_wallpaper_kind docs).
        let outputs = parse_authority_outputs(
            r#"{"outputs": [
                {"name": "DP-3", "current": "/vid/loop.mp4", "path": "/vid/loop.mp4",
                 "type": "video", "connected": true},
                {"name": "HDMI-A-1", "current": "/pic/a.png", "path": "/pic/a.png",
                 "type": "static", "connected": true}
            ]}"#,
        )
        .unwrap();
        assert_eq!(decide_wallpaper_kind(&outputs), Some(WallpaperKind::Video));
    }

    #[test]
    fn disconnected_outputs_are_ignored() {
        // Only a disconnected video: the connected static output decides.
        let outputs = parse_authority_outputs(
            r#"{"outputs": [
                {"name": "FALLBACK", "current": "/vid/loop.mp4", "path": "/vid/loop.mp4",
                 "type": "video", "connected": false},
                {"name": "DP-3", "current": "/pic/a.png", "path": "/pic/a.png",
                 "type": "static", "connected": true}
            ]}"#,
        )
        .unwrap();
        assert_eq!(decide_wallpaper_kind(&outputs), Some(WallpaperKind::Static));
    }

    #[test]
    fn empty_outputs_decide_nothing() {
        let outputs = parse_authority_outputs(r#"{"outputs": []}"#).unwrap();
        assert!(outputs.is_empty());
        assert_eq!(decide_wallpaper_kind(&outputs), None);
        assert_eq!(plan_from_kind(None), SavePlan::Unknown);
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_authority_outputs("not json at all").is_err());
        assert!(parse_authority_outputs(r#"{"outputs": [{"name": "DP-3"}]}"#).is_err());
    }

    #[test]
    fn plan_maps_kind_to_capture() {
        assert_eq!(plan_from_kind(Some(WallpaperKind::Static)), SavePlan::StaticOnly);
        assert_eq!(plan_from_kind(Some(WallpaperKind::Video)), SavePlan::VideoOnly);
        assert_eq!(plan_from_kind(None), SavePlan::Unknown);
    }

    #[test]
    fn authority_prefers_skwd_helm_then_wall_v2() {
        assert_eq!(AUTHORITY_BINARIES, ["skwd-helm", "skwd-wall-v2"]);
    }

    #[test]
    fn authority_timeout_is_bounded() {
        // UI-thread budget (see the module docs for the math): the old 3 s
        // bound let one hung daemon freeze the panel for seconds — ~12 s
        // worst case across the dual save. Every helper command is now
        // bounded by ≤500 ms.
        assert!(!AUTHORITY_TIMEOUT.is_zero());
        assert!(AUTHORITY_TIMEOUT <= Duration::from_millis(500));
    }

    /// Save must consult the authority: the noctalia-v5 save path calls the
    /// single query function and branches on the plan, so a future manager
    /// swap stays a one-place change. Comment lines are stripped first so a
    /// commented-out call cannot satisfy this.
    #[test]
    fn noctalia_v5_save_consults_the_authority_by_construction() {
        let src = std::fs::read_to_string("src/providers/noctalia.rs")
            .expect("src/providers/noctalia.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            code.contains("query_active_wallpaper_kind"),
            "noctalia.rs save must call query_active_wallpaper_kind"
        );
        assert!(
            code.contains("SavePlan::StaticOnly") && code.contains("SavePlan::VideoOnly"),
            "noctalia.rs save must branch on both SavePlan arms"
        );
    }

    // ── Compositor layer-stack tests ──
    //
    // `hyprctl -j layers` is the source of truth for which background is
    // actually on top ("lo que se ve encima"). The daemon (`skwd-helm`)
    // only knows its own last-set wallpaper and is blind to other painters
    // (verified live: daemon said "static" while mpvpaper's video covered
    // the screen). Within background level "0", LATER entries draw ON TOP.

    fn layer(namespace: &str, alpha: f32, pid: u32) -> String {
        format!(
            r#"{{"address":"0x1","x":320,"y":1440,"w":1440,"h":900,"alpha":{},"namespace":"{}","pid":{}}}"#,
            alpha, namespace, pid
        )
    }

    fn layers_doc(hdmi_bg: &str, dp_bg: &str) -> String {
        format!(
            r#"{{"HDMI-A-1":{{"levels":{{"0":[{}],"1":[],"2":[]}}}},"DP-3":{{"levels":{{"0":[{}],"1":[]}}}}}}"#,
            hdmi_bg, dp_bg
        )
    }

    #[test]
    fn compositor_video_on_top_decides_video() {
        // mpvpaper is video by nature: no daemon signal needed, even when
        // the daemon insists everything is static.
        let json = layers_doc(
            &format!("{},{}", layer("skwd-paper", 1.0, 439101), layer("mpvpaper", 1.0, 507331)),
            &layer("skwd-paper", 1.0, 439101),
        );
        let daemon = r#"{"outputs": [
            {"name": "HDMI-A-1", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true},
            {"name": "DP-3", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true}
        ]}"#;
        assert_eq!(
            decide_active_kind(Some(&json), Some(daemon)),
            Some(WallpaperKind::Video)
        );
    }

    #[test]
    fn compositor_static_covering_video_defers_to_daemon() {
        // Same painters, reversed paint order: the skwd layer is on top,
        // so its kind comes from the daemon — a static report decides
        // static, and a video report would decide video (never the name).
        let json = layers_doc(
            &format!("{},{}", layer("mpvpaper", 1.0, 507331), layer("skwd-paper", 1.0, 439101)),
            &layer("skwd-paper", 1.0, 439101),
        );
        let outputs = parse_compositor_layers(&json).unwrap();
        let hdmi = outputs.iter().find(|o| o.name == "HDMI-A-1").unwrap();
        assert_eq!(hdmi.top_namespace, "skwd-paper");
        assert_eq!(hdmi.kind, None);
        let daemon = r#"{"outputs": [
            {"name": "HDMI-A-1", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true},
            {"name": "DP-3", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true}
        ]}"#;
        assert_eq!(
            decide_active_kind(Some(&json), Some(daemon)),
            Some(WallpaperKind::Static)
        );
    }

    #[test]
    fn compositor_unknown_painter_decides_nothing() {
        // An unclassifiable topmost layer must NOT be guessed: no kind,
        // so save keeps the legacy capture-everything behaviour. The
        // namespace is kept for the honest log line.
        let json = layers_doc(
            &format!("{},{}", layer("skwd-paper", 1.0, 439101), layer("mystery-layer", 1.0, 999)),
            &layer("skwd-paper", 1.0, 439101),
        );
        let outputs = parse_compositor_layers(&json).unwrap();
        let hdmi = outputs.iter().find(|o| o.name == "HDMI-A-1").unwrap();
        assert_eq!(hdmi.kind, None);
        assert_eq!(hdmi.top_namespace, "mystery-layer");
        // DP-3 attributes to static but HDMI is unknown: an unknown
        // topmost layer vetoes the whole decision even when the daemon is
        // decisive (it only knows its OWN layers — guessing static would
        // delete the live record of whatever the mystery painter shows).
        let daemon = daemon_kind_doc("static");
        assert_eq!(decide_active_kind(Some(&json), Some(&daemon)), None);
        assert_eq!(plan_from_kind(None), SavePlan::Unknown);
    }

    #[test]
    fn compositor_empty_background_level_decides_nothing() {
        let json = layers_doc("", "");
        let outputs = parse_compositor_layers(&json).unwrap();
        assert!(outputs.iter().all(|o| o.kind.is_none()));
        let daemon = daemon_kind_doc("static");
        assert_eq!(decide_active_kind(Some(&json), Some(&daemon)), None);
    }

    #[test]
    fn compositor_missing_level_key_decides_nothing() {
        // An output with no background level ("0") carries no signal —
        // treated as unknown, never an error and never a guess.
        let json = r#"{"HDMI-A-1": {"levels": {"1": [], "2": []}}}"#;
        let outputs = parse_compositor_layers(json).unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].kind, None);
        let daemon = daemon_kind_doc("static");
        assert_eq!(decide_active_kind(Some(json), Some(&daemon)), None);
    }

    #[test]
    fn compositor_malformed_json_is_an_error() {
        assert!(parse_compositor_layers("not json at all").is_err());
        assert!(parse_compositor_layers(r#"[1, 2, 3]"#).is_err());
    }

    #[test]
    fn compositor_per_output_disagreement_decides_video() {
        // Multi-output rule (same as the daemon policy): any output whose
        // topmost opaque layer is video means a live video is visible.
        let json = layers_doc(
            &layer("skwd-paper", 1.0, 439101),
            &format!("{},{}", layer("skwd-paper", 1.0, 439101), layer("mpvpaper", 1.0, 507331)),
        );
        let daemon = r#"{"outputs": [
            {"name": "HDMI-A-1", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true},
            {"name": "DP-3", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true}
        ]}"#;
        assert_eq!(
            decide_active_kind(Some(&json), Some(daemon)),
            Some(WallpaperKind::Video)
        );
    }

    #[test]
    fn compositor_all_outputs_static_decides_static() {
        // Skwd layers on top everywhere + a static daemon report for both
        // outputs: attribution by output name decides static.
        let json = layers_doc(
            &layer("skwd-paper", 1.0, 439101),
            &layer("skwd-paper", 1.0, 439101),
        );
        let daemon = r#"{"outputs": [
            {"name": "HDMI-A-1", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true},
            {"name": "DP-3", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true}
        ]}"#;
        assert_eq!(
            decide_active_kind(Some(&json), Some(daemon)),
            Some(WallpaperKind::Static)
        );
    }

    #[test]
    fn compositor_live_shape_regression() {
        // The keeper's exact live shape: daemon reported static for both
        // outputs while the compositor showed mpvpaper ON TOP of
        // skwd-paper at the background level. The compositor must win.
        let json = r#"{
          "HDMI-A-1": {"levels": {
            "0": [
              {"address":"0x55fe0399b650","x":320,"y":1440,"w":1440,"h":900,"alpha":1,"namespace":"skwd-paper","pid":439101},
              {"address":"0x55fe01d488e0","x":320,"y":1440,"w":1440,"h":900,"alpha":1,"namespace":"mpvpaper","pid":507331}
            ],
            "1": [],
            "2": [{"address":"0x1","alpha":1,"namespace":"noctalia-bar-default","pid":250289}]
          }},
          "DP-3": {"levels": {
            "0": [
              {"address":"0x55fe0399b650","x":320,"y":1440,"w":1440,"h":900,"alpha":1,"namespace":"skwd-paper","pid":439101},
              {"address":"0x55fe01d488e0","x":320,"y":1440,"w":1440,"h":900,"alpha":1,"namespace":"mpvpaper","pid":507331}
            ],
            "1": [],
            "2": []
          }}
        }"#;
        let outputs = parse_compositor_layers(json).expect("live shape must parse");
        assert_eq!(outputs.len(), 2);
        assert!(
            outputs.iter().all(|o| o.kind == Some(WallpaperKind::Video)),
            "video on top on every output must decide video, got {outputs:?}"
        );
        assert_eq!(decide_active_kind(Some(json), None), Some(WallpaperKind::Video));
    }

    #[test]
    fn classify_painter_maps_known_painters() {
        assert_eq!(classify_painter("mpvpaper", None), Some(WallpaperKind::Video));
        // Skwd-suite layers never classify by name: their kind comes from
        // the daemon (the suite paints animated wallpapers too).
        assert_eq!(classify_painter("skwd-paper", None), None);
        assert_eq!(classify_painter("skwd-paper-v2-HDMI-A-1", None), None);
        assert_eq!(classify_painter("mystery-layer", None), None);
        // The pid's process name is the fallback signal when the
        // namespace is generic or versioned.
        assert_eq!(
            classify_painter("mystery-layer", Some("mpvpaper")),
            Some(WallpaperKind::Video)
        );
        assert_eq!(classify_painter("mystery-layer", Some("skwd-wall-vk")), None);
    }

    #[test]
    fn skwd_suite_membership_routes_to_daemon() {
        // Every verified skwd-suite identity (namespaces and process
        // names) must route to daemon attribution — never to a kind, and
        // never to the unknown-painter veto.
        for name in [
            "skwd-paper",
            "skwd-wall",
            "skwd-paper-v2-HDMI-A-1",
            "skwd-paper-v2-DP-3",
        ] {
            assert_eq!(layer_owner(name, None), LayerOwner::Skwd, "{}", name);
        }
        assert_eq!(layer_owner("wallpaper", Some("skwd-paper-v2")), LayerOwner::Skwd);
        assert_eq!(layer_owner("wallpaper", Some("skwd-wall-vk")), LayerOwner::Skwd);
        assert_eq!(layer_owner("mpvpaper", None), LayerOwner::Video);
        assert_eq!(layer_owner("mystery-layer", None), LayerOwner::Unknown);
    }

    #[test]
    fn compositor_timeout_is_small() {
        // ≤500 ms per command: the save path runs on the Slint UI thread,
        // so a hung `hyprctl` must never freeze the panel.
        assert!(!COMPOSITOR_TIMEOUT.is_zero());
        assert!(COMPOSITOR_TIMEOUT <= Duration::from_millis(500));
    }

    /// The active kind comes from the COMPOSITOR's layer stack, typed by
    /// the daemon for skwd layers only: the wiring must query the
    /// compositor (`hyprctl`) inside the single active-kind entry point
    /// and decide through `decide_active_kind` — and must NEVER backfill
    /// a dead compositor query from the daemon (that fallback resurrected
    /// the v1 bug). Comment lines are stripped first so a commented-out
    /// call cannot satisfy this (block/trailing comments are not
    /// stripped).
    #[test]
    fn active_kind_comes_from_the_compositor_by_construction() {
        let src = std::fs::read_to_string("src/providers/wallpaper_authority.rs")
            .expect("src/providers/wallpaper_authority.rs must exist");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            code.contains("query_active_wallpaper_kind"),
            "wallpaper_authority must expose a single active-kind entry point"
        );
        assert!(
            code.contains("hyprctl"),
            "the compositor query must run `hyprctl -j layers`"
        );
        assert!(
            code.contains("decide_active_kind"),
            "the decision must go through the combined compositor+daemon function"
        );
        // The forbidden fallback message is assembled via concat so this
        // very assertion cannot satisfy the search itself.
        let daemon_fallback = ["falling back", " to daemon"].concat();
        assert!(
            !code.contains(&daemon_fallback),
            "a dead compositor query must never fall back to the daemon"
        );
    }

    #[test]
    fn docs_state_the_real_freeze_bound_by_construction() {
        // D3: the authority-query budget (~750 ms) must never be sold as
        // the whole-save bound while save() also performs unbounded
        // `noctalia_msg` calls. The module docs must name them.
        let src = std::fs::read_to_string("src/providers/wallpaper_authority.rs")
            .expect("src/providers/wallpaper_authority.rs must exist");
        assert!(
            src.contains("UNBOUNDED `noctalia_msg`"),
            "docs must flag the unbounded noctalia_msg calls"
        );
        assert!(
            src.contains("color-scheme-get") && src.contains("wallpaper-get"),
            "docs must name the unbounded calls (color-scheme-get, wallpaper-get)"
        );
    }

    // ── Combined decision tests (D4) ──
    //
    // The compositor says WHICH layer is on top; the daemon says the TYPE
    // of its own (skwd-suite) layers. `decide_active_kind` takes both JSON
    // signals and returns the kind. Painter names are never trusted for
    // the skwd suite (skwd-wall paints animated wallpapers too), and a
    // missing compositor signal is never backfilled from the daemon.

    fn skwd_layer_doc(namespace: &str) -> String {
        format!(
            r#"{{"HDMI-A-1":{{"levels":{{"0":[{}],"1":[]}}}}}}"#,
            layer(namespace, 1.0, 111)
        )
    }

    fn daemon_kind_doc(kind_word: &str) -> String {
        format!(
            r#"{{"outputs": [{{"name": "HDMI-A-1", "current": "/x", "path": "/x",
                 "type": "{}", "connected": true}}]}}"#,
            kind_word
        )
    }

    #[test]
    fn active_kind_skwd_topmost_daemon_static_is_static() {
        let compositor = skwd_layer_doc("skwd-paper-v2-HDMI-A-1");
        let daemon = daemon_kind_doc("static");
        assert_eq!(
            decide_active_kind(Some(&compositor), Some(&daemon)),
            Some(WallpaperKind::Static)
        );
    }

    #[test]
    fn active_kind_skwd_topmost_daemon_video_is_video() {
        // D1 killer: the old name table said Static for anything
        // containing "skwd" while a video was on screen.
        let compositor = skwd_layer_doc("skwd-paper-v2-HDMI-A-1");
        let daemon = daemon_kind_doc("video");
        assert_eq!(
            decide_active_kind(Some(&compositor), Some(&daemon)),
            Some(WallpaperKind::Video)
        );
    }

    #[test]
    fn active_kind_skwd_topmost_daemon_we_is_video() {
        // "we" (Wallpaper Engine) is animated: it must capture as video.
        let compositor = skwd_layer_doc("skwd-wall");
        let daemon = daemon_kind_doc("we");
        assert_eq!(
            decide_active_kind(Some(&compositor), Some(&daemon)),
            Some(WallpaperKind::Video)
        );
    }

    #[test]
    fn active_kind_skwd_topmost_daemon_unavailable_is_unknown() {
        // Never guess a kind from the painter's name alone.
        let compositor = skwd_layer_doc("skwd-paper-v2-HDMI-A-1");
        assert_eq!(decide_active_kind(Some(&compositor), None), None);
        assert_eq!(
            decide_active_kind(Some(&compositor), Some("not json")),
            None
        );
    }

    #[test]
    fn active_kind_dedicated_video_painter_is_video() {
        // mpvpaper paints video by nature: no daemon signal needed.
        let json = layers_doc(&layer("mpvpaper", 1.0, 507331), &layer("mpvpaper", 1.0, 507331));
        assert_eq!(
            decide_active_kind(Some(&json), None),
            Some(WallpaperKind::Video)
        );
    }

    #[test]
    fn active_kind_unknown_painter_is_unknown() {
        // An unknown painter vetoes even a decisive daemon: the daemon
        // only knows its OWN layers, not what the mystery painter shows.
        let json = layers_doc(
            &layer("mystery-layer", 1.0, 999),
            &layer("mystery-layer", 1.0, 999),
        );
        let daemon = daemon_kind_doc("static");
        assert_eq!(decide_active_kind(Some(&json), Some(&daemon)), None);
    }

    #[test]
    fn active_kind_compositor_unavailable_is_unknown() {
        // D2 regression: a dead compositor query must NEVER be backfilled
        // from the daemon (that resurrects the v1 bug: daemon "static"
        // while a video covers the screen).
        let daemon = daemon_kind_doc("static");
        assert_eq!(decide_active_kind(None, Some(&daemon)), None);
        assert_eq!(
            decide_active_kind(None, Some(&daemon_kind_doc("video"))),
            None
        );
    }

    #[test]
    fn active_kind_per_output_disagreement_is_video() {
        // One output shows skwd/static, the other a live video on top.
        let json = layers_doc(
            &layer("skwd-paper", 1.0, 439101),
            &format!(
                "{},{}",
                layer("skwd-paper", 1.0, 439101),
                layer("mpvpaper", 1.0, 507331)
            ),
        );
        let daemon = r#"{"outputs": [
            {"name": "HDMI-A-1", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true},
            {"name": "DP-3", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true}
        ]}"#;
        assert_eq!(
            decide_active_kind(Some(&json), Some(daemon)),
            Some(WallpaperKind::Video)
        );
    }

    #[test]
    fn active_kind_transparent_topmost_is_ignored() {
        // A fading-out (alpha 0) video above an opaque skwd layer: the
        // visible background is whatever the daemon reports for skwd.
        let json = layers_doc(
            &format!(
                "{},{}",
                layer("skwd-paper", 1.0, 439101),
                layer("mpvpaper", 0.0, 507331)
            ),
            &layer("skwd-paper", 1.0, 439101),
        );
        let daemon = r#"{"outputs": [
            {"name": "HDMI-A-1", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true},
            {"name": "DP-3", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true}
        ]}"#;
        assert_eq!(
            decide_active_kind(Some(&json), Some(daemon)),
            Some(WallpaperKind::Static)
        );
    }

    #[test]
    fn active_kind_empty_level_is_unknown() {
        let json = layers_doc("", "");
        let daemon = daemon_kind_doc("static");
        assert_eq!(decide_active_kind(Some(&json), Some(&daemon)), None);
    }

    #[test]
    fn active_kind_malformed_json_is_unknown() {
        let daemon = daemon_kind_doc("static");
        assert_eq!(decide_active_kind(Some("not json"), Some(&daemon)), None);
        // Malformed daemon JSON with a skwd layer on top: no attribution
        // possible, so unknown — never a name-based guess.
        let compositor = skwd_layer_doc("skwd-paper");
        assert_eq!(
            decide_active_kind(Some(&compositor), Some("not json")),
            None
        );
    }

    #[test]
    fn active_kind_skwd_layer_without_matching_daemon_output_is_unknown() {
        // Attribution is by output name: a daemon report for another
        // output (or a disconnected one) must not decide this layer.
        let compositor = skwd_layer_doc("skwd-paper");
        let other_output = r#"{"outputs": [
            {"name": "DP-3", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true}
        ]}"#;
        assert_eq!(
            decide_active_kind(Some(&compositor), Some(other_output)),
            None
        );
        let disconnected = r#"{"outputs": [
            {"name": "HDMI-A-1", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": false}
        ]}"#;
        assert_eq!(
            decide_active_kind(Some(&compositor), Some(disconnected)),
            None
        );
    }
}
