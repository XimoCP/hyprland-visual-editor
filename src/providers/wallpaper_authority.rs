//! Single owner of "who controls the wallpaper right now".
//!
//! Two signals, each used for what it actually knows:
//! - The COMPOSITOR's layer stack (`hyprctl -j layers`) says WHICH layer
//!   is on top ("what is seen on top") — ground truth for visibility.
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
//! Background-capable levels are `"0"` (background) and `"1"` (bottom):
//! verified live, skwd static images and mpvpaper videos paint at level 0
//! while skwd's own video renderer (`skwd-wall-vk`) paints at level 1.
//! Higher level wins; within a level, the later entry wins. Level 1 also
//! carries small Noctalia desktop widgets and level 2 bars/docks, so a
//! layer only counts as a background candidate when it is opaque AND
//! covers essentially its whole output (see [`BG_COVERAGE_PERCENT`]):
//! size is RELATIVE to the output — the compositor reports layers in
//! scaled logical pixels (a 1920x1080 output at 200% paints a 960x540
//! layer), so no absolute pixel constant can decide this. Smaller layers
//! are furniture and can neither win nor veto. A fullscreen unknown
//! painter where a background could be still vetoes — it is never
//! silently ignored. When no output reference can be established
//! (monitors unreachable, daemon silent about sizes), opaque layers are
//! kept and the gap is logged — a real background is never excluded
//! silently.
//!
//! Video identity is structural, never positional: for skwd-suite layers
//! the daemon's `current` for the same output comes first (it names the
//! video the suite was told to render); otherwise the media path that
//! follows the painter argv's `*` output marker is the current video
//! (verified for `mpvpaper` and `skwd-wall-vk`). The previous background
//! rides behind `--transition-from`, so "last media path" would pick the
//! old video on a video-to-video switch; an argv that stays ambiguous
//! (no marker with several candidates, several markers) vetoes instead of
//! guessing. mpvpaper's on-disk state file is NOT consulted: it survives the
//! plugin's death and still names the previous video.
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
//! Timing budget: the AUTHORITY snapshot this module captures per save (1 ×
//! compositor + 2 × daemon attempts + 1 × monitors, each bounded by ≤250 ms)
//! blocks at most ~1 s even with every helper hung (previously ~12 s:
//! 2 saves × 2 binaries × 3 s; ~1.5 s across the dual main + gallery save in
//! that pathological case; ~2 s while the save ran the capture once for the
//! kind decision and again inside the static path). The save captures ONE
//! snapshot and shares it across the kind, video-identity and static-path
//! decisions. That bound covers ONLY this module's queries: the
//! same `save()` additionally performs two UNBOUNDED `noctalia_msg` calls
//! (`color-scheme-get`, `wallpaper-get` — `noctalia_runtime.rs` uses a
//! bare `.output()` with no timeout), so a full panel save can block
//! longer than the authority budget when the Noctalia IPC itself hangs.
//! A live query answers in single-digit ms; expiry falls back to the
//! lossless capture-everything path, so tight bounds cost precision,
//! never data.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    /// Logical size of the output as reported by the daemon
    /// (`logical_width` / `logical_height`, falling back to
    /// `width` / `height` when the logical fields are absent or zero).
    /// `0` means unknown — never used as a size reference.
    pub logical_width: u32,
    pub logical_height: u32,
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
    /// Second size reference for the relative-fullscreen check (D3).
    /// Absent means unknown (0), never a guess.
    #[serde(default)]
    logical_width: u32,
    #[serde(default)]
    logical_height: u32,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
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
            // Logical size first, physical size as fallback, zero when
            // neither is known (never a reference then).
            let logical_width = if e.logical_width > 0 {
                e.logical_width
            } else {
                e.width
            };
            let logical_height = if e.logical_height > 0 {
                e.logical_height
            } else {
                e.height
            };
            Ok(AuthorityOutput {
                name,
                kind,
                connected: e.connected,
                current,
                logical_width,
                logical_height,
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

/// Logical size of one output, in the compositor's (scaled logical pixel)
/// coordinate space — the same space `hyprctl -j layers` reports `w`/`h`
/// in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSize {
    pub width: u32,
    pub height: u32,
}

/// Bounded wait for one `hyprctl -j monitors` call: the primary output-size
/// reference. Expires into the daemon-logical fallback, never a hang.
pub const MONITORS_TIMEOUT: Duration = Duration::from_millis(250);

/// How much of the output a layer must cover in BOTH dimensions to count
/// as a background (percent). 90% leaves slack for rounding between the
/// compositor's scaled logical geometry and the references, while bars,
/// docks and desktop widgets — which hug one edge and collapse on the
/// other axis — never come close.
pub const BG_COVERAGE_PERCENT: u32 = 90;

/// True when a layer entry can be a background: opaque AND covering
/// essentially the whole output (`reference`). Small opaque layers
/// (desktop widgets, bars, docks) are furniture: they can neither win the
/// decision nor veto it. With no reference (`None`) the layer is kept —
/// excluding it would be a silent veto of a possibly real background, so
/// the gap is logged instead and the decision stays honest downstream.
fn is_background_candidate(alpha: f32, w: u32, h: u32, reference: Option<OutputSize>) -> bool {
    if alpha <= 0.0 {
        return false;
    }
    match reference {
        Some(r) if r.width > 0 && r.height > 0 => {
            (w as u64) * 100 >= (r.width as u64) * (BG_COVERAGE_PERCENT as u64)
                && (h as u64) * 100 >= (r.height as u64) * (BG_COVERAGE_PERCENT as u64)
        }
        _ => {
            tracing::warn!(
                "[wallpaper] No output-size reference; keeping {}x{} opaque layer \
                 instead of excluding it silently",
                w,
                h,
            );
            true
        }
    }
}

/// Background-capable compositor levels, lowest first: `"0"` (background)
/// < `"1"` (bottom). Verified live on the keeper's machine: skwd static
/// images (`skwd-paper`) and mpvpaper videos paint at 0, skwd's own video
/// renderer (`skwd-wall-vk`) paints at 1. Levels 2 (top) and 3 (overlay)
/// carry bars, docks and overlay UI — never backgrounds.
const BACKGROUND_LEVELS: [&str; 2] = ["0", "1"];

/// One background layer entry from `hyprctl -j layers`. Only `alpha`,
/// geometry (`w`, `h`), `namespace` and `pid` matter here; the rest is
/// ignored. A missing `alpha` defaults to 0.0 (transparent) and missing
/// geometry to 0x0: unconfirmed opacity or size is not trusted, the entry
/// is skipped rather than guessed.
#[derive(Debug, Deserialize)]
struct LayerEntry {
    #[serde(default)]
    alpha: f32,
    #[serde(default)]
    w: u32,
    #[serde(default)]
    h: u32,
    #[serde(default)]
    namespace: String,
    #[serde(default)]
    pid: u32,
}

/// One monitor from `hyprctl -j layers`: `{ "levels": { "0": [...],
/// "1": [...] } }`. Within one level, LATER entries draw ON TOP; level
/// `"1"` draws above level `"0"`. A missing level key means no signal at
/// that level for that output.
#[derive(Debug, Deserialize)]
struct LayersOutput {
    #[serde(default)]
    levels: HashMap<String, Vec<LayerEntry>>,
}

/// What the compositor shows on top for one output. `kind` is `None` when
/// there is no opaque fullscreen background layer or the topmost painter
/// is unknown — both map to [`SavePlan::Unknown`] (capture everything,
/// never guess). `top_namespace` / `top_pid` name the painter for the
/// honest log line; `top_level` is the winning background level (`"0"` or
/// `"1"`), `None` when no candidate layer exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositorOutput {
    pub name: String,
    pub kind: Option<WallpaperKind>,
    pub top_namespace: String,
    pub top_pid: u32,
    pub top_level: Option<u8>,
}

/// Parse the compositor's layer JSON into per-output topmost layers.
///
/// Scans background-capable levels `"0"` then `"1"` (higher wins); within
/// a level the LAST opaque covering entry wins. Small opaque layers
/// (widgets, bars) are furniture: skipped, never winners, never vetoes.
/// A covering unknown painter still vetoes downstream — it is kept with
/// its namespace and pid, never silently ignored.
///
/// No-reference wrapper: every opaque layer is kept (see
/// [`is_background_candidate`]) — without `hyprctl -j monitors` or daemon
/// logical sizes there is nothing truthful to compare against. Prefer
/// [`parse_compositor_layers_with_reference`] whenever a reference map is
/// available.
///
/// Pure and fixture-driven: classification uses the namespace only. Only
/// dedicated video painters yield a kind here; skwd-suite layers yield
/// `None` — their kind comes from the daemon via [`decide_active_kind`],
/// never from the name. The live decision ([`snapshot_wallpaper_kind`])
/// additionally resolves the pid's process name for topmost layers the
/// namespace alone cannot classify — `/proc` access must never leak into
/// the pure path, or tests would depend on the machine's live process
/// table.
pub fn parse_compositor_layers(json: &str) -> Result<Vec<CompositorOutput>, String> {
    parse_compositor_layers_with_reference(json, &HashMap::new())
}

/// Reference-aware [`parse_compositor_layers`]: `reference` maps output
/// name to its logical size (see [`output_reference_sizes`]). Outputs
/// missing from the map are kept permissively with a log line, never
/// excluded silently.
pub fn parse_compositor_layers_with_reference(
    json: &str,
    reference: &HashMap<String, OutputSize>,
) -> Result<Vec<CompositorOutput>, String> {
    let doc: HashMap<String, LayersOutput> =
        serde_json::from_str(json).map_err(|e| format!("Cannot parse layers JSON: {}", e))?;
    let mut outputs: Vec<CompositorOutput> = doc
        .into_iter()
        .map(|(name, out)| {
            let reference_for_output = reference.get(&name).copied();
            let mut top: Option<(&LayerEntry, u8)> = None;
            for (level_no, level_key) in [0u8, 1u8].iter().zip(BACKGROUND_LEVELS) {
                if let Some(entries) = out.levels.get(level_key) {
                    if let Some(entry) = entries
                        .iter()
                        .filter(|e| {
                            is_background_candidate(e.alpha, e.w, e.h, reference_for_output)
                        })
                        .last()
                    {
                        top = Some((entry, *level_no));
                    }
                }
            }
            match top {
                Some((entry, level_no)) => CompositorOutput {
                    name,
                    kind: classify_painter(&entry.namespace, None),
                    top_namespace: entry.namespace.clone(),
                    top_pid: entry.pid,
                    top_level: Some(level_no),
                },
                None => CompositorOutput {
                    name,
                    kind: None,
                    top_namespace: String::new(),
                    top_pid: 0,
                    top_level: None,
                },
            }
        })
        .collect();
    outputs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(outputs)
}

/// One monitor entry from `hyprctl -j monitors`: physical `width` /
/// `height` plus the `scale` factor. The reference size is the logical
/// size (`physical / scale`, rounded) — the coordinate space layers use.
/// The name is read from `name`, falling back to `output`, `monitor` and
/// `connector`. Both a top-level array and a `{"monitors": [...]}` object
/// are accepted; entries without usable geometry are skipped, never a
/// guess.
#[derive(Debug, Deserialize)]
struct MonitorEntry {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    monitor: Option<String>,
    #[serde(default)]
    connector: Option<String>,
    #[serde(default)]
    width: f64,
    #[serde(default)]
    height: f64,
    #[serde(default)]
    scale: f64,
}

/// Parse `hyprctl -j monitors` into per-output logical sizes.
pub fn parse_monitor_sizes(json: &str) -> Result<HashMap<String, OutputSize>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Cannot parse monitors JSON: {}", e))?;
    let entries: Vec<MonitorEntry> = match &value {
        serde_json::Value::Array(_) => serde_json::from_value(value)
            .map_err(|e| format!("Cannot parse monitors array: {}", e))?,
        serde_json::Value::Object(_) => {
            let doc: HashMap<String, Vec<MonitorEntry>> = serde_json::from_value(value)
                .map_err(|e| format!("Cannot parse monitors object: {}", e))?;
            doc.into_values().next().unwrap_or_default()
        }
        _ => return Err("Monitors JSON is neither an array nor an object".to_string()),
    };
    let mut sizes = HashMap::new();
    for e in entries {
        let name = e
            .name
            .or(e.output)
            .or(e.monitor)
            .or(e.connector)
            .unwrap_or_default();
        if name.is_empty() || e.width <= 0.0 || e.height <= 0.0 {
            continue;
        }
        let scale = if e.scale > 0.0 { e.scale } else { 1.0 };
        sizes.insert(
            name,
            OutputSize {
                width: (e.width / scale).round() as u32,
                height: (e.height / scale).round() as u32,
            },
        );
    }
    Ok(sizes)
}

/// Build the output-size reference map: `hyprctl -j monitors` (primary —
/// it speaks the compositor's scaled-logical coordinate space) wins on
/// conflict; the daemon's per-output logical sizes fill outputs the
/// monitors query missed. Outputs with no size anywhere stay absent: the
/// parser keeps their opaque layers permissively and logs the gap.
pub fn output_reference_sizes(
    monitors_json: Option<&str>,
    daemon: &[AuthorityOutput],
) -> HashMap<String, OutputSize> {
    let mut sizes = monitors_json
        .and_then(|json| parse_monitor_sizes(json).ok())
        .unwrap_or_default();
    for o in daemon {
        if !o.connected || o.logical_width == 0 || o.logical_height == 0 {
            continue;
        }
        sizes.entry(o.name.clone()).or_insert(OutputSize {
            width: o.logical_width,
            height: o.logical_height,
        });
    }
    sizes
}

/// Live monitors capture: one bounded `hyprctl -j monitors` query
/// (≤250 ms). `None` (with a debug line) when unreachable — the caller
/// degrades to the daemon logical sizes, never a hang, never a guess.
fn query_monitors_json() -> Option<String> {
    match run_bounded_command("hyprctl", &["-j", "monitors"], MONITORS_TIMEOUT) {
        Ok(json) => Some(json),
        Err(reason) => {
            tracing::debug!(
                "[wallpaper] Monitors reference unavailable ({}); \
                 daemon logical sizes fill the gap",
                reason,
            );
            None
        }
    }
}

/// Outputs that carry a background vote: entries with no background layer
/// at all (empty top) are ABSENCE, not disagreement — the output simply
/// has nothing to say (e.g. a widget-only level the size reference
/// excluded as furniture). Every decision ignores them; any
/// attributable disagreement (a video, an unknown painter, an
/// unattributable skwd layer) still vetoes downstream, never silently.
fn voting_outputs(compositor: &[CompositorOutput]) -> Vec<CompositorOutput> {
    compositor
        .iter()
        .filter(|o| !o.top_namespace.is_empty())
        .cloned()
        .collect()
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
/// Multi-output rule: any video wins; any ATTRIBUTABLE veto (unknown
/// painter, unattributable skwd layer) sinks the whole decision, while an
/// output with no background layer at all carries no vote (absence is not
/// disagreement — see [`voting_outputs`]). No outputs at all yields `None`.
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
            // Absence is not disagreement (same rule as `voting_outputs`):
            // an output with NO background layer has nothing to say — e.g.
            // a furniture/widget layer the size reference excluded — and
            // must not sink a kind another output proves. An ATTRIBUTABLE
            // disagreement (unknown painter, unattributable skwd layer)
            // still vetoes below. Vetoing on absence sent the save to
            // `SavePlan::Unknown`, which captured a stale video manifest
            // next to the static record.
            continue;
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
/// top. The size reference comes from the daemon's logical sizes here;
/// prefer [`decide_active_kind_with_monitors`] when a monitors capture is
/// available (scaled outputs need it — see D3).
pub fn decide_active_kind(
    compositor_json: Option<&str>,
    daemon_json: Option<&str>,
) -> Option<WallpaperKind> {
    decide_active_kind_with_monitors(compositor_json, daemon_json, None)
}

/// Reference-aware [`decide_active_kind`]: `monitors_json` is a
/// `hyprctl -j monitors` capture (primary size reference for scaled
/// outputs); the daemon's logical sizes fill the gaps. `None` behaves
/// like [`decide_active_kind`].
pub fn decide_active_kind_with_monitors(
    compositor_json: Option<&str>,
    daemon_json: Option<&str>,
    monitors_json: Option<&str>,
) -> Option<WallpaperKind> {
    let compositor_raw = compositor_json?;
    let daemon: Vec<AuthorityOutput> = daemon_json
        .and_then(|json| parse_authority_outputs(json).ok())
        .unwrap_or_default();
    let reference = output_reference_sizes(monitors_json, &daemon);
    let compositor = parse_compositor_layers_with_reference(compositor_raw, &reference).ok()?;
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

/// Best-effort argv for a layer pid (`/proc/<pid>/cmdline`, NUL
/// separated). Empty on any failure — the caller treats it as "no argv
/// signal" and moves on to the daemon fallback. A local procfs read:
/// no external command, no timeout needed, never held across I/O locks.
fn argv_for_pid(pid: u32) -> Vec<String> {
    std::fs::read(format!("/proc/{}/cmdline", pid))
        .map(|bytes| {
            bytes
                .split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Keep a candidate video path only when it names an existing absolute
/// media file. A stale record (deleted video, daemon lag) must veto, not
/// resurrect.
fn existing_media_path(candidate: Option<String>) -> Option<PathBuf> {
    candidate
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_file())
}

/// Resolve which video file is actually painted, from parsed signals plus
/// live painter argv. Fallback order per output: for skwd-suite layers the
/// daemon's `current` for the same output comes FIRST (it names the video
/// the suite was told to render — verified correct on the keeper's
/// machine), then the topmost layer's process argv (structural `*`-marker
/// identity, works for every painter). mpvpaper's on-disk state file is
/// deliberately never consulted: it outlives the plugin and names the
/// previous video. Returns `None` when neither yields an existing file:
/// the caller falls back honestly and says so.
fn resolve_video_identity(
    compositor: &[CompositorOutput],
    daemon: &[AuthorityOutput],
) -> Option<PathBuf> {
    for o in compositor {
        let argv = if o.top_pid == 0 {
            Vec::new()
        } else {
            argv_for_pid(o.top_pid)
        };
        let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        // The argv's own binary name sharpens the owner check for generic
        // namespaces (same signal as /proc comm, minus one file read).
        let proc_hint = argv.first().and_then(|bin| {
            Path::new(bin.as_str())
                .file_name()
                .and_then(|f| f.to_str())
        });
        let owner = layer_owner(&o.top_namespace, proc_hint);
        let attributed_video = o.kind == Some(WallpaperKind::Video)
            || (owner == LayerOwner::Skwd
                && attribute_skwd_layer(&o.name, daemon) == Some(WallpaperKind::Video));
        if !attributed_video {
            continue;
        }
        if owner == LayerOwner::Skwd {
            if let Some(path) = existing_media_path(daemon_video_current(daemon, &o.name)) {
                return Some(path);
            }
        }
        if let Some(path) = existing_media_path(extract_video_path_from_argv(&argv_refs)) {
            return Some(path);
        }
    }
    None
}

/// The active background with its painter identity: kind (what the
/// compositor shows on top), who paints it (namespace / pid / process
/// name), and — for video — which file is actually rendered.
#[derive(Debug, Clone)]
pub struct ActiveBackground {
    pub kind: WallpaperKind,
    pub top_namespace: String,
    pub top_pid: u32,
    pub painter_proc: Option<String>,
    pub video_path: Option<PathBuf>,
}

/// One bounded capture of every authority input (compositor layers,
/// daemon types, monitors sizes): the save captures once and shares it
/// across the kind, video-identity and static-path decisions instead of
/// re-running the full capture per decision. A standalone decision
/// captures one snapshot implicitly (see the `query_*` wrappers); one
/// snapshot never hangs past `COMPOSITOR_TIMEOUT + 2 * AUTHORITY_TIMEOUT
/// + MONITORS_TIMEOUT` (≈1 s with every helper hung).
#[derive(Debug, Clone)]
pub struct AuthoritySnapshot {
    /// `hyprctl -j layers` output; `None` when the compositor was
    /// unreachable (the reason lives in `compositor_error`).
    pub compositor_json: Option<String>,
    /// Why the compositor capture failed, for the honest log line.
    pub compositor_error: Option<String>,
    /// Daemon `outputs --json`; `None` vetoes every skwd layer downstream.
    pub daemon_json: Option<String>,
    /// `hyprctl -j monitors` output; `None` degrades to the daemon sizes.
    pub monitors_json: Option<String>,
}

/// Capture every authority input once (see [`AuthoritySnapshot`]).
/// A failed capture yields `None` fields, never a panic — each decision
/// falls back honestly from the same inputs.
pub fn query_authority_snapshot() -> AuthoritySnapshot {
    let (compositor_json, compositor_error) =
        match run_bounded_command("hyprctl", &["-j", "layers"], COMPOSITOR_TIMEOUT) {
            Ok(json) => (Some(json), None),
            Err(reason) => (None, Some(reason)),
        };
    let daemon_json = query_daemon_json();
    let monitors_json = query_monitors_json();
    AuthoritySnapshot {
        compositor_json,
        compositor_error,
        daemon_json,
        monitors_json,
    }
}

/// Ask which background is active AND who paints it, so a theme can record
/// a video exactly (kind + path + painter) and restore it through the
/// manager in charge. Decides from a shared [`AuthoritySnapshot`] — the
/// same inputs [`snapshot_wallpaper_kind`] sees, so the kind cannot flip
/// between the two. A video kind with no resolvable file keeps
/// `video_path == None` — the caller must veto honestly, never invent one.
pub fn snapshot_background(snapshot: &AuthoritySnapshot) -> Result<ActiveBackground, String> {
    let compositor_json = snapshot.compositor_json.clone().ok_or_else(|| {
        format!(
            "compositor unavailable: {}",
            snapshot.compositor_error.as_deref().unwrap_or("unknown")
        )
    })?;
    let daemon_json = snapshot.daemon_json.clone();
    let monitors_json = snapshot.monitors_json.clone();
    let mut outputs = {
        let daemon_outputs: Vec<AuthorityOutput> = daemon_json
            .as_deref()
            .and_then(|json| parse_authority_outputs(json).ok())
            .unwrap_or_default();
        let reference = output_reference_sizes(monitors_json.as_deref(), &daemon_outputs);
        // Empty map behaves identically to the no-reference parse; the
        // branch keeps the single-reference entry point in live use.
        if reference.is_empty() {
            parse_compositor_layers(&compositor_json)?
        } else {
            parse_compositor_layers_with_reference(&compositor_json, &reference)?
        }
    };
    enrich_with_proc_names(&mut outputs);
    let daemon_outputs: Vec<AuthorityOutput> = daemon_json
        .as_deref()
        .and_then(|json| parse_authority_outputs(json).ok())
        .unwrap_or_default();
    let kind = decide_active_kind_from_outputs(&outputs, &daemon_outputs).ok_or_else(|| {
        "compositor layer stack has no attributable opaque background layer".to_string()
    })?;
    log_daemon_disagreement(daemon_json.as_deref(), kind);
    // The winning output names the painter: prefer the video output when
    // the kind is video, else the first attributed output.
    let winner = outputs
        .iter()
        .find(|o| o.kind == Some(WallpaperKind::Video) && kind == WallpaperKind::Video)
        .or_else(|| outputs.iter().find(|o| !o.top_namespace.is_empty()));
    let video_path = if kind == WallpaperKind::Video {
        resolve_video_identity(&outputs, &daemon_outputs)
    } else {
        None
    };
    let (top_namespace, top_pid) = match winner {
        Some(o) => (o.top_namespace.clone(), o.top_pid),
        None => (String::new(), 0),
    };
    let painter_proc = if top_pid == 0 {
        None
    } else {
        proc_name_for_pid(top_pid)
    };
    Ok(ActiveBackground {
        kind,
        top_namespace,
        top_pid,
        painter_proc,
        video_path,
    })
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

/// Ask which background is active right now from a shared
/// [`AuthoritySnapshot`]: the COMPOSITOR says which layer is on top, the
/// daemon says the type of the skwd suite's layers (see
/// [`decide_active_kind`]).
///
/// Pure fast path first (no `/proc` touch): it decides whenever every
/// topmost layer is a dedicated video painter or an attributable skwd
/// layer. Only when an unknown painter vetoes that path are pid process
/// names resolved for a second chance before giving up.
///
/// If the compositor capture failed, an `Err` naming the reason is returned
/// so the caller keeps the legacy capture-everything behaviour. The
/// daemon alone NEVER decides: it is blind to other painters (verified
/// live reporting "static" while a video covered the screen), so a
/// compositor outage backfilled from the daemon would resurrect the very
/// bug this module exists to kill. Never panics; one snapshot never hangs
/// past `COMPOSITOR_TIMEOUT + 2 * AUTHORITY_TIMEOUT + MONITORS_TIMEOUT`
/// (≈1 s).
pub fn snapshot_wallpaper_kind(snapshot: &AuthoritySnapshot) -> Result<WallpaperKind, String> {
    let compositor_json = snapshot.compositor_json.clone().ok_or_else(|| {
        format!(
            "compositor unavailable: {}",
            snapshot.compositor_error.as_deref().unwrap_or("unknown")
        )
    })?;
    let daemon_json = snapshot.daemon_json.clone();
    let monitors_json = snapshot.monitors_json.clone();
    // No monitors capture degrades to the daemon-sizes-only decision; the
    // branch keeps the single-reference entry point in live use.
    let fast_kind = match monitors_json.as_deref() {
        Some(monitors) => decide_active_kind_with_monitors(
            Some(&compositor_json),
            daemon_json.as_deref(),
            Some(monitors),
        ),
        None => decide_active_kind(Some(&compositor_json), daemon_json.as_deref()),
    };
    if let Some(kind) = fast_kind {
        log_daemon_disagreement(daemon_json.as_deref(), kind);
        return Ok(kind);
    }
    let daemon_outputs: Vec<AuthorityOutput> = daemon_json
        .as_deref()
        .and_then(|json| parse_authority_outputs(json).ok())
        .unwrap_or_default();
    let reference = output_reference_sizes(monitors_json.as_deref(), &daemon_outputs);
    let mut outputs = if reference.is_empty() {
        parse_compositor_layers(&compositor_json)?
    } else {
        parse_compositor_layers_with_reference(&compositor_json, &reference)?
    };
    enrich_with_proc_names(&mut outputs);
    let kind = decide_active_kind_from_outputs(&outputs, &daemon_outputs).ok_or_else(|| {
        "compositor layer stack has no attributable opaque background layer".to_string()
    })?;
    log_daemon_disagreement(daemon_json.as_deref(), kind);
    Ok(kind)
}

/// Media extensions recognised in a painter's argv. The match is on the
/// absolute path's extension only — flags, sockets and bare names never
/// qualify, so no path is ever invented.
const MEDIA_EXTENSIONS: [&str; 9] = [
    "mp4", "mkv", "webm", "avi", "mov", "m4v", "wmv", "ogv", "gif",
];

/// Derive the painted video path from a painter process argv.
///
/// Structural identity (D2), never "last media path": both painters render
/// the output named right after the `*` output marker (`mpvpaper ...
/// * <video>`, `skwd-wall-vk * <video> --transition-from <previous> ...`),
/// while a video-to-video switch appends the PREVIOUS video behind
/// `--transition-from` — the last media argument is then the old video.
/// Rules:
/// - exactly one `*`: the first absolute media path after it (or `None`
///   when there is none — arguments before the marker are the previous
///   background, never the identity);
/// - several `*` (per-output painters in one argv): ambiguous, `None`;
/// - no `*`: only a single absolute media candidate is unambiguous
///   (`None` when there are zero or several).
/// Existence on disk is checked by the live caller, never here, so tests
/// stay hermetic.
pub fn extract_video_path_from_argv(argv: &[&str]) -> Option<String> {
    let is_media_path = |arg: &str| -> bool {
        let path = Path::new(arg);
        if !path.is_absolute() {
            return false;
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => MEDIA_EXTENSIONS.contains(&ext.to_lowercase().as_str()),
            None => false,
        }
    };
    let star_count = argv.iter().filter(|arg| **arg == "*").count();
    if star_count == 1 {
        let star = argv.iter().position(|arg| *arg == "*").unwrap_or(0);
        return argv[star + 1..]
            .iter()
            .find_map(|arg| is_media_path(arg).then(|| arg.to_string()));
    }
    if star_count > 1 {
        return None;
    }
    let mut found: Option<String> = None;
    for arg in argv {
        if is_media_path(arg) {
            if found.is_some() {
                return None;
            }
            found = Some(arg.to_string());
        }
    }
    found
}

/// Fallback video identity for skwd-suite layers: the daemon's `current`
/// for the same output name (connected outputs only). Returns `None` when
/// the daemon is silent for that output — the caller then vetoes honestly
/// instead of inventing a path.
pub fn daemon_video_current(daemon: &[AuthorityOutput], output_name: &str) -> Option<String> {
    daemon
        .iter()
        .find(|o| o.connected && o.name == output_name)
        .map(|o| o.current.clone())
        .filter(|s| !s.is_empty())
}

/// Daemon `current` for a STATIC entry: the suite's own record of the
/// image it painted on `output_name` (connected outputs only).
///
/// Mirror of [`daemon_video_current`]: `None` for a missing entry, a
/// disconnected one, a non-static kind, or an empty path — the caller
/// then falls back honestly instead of recording a guess.
pub fn daemon_static_current(daemon: &[AuthorityOutput], output_name: &str) -> Option<String> {
    daemon
        .iter()
        .find(|o| o.connected && o.name == output_name && o.kind == WallpaperKind::Static)
        .map(|o| o.current.clone())
        .filter(|s| !s.is_empty())
}

/// Pick the single static path a theme records.
///
/// WHY one path: the theme model stores ONE wallpaper, but the painter
/// keeps one per output. Rule: the lexicographically-first output whose
/// topmost layer the compositor proved is skwd-painted, typed static by
/// the daemon for that same output name. Sorted names keep it
/// deterministic no matter the parse order; when every output agrees (the
/// usual case) any choice is the same image.
///
/// Absence is not disagreement: an output with no background layer at all
/// carries no vote and is ignored (a widget-only output must not veto the
/// proven one). A genuine conflict still vetoes to `None`, never a partial
/// guess: a video anywhere, an unknown painter on top, or a skwd layer the
/// daemon cannot attribute for that output. Like the
/// legacy `wallpaper-get` record, the path is stored as reported (no
/// existence gate): whether the file survived is an apply-time concern,
/// and gating here would make the record differ from what the painter
/// reports.
pub fn resolve_static_path(
    compositor: &[CompositorOutput],
    daemon: &[AuthorityOutput],
) -> Option<String> {
    let voting = voting_outputs(compositor);
    if voting.is_empty() {
        return None;
    }
    if decide_active_kind_from_outputs(&voting, daemon) != Some(WallpaperKind::Static) {
        return None;
    }
    let mut names: Vec<&str> = voting
        .iter()
        .filter(|o| layer_owner(&o.top_namespace, None) == LayerOwner::Skwd)
        .map(|o| o.name.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .find_map(|name| daemon_static_current(daemon, name))
}

/// Live version of [`resolve_static_path`]: which static image the skwd
/// suite actually paints, so save records the daemon's truth instead of a
/// stale third party value.
///
/// Decides from a shared [`AuthoritySnapshot`] so the save pays the
/// capture once (see [`query_authority_snapshot`] for the bound); reuses
/// the existing layer and daemon parsers, never a second copy of either.
///
/// Returns `Ok(Some(path))` only for a compositor-proven skwd static
/// scene with a usable daemon `current`. Outputs with no background layer
/// at all are ignored (absence, not disagreement); an attributable
/// disagreement (a video, an unknown painter) still yields `Ok(None)`. `Ok(None)` means "not the
/// suite's static scene" (another painter, a video, an unknown layer, an
/// unreachable compositor) — the caller keeps today's behaviour, quietly
/// at debug level (the kind query already logged its reason). `Err(reason)`
/// means the compositor proved the suite paints but the daemon cannot
/// complete the record (unreachable, unparseable, silent for that output,
/// or an empty `current`): the reason names the output and the cause, so
/// the caller can warn honestly while falling back — never an empty path,
/// never a guess.
///
/// Bounded like the other snapshot decisions (see [`query_authority_snapshot`]).
pub fn snapshot_static_path(snapshot: &AuthoritySnapshot) -> Result<Option<String>, String> {
    let compositor_json = match snapshot.compositor_json.clone() {
        Some(json) => json,
        None => {
            tracing::debug!(
                "[wallpaper] static path: compositor unavailable ({}); keeping wallpaper-get",
                snapshot.compositor_error.as_deref().unwrap_or("unknown"),
            );
            return Ok(None);
        }
    };
    let daemon_json = snapshot.daemon_json.clone();
    let monitors_json = snapshot.monitors_json.clone();
    let daemon_usable = daemon_json.is_some();
    let daemon: Vec<AuthorityOutput> = daemon_json
        .as_deref()
        .and_then(|json| parse_authority_outputs(json).ok())
        .unwrap_or_default();
    let reference = output_reference_sizes(monitors_json.as_deref(), &daemon);
    let compositor = match parse_compositor_layers_with_reference(&compositor_json, &reference) {
        Ok(outputs) => outputs,
        Err(reason) => {
            tracing::debug!(
                "[wallpaper] static path: layers unparseable ({}); keeping wallpaper-get",
                reason,
            );
            return Ok(None);
        }
    };
    // Outputs the compositor proved are skwd-painted, first sorted first
    // (the deterministic single-path rule of `resolve_static_path`).
    let mut skwd_names: Vec<&str> = compositor
        .iter()
        .filter(|o| layer_owner(&o.top_namespace, None) == LayerOwner::Skwd)
        .map(|o| o.name.as_str())
        .collect();
    skwd_names.sort_unstable();
    skwd_names.dedup();
    let first = match skwd_names.first() {
        Some(name) => *name,
        None => {
            tracing::debug!(
                "[wallpaper] static path: no skwd layer on top; keeping wallpaper-get",
            );
            return Ok(None);
        }
    };
    // The pure rule decides: only a compositor-proven static scene with a
    // usable daemon current for a painted output yields a path.
    if let Some(path) = resolve_static_path(&compositor, &daemon) {
        return Ok(Some(path));
    }
    // No path: tell a missing daemon half apart from "not ours to
    // override" (a video, an unknown painter, or a veto elsewhere keeps
    // today's value quietly — the kind query already logged its reason).
    // Absence-tolerant like the pure rule above: outputs with no
    // background layer carry no vote, so a proven scene with a silent
    // daemon still warns honestly instead of falling back silently.
    let proven_static = decide_active_kind_from_outputs(&voting_outputs(&compositor), &daemon)
        == Some(WallpaperKind::Static);
    let all_skwd = compositor.iter().any(|o| !o.top_namespace.is_empty())
        && compositor
            .iter()
            .filter(|o| !o.top_namespace.is_empty())
            .all(|o| layer_owner(&o.top_namespace, None) == LayerOwner::Skwd);
    if proven_static || (all_skwd && !daemon_usable) {
        let missing_current = if daemon_usable {
            "no connected static entry with a non-empty current for that output"
        } else {
            "daemon type signal unavailable"
        };
        return Err(format!(
            "skwd paints '{}' but the daemon reports no usable static current ({})",
            first, missing_current,
        ));
    }
    tracing::debug!(
        "[wallpaper] static path: scene is not a proven skwd static one; keeping wallpaper-get",
    );
    Ok(None)
}

/// File HVE stores inside the theme provider dir for a painter-identified
/// video (kind + path + who painted it). Owned by the noctalia-v5 save path;
/// restoring it routes through the manager in charge, never the plugin.
pub const PAINTER_VIDEO_FILE: &str = "video.txt";

/// Serialize the painter-identified video record stored as
/// [`PAINTER_VIDEO_FILE`]: kind (implied video) + path + who painted it
/// (process / namespace / pid), one `key=value` per line. Only `path` is
/// required on read; the rest is provenance for the honest log line.
pub fn format_painter_video_record(
    path: &Path,
    painter_proc: Option<&str>,
    namespace: &str,
    pid: u32,
) -> String {
    format!(
        "path={}\npainter={}\nnamespace={}\npid={}\n",
        path.display(),
        painter_proc.unwrap_or(""),
        namespace,
        pid,
    )
}

/// Parse a [`PAINTER_VIDEO_FILE`] record back to the video path. `None`
/// (never a guess) when no usable absolute `path=` line exists: a relative
/// path would resolve against HVE's working directory, so it is rejected
/// just like a missing one.
pub fn parse_painter_video_record(text: &str) -> Option<PathBuf> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix("path=")?.trim();
        if rest.is_empty() {
            return None;
        }
        let path = PathBuf::from(rest);
        if path.is_absolute() {
            Some(path)
        } else {
            None
        }
    })
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

    /// Save must consult the authority: the noctalia-v5 save path captures
    /// the single snapshot and branches on the plan, so a future manager
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
            code.contains("query_authority_snapshot"),
            "noctalia.rs save must capture the authority snapshot once"
        );
        assert!(
            code.contains("snapshot_wallpaper_kind"),
            "noctalia.rs save must decide the plan from the snapshot"
        );
        assert!(
            code.contains("snapshot_background"),
            "noctalia.rs save must read the painter video identity from the snapshot"
        );
        assert!(
            code.contains("snapshot_static_path"),
            "noctalia.rs save must read the static path from the snapshot"
        );
        assert!(
            code.contains("SavePlan::StaticOnly") && code.contains("SavePlan::VideoOnly"),
            "noctalia.rs save must branch on both SavePlan arms"
        );
    }

    // ── Compositor layer-stack tests ──
    //
    // `hyprctl -j layers` is the source of truth for which background is
    // actually on top ("what is seen on top"). The daemon (`skwd-helm`)
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
    fn partial_scene_decides_static_when_an_output_has_no_background_layer() {
        // The keeper's live partial scene: HDMI-A-1 is skwd-painted static
        // while DP-3 carries NO background layer at all (absence, e.g. its
        // furniture layer was excluded by the size reference). Absence is
        // not disagreement, so the kind must come from the proven output.
        // Vetoing here would send the save to `SavePlan::Unknown`, which
        // also re-captures a stale video manifest next to the static
        // record — how a theme could restore an old video.
        let json = layers_doc(&layer("skwd-paper", 1.0, 439101), "");
        let daemon = r#"{"outputs": [
            {"name": "HDMI-A-1", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true},
            {"name": "DP-3", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true}
        ]}"#;
        let kind = decide_active_kind(Some(&json), Some(daemon));
        assert_eq!(kind, Some(WallpaperKind::Static));
        assert_eq!(
            plan_from_kind(kind),
            SavePlan::StaticOnly,
            "a proven static scene must not fall back to capture-everything"
        );
    }

    #[test]
    fn every_output_without_a_background_layer_decides_nothing() {
        // All absence: nothing proves a background, so no kind is invented.
        let json = layers_doc("", "");
        let daemon = r#"{"outputs": [
            {"name": "HDMI-A-1", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true}
        ]}"#;
        assert_eq!(decide_active_kind(Some(&json), Some(daemon)), None);
    }

    #[test]
    fn absent_output_does_not_mask_video_on_the_other_output() {
        // Absence must not dilute a PROVEN kind: a video on one output
        // still wins while the other has no background layer.
        let json = layers_doc(&layer("mpvpaper", 1.0, 507331), "");
        assert_eq!(
            decide_active_kind(Some(&json), None),
            Some(WallpaperKind::Video)
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
    /// compositor (`hyprctl`) inside the single snapshot-based entry point
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
            code.contains("snapshot_wallpaper_kind"),
            "wallpaper_authority must expose a single snapshot-based active-kind entry point"
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
        // D3: the authority-snapshot budget (~1 s) must never be sold as
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

    // ── G1/G2 real-capture tests ──
    //
    // Ground truth from the keeper's machine (/tmp/opencode/wall-*/):
    // - baseline: mpvpaper video on top at level 0, daemon says static.
    // - after: skwd static on top at level 0, daemon says static.
    // - skwdvideo: skwd-wall-vk video at level 1, daemon says video with
    //   the slugcat current, no mpvpaper running. Level 1 also carries
    //   small Noctalia desktop widgets that must never win or veto.
    // Rule under test: scan background-capable levels 0 and 1 (higher
    // wins; later entry wins within a level), skip non-opaque and
    // non-fullscreen (furniture) entries, attribute as before.

    fn layer_geo(namespace: &str, alpha: f32, pid: u32, w: u32, h: u32) -> String {
        format!(
            r#"{{"address":"0x1","x":0,"y":0,"w":{},"h":{},"alpha":{},"namespace":"{}","pid":{}}}"#,
            w, h, alpha, namespace, pid
        )
    }

    fn widget(namespace: &str) -> String {
        // Noctalia desktop widgets: small rectangles at level 1.
        layer_geo(namespace, 1.0, 250289, 272, 144)
    }

    /// Keeper's skwdvideo capture (essential shape): empty level 0, level 1
    /// mixes small widgets with one fullscreen skwd-wall-vk layer last.
    fn skwdvideo_layers_doc() -> String {
        format!(
            r#"{{"HDMI-A-1":{{"levels":{{"0":[],"1":[{}]}}}},"DP-3":{{"levels":{{"0":[],"1":[{},{},{}]}}}}}}"#,
            layer_geo("skwd-wall-vk", 1.0, 606866, 1440, 900),
            widget("noctalia-desktop-widget-weather-0000000000000001"),
            widget("noctalia-desktop-widget-sysmon-0000000000000002"),
            layer_geo("skwd-wall-vk", 1.0, 606866, 2560, 1440),
        )
    }

    fn skwdvideo_daemon_doc() -> String {
        r#"{"outputs": [
            {"name": "DP-3", "current": "/home/ximo/Pictures/LiveWallpapers/slugcat-rain-world-moewalls-com.mp4",
             "path": "/home/ximo/Pictures/LiveWallpapers/slugcat-rain-world-moewalls-com.mp4",
             "type": "video", "connected": true},
            {"name": "HDMI-A-1", "current": "/home/ximo/Pictures/LiveWallpapers/slugcat-rain-world-moewalls-com.mp4",
             "path": "/home/ximo/Pictures/LiveWallpapers/slugcat-rain-world-moewalls-com.mp4",
             "type": "video", "connected": true}
        ]}"#
        .to_string()
    }

    /// Keeper's baseline capture (essential shape): mpvpaper video on top
    /// of skwd-paper at level 0 while the daemon insists on static.
    fn baseline_layers_doc() -> String {
        let hdmi = format!(
            "{},{}",
            layer_geo("skwd-paper", 1.0, 439101, 1440, 900),
            layer_geo("mpvpaper", 1.0, 507331, 1440, 900),
        );
        let dp = format!(
            "{},{}",
            layer_geo("skwd-paper", 1.0, 439101, 2560, 1440),
            layer_geo("mpvpaper", 1.0, 507331, 2560, 1440),
        );
        format!(
            r#"{{"HDMI-A-1":{{"levels":{{"0":[{}],"1":[]}}}},"DP-3":{{"levels":{{"0":[{}],"1":[]}}}}}}"#,
            hdmi, dp
        )
    }

    /// Keeper's after capture (essential shape): skwd static repainted on
    /// top of the still-running mpvpaper at level 0; daemon says static.
    fn after_layers_doc() -> String {
        let hdmi = format!(
            "{},{}",
            layer_geo("mpvpaper", 1.0, 507331, 1440, 900),
            layer_geo("skwd-paper", 1.0, 588760, 1440, 900),
        );
        let dp = format!(
            "{},{}",
            layer_geo("mpvpaper", 1.0, 507331, 2560, 1440),
            layer_geo("skwd-paper", 1.0, 588760, 2560, 1440),
        );
        format!(
            r#"{{"HDMI-A-1":{{"levels":{{"0":[{}],"1":[]}}}},"DP-3":{{"levels":{{"0":[{}],"1":[]}}}}}}"#,
            hdmi, dp
        )
    }

    fn static_daemon_for_both() -> String {
        r#"{"outputs": [
            {"name": "HDMI-A-1", "current": "/pic/joker2.png", "path": "/pic/joker2.png",
             "type": "static", "connected": true},
            {"name": "DP-3", "current": "/pic/joker2.png", "path": "/pic/joker2.png",
             "type": "static", "connected": true}
        ]}"#
        .to_string()
    }

    #[test]
    fn real_baseline_capture_decides_video() {
        // mpvpaper on top at level 0; daemon static must not win.
        let json = baseline_layers_doc();
        assert_eq!(
            decide_active_kind(Some(&json), Some(&static_daemon_for_both())),
            Some(WallpaperKind::Video)
        );
    }

    #[test]
    fn real_after_capture_decides_static() {
        // skwd repainted on top at level 0; the covered video must not win.
        let json = after_layers_doc();
        assert_eq!(
            decide_active_kind(Some(&json), Some(&static_daemon_for_both())),
            Some(WallpaperKind::Static)
        );
    }

    #[test]
    fn real_skwdvideo_capture_decides_video() {
        // skwd-wall-vk paints at level 1 (not 0); widgets share the level
        // but are not fullscreen and must be skipped. Daemon says video.
        let json = skwdvideo_layers_doc();
        let daemon = skwdvideo_daemon_doc();
        assert_eq!(
            decide_active_kind(Some(&json), Some(&daemon)),
            Some(WallpaperKind::Video)
        );
    }

    #[test]
    fn widgets_alone_never_decide() {
        // Level 1 with ONLY small widgets (no fullscreen layer anywhere):
        // nothing wins, nothing vetoes with a crash — just unknown.
        let json = format!(
            r#"{{"HDMI-A-1":{{"levels":{{"0":[],"1":[{}]}}}}}}"#,
            widget("noctalia-desktop-widget-weather-0000000000000001"),
        );
        let daemon = daemon_kind_doc("video");
        assert_eq!(decide_active_kind(Some(&json), Some(&daemon)), None);
    }

    #[test]
    fn unknown_fullscreen_painter_at_level1_vetoes() {
        // Level 0 says skwd/static but a fullscreen unknown painter sits at
        // level 1 on top: it could be a background, so it must veto (None),
        // never be silently skipped. Namespace + pid are kept for the log.
        let json = format!(
            r#"{{"HDMI-A-1":{{"levels":{{"0":[{}],"1":[{}]}}}}}}"#,
            layer_geo("skwd-paper", 1.0, 439101, 1440, 900),
            layer_geo("mystery-layer", 1.0, 999, 1440, 900),
        );
        let outputs = parse_compositor_layers(&json).unwrap();
        assert_eq!(outputs[0].top_namespace, "mystery-layer");
        assert_eq!(outputs[0].top_pid, 999);
        let daemon = daemon_kind_doc("static");
        assert_eq!(decide_active_kind(Some(&json), Some(&daemon)), None);
    }

    #[test]
    fn higher_level_wins_over_lower() {
        // Video at level 0 but a skwd layer paints above it at level 1:
        // the compositor shows what is on top, so the daemon's static
        // report for skwd decides static.
        let json = format!(
            r#"{{"HDMI-A-1":{{"levels":{{"0":[{}],"1":[{}]}}}}}}"#,
            layer_geo("mpvpaper", 1.0, 507331, 1440, 900),
            layer_geo("skwd-paper", 1.0, 439101, 1440, 900),
        );
        let daemon = daemon_kind_doc("static");
        assert_eq!(
            decide_active_kind(Some(&json), Some(&daemon)),
            Some(WallpaperKind::Static)
        );
    }

    #[test]
    fn transparent_level1_falls_back_to_level0() {
        // A fading (alpha 0) level-1 layer is not visible: the opaque
        // level-0 layer below decides.
        let json = format!(
            r#"{{"HDMI-A-1":{{"levels":{{"0":[{}],"1":[{}]}}}}}}"#,
            layer_geo("skwd-paper", 1.0, 439101, 1440, 900),
            layer_geo("skwd-wall-vk", 0.0, 606866, 1440, 900),
        );
        let daemon = daemon_kind_doc("static");
        assert_eq!(
            decide_active_kind(Some(&json), Some(&daemon)),
            Some(WallpaperKind::Static)
        );
    }

    #[test]
    fn empty_levels_and_malformed_stay_unknown() {
        let json = r#"{"HDMI-A-1": {"levels": {"0": [], "1": []}}}"#;
        let daemon = daemon_kind_doc("static");
        assert_eq!(decide_active_kind(Some(json), Some(&daemon)), None);
        assert_eq!(decide_active_kind(Some("not json"), Some(&daemon)), None);
    }

    #[test]
    fn multi_output_disagreement_across_levels_is_video() {
        // HDMI shows skwd/static at level 0; DP-3 runs a skwd video at
        // level 1 above an empty level 0. Any visible video wins.
        let json = format!(
            r#"{{"HDMI-A-1":{{"levels":{{"0":[{}],"1":[]}}}},"DP-3":{{"levels":{{"0":[],"1":[{}]}}}}}}"#,
            layer_geo("skwd-paper", 1.0, 439101, 1440, 900),
            layer_geo("skwd-wall-vk", 1.0, 606866, 2560, 1440),
        );
        let daemon = r#"{"outputs": [
            {"name": "HDMI-A-1", "current": "/pic/a.png", "path": "/pic/a.png",
             "type": "static", "connected": true},
            {"name": "DP-3", "current": "/vid/slugcat.mp4", "path": "/vid/slugcat.mp4",
             "type": "video", "connected": true}
        ]}"#;
        assert_eq!(
            decide_active_kind(Some(&json), Some(daemon)),
            Some(WallpaperKind::Video)
        );
    }

    // ── G2 video-identity tests ──
    //
    // The video record must come from the process that paints the topmost
    // layer (its argv carries the media path), falling back to the
    // daemon's `current` for skwd-suite layers — never from mpvpaper's
    // stale state file.

    fn mpvpaper_argv() -> Vec<&'static str> {
        // Keeper's verbatim mpvpaper command line (wall-baseline/mpvpaper.txt).
        vec![
            "mpvpaper",
            "--auto-pause",
            "--auto-mode",
            "FULL",
            "-o",
            "loop-file=inf",
            "panscan=1.0",
            "no-audio",
            "hwdec=auto",
            "input-ipc-server=/home/ximo/.local/state/noctalia/mpvpaper/ipc-_.sock",
            "*",
            "/home/ximo/Pictures/LiveWallpapers/ellen-joe-neon-alley-zenless-zone-zero-moewalls-com.mp4",
        ]
    }

    fn skwd_wall_vk_argv() -> Vec<&'static str> {
        // Keeper's verified skwd-wall-vk command line shape.
        vec![
            "/usr/bin/skwd-wall-vk",
            "*",
            "/home/ximo/Pictures/LiveWallpapers/slugcat-rain-world-moewalls-com.mp4",
            "--transition",
            "fade",
        ]
    }

    #[test]
    fn identity_comes_from_mpvpaper_argv() {
        assert_eq!(
            extract_video_path_from_argv(&mpvpaper_argv()),
            Some(
                "/home/ximo/Pictures/LiveWallpapers/ellen-joe-neon-alley-zenless-zone-zero-moewalls-com.mp4"
                    .to_string()
            )
        );
    }

    #[test]
    fn identity_comes_from_skwd_wall_vk_argv() {
        assert_eq!(
            extract_video_path_from_argv(&skwd_wall_vk_argv()),
            Some(
                "/home/ximo/Pictures/LiveWallpapers/slugcat-rain-world-moewalls-com.mp4".to_string()
            )
        );
    }

    /// D2: the exact live argv from the verifier. `--transition-from`
    /// carries the PREVIOUS background (here a static image); the identity
    /// is the media path that follows the `*` marker — the CURRENT video —
    /// never the last media-looking argument.
    #[test]
    fn identity_skwd_vk_transition_from_png_yields_current_video() {
        let argv = vec![
            "/usr/bin/skwd-wall-vk",
            "*",
            "/home/ximo/Pictures/LiveWallpapers/slugcat-rain-world-moewalls-com.mp4",
            "--transition-from",
            "/home/ximo/Pictures/Wallpapers/Joker/joker2.png",
            "--shader",
            "random",
        ];
        assert_eq!(
            extract_video_path_from_argv(&argv),
            Some(
                "/home/ximo/Pictures/LiveWallpapers/slugcat-rain-world-moewalls-com.mp4".to_string()
            )
        );
    }

    /// D2: on a video-to-video switch the transition argument is ANOTHER
    /// video, so "last media path" returns the PREVIOUS one. The identity
    /// must be structural: the media path following `*` is the current
    /// video.
    #[test]
    fn identity_video_to_video_switch_yields_current_video() {
        let argv = vec![
            "/usr/bin/skwd-wall-vk",
            "*",
            "/home/ximo/Pictures/LiveWallpapers/new-video.mp4",
            "--transition-from",
            "/home/ximo/Pictures/LiveWallpapers/old-video.mp4",
            "--shader",
            "random",
        ];
        assert_eq!(
            extract_video_path_from_argv(&argv),
            Some("/home/ximo/Pictures/LiveWallpapers/new-video.mp4".to_string())
        );
    }

    /// D2: when the identity is still ambiguous (no `*` marker with several
    /// media candidates, or several `*` outputs in one argv), do NOT guess —
    /// veto honestly so the caller falls back instead of recording the
    /// wrong video.
    #[test]
    fn identity_ambiguous_argv_is_none() {
        // Two videos, no `*` marker to attribute them: unresolvable.
        assert_eq!(
            extract_video_path_from_argv(&[
                "/usr/bin/skwd-wall-vk",
                "/home/ximo/Pictures/LiveWallpapers/a.mp4",
                "--transition-from",
                "/home/ximo/Pictures/LiveWallpapers/b.mp4",
            ]),
            None
        );
        // Two `*` outputs in one argv (per-output painters): attribution
        // is ambiguous, never a guess.
        assert_eq!(
            extract_video_path_from_argv(&[
                "/usr/bin/skwd-wall-vk",
                "*",
                "/home/ximo/Pictures/LiveWallpapers/a.mp4",
                "*",
                "/home/ximo/Pictures/LiveWallpapers/b.mp4",
            ]),
            None
        );
        // A `*` with no media after it: the paths before it are the
        // previous background, never the identity.
        assert_eq!(
            extract_video_path_from_argv(&[
                "/usr/bin/skwd-wall-vk",
                "/home/ximo/Pictures/LiveWallpapers/old.mp4",
                "*",
                "--shader",
                "random",
            ]),
            None
        );
    }

    #[test]
    fn identity_argv_without_media_path_is_none() {
        // Flags, sockets, the "*" connector and relative names are never
        // mistaken for a video: no path is invented.
        assert_eq!(
            extract_video_path_from_argv(&["skwd-wall-vk", "*", "--transition", "fade"]),
            None
        );
        assert_eq!(
            extract_video_path_from_argv(&[
                "mpvpaper",
                "input-ipc-server=/home/ximo/.local/state/noctalia/mpvpaper/ipc-_.sock",
                "*",
                "relative-video.mp4",
            ]),
            None
        );
        assert_eq!(extract_video_path_from_argv(&[]), None);
    }

    #[test]
    fn identity_daemon_current_fallback() {
        // Skwd-suite layer whose painter argv yields nothing: the daemon's
        // `current` for the same output is the fallback.
        let daemon = parse_authority_outputs(&skwdvideo_daemon_doc()).unwrap();
        assert_eq!(
            daemon_video_current(&daemon, "DP-3"),
            Some(
                "/home/ximo/Pictures/LiveWallpapers/slugcat-rain-world-moewalls-com.mp4".to_string()
            )
        );
        assert_eq!(daemon_video_current(&daemon, "NOPE"), None);
        let disconnected = parse_authority_outputs(
            r#"{"outputs": [{"name": "DP-3", "current": "/vid/a.mp4", "path": "/vid/a.mp4",
                 "type": "video", "connected": false}]}"#,
        )
        .unwrap();
        assert_eq!(daemon_video_current(&disconnected, "DP-3"), None);
    }

    #[test]
    fn identity_daemon_duplicate_names_prefer_connected() {
        // Ground truth from the keeper's skwdvideo capture: the daemon
        // emits a stale DISCONNECTED duplicate per output after the real
        // CONNECTED entry. Attribution must skip the stale duplicate even
        // when it comes first.
        let daemon = parse_authority_outputs(
            r#"{"outputs": [
                {"name": "HDMI-A-1", "current": "/pic/stale.png", "path": "/pic/stale.png",
                 "type": "static", "connected": false},
                {"name": "HDMI-A-1", "current": "/vid/slugcat.mp4", "path": "/vid/slugcat.mp4",
                 "type": "video", "connected": true}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            daemon_video_current(&daemon, "HDMI-A-1"),
            Some("/vid/slugcat.mp4".to_string())
        );
    }

    #[test]
    fn real_baseline_level3_overlay_is_ignored() {
        // The keeper's baseline capture also carries a fullscreen skwd-wall
        // layer at level 3 (overlay). Levels 2+ never hold backgrounds, so
        // the decision still comes from level 0's mpvpaper-on-top: video.
        let level0 = format!(
            "{},{}",
            layer_geo("skwd-paper", 1.0, 439101, 1440, 900),
            layer_geo("mpvpaper", 1.0, 507331, 1440, 900),
        );
        let json = format!(
            r#"{{"HDMI-A-1":{{"levels":{{"0":[{level0}],"1":[],"2":[],"3":[{}]}}}}}}"#,
            layer_geo("skwd-wall", 1.0, 572518, 1440, 900),
        );
        assert_eq!(
            decide_active_kind(Some(&json), Some(&static_daemon_for_both())),
            Some(WallpaperKind::Video)
        );
    }

    // ── G2/G3 painter video record (video.txt) ──
    //
    // A painter-identified video is recorded exactly (kind + path + who
    // painted it) so apply can restore it through the manager in charge.
    // Format: `key=value` lines; only `path` is required on read.

    #[test]
    fn painter_video_record_roundtrips() {
        let record = format_painter_video_record(
            Path::new("/home/ximo/Pictures/LiveWallpapers/slugcat-rain-world-moewalls-com.mp4"),
            Some("skwd-wall-vk"),
            "skwd-wall-vk",
            606866,
        );
        assert_eq!(
            parse_painter_video_record(&record),
            Some(PathBuf::from(
                "/home/ximo/Pictures/LiveWallpapers/slugcat-rain-world-moewalls-com.mp4"
            ))
        );
    }

    #[test]
    fn painter_video_record_without_path_is_none() {
        // No path, no record: apply must fall back honestly, never invent one.
        assert_eq!(parse_painter_video_record(""), None);
        assert_eq!(parse_painter_video_record("painter=skwd-wall-vk\n"), None);
        assert_eq!(parse_painter_video_record("path=\n"), None);
        assert_eq!(parse_painter_video_record("path=relative.mp4\n"), None);
    }

    // ── D3 relative-fullscreen tests ──
    //
    // "Fullscreen" is relative to the output, never an absolute pixel
    // constant: a 1920x1080 output at 200% scale reports a 960x540 layer
    // for its real fullscreen wallpaper, which an absolute 800x600 gate
    // excludes (540 < 600) — a silent veto that loses the background.

    /// `hyprctl -j monitors` shape (array, physical size + scale): the
    /// reference size is the logical size (physical / scale).
    fn scaled_monitors_doc() -> String {
        r#"[{
            "name": "HDMI-A-1", "description": "Test Monitor",
            "width": 1920, "height": 1080, "x": 0, "y": 0, "scale": 2.0
        }]"#
        .to_string()
    }

    fn scaled_layers_doc() -> String {
        // The real fullscreen wallpaper on that output: 960x540 opaque.
        r#"{"HDMI-A-1": {"levels": {"0": [
            {"address":"0x1","x":0,"y":0,"w":960,"h":540,"alpha":1,
             "namespace":"skwd-paper","pid":111}
        ], "1": []}}}"#
            .to_string()
    }

    #[test]
    fn scaled_output_fullscreen_layer_is_a_background() {
        // 1920x1080 at 200%: the 960x540 fullscreen layer must be kept and
        // attributed (daemon says static), not vetoed by an absolute gate.
        let daemon = daemon_kind_doc("static");
        let monitors = scaled_monitors_doc();
        assert_eq!(
            decide_active_kind_with_monitors(
                Some(&scaled_layers_doc()),
                Some(&daemon),
                Some(&monitors),
            ),
            Some(WallpaperKind::Static)
        );
    }

    #[test]
    fn small_output_fullscreen_layer_is_a_background() {
        // A tiny 640x360 output: its fullscreen layer is far below any
        // absolute gate but covers 100% of its output.
        let json = r#"{"DP-3": {"levels": {"0": [
            {"address":"0x1","x":0,"y":0,"w":640,"h":360,"alpha":1,
             "namespace":"skwd-paper","pid":111}
        ], "1": []}}}"#;
        let monitors = r#"[{
            "name": "DP-3", "description": "Tiny",
            "width": 640, "height": 360, "x": 0, "y": 0, "scale": 1.0
        }]"#;
        let daemon = r#"{"outputs": [{"name": "DP-3", "current": "/x", "path": "/x",
             "type": "static", "connected": true}]}"#;
        assert_eq!(
            decide_active_kind_with_monitors(Some(json), Some(daemon), Some(monitors)),
            Some(WallpaperKind::Static)
        );
    }

    #[test]
    fn furniture_still_excluded_with_reference() {
        // A 272x144 widget on a 2560x1440 output covers a corner, not the
        // screen: it must be skipped (no winner, no veto crash — unknown).
        let json = format!(
            r#"{{"DP-3":{{"levels":{{"0":[],"1":[{}]}}}}}}"#,
            widget("noctalia-desktop-widget-weather-0000000000000001"),
        );
        let monitors = r#"[{
            "name": "DP-3", "description": "Big",
            "width": 2560, "height": 1440, "x": 0, "y": 0, "scale": 1.0
        }]"#;
        assert_eq!(
            decide_active_kind_with_monitors(Some(&json), Some(&daemon_kind_doc("video")), Some(monitors)),
            None
        );
    }

    #[test]
    fn no_reference_never_silently_excludes() {
        // Without any output reference (monitors unreachable, daemon
        // silent about sizes) an opaque layer must NOT be size-excluded:
        // it stays visible to the decision instead of vanishing silently.
        let json = r#"{"HDMI-A-1": {"levels": {"0": [
            {"address":"0x1","x":0,"y":0,"w":960,"h":540,"alpha":1,
             "namespace":"skwd-paper","pid":111}
        ], "1": []}}}"#;
        let outputs =
            parse_compositor_layers_with_reference(json, &std::collections::HashMap::new())
                .expect("layers must parse");
        assert_eq!(
            outputs[0].top_namespace, "skwd-paper",
            "no reference means no size exclusion, never a silent veto"
        );
    }

    #[test]
    fn monitors_parser_divides_physical_by_scale() {
        let sizes = parse_monitor_sizes(&scaled_monitors_doc()).expect("monitors must parse");
        let hdmi = sizes.get("HDMI-A-1").expect("HDMI-A-1 must be present");
        assert_eq!((hdmi.width, hdmi.height), (960, 540));
    }

    // ── Save-side static path tests ──
    //
    // The static arm records the daemon's `current` when the compositor
    // proves the skwd suite paints a static scene (the keeper's bug: a
    // stale `wallpaper-get` was recorded instead). The theme model holds
    // ONE path, so the rule is the lexicographically-first skwd-painted
    // output — deterministic no matter the parse order.

    #[test]
    fn static_current_needs_a_connected_static_entry() {
        let daemon = parse_authority_outputs(
            r#"{"outputs": [
                {"name": "DP-3", "current": "/pic/joker1.png", "path": "/pic/joker1.png",
                 "type": "static", "connected": true},
                {"name": "HDMI-A-1", "current": "/vid/loop.mp4", "path": "/vid/loop.mp4",
                 "type": "video", "connected": true},
                {"name": "OLD", "current": "/pic/stale.png", "path": "/pic/stale.png",
                 "type": "static", "connected": false}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            daemon_static_current(&daemon, "DP-3"),
            Some("/pic/joker1.png".to_string())
        );
        // A video kind is not a static image, even with a current.
        assert_eq!(daemon_static_current(&daemon, "HDMI-A-1"), None);
        // A disconnected duplicate must not decide, even when named.
        assert_eq!(daemon_static_current(&daemon, "OLD"), None);
        assert_eq!(daemon_static_current(&daemon, "NOPE"), None);
    }

    #[test]
    fn static_path_picks_first_sorted_output() {
        // Both outputs skwd-painted static with different currents: the
        // record is DP-3's (sorted first), whatever the input order.
        let daemon = parse_authority_outputs(
            r#"{"outputs": [
                {"name": "HDMI-A-1", "current": "/pic/b.png", "path": "/pic/b.png",
                 "type": "static", "connected": true},
                {"name": "DP-3", "current": "/pic/a.png", "path": "/pic/a.png",
                 "type": "static", "connected": true}
            ]}"#,
        )
        .unwrap();
        let json = layers_doc(&layer("skwd-paper", 1.0, 439101), &layer("skwd-paper", 1.0, 439101));
        let compositor = parse_compositor_layers(&json).unwrap();
        assert_eq!(
            resolve_static_path(&compositor, &daemon),
            Some("/pic/a.png".to_string())
        );
    }

    #[test]
    fn static_path_vetoes_video_and_unknown_painters() {
        // A video anywhere, or an unknown painter on top, vetoes: a
        // partial static record would hide what is really showing.
        let daemon = parse_authority_outputs(&static_daemon_for_both()).unwrap();
        let video_json = layers_doc(
            &layer("skwd-paper", 1.0, 439101),
            &format!(
                "{},{}",
                layer("skwd-paper", 1.0, 439101),
                layer("mpvpaper", 1.0, 507331)
            ),
        );
        let video = parse_compositor_layers(&video_json).unwrap();
        assert_eq!(resolve_static_path(&video, &daemon), None);
        let unknown_json = layers_doc(
            &layer("mystery-layer", 1.0, 999),
            &layer("skwd-paper", 1.0, 439101),
        );
        let unknown = parse_compositor_layers(&unknown_json).unwrap();
        assert_eq!(resolve_static_path(&unknown, &daemon), None);
    }

    #[test]
    fn static_path_ignores_output_with_no_background_layer() {
        // The keeper's partial scene: DP-3 carries no background layer at
        // all (absence — e.g. only a small widget the reference excluded),
        // HDMI-A-1 is skwd-painted static. Absence is not disagreement:
        // the proven output's daemon current wins instead of the stale
        // third party value.
        let daemon = parse_authority_outputs(
            r#"{"outputs": [
                {"name": "DP-3", "current": "/pic/dp3.png", "path": "/pic/dp3.png",
                 "type": "static", "connected": true},
                {"name": "HDMI-A-1", "current": "/pic/hdmi.png", "path": "/pic/hdmi.png",
                 "type": "static", "connected": true}
            ]}"#,
        )
        .unwrap();
        let compositor = vec![
            CompositorOutput {
                name: "DP-3".to_string(),
                kind: None,
                top_namespace: String::new(),
                top_pid: 0,
                top_level: None,
            },
            CompositorOutput {
                name: "HDMI-A-1".to_string(),
                kind: None,
                top_namespace: "skwd-paper".to_string(),
                top_pid: 439102,
                top_level: Some(0),
            },
        ];
        assert_eq!(
            resolve_static_path(&compositor, &daemon),
            Some("/pic/hdmi.png".to_string())
        );
    }

    #[test]
    fn static_path_vetoes_skwd_static_vs_video_conflict() {
        // A genuine conflict: one output proven skwd-painted static, another
        // proven skwd-painted video. A partial static record would hide the
        // live video, so the decision vetoes to None (legacy) exactly as
        // for an unknown painter.
        let daemon = parse_authority_outputs(
            r#"{"outputs": [
                {"name": "DP-3", "current": "/pic/a.png", "path": "/pic/a.png",
                 "type": "static", "connected": true},
                {"name": "HDMI-A-1", "current": "/vid/b.mp4", "path": "/vid/b.mp4",
                 "type": "video", "connected": true}
            ]}"#,
        )
        .unwrap();
        let json = layers_doc(
            &layer("skwd-paper", 1.0, 439101),
            &layer("skwd-wall-vk", 1.0, 606866),
        );
        let compositor = parse_compositor_layers(&json).unwrap();
        assert_eq!(
            decide_active_kind_from_outputs(&compositor, &daemon),
            Some(WallpaperKind::Video)
        );
        assert_eq!(resolve_static_path(&compositor, &daemon), None);
    }

    #[test]
    fn static_path_all_absent_is_none() {
        // No output carries a background layer at all: nothing is proven,
        // so there is no path — the caller keeps the legacy value quietly.
        let daemon = parse_authority_outputs(&static_daemon_for_both()).unwrap();
        let compositor = vec![
            CompositorOutput {
                name: "DP-3".to_string(),
                kind: None,
                top_namespace: String::new(),
                top_pid: 0,
                top_level: None,
            },
            CompositorOutput {
                name: "HDMI-A-1".to_string(),
                kind: None,
                top_namespace: String::new(),
                top_pid: 0,
                top_level: None,
            },
        ];
        assert_eq!(resolve_static_path(&compositor, &daemon), None);
    }
}
